// SPDX-License-Identifier: Apache-2.0
//! Coil construction refusal and native retention.

use std::collections::HashSet;

use cadmpeg_core::decode::unique_index;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

use super::{
    ClassId, FeatureInventory, PmDcFeature, PmDcFeatureEnumFamily, PmDcFeatureProperty,
    PmDcFeaturePropertyKind, PmDcFeatureReferenceFamily,
};
use crate::loss::InventorLossCode;

pub(crate) const CLASS_BYTES: [u8; 16] = [
    0xda, 0x14, 0x19, 0xb4, 0xd2, 0x11, 0xda, 0xd0, 0x60, 0x00, 0x12, 0xa9, 0x9d, 0xcc, 0xef, 0xb0,
];
pub(crate) const CLASS_ID: ClassId =
    ClassId(cadmpeg_ir::native::bytes::NativeBytes::new(CLASS_BYTES));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReason {
    Marker,
    Slots,
    Operation,
    Profile,
    Axis,
    Polarity,
}

impl NativeReason {
    pub(crate) const fn detail(self) -> &'static str {
        match self {
            Self::Marker => "property vector requires the type-2 form",
            Self::Slots => "property vector requires its operand layout",
            Self::Operation => "slot 0 requires its Coil operation and input/result body joins",
            Self::Profile => "slot 1 requires its Coil profile selection",
            Self::Axis => "slot 2 requires its Coil axis controls and orientation",
            Self::Polarity => "slot 3 Boolean construction role and polarity are unresolved",
        }
    }

    fn note(self) -> LossNote {
        InventorLossCode::FeatureCoilOperandsNative.note(format!(
            "Retained Inventor Coil natively: {}.",
            self.detail()
        ))
    }
}

pub(crate) fn admission<'a>(
    source: &PmDcFeature,
    mut property: impl FnMut(u32) -> Option<&'a PmDcFeatureProperty>,
) -> Result<(), NativeReason> {
    if source.properties.marker() != 2 {
        return Err(NativeReason::Marker);
    }
    if source.properties.references().len() != 20 {
        return Err(NativeReason::Slots);
    }
    let mut slot = |index: usize, expected| {
        let reference = source.properties.references()[index];
        let ordinal = reference.record_ordinal()?;
        let record = property(ordinal)?;
        (reference.qualified()
            && record.identity.segment_token == source.identity.segment_token
            && record.identity.record_ordinal == ordinal
            && record.save_version_major == source.save_version_major
            && record.identity.type_id.as_str() == crate::pmdc::type_id_string(expected))
        .then_some(record)
    };
    if !slot(0, super::PART_OPERATION_TYPE).is_some_and(|record| {
        matches!(
            record.kind,
            PmDcFeaturePropertyKind::Enumeration {
                family: PmDcFeatureEnumFamily::PartOperation,
                type_value: 5,
                value: 1,
            }
        )
    }) {
        return Err(NativeReason::Operation);
    }
    if !slot(1, super::BOUNDARY_PATCH_TYPE).is_some_and(|record| {
        matches!(
            &record.kind,
            PmDcFeaturePropertyKind::References {
                family: PmDcFeatureReferenceFamily::BoundaryPatch, items,
            } if items.marker() == 2 && items.references().len() == 1
                && items.references()[0].qualified() && items.references()[0].index() != 0
        )
    }) {
        return Err(NativeReason::Profile);
    }
    if !slot(2, super::WORK_AXIS_TYPE).is_some_and(|record| {
        matches!(
            &record.kind,
            PmDcFeaturePropertyKind::WorkAxis {
                entity_flags: 0, values, references, marker: false, state: 0 | 1, span, ..
            } if values.len() == if source.save_version_major > 21 { 3 } else { 2 }
                && values.iter().all(|value| *value == 0) && references.marker() == 2
                && references.references().is_empty()
                && span.iter().any(|component| component.get() != 0.0)
        )
    }) {
        return Err(NativeReason::Axis);
    }
    Err(NativeReason::Polarity)
}

pub(crate) fn definition_matches(
    feature: &Feature,
    source: &PmDcFeature,
    has_result: bool,
    labels: &[super::PmDcFeatureLabel],
    properties: &[PmDcFeatureProperty],
) -> bool {
    let unique_label = labels
        .iter()
        .filter(|label| {
            label.identity.segment_token == source.identity.segment_token
                && label.header.owner.record_ordinal() == Some(source.identity.record_ordinal)
        })
        .take(2)
        .count()
        == 1;
    let properties = unique_index(properties.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    admission(source, |ordinal| {
        properties
            .get(&(source.identity.segment_token.as_str(), ordinal))?
            .as_ref()
            .copied()
    })
    .is_err()
        && !has_result
        && feature.evaluation.definition()
            == &FeatureDefinition::Operation(FeatureOperation::Native {
                kind: super::native_kind(unique_label.then_some(CLASS_ID)),
                parameters: std::collections::BTreeMap::new(),
            })
}

pub(crate) fn native_losses(inventory: &FeatureInventory, features: &[Feature]) -> Vec<LossNote> {
    if !inventory
        .labels
        .iter()
        .any(|label| label.class_id() == CLASS_ID)
    {
        return Vec::new();
    }
    let properties = unique_index(inventory.properties.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
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
        .filter_map(|source| {
            admission(source, |ordinal| {
                properties
                    .get(&(source.identity.segment_token.as_str(), ordinal))?
                    .as_ref()
                    .copied()
            })
            .err()
        })
        .map(NativeReason::note)
        .collect()
}

#[cfg(test)]
pub(crate) mod tests;
