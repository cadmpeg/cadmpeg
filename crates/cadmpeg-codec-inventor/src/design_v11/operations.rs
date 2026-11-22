// SPDX-License-Identifier: Apache-2.0
//! Typed operations from the admitted Inventor operands.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureDefinition, FeatureOperation, StoredOrigin};
use cadmpeg_ir::design::options::NativeOption;
use cadmpeg_ir::features::FeatureOperation as LegacyOperation;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::loss::InventorLossCode;

mod body;
mod dressup;
mod selections;
mod sheet_metal;
mod swept;

pub(super) fn project(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    if ir.model.product_definitions.is_empty() {
        return Ok(());
    }
    for source in &ir.model.features {
        if !projects(source.evaluation.definition().operation()) {
            continue;
        }
        let index = ir
            .design
            .features
            .binary_search_by(|item| item.id.cmp(&source.id))
            .map_err(|_| {
                CodecError::malformed("Inventor v11 feature is absent from its history")
            })?;
        let target = &mut ir.design.features[index];
        ctx.charge_entities(1, "project Inventor v11 operation")?;
        let (definition, reason) = definition(
            source,
            ir.model.sketches.as_slice(),
            &ir.model.feature_input_topologies,
        )?;
        target.definition = definition;
        if let Some(reason) = reason {
            losses.push(InventorLossCode::FeatureDesignOperationNative.note(format!(
                "Feature {} keeps its operation native: {reason}.",
                source.id
            )));
        }
    }
    Ok(())
}

fn projects(operation: &LegacyOperation) -> bool {
    !matches!(
        operation,
        LegacyOperation::Native { .. }
            | LegacyOperation::Unresolved { .. }
            | LegacyOperation::TreeNode { .. }
            | LegacyOperation::Sketch { .. }
            | LegacyOperation::SpatialSketch { .. }
            | LegacyOperation::SketchBlockDefinition { .. }
            | LegacyOperation::DatumPrincipalPlane { .. }
            | LegacyOperation::DatumPlane { .. }
            | LegacyOperation::DatumThreePointPlane { .. }
            | LegacyOperation::DatumOffsetPlane { .. }
            | LegacyOperation::DatumAxis { .. }
            | LegacyOperation::DatumPoint { .. }
            | LegacyOperation::DatumCoordinateSystem { .. }
    )
}

fn definition(
    source: &cadmpeg_ir::features::Feature,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    inputs: &[cadmpeg_ir::features::evaluation::FeatureInputTopology],
) -> Result<(FeatureDefinition, Option<&'static str>), CodecError> {
    let operation = source.evaluation.definition().operation();
    let (mut operation, reason) = match typed(operation, sketches, inputs) {
        Ok(value) => (value, None),
        Err(reason) => (retained(operation)?, Some(reason)),
    };
    if let FeatureOperation::Stored { bodies, .. } = &mut operation {
        *bodies =
            (!source.evaluation.outputs().is_empty()).then(|| source.evaluation.outputs().clone());
    }
    let mut result = FeatureDefinition {
        operation,
        refine: None,
        fuzzy_tolerance: None,
    };
    if let cadmpeg_ir::features::FeatureDefinition::PostProcess {
        refine,
        fuzzy_tolerance,
        ..
    } = source.evaluation.definition()
    {
        use cadmpeg_ir::design::feature::FuzzyTolerance;
        result.refine = Some(*refine);
        result.fuzzy_tolerance = Some(match fuzzy_tolerance {
            cadmpeg_ir::features::operation_settings::FuzzyTolerance::KernelDefault => {
                FuzzyTolerance::KernelDefault
            }
            cadmpeg_ir::features::operation_settings::FuzzyTolerance::Automatic => {
                FuzzyTolerance::Automatic
            }
            cadmpeg_ir::features::operation_settings::FuzzyTolerance::Explicit(value) => {
                FuzzyTolerance::Explicit(Dimension::Literal(*value))
            }
        });
    }
    Ok((result, reason))
}

#[cfg(test)]
pub(super) fn validate(ir: &CadIr) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
    use cadmpeg_core::decode::unique_index;
    use cadmpeg_ir::report::{
        check::{Check, Finding},
        Severity,
    };
    let mut findings = Vec::new();
    if ir.model.product_definitions.is_empty() {
        return Ok(findings);
    }
    let targets = unique_index(ir.design.features.iter().map(|target| (&target.id, target)));
    for source in &ir.model.features {
        if !projects(source.evaluation.definition().operation()) {
            continue;
        }
        let (expected, _) = definition(
            source,
            &ir.model.sketches,
            &ir.model.feature_input_topologies,
        )?;
        let target = targets.get(&source.id).and_then(|target| *target);
        if target.is_none_or(|target| target.definition != expected) {
            findings.push(Finding {
                check: Check::NativeLinks,
                severity: Severity::Error,
                message: format!(
                    "Inventor v11 operation {} differs from its admitted source operands",
                    source.id
                ),
                entity: Some(source.id.to_string()),
                related: Vec::new(),
            });
        }
    }
    Ok(findings)
}

type Projected<T> = Result<T, &'static str>;

fn typed(
    operation: &LegacyOperation,
    sketches: &[cadmpeg_ir::sketches::Sketch],
    inputs: &[cadmpeg_ir::features::evaluation::FeatureInputTopology],
) -> Projected<FeatureOperation> {
    if let Some(value) = swept::operation(operation, sketches) {
        return value;
    }
    if let Some(value) = dressup::operation(operation, sketches, inputs) {
        return value;
    }
    if let Some(value) = body::operation(operation) {
        return value;
    }
    if let Some(value) = sheet_metal::operation(operation, sketches) {
        return value;
    }
    match operation {
        LegacyOperation::StoredGeometry {} => Ok(FeatureOperation::Stored {
            bodies: None,
            tessellations: Vec::new(),
            origin: StoredOrigin::Base {},
        }),
        _ => Err("the operation has operands outside the v11 operation contract"),
    }
}

fn retained(operation: &LegacyOperation) -> Result<FeatureOperation, CodecError> {
    let value = cadmpeg_ir::hash::finite_json::to_canonical_json_string(operation)
        .map_err(|error| CodecError::malformed(error.to_string()))?;
    Ok(FeatureOperation::Native {
        kind: cadmpeg_core::nonblank_literal!("inventor:operation"),
        native_options: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("inventor:legacy_operation"),
            NativeOption::Text(Dimension::Literal(
                NonBlankString::new(value)
                    .ok_or_else(|| CodecError::malformed("Inventor operation is blank"))?,
            )),
        )]),
        source_settings: BTreeMap::default(),
    })
}

#[cfg(test)]
mod tests;
