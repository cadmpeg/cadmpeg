// SPDX-License-Identifier: Apache-2.0
//! `DeleteFace` construction admission and native losses.

use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashSet;

use super::{ClassId, FeatureInventory, PmDcFeature};
use crate::loss::InventorLossCode;

pub(super) const CLASS_BYTES: [u8; 16] = [
    0x68, 0x6e, 0x16, 0xd3, 0x33, 0x4f, 0x63, 0x71, 0x61, 0xa6, 0xa1, 0x9b, 0xdd, 0x3d, 0x2b, 0xe8,
];
pub(crate) const CLASS_ID: ClassId =
    ClassId(cadmpeg_ir::native::bytes::NativeBytes::new(CLASS_BYTES));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Marker,
    Slots,
    Controls,
}

impl NativeReason {
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::Marker => "property vector marker differs from type 2",
            Self::Slots => "property vector differs from the four-slot form",
            Self::Controls => "slots 1 and 2 Boolean construction roles are unresolved",
        }
    }

    pub(crate) fn note(self) -> LossNote {
        InventorLossCode::FeatureDeleteFaceOperandsNative.note(format!(
            "Retained Inventor DeleteFace natively: {}.",
            self.detail()
        ))
    }
}

pub(crate) fn admission(source: &PmDcFeature) -> Result<(), NativeReason> {
    if source.properties.marker() != 2 {
        return Err(NativeReason::Marker);
    }
    if source.properties.references().len() != 4 {
        return Err(NativeReason::Slots);
    }
    Err(NativeReason::Controls)
}

/// One loss for each `DeleteFace` record that the projected history retains
/// as a native feature. A record that no model feature places has no loss.
pub(crate) fn native_losses(
    inventory: &FeatureInventory,
    features: &[cadmpeg_ir::features::Feature],
) -> Vec<LossNote> {
    let placed = features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Native { .. }
                )
            )
        })
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let owners = inventory
        .labels
        .iter()
        .filter(|label| label.class_id() == CLASS_ID)
        .filter_map(|label| {
            Some((
                label.identity.segment_token.as_str(),
                label.header.owner.record_ordinal()?,
            ))
        })
        .collect::<HashSet<_>>();
    inventory
        .features
        .iter()
        .filter(|source| {
            owners.contains(&(
                source.identity.segment_token.as_str(),
                source.identity.record_ordinal,
            ))
        })
        .filter(|source| placed.contains(source.id().as_str()))
        .filter_map(|source| admission(source).err())
        .map(NativeReason::note)
        .collect()
}

#[cfg(test)]
mod tests;
