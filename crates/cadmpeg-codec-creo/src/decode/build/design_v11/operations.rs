// SPDX-License-Identifier: Apache-2.0
//! Typed operations from the operands the v10 projection reads.
//!
//! An operation is typed where the v10 operation carries operands that map
//! directly onto its §6 arm; every operand the v10 projection does not read
//! stays unstated. An operation of which no operand is read stays
//! `Unresolved` under its family.

use std::collections::BTreeMap;

use cadmpeg_ir::design::construction::FrameValue;
use cadmpeg_ir::design::feature::FeatureOperation;
use cadmpeg_ir::features::{FeatureId, FeatureOperation as LegacyOperation};
use cadmpeg_ir::sketches::SketchId;

use super::names::Names;

mod datums;
mod dressup;
mod selections;
mod surfacing;
mod swept;

/// What a typed operation reads besides its own operands: the v11 sketches
/// the projection states, with their resolved frames, and the names its
/// picks take.
pub(super) struct Stated<'a> {
    pub(super) sketches: BTreeMap<&'a SketchId, Option<&'a FrameValue>>,
    pub(super) names: &'a Names,
}

impl<'a> Stated<'a> {
    pub(super) fn new(
        sketches: &'a [cadmpeg_ir::design::ops::sketch::Sketch],
        names: &'a Names,
    ) -> Self {
        use cadmpeg_ir::design::ops::sketch::SketchPlane;
        Self {
            names,
            sketches: sketches
                .iter()
                .map(|sketch| {
                    let frame = match &sketch.plane {
                        SketchPlane::Placed(placement) => placement.resolved.as_ref(),
                        SketchPlane::Block => None,
                    };
                    (&sketch.id, frame)
                })
                .collect(),
        }
    }

    fn frame(&self, sketch: &SketchId) -> Option<&FrameValue> {
        self.sketches.get(sketch).copied().flatten()
    }
}

/// The typed operation of feature `holder`'s v10 operation, where its
/// operands map onto one.
pub(super) fn typed(
    operation: &LegacyOperation,
    holder: &FeatureId,
    stated: &Stated<'_>,
) -> Option<FeatureOperation> {
    match operation {
        LegacyOperation::Extrude { .. } => swept::extrude(operation, stated),
        LegacyOperation::Revolve { .. } => swept::revolve(operation, stated),
        LegacyOperation::DatumPlane { .. } | LegacyOperation::DatumCoordinateSystem { .. } => {
            datums::datum(operation)
        }
        LegacyOperation::Fillet { .. }
        | LegacyOperation::Chamfer { .. }
        | LegacyOperation::Hole { .. } => dressup::operation(operation, holder, stated),
        LegacyOperation::FilledSurface { .. } => surfacing::operation(operation, stated),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
