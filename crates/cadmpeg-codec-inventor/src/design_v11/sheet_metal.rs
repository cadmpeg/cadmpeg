// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal style defaults in the part scope.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ids::SheetMetalRuleId;
use cadmpeg_ir::design::ops::sheet_metal::{
    BendAllowance, BendSpec, Relief, ReliefShape, SheetMetalRule,
};
use cadmpeg_ir::features::parameters::ParameterValue;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, Length, PositiveLength};
use cadmpeg_ir::CadIr;

use crate::feature::sheet_metal::{style::Style, unfold};
use crate::feature::FeatureInventory;
use crate::loss::InventorLossCode;
use crate::pmdc::PmDcReference;

pub(crate) mod attachments;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    inventory: &FeatureInventory,
    parameters: &[crate::design::PmDcParameter],
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let raw = unique_index(parameters.iter().map(|parameter| {
        (
            (
                parameter.identity.segment_token.as_str(),
                parameter.identity.record_ordinal,
            ),
            parameter,
        )
    }));
    let properties = property_index(inventory);
    let values = unique_index(ir.model.parameters.iter().filter_map(|parameter| {
        Some((parameter.native_ref.as_deref()?, parameter.value.as_ref()?))
    }));
    let length = |style: &Style, reference: PmDcReference| {
        let parameter = raw
            .get(&(
                style.identity.segment_token.as_str(),
                reference.record_ordinal()?,
            ))?
            .as_ref()?;
        match values.get(parameter.id().as_str())?.as_ref()? {
            ParameterValue::Length(value) => Some(*value),
            _ => None,
        }
    };
    let style_factor = |style: &Style| {
        let parameter = match style_factor_source(parameters, style) {
            FactorSource::Absent => return StyleFactor::Absent,
            FactorSource::Several => return StyleFactor::Unresolved,
            FactorSource::One(parameter) => parameter,
        };
        match values.get(parameter.id().as_str()).and_then(Option::as_ref) {
            Some(ParameterValue::Real(value)) => StyleFactor::Value(*value),
            _ => StyleFactor::Unresolved,
        }
    };
    let active = unfold::active_rule(
        &inventory.sheet_metal_unfold_rules,
        &inventory.sheet_metal_unfold_selections,
    )
    .map(|rule| DocumentRule {
        id: rule.id(),
        expression: &rule.expression,
    });
    let design = unique_index(
        ir.design
            .parameters
            .iter()
            .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter))),
    );
    let binding = |style: &Style, reference: PmDcReference| {
        let parameter = raw
            .get(&(
                style.identity.segment_token.as_str(),
                reference.record_ordinal()?,
            ))?
            .as_ref()?;
        let parameter = design.get(parameter.id().as_str())?.as_ref()?;
        (parameter.kind
            == cadmpeg_ir::design::units::ValueKind::Quantity(
                cadmpeg_ir::design::units::Quantity::LENGTH,
            ))
        .then(|| Dimension::Parameter(parameter.id.clone()))
    };
    let mut projected = rules(
        ctx,
        ir,
        &inventory.sheet_metal_styles,
        length,
        style_factor,
        active.as_ref(),
        losses,
    )?;
    for (rule, style) in projected.iter_mut().zip(&inventory.sheet_metal_styles) {
        if active.is_none() && matches!(rule.allowance, Some(BendAllowance::KFactor(_))) {
            if let FactorSource::One(source) = style_factor_source(parameters, style) {
                if let Some(parameter) = design
                    .get(source.id().as_str())
                    .and_then(Option::as_ref)
                    .filter(|parameter| {
                        parameter.kind
                            == cadmpeg_ir::design::units::ValueKind::Quantity(
                                cadmpeg_ir::design::units::Quantity::REAL,
                            )
                    })
                {
                    rule.allowance = Some(BendAllowance::KFactor(Dimension::Parameter(
                        parameter.id.clone(),
                    )));
                }
            }
        }
        rule.thickness = binding(style, style.thickness).or_else(|| rule.thickness.take());
        rule.bend.radius = binding(style, style.bend_radius).or_else(|| rule.bend.radius.take());
        let shape = style
            .bend_relief_form
            .record_ordinal()
            .and_then(|ordinal| properties.get(&(style.identity.segment_token.as_str(), ordinal)))
            .and_then(Option::as_ref)
            .and_then(|property| crate::feature::sheet_metal::operands::enumeration(property))
            .and_then(|(family, value)| {
                cadmpeg_core::text::NonBlankString::new(format!(
                    "inventor:bend_relief:{family}:{value}"
                ))
            })
            .map(ReliefShape::Native);
        let positive = |reference| {
            let value = PositiveLength::try_from(length(style, reference)?).ok()?;
            Some(binding(style, reference).unwrap_or(Dimension::Literal(value)))
        };
        let width = positive(style.bend_relief_width);
        let depth = positive(style.bend_relief_depth);
        if style.bend_relief_form.index() != 0
            || style.bend_relief_width.index() != 0
            || style.bend_relief_depth.index() != 0
        {
            losses.push(InventorLossCode::FeatureSheetMetalReliefNative.note(format!(
                "Sheet-metal rule {} keeps its relief-form source and unadmitted relief dimensions in native records.", rule.id
            )));
        }
        if shape.is_some() || width.is_some() || depth.is_some() {
            rule.bend.relief = Some(Relief {
                shape,
                width,
                width_ratio: None,
                depth,
            });
        }
        if let Some(active) = &active {
            rule.source_settings.insert(
                cadmpeg_core::nonblank_literal!("inventor:unfold_rule"),
                cadmpeg_ir::design::options::NativeOption::Text(Dimension::Literal(
                    cadmpeg_core::text::NonBlankString::new(active.id.clone()).ok_or_else(
                        || CodecError::malformed("Inventor unfold rule identity is blank"),
                    )?,
                )),
            );
        }
    }
    ir.design.sheet_metal_rules.extend(projected);
    ir.design.sheet_metal_rules.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// Name of the unitless K-factor parameter in the parameter segment of a
