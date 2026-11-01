// SPDX-License-Identifier: Apache-2.0
//! Native Tagging selection diagnostics.

use std::collections::{BTreeMap, HashSet};

use cadmpeg_core::decode::unique_index;
use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::report::loss::LossNote;

use super::{
    ClassId, FeatureInventory, PmDcFeature, PmDcFeatureProperty, PmDcFeaturePropertyKind,
    PmDcFeatureReferenceFamily, OBJECT_COLLECTION_TYPE, SURFACE_BODY_TYPE,
};
use crate::loss::InventorLossCode;
use crate::pmdc::{type_id_string, PmDcReference};

pub(crate) const CLASS_ID: ClassId = ClassId(cadmpeg_ir::native::bytes::NativeBytes::new([
    0xe8, 0x99, 0xdb, 0xdf, 0xd3, 0x11, 0x65, 0xaf, 0xc0, 0x00, 0x01, 0x85, 0xe8, 0x9c, 0x6b, 0x4f,
]));

pub(crate) fn definition(unique_label: bool) -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Native {
        kind: super::native_kind(unique_label.then_some(CLASS_ID)),
        parameters: BTreeMap::new(),
    })
}

pub(crate) fn unique_owner(labels: &[super::PmDcFeatureLabel], feature: &PmDcFeature) -> bool {
    labels
        .iter()
        .filter(|label| {
            label.identity.segment_token == feature.identity.segment_token
                && label.header.owner.record_ordinal() == Some(feature.identity.record_ordinal)
        })
        .take(2)
        .count()
        == 1
}

fn local(reference: PmDcReference) -> bool {
    reference.qualified() && reference.index() != 0
}

fn refusal<'a>(
    feature: &PmDcFeature,
    mut property: impl FnMut(u32) -> Option<&'a PmDcFeatureProperty>,
) -> &'static str {
    if feature.properties.marker() != 2 {
        return "property-vector marker requires the type-2 form";
    }
    let [reference] = feature.properties.references() else {
        return "property vector requires one slot";
    };
    if !local(*reference) {
        return "slot 0 requires a qualified non-null collection reference";
    }
    let collection_ordinal = reference.index() - 1;
    let Some(collection) = property(collection_ordinal) else {
        return "slot 0 requires one typed collection record";
    };
    if collection.identity.segment_token != feature.identity.segment_token
        || collection.identity.record_ordinal != collection_ordinal
        || collection.save_version_major != feature.save_version_major
        || collection.identity.type_id.as_str() != type_id_string(OBJECT_COLLECTION_TYPE)
    {
        return "slot 0 requires the same-segment object-collection type and version";
    }
    let PmDcFeaturePropertyKind::References {
        family: PmDcFeatureReferenceFamily::ObjectCollection,
        items,
    } = &collection.kind
    else {
        return "slot 0 requires the object-collection property form";
    };
    if items.marker() != 2 {
        return "slot 0 collection requires the type-2 member list";
    }
    let [member] = items.references() else {
        return "slot 0 collection requires one surface-body member";
    };
    if !local(*member) {
        return "slot 0 member requires a qualified non-null body-wrapper reference";
    }
    let body_ordinal = member.index() - 1;
    let Some(body) = property(body_ordinal) else {
        return "slot 0 member requires one typed surface-body wrapper";
    };
    if body.identity.segment_token != feature.identity.segment_token
        || body.identity.record_ordinal != body_ordinal
        || body.save_version_major != feature.save_version_major
        || body.identity.type_id.as_str() != type_id_string(SURFACE_BODY_TYPE)
        || !matches!(body.kind, PmDcFeaturePropertyKind::SurfaceBody { .. })
    {
        return "slot 0 member requires the same-segment surface-body type, form, and version";
    }
    "slot 0 body selection requires its input/result revision and topology-naming effect"
}

pub(crate) fn native_losses(inventory: &FeatureInventory) -> Vec<LossNote> {
    if !inventory
        .labels
        .iter()
        .any(|label| label.class_id() == CLASS_ID)
    {
        return Vec::new();
    }
    let features = unique_index(inventory.features.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let properties = unique_index(inventory.properties.iter().map(|record| {
        (
            (
                record.identity.segment_token.as_str(),
                record.identity.record_ordinal,
            ),
            record,
        )
    }));
    let mut counts = BTreeMap::new();
    let mut owners = HashSet::new();
    for label in inventory
        .labels
        .iter()
        .filter(|label| label.class_id() == CLASS_ID)
    {
        let token = label.identity.segment_token.as_str();
        let owner = label
            .header
            .owner
            .record_ordinal()
            .ok_or(label.identity.record_ordinal);
        if !owners.insert((token, owner)) {
            continue;
        }
        let reason = label
            .header
            .owner
            .record_ordinal()
            .and_then(|ordinal| features.get(&(token, ordinal)))
            .and_then(Option::as_ref)
            .map_or("label owner requires one typed feature record", |feature| {
                if unique_owner(&inventory.labels, feature) {
                    refusal(feature, |ordinal| {
                        properties.get(&(token, ordinal))?.as_ref().copied()
                    })
                } else {
                    "label owner requires one feature label"
                }
            });
        let derived = inventory.labels.iter().any(|other| {
            other.identity.segment_token == label.identity.segment_token
                && other.class_id() == super::derived::CLASS_ID
        });
        *counts.entry((derived, reason)).or_insert(0_usize) += 1;
    }
    counts
        .into_iter()
        .map(|((derived, reason), count)| {
            let code = if derived {
                InventorLossCode::FeatureDerivedTaggingNative
            } else {
                InventorLossCode::FeatureTaggingNative
            };
            code.note(format!(
                "{count} Tagging feature(s) remain native: {reason}."
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests;
