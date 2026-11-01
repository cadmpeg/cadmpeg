// SPDX-License-Identifier: Apache-2.0

use super::{definition, native_losses, refusal, CLASS_ID};
use crate::feature::{FeatureInventory, PmDcFeature, PmDcFeatureProperty};
use crate::pmdc::{PmDcListMetadata, PmDcReference, PmDcReferenceList};

fn list(marker: u16, refs: &[u32]) -> PmDcReferenceList {
    PmDcReferenceList::new(
        marker,
        (!refs.is_empty()).then_some(PmDcListMetadata::U32([
            u32::try_from(refs.len()).expect("count"),
            0,
        ])),
        refs.iter()
            .copied()
            .map(PmDcReference::from_packed)
            .collect(),
    )
    .expect("list")
}

fn feature() -> PmDcFeature {
    serde_json::from_value(serde_json::json!({
        "id": "inventor:pmdc:feature#generated-0",
        "segment_token": "generated", "record_ordinal": 0,
        "type_id": "914d8790d011f8d10008cabc0663dc09", "save_version_major": 19,
        "header": {"header_value": 0, "header_id": 0, "next": {"index": 0, "qualified": false},
            "flags": 0, "context": {"index": 0, "qualified": false}, "source_index": 1},
        "state": 0, "outline_value": 0, "value": 0,
        "properties": list(2, &[0x8000_0002])
    }))
    .expect("feature")
}

fn properties() -> Vec<PmDcFeatureProperty> {
    [
        (1, "ae70680ed14a1e86d76248b03c2a96e1", serde_json::json!({
            "form": "references", "family": "object_collection", "items": list(2, &[0x8000_0003])
        })),
        (2, "474d8790d011f8d10008cabc0663dc09", serde_json::json!({
            "form": "surface_body", "body": {"index": 1, "qualified": false}
        })),
    ].into_iter().map(|(ordinal, type_id, kind)| {
        serde_json::from_value(serde_json::json!({
            "id": format!("inventor:pmdc:feature-property#generated-{ordinal}"),
            "segment_token": "generated", "record_ordinal": ordinal,
            "type_id": type_id, "save_version_major": 19,
            "header": {"header_value": 0, "header_id": 0, "next": {"index": 0, "qualified": false},
                "flags": 0, "context": {"index": 0, "qualified": false}, "source_index": ordinal + 1},
            "kind": kind
        })).expect("property")
    }).collect()
}

fn reason(feature: &PmDcFeature, properties: &[PmDcFeatureProperty]) -> &'static str {
    refusal(feature, |ordinal| {
        properties
            .iter()
            .find(|p| p.identity.record_ordinal == ordinal)
    })
}

fn inventory(derived: bool) -> FeatureInventory {
    let mut inv = FeatureInventory {
        features: vec![feature()],
        properties: properties(),
        ..FeatureInventory::default()
    };
    let label = |ordinal: u32, class: super::ClassId| {
        serde_json::from_value(serde_json::json!({
        "id": format!("inventor:pmdc:feature-label#generated-{ordinal}"),
        "segment_token": "generated", "record_ordinal": ordinal,
        "type_id": "2ba4482bd2115864600074b79b49ebb0", "save_version_major": 19,
        "header": {"header_value": 0, "header_id": 0, "values": [0,0],
            "owner": {"index": if class == CLASS_ID { 1 } else { 6 }, "qualified": true}, "parent": {"index": 0, "qualified": false},
            "next": {"index": 0, "qualified": false}},
        "index": 1, "name": "", "class_id": class,
        "participants": list(2, &[])
    })).expect("label")
    };
    inv.labels.push(label(3, CLASS_ID));
    if derived {
        inv.labels.push(label(4, crate::feature::derived::CLASS_ID));
    }
    inv
}

#[test]
fn tagging_body_selection_keeps_native_definition_and_pinned_loss() {
    assert_eq!(
        reason(&feature(), &properties()),
        "slot 0 body selection requires its input/result revision and topology-naming effect"
    );
    for derived in [false, true] {
        let notes = native_losses(&inventory(derived));
        let code = if derived {
            "feature.derived-tagging-native"
        } else {
            "feature.tagging-native"
        };
        assert_eq!(notes.len(), 1);
        let expected = if derived {
            crate::loss::InventorLossCode::FeatureDerivedTaggingNative.note("selection effect")
        } else {
            crate::loss::InventorLossCode::FeatureTaggingNative.note("selection effect")
        };
        assert_eq!(notes[0].code, expected.code);
        assert_eq!(notes[0].code.namespace(), "inventor");
        assert_eq!(notes[0].code.local_code(), code);
        assert_eq!(notes[0].message, "1 Tagging feature(s) remain native: slot 0 body selection requires its input/result revision and topology-naming effect.");
    }
    assert!(
        matches!(definition(true), cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Native {kind: cadmpeg_ir::features::NativeFeatureKind::Other(ref kind), ref parameters}
    ) if kind == "Tagging" && parameters.is_empty())
    );
}

fn changed_feature(version: u8, marker: u16, references: &[u32]) -> PmDcFeature {
    let original = feature();
    let mut payload = (*original).clone();
    payload.save_version_major = version;
    payload.properties = list(marker, references);
    crate::record_identity::Located::new(
        payload,
        original.identity.type_id,
        original.identity.segment_token,
        original.identity.record_ordinal,
    )
}