/// sheet-metal style.
const STYLE_FACTOR_NAME: &str = "KFactor";

/// The `KFactor` parameters of a style's segment.
enum FactorSource<'a> {
    Absent,
    Several,
    One(&'a crate::design::PmDcParameter),
}

/// The `KFactor` parameter of `style`'s segment.
fn style_factor_source<'a>(
    parameters: &'a [crate::design::PmDcParameter],
    style: &Style,
) -> FactorSource<'a> {
    let mut named = parameters.iter().filter(|parameter| {
        parameter.identity.segment_token == style.identity.segment_token
            && parameter.name == STYLE_FACTOR_NAME
    });
    match (named.next(), named.next()) {
        (None, _) => FactorSource::Absent,
        (Some(parameter), None) => FactorSource::One(parameter),
        (Some(_), Some(_)) => FactorSource::Several,
    }
}

/// The feature properties of `inventory` by segment token and record
/// ordinal; a repeated key holds `None`.
pub(super) fn property_index(
    inventory: &FeatureInventory,
) -> std::collections::HashMap<(&str, u32), Option<&crate::feature::PmDcFeatureProperty>> {
    unique_index(inventory.properties.iter().map(|property| {
        (
            (
                property.identity.segment_token.as_str(),
                property.identity.record_ordinal,
            ),
            property,
        )
    }))
}

/// The active unfold rule of the document.
struct DocumentRule<'a> {
    id: String,
    expression: &'a str,
}

/// The `KFactor` parameter in the parameter segment of a style.
#[derive(Debug, Clone, Copy, PartialEq)]
enum StyleFactor {
    /// The segment has no `KFactor` parameter.
    Absent,
    /// The segment has one `KFactor` parameter with this unitless value.
    Value(FiniteReal),
    /// The segment has several `KFactor` parameters, or the value is not a
    /// unitless real.
    Unresolved,
}

