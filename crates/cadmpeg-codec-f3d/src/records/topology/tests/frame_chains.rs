// SPDX-License-Identifier: Apache-2.0

use crate::records::topology::{
    DesignBodyRecipeOperand, DesignConstructionOperandTransform,
    DesignConstructionPersistentIdentity, DesignConstructionTrackingPath,
    DesignEntitySelectionOperand,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::{json, Value};

fn rejects_changed_fields<T: DeserializeOwned + Serialize>(wire: Value, fields: &[&str]) {
    let admitted: T = serde_json::from_value(wire).unwrap();
    let wire = serde_json::to_value(admitted).unwrap();
    for field in fields {
        let mut invalid = wire.clone();
        invalid[field] = (wire[field].as_u64().unwrap() + 1).into();
        assert!(serde_json::from_value::<T>(invalid).is_err(), "{field}");
    }
}

#[test]
fn trailing_transform_rejects_displaced_frames_and_overflow() {
    let wire = json!({
        "record_index": 7, "byte_offset": 100, "class_tag": "300",
        "transform": crate::records::IDENTITY_MATRIX, "transform_offset": 122,
        "following_record_index": 8, "following_byte_offset": 252,
        "following_class_tag": "301"
    });
    rejects_changed_fields::<DesignConstructionOperandTransform>(
        wire.clone(),
        &[
            "record_index",
            "byte_offset",
            "transform_offset",
            "following_record_index",
            "following_byte_offset",
        ],
    );
    let mut boundary = wire;
    boundary["record_index"] = (u32::MAX - 1).into();
    boundary["following_record_index"] = u32::MAX.into();
    boundary["byte_offset"] = (u64::MAX - 152).into();
    boundary["transform_offset"] = (u64::MAX - 130).into();
    boundary["following_byte_offset"] = u64::MAX.into();
    let admitted: DesignConstructionOperandTransform =
        serde_json::from_value(boundary.clone()).unwrap();
    let mut draft = admitted.into_draft();
    draft.record_index = u32::MAX;
    assert!(DesignConstructionOperandTransform::try_new(draft).is_err());
    boundary["byte_offset"] = (u64::MAX - 151).into();
    assert!(serde_json::from_value::<DesignConstructionOperandTransform>(boundary).is_err());
}

#[test]
fn tracking_optional_identities_select_the_frame_extent() {
    for first in [false, true] {
        for second in [false, true] {
            let mut wire = json!({
                "wrapper_record_index": 7, "wrapper_byte_offset": 100, "wrapper_class_tag": "300",
                "carrier_record_index": 8, "carrier_byte_offset": 133, "carrier_class_tag": "301",
                "primary_identity": 9, "primary_identity_offset": 170,
                "selector": -1, "selector_offset": 190, "kind": 3, "kind_offset": 194,
                "following_record_index": 9, "following_byte_offset": 214 + u64::from(first) * 8 + u64::from(second) * 8,
                "following_class_tag": "302"
            });
            if first {
                wire["first_related_identity"] = 0.into();
                wire["first_related_identity_offset"] = 210.into();
            }
            if second {
                wire["second_related_identity"] = 0.into();
                wire["second_related_identity_offset"] = (214 + u64::from(first) * 8).into();
            }
            rejects_changed_fields::<DesignConstructionTrackingPath>(
                wire,
                &[
                    "carrier_record_index",
                    "carrier_byte_offset",
                    "primary_identity_offset",
                    "selector_offset",
                    "kind_offset",
                    "following_record_index",
                    "following_byte_offset",
                ],
            );
        }
    }
}

#[test]
fn persistent_identity_admits_both_tail_extents_and_rejects_displaced_offsets() {
    for (tail, next) in [(0, 190), (185, 200)] {
        let wire = json!({
            "local_id": 1, "local_id_offset": 21,
            "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 33,
            "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 109,
            "tail_slot_present": false, "tail_slot_offset": tail,
            "next_record_index": 0, "next_byte_offset": next
        });
        rejects_changed_fields::<DesignConstructionPersistentIdentity>(
            wire,
            &["local_id_offset", "asset_id_offset", "next_byte_offset"],
        );
    }
}

#[test]
fn body_recipe_reference_count_fixes_nested_frame_offsets() {
    let wire = json!({
        "id": "operand", "scope_record_index": 1, "scope_reference_ordinal": 0,
        "record_index": 2, "byte_offset": 0, "class_tag": "365",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 56,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 150,
        "references": [{"design_reference": 1, "design_reference_offset": 25, "form": 0, "form_offset": 33}],
        "nested_record_index": 5, "nested_record_index_offset": 38, "recipe_id": "recipe",
        "next_record_index": 6, "next_byte_offset": 240
    });
    rejects_changed_fields::<DesignBodyRecipeOperand>(
        wire.clone(),
        &[
            "nested_record_index",
            "nested_record_index_offset",
            "asset_id_offset",
            "next_record_index",
        ],
    );
    let mut admitted = serde_json::from_value::<DesignBodyRecipeOperand>(wire.clone())
        .unwrap()
        .into_draft();
    admitted.references.clear();
    assert!(DesignBodyRecipeOperand::try_new(admitted).is_err());
    for field in ["design_reference_offset", "form_offset"] {
        let mut invalid = wire.clone();
        invalid["references"][0][field] = 0.into();
        assert!(serde_json::from_value::<DesignBodyRecipeOperand>(invalid).is_err());
    }
}

#[test]
fn entity_selection_retains_primary_paired_and_class_338_forms() {
    let base = json!({
        "id": "operand", "scope_record_index": 1, "group_record_index": 2, "group_member_ordinal": 0,
        "record_index": 7, "byte_offset": 100, "class_tag": "338",
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 130,
        "context_id": "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e", "context_id_offset": 210,
        "identity_record_index": 10, "identity_record_offset": 300,
        "primary_identity": 1, "primary_identity_offset": 321,
        "next_record_index": 99, "next_byte_offset": 329
    });
    for (primary, secondary, next) in [
        (321, None, 329),
        (329, Some(337), 345),
        (333, Some(341), 349),
    ] {
        let mut wire = base.clone();
        wire["primary_identity_offset"] = primary.into();
        wire["next_byte_offset"] = next.into();
        if let Some(offset) = secondary {
            wire["secondary_identity"] = 2.into();
            wire["secondary_identity_offset"] = offset.into();
            wire["next_record_index"] = 11.into();
        }
        rejects_changed_fields::<DesignEntitySelectionOperand>(
            wire,
            &[
                "identity_record_index",
                "primary_identity_offset",
                "next_byte_offset",
            ],
        );
    }
}

#[test]
fn sketch_profile_offsets_must_follow_their_predecessors() {
    use crate::records::topology::DesignSketchProfileOperand;
    let wire = json!({
        "scope_reference_ordinal": 0, "record_index": 7, "byte_offset": 0,
        "class_tag": "300", "paired_class_tag": "301", "paired_byte_offset": 160,
        "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d", "asset_id_offset": 32,
        "entity_id": "Sketch_1", "entity_suffix": 1, "entity_reference_offset": 80
    });
    let admitted: DesignSketchProfileOperand = serde_json::from_value(wire.clone()).unwrap();
    let mut draft = admitted.into_draft();
    draft.byte_offset = draft.asset_id_offset;
    assert!(DesignSketchProfileOperand::try_new(draft).is_err());
    for (field, preceding) in [
        ("asset_id_offset", 0),
        ("entity_reference_offset", 32),
        ("paired_byte_offset", 80),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = preceding.into();
        assert!(serde_json::from_value::<DesignSketchProfileOperand>(invalid).is_err());
    }
}
