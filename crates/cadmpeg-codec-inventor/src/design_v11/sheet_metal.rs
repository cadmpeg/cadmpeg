// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal style defaults in the part scope.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ids::SheetMetalRuleId;
use cadmpeg_ir::design::ops::sheet_metal::{BendAllowance, BendSpec, SheetMetalRule};
use cadmpeg_ir::features::ParameterValue;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::{FiniteReal, Length, PositiveLength};
use cadmpeg_ir::CadIr;

use crate::feature::sheet_metal::{style::Style, unfold};
use crate::feature::FeatureInventory;
use crate::loss::InventorLossCode;
use crate::pmdc::PmDcReference;

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
        let mut named = parameters.iter().filter(|parameter| {
            parameter.identity.segment_token == style.identity.segment_token
                && parameter.name == STYLE_FACTOR_NAME
        });
        let Some(parameter) = named.next() else {
            return StyleFactor::Absent;
        };
        if named.next().is_some() {
            return StyleFactor::Unresolved;
        }
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
        rule.thickness = binding(style, style.thickness).or_else(|| rule.thickness.take());
        rule.bend.radius = binding(style, style.bend_radius).or_else(|| rule.bend.radius.take());
    }
    ir.design.sheet_metal_rules.extend(projected);
    ir.design.sheet_metal_rules.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// Name of the unitless K-factor parameter in the parameter segment of a
/// sheet-metal style.
const STYLE_FACTOR_NAME: &str = "KFactor";

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