fn rules(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    styles: &[Style],
    length: impl Fn(&Style, PmDcReference) -> Option<Length>,
    style_factor: impl Fn(&Style) -> StyleFactor,
    active: Option<&DocumentRule<'_>>,
    losses: &mut Vec<LossNote>,
) -> Result<Vec<SheetMetalRule>, CodecError> {
    let Some(part) = ir.model.product_definitions.first() else {
        return Ok(Vec::new());
    };
    let factor = active.and_then(|rule| unfold::factor(rule.expression));
    if let Some(rule) = active.filter(|_| factor.is_none() && !styles.is_empty()) {
        losses.push(InventorLossCode::FeatureSheetMetalUnfoldFactorNative.note(format!(
            "Active unfold rule {} has expression {:?}, which states no decimal K-factor; the sheet-metal rules keep no bend allowance and the rule stays native.",
            rule.id, rule.expression
        )));
    }
    if active.is_none() && !styles.is_empty() {
        losses.push(InventorLossCode::FeatureSheetMetalUnfoldRuleUnresolved.note(
            "No active unfold rule resolves: the document has no single SheetMetalUnfold category with one entry that names an unfold rule by name and index; the unfold records stay native.",
        ));
    }
    let mut rules = Vec::new();
    for style in styles {
        ctx.charge_entities(1, "project Inventor v11 sheet-metal rule")?;
        let literal = |reference| {
            PositiveLength::try_from(length(style, reference)?)
                .ok()
                .map(Dimension::Literal)
        };
        let id =
            SheetMetalRuleId::mint(crate::feature::history_feature_id(&style.identity)?.as_str())
                .map_err(CodecError::malformed)?;
        let allowance = match (active.is_some(), factor, style_factor(style)) {
            (_, Some(rule), StyleFactor::Absent) => Some(rule),
            (_, Some(rule), StyleFactor::Value(parameter)) if parameter == rule => Some(rule),
            (_, Some(rule), other) => {
                let parameter = match other {
                    StyleFactor::Value(value) => format!("value {}", value.get()),
                    _ => "an unresolved value".to_owned(),
                };
                losses.push(InventorLossCode::FeatureSheetMetalStyleFactorConflict.note(format!(
                    "Sheet-metal rule {id} has a style KFactor parameter with {parameter} and an active unfold rule with K-factor {}; the rule keeps no bend allowance and both sources stay native.",
                    rule.get()
                )));
                None
            }
            (false, None, StyleFactor::Value(value)) => {
                losses.push(InventorLossCode::FeatureSheetMetalStyleFactorAssumed.note(format!(
                    "Sheet-metal rule {id} takes K-factor {} from its style KFactor parameter because no active unfold rule resolves.",
                    value.get()
                )));
                Some(value)
            }
            (false, None, StyleFactor::Unresolved) => {
                losses.push(InventorLossCode::FeatureSheetMetalStyleFactorConflict.note(format!(
                    "Sheet-metal rule {id} has a style KFactor parameter with an unresolved value and no active unfold rule; the rule keeps no bend allowance."
                )));
                None
            }
            (false, None, StyleFactor::Absent) | (true, None, _) => None,
        };
        rules.push(SheetMetalRule {
            id,
            scope: part.id.clone(),
            name: None,
            thickness: literal(style.thickness),
            bend: BendSpec {
                radius: literal(style.bend_radius),
                radius_side: None,
                relief: None,
            },
            allowance: allowance.map(|value| BendAllowance::KFactor(Dimension::Literal(value))),
            native_options: BTreeMap::default(),
            source_settings: BTreeMap::default(),
            native_ref: Some(style.id()),
        });
    }
    Ok(rules)
}

#[cfg(test)]
mod tests;

