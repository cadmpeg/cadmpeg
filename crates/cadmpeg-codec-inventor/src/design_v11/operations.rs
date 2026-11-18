// SPDX-License-Identifier: Apache-2.0
//! Typed operations from the admitted Inventor operands.

use std::collections::BTreeMap;

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::design::dimension::Dimension;
use cadmpeg_ir::design::feature::{FeatureOperation, StoredOrigin};
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
        let operation = source.evaluation.definition().operation();
        if matches!(
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
        ) {
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
        target.definition.operation = match typed(operation, ir.model.sketches.as_slice()) {
            Ok(value) => value,
            Err(reason) => {
                losses.push(InventorLossCode::FeatureDesignOperationNative.note(format!(
                    "Feature {} keeps its operation native: {reason}.",
                    source.id
                )));
                retained(operation)?
            }
        };
        if matches!(operation, LegacyOperation::StoredGeometry {}) {
            target.definition.operation = FeatureOperation::Stored {
                bodies: (!source.evaluation.outputs().is_empty())
                    .then(|| source.evaluation.outputs().clone()),
                tessellations: Vec::new(),
                origin: StoredOrigin::Base {},
            };
        }
        if let cadmpeg_ir::features::FeatureDefinition::PostProcess {
            refine,
            fuzzy_tolerance,
            ..
        } = source.evaluation.definition()
        {
            use cadmpeg_ir::design::feature::FuzzyTolerance;
            target.definition.refine = Some(*refine);
            target.definition.fuzzy_tolerance = Some(match fuzzy_tolerance {
                cadmpeg_ir::features::FuzzyTolerance::KernelDefault => {
                    FuzzyTolerance::KernelDefault
                }
                cadmpeg_ir::features::FuzzyTolerance::Automatic => FuzzyTolerance::Automatic,
                cadmpeg_ir::features::FuzzyTolerance::Explicit(value) => {
                    FuzzyTolerance::Explicit(Dimension::Literal(*value))
                }
            });
        }
    }
    Ok(())
}

type Projected<T> = Result<T, &'static str>;

fn typed(
    operation: &LegacyOperation,
    sketches: &[cadmpeg_ir::sketches::Sketch],
) -> Projected<FeatureOperation> {
    if let Some(value) = swept::operation(operation, sketches) {
        return value;
    }
    if let Some(value) = dressup::operation(operation, sketches) {
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
    let value = serde_json::to_string(operation)
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
