// SPDX-License-Identifier: Apache-2.0
//! Sheet-metal style defaults in the part scope.

use std::collections::BTreeMap;

use cadmpeg_core::decode::{unique_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::ids::SheetMetalRuleId;
use cadmpeg_ir::design::ops::sheet_metal::{BendAllowance, BendSpec, SheetMetalRule};
use cadmpeg_ir::features::ParameterValue;
use cadmpeg_ir::scalar::{FiniteReal, Length, PositiveLength};
use cadmpeg_ir::CadIr;

use crate::feature::sheet_metal::{style::Style, unfold};
use crate::feature::FeatureInventory;
use crate::pmdc::PmDcReference;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    inventory: &FeatureInventory,
    parameters: &[crate::design::PmDcParameter],
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
    let factor = unfold::neutral_factor(
        &inventory.sheet_metal_unfold_rules,
        &inventory.sheet_metal_unfold_selections,
    );
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
    let mut projected = rules(ctx, ir, &inventory.sheet_metal_styles, length, factor)?;
    for (rule, style) in projected.iter_mut().zip(&inventory.sheet_metal_styles) {
        rule.thickness = binding(style, style.thickness).or_else(|| rule.thickness.take());
        rule.bend.radius = binding(style, style.bend_radius).or_else(|| rule.bend.radius.take());
    }
    ir.design.sheet_metal_rules.extend(projected);
    ir.design.sheet_metal_rules.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

fn rules(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    styles: &[Style],
    length: impl Fn(&Style, PmDcReference) -> Option<Length>,
    factor: Option<FiniteReal>,
) -> Result<Vec<SheetMetalRule>, CodecError> {
    let Some(part) = ir.model.product_definitions.first() else {
        return Ok(Vec::new());
    };
    let mut rules = Vec::new();
    for style in styles {
        ctx.charge_entities(1, "project Inventor v11 sheet-metal rule")?;
        let literal = |reference| {
            PositiveLength::try_from(length(style, reference)?)
                .ok()
                .map(Dimension::Literal)
        };
        rules.push(SheetMetalRule {
            id: SheetMetalRuleId::mint(
                crate::feature::history_feature_id(&style.identity)?.as_str(),
            )
            .map_err(CodecError::malformed)?,
            scope: part.id.clone(),
            name: None,
            thickness: literal(style.thickness),
            bend: BendSpec {
                radius: literal(style.bend_radius),
                radius_side: None,
                relief: None,
            },
            allowance: factor.map(|value| BendAllowance::KFactor(Dimension::Literal(value))),
            native_options: BTreeMap::default(),
            source_settings: BTreeMap::default(),
            native_ref: Some(style.id()),
        });
    }
    Ok(rules)
}

#[cfg(test)]
mod tests;