pub(super) fn feature_reliefs(
    ir: &mut CadIr,
    inventory: &FeatureInventory,
    parameters: &[crate::design::PmDcParameter],
) -> Result<Vec<LossNote>, CodecError> {
    use crate::feature::sheet_metal::{operands, SheetMetalProperty};
    use cadmpeg_ir::design::feature::FeatureOperation;
    use cadmpeg_ir::design::ops::sheet_metal::SheetMetalKind;
    let mut losses = Vec::new();
    let styles = unique_index(
        inventory
            .sheet_metal_styles
            .iter()
            .map(|style| (style.identity.segment_token.as_str(), style)),
    );
    let sources = unique_index(
        inventory
            .features
            .iter()
            .map(|source| (source.id(), source)),
    );
    let properties = property_index(inventory);
    let raw = unique_index(parameters.iter().map(|parameter| {
        (
            (
                parameter.identity.segment_token.as_str(),
                parameter.identity.record_ordinal,
            ),
            parameter,
        )
    }));
    let values = unique_index(
        ir.model
            .parameters
            .iter()
            .filter_map(|parameter| Some((parameter.native_ref.as_deref()?, parameter))),
    );
    for feature in &mut ir.design.features {
        let FeatureOperation::SheetMetal(operation) = &mut feature.definition.operation else {
            continue;
        };
        let SheetMetalKind::ContourFlange { bend, .. } = &mut operation.kind else {
            continue;
        };
        let Some(source) = feature
            .native_ref
            .as_deref()
            .and_then(|id| sources.get(id))
            .and_then(Option::as_ref)
        else {
            continue;
        };
        let token = source.identity.segment_token.as_str();
        let property = |reference: PmDcReference| {
            properties
                .get(&(token, reference.record_ordinal()?))
                .and_then(Option::as_ref)
                .copied()
        };
        let Some(relief) = source
            .properties
            .references()
            .get(10)
            .and_then(|reference| property(*reference))
            .and_then(operands::sheet)
        else {
            continue;
        };
        let SheetMetalProperty::Relief {
            control,
            width,
            depth,
            form,
            minimum_remnant,
            seam_gap,
            ..
        } = relief
        else {
            continue;
        };
        let local_form = property(*form).and_then(operands::enumeration);
        let style = styles.get(token).and_then(Option::as_ref);
        let admitted = style.is_some_and(|style| {
            style.save_version_major == source.save_version_major()
                && local_form.is_some()
                && property(style.bend_relief_form).and_then(operands::enumeration) == local_form
        }) && property(*control).and_then(operands::boolean).is_some();
        losses.push(
            InventorLossCode::FeatureSheetMetalReliefNative.note(format!(
                "Feature {} retains its relief form and unadmitted relief controls natively.",
                feature.id
            )),
        );
        if !admitted {
            continue;
        }
        let shape = local_form
            .and_then(|(family, value)| {
                cadmpeg_core::text::NonBlankString::new(format!(
                    "inventor:bend_relief:{family}:{value}"
                ))
            })
            .map(ReliefShape::Native);
        let length = |reference: PmDcReference| {
            let parameter = raw
                .get(&(token, reference.record_ordinal()?))
                .and_then(Option::as_ref)?;
            let parameter = values
                .get(parameter.id().as_str())
                .and_then(Option::as_ref)?;
            let Some(ParameterValue::Length(value)) = parameter.value else {
                return None;
            };
            Some((parameter.id.clone(), value))
        };
        let positive = |reference| {
            let (id, value) = length(reference)?;
            PositiveLength::try_from(value).ok()?;
            Some(Dimension::Parameter(id))
        };
        bend.relief = Some(Relief {
            shape,
            width: positive(*width),
            width_ratio: None,
            depth: positive(*depth),
        });
        for (key, reference) in [
            ("inventor:minimum_remnant", minimum_remnant),
            ("inventor:seam_gap", seam_gap),
        ] {
            if let Some((id, _)) = length(*reference) {
                let key = cadmpeg_core::text::NonBlankString::new(key)
                    .ok_or_else(|| CodecError::malformed("Inventor relief key is blank"))?;
                operation.native_options.insert(
                    key,
                    cadmpeg_ir::design::options::NativeOption::Length(Dimension::Parameter(id)),
                );
            }
        }
    }
    Ok(losses)
}
