// SPDX-License-Identifier: Apache-2.0
//! `DeleteFace` operands, admission, and native losses
//! ([spec §13.7](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/inventor.md#137-deleteface-property-vector)).

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::HashSet;

use super::{
    ClassId, FeatureInventory, PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily,
};
use crate::loss::InventorLossCode;
use crate::pmdc::PmDcReference;

pub(super) const CLASS_BYTES: [u8; 16] = [
    0x68, 0x6e, 0x16, 0xd3, 0x33, 0x4f, 0x63, 0x71, 0x61, 0xa6, 0xa1, 0x9b, 0xdd, 0x3d, 0x2b, 0xe8,
];
pub(crate) const CLASS_ID: ClassId =
    ClassId(cadmpeg_ir::native::bytes::NativeBytes::new(CLASS_BYTES));

/// Record type of a surface-body wrapper.
const WRAPPER_TYPE_ID: &str = "474d8790d011f8d10008cabc0663dc09";
/// Record type of a face-selection item, the member type of a face collection.
const FACE_ITEM_TYPE_ID: &str = "ccb59f50d1114f6b0008a1ba32a3dc09";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Marker,
    Slots,
    Faces,
    Slot1,
    Slot2,
    Selection,
    Controls,
}

impl NativeReason {
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::Marker => "property vector marker differs from type 2",
            Self::Slots => "property vector differs from the four-slot form",
            Self::Faces => "slot 0 is not a face collection of distinct face items",
            Self::Slot1 => "slot 1 is not a Boolean",
            Self::Slot2 => "slot 2 is not a Boolean",
            Self::Selection => "slot 3 is not a body selection of distinct surface-body wrappers",
            Self::Controls => "the slot 1 and 2 Boolean pair has no resolved heal mode",
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

/// The reason that a four-slot `DeleteFace` record stays native. The first
/// slot that differs from its stored form is named.
pub(crate) fn refusal<'a>(
    source: &PmDcFeature,
    property: impl Fn(PmDcReference) -> Option<&'a PmDcFeatureProperty>,
    record_type: impl Fn(u32) -> Option<&'a str>,
) -> NativeReason {
    if let Err(reason @ (NativeReason::Marker | NativeReason::Slots)) = admission(source) {
        return reason;
    }
    let [faces, slot_1, slot_2, selection] = source.properties.references() else {
        return NativeReason::Slots;
    };
    let mut distinct = HashSet::new();
    let faces_valid = matches!(
        property(*faces).map(|property| &property.kind),
        Some(PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::FaceCollection,
            items,
        }) if items
            .references()
            .iter()
            .all(|member| member.index() != 0
                && member.record_ordinal().and_then(&record_type) == Some(FACE_ITEM_TYPE_ID)
                && distinct.insert(member.index()))
    );
    if !faces_valid {
        return NativeReason::Faces;
    }
    let boolean = |reference: &PmDcReference| {
        matches!(
            property(*reference).map(|property| &property.kind),
            Some(PmDcFeaturePropertyKind::Boolean { .. })
        )
    };
    if !boolean(slot_1) {
        return NativeReason::Slot1;
    }
    if !boolean(slot_2) {
        return NativeReason::Slot2;
    }
    // A null member selects no body; each non-null member names a distinct
    // surface-body wrapper (section 13, ordered body selection).
    let mut wrappers = HashSet::new();
    let selection_valid = matches!(
        property(*selection).map(|property| &property.kind),
        Some(PmDcFeaturePropertyKind::References {
            family: PmDcFeatureReferenceFamily::BodySelection,
            items,
        }) if items.references().iter().all(|member| member.index() == 0
            || (member.record_ordinal().and_then(&record_type) == Some(WRAPPER_TYPE_ID)
                && wrappers.insert(member.index())))
    );
    if selection_valid {
        NativeReason::Controls
    } else {
        NativeReason::Selection
    }
}

/// The reason that `source` stays native, read from the properties of `inventory`.
fn inventory_refusal(inventory: &FeatureInventory, source: &PmDcFeature) -> NativeReason {
    let token = source.identity.segment_token.as_str();
    let properties = inventory
        .properties
        .iter()
        .filter(|property| property.identity.segment_token.as_str() == token)
        .map(|property| (property.identity.record_ordinal, property))
        .collect::<std::collections::HashMap<_, _>>();
    let types = inventory.record_types.get(&source.identity.segment_token);
    refusal(
        source,
        |reference| properties.get(&reference.record_ordinal()?).copied(),
        |ordinal| types?.get(&ordinal)?.as_deref(),
    )
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
                FeatureDefinition::Operation(FeatureOperation::Native { .. })
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
        .map(|source| inventory_refusal(inventory, source).note())
        .collect()
}

#[cfg(test)]
mod tests;