#[test]
fn tagging_reports_first_refused_feature_check() {
    assert_eq!(
        reason(&changed_feature(19, 3, &[]), &[]),
        "property-vector marker requires the type-2 form"
    );
    for refs in [&[][..], &[0x8000_0002, 0x8000_0003][..]] {
        assert_eq!(
            reason(&changed_feature(19, 2, refs), &[]),
            "property vector requires one slot"
        );
    }
    for reference in [0, 0x8000_0000, 2] {
        assert_eq!(
            reason(&changed_feature(19, 2, &[reference]), &[]),
            "slot 0 requires a qualified non-null collection reference"
        );
    }
    assert_eq!(
        reason(&feature(), &[]),
        "slot 0 requires one typed collection record"
    );
}

fn sync_property_id(wire: &mut serde_json::Value) {
    wire["id"] = format!(
        "inventor:pmdc:feature-property#{}-{}",
        wire["segment_token"].as_str().expect("token"),
        wire["record_ordinal"].as_u64().expect("ordinal")
    )
    .into();
}

#[test]
fn tagging_reports_each_collection_and_member_refusal() {
    let f = feature();
    let p = properties();
    for field in [
        "segment_token",
        "record_ordinal",
        "save_version_major",
        "type_id",
    ] {
        let mut wire = serde_json::to_value(&p[0]).expect("wire");
        wire[field] = match field {
            "record_ordinal" => 4.into(),
            "save_version_major" => 20.into(),
            "type_id" => "474d8790d011f8d10008cabc0663dc09".into(),
            _ => "other".into(),
        };
        sync_property_id(&mut wire);
        let changed = serde_json::from_value(wire).expect("changed collection");
        assert_eq!(
            refusal(&f, |_| Some(&changed)),
            "slot 0 requires the same-segment object-collection type and version"
        );
    }
    let replace = |kind| {
        let mut wire = serde_json::to_value(&p[0]).expect("wire");
        wire["kind"] = kind;
        serde_json::from_value::<PmDcFeatureProperty>(wire).expect("collection")
    };
    let changed = replace(
        serde_json::json!({"form": "surface_body", "body": {"index": 1, "qualified": false}}),
    );
    assert_eq!(
        refusal(&f, |_| Some(&changed)),
        "slot 0 requires the object-collection property form"
    );
    for (marker, refs, expected) in [
        (
            3,
            vec![0x8000_0003],
            "slot 0 collection requires the type-2 member list",
        ),
        (
            2,
            vec![],
            "slot 0 collection requires one surface-body member",
        ),
        (
            2,
            vec![0x8000_0003, 0x8000_0004],
            "slot 0 collection requires one surface-body member",
        ),
        (
            2,
            vec![0],
            "slot 0 member requires a qualified non-null body-wrapper reference",
        ),
        (
            2,
            vec![3],
            "slot 0 member requires a qualified non-null body-wrapper reference",
        ),
    ] {
        let changed = replace(
            serde_json::json!({"form": "references", "family": "object_collection", "items": list(marker, &refs)}),
        );
        assert_eq!(refusal(&f, |_| Some(&changed)), expected);
    }
    assert_eq!(
        refusal(&f, |ordinal| (ordinal == 1).then_some(&p[0])),
        "slot 0 member requires one typed surface-body wrapper"
    );
    assert_eq!(
        refusal(&f, |_| Some(&p[0])),
        "slot 0 member requires the same-segment surface-body type, form, and version"
    );
}

#[test]
fn tagging_unresolved_and_duplicate_owners_or_properties_emit_losses() {
    for variant in 0..4 {
        let mut inv = inventory(false);
        match variant {
            0 => inv.features.clear(),
            1 => inv.features.push(feature()),
            2 => inv.properties.clear(),
            _ => inv.properties.push(properties().remove(0)),
        }
        let notes = native_losses(&inv);
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].code.local_code(), "feature.tagging-native");
        assert!(notes[0].message.contains(if variant < 2 {
            "label owner requires one typed feature record"
        } else {
            "slot 0 requires one typed collection record"
        }));
    }
}

#[test]
fn tagging_loss_scan_is_empty_without_a_tagging_label() {
    assert!(native_losses(&FeatureInventory::default()).is_empty());
}

#[test]
fn tagging_reports_each_body_wrapper_identity_type_and_form_refusal() {
    let f = feature();
    let p = properties();
    for field in [
        "segment_token",
        "record_ordinal",
        "save_version_major",
        "type_id",
        "kind",
    ] {
        let mut wire = serde_json::to_value(&p[1]).expect("wire");
        wire[field] = match field {
            "record_ordinal" => 4.into(),
            "save_version_major" => 20.into(),
            "type_id" => "ae70680ed14a1e86d76248b03c2a96e1".into(),
            "kind" => {
                serde_json::json!({"form": "references", "family": "object_collection", "items": list(2, &[])})
            }
            _ => "other".into(),
        };
        sync_property_id(&mut wire);
        let changed = serde_json::from_value(wire).expect("changed body");
        assert_eq!(
            refusal(&f, |ordinal| Some(if ordinal == 1 {
                &p[0]
            } else {
                &changed
            })),
            "slot 0 member requires the same-segment surface-body type, form, and version"
        );
    }
}

#[test]
fn tagging_ambiguous_labels_report_the_first_refusal() {
    let mut inv = inventory(false);
    inv.labels.push(inv.labels[0].clone());
    assert!(!super::unique_owner(&inv.labels, &inv.features[0]));
    let notes = native_losses(&inv);
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].code.local_code(), "feature.tagging-native");
    assert_eq!(
        notes[0].message,
        "1 Tagging feature(s) remain native: label owner requires one feature label."
    );
}
