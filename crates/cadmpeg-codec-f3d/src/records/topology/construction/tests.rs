// SPDX-License-Identifier: Apache-2.0
//! Construction operand groups, frames, paths and identities on the wire.

use super::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandGroupFrameWire, DesignConstructionOperandTransform,
    DesignConstructionPersistentIdentity, DesignConstructionTrackingPath,
};
use crate::records::identity::Located;
use cadmpeg_test_support::refusal::{refusal, states_the_key};
use serde_json::json;
fn frame_wire() -> DesignConstructionOperandGroupFrameWire {
    let matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    serde_json::from_value(json!({
        "opaque_index": 1,
        "opaque_scalar": 0.0, "variant": false,
        "trailing_record_indices": [300],
        "trailing_flags": [{"record_index":300,"class_tag":"277","value":false}],
        "trailing_transforms": [{"record_index":300,"class_tag":"277","transform":matrix,"following_record_index":301,"following_class_tag":"278"}],
        "trailing_dual_transforms": [{"record_index":300,"class_tag":"277","first_transform":matrix,"second_transform":matrix}],
        "auxiliary_paths": [{"record_index":400,"class_tag":"277","entity_ref":1,"compact_variant":false,"scope_record_index":7,"nested_record_index":402,"following_record_index":401,"following_class_tag":"278"}]
    })).unwrap()
}

#[test]
fn construction_transform_borrowed_wire_matches_owned_wire_bytes() {
    let transform = frame_wire().trailing_transforms.into_iter().next().unwrap();
    let owned = super::DesignConstructionOperandTransformContent::from(transform.clone());
    assert_eq!(
        serde_json::to_vec(&transform).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn construction_transform_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignConstructionOperandTransform,
    }
    let transform = frame_wire().trailing_transforms.into_iter().next().unwrap();
    let record = NestedRecord {
        id: "f3d:native:construction-transform#0",
        value: &transform,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::CONSTRUCTION_TRANSFORM_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_TRANSFORM_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn construction_frame_rejects_invalid_scalars_offsets_and_trailing_arity() {
    for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut wire = frame_wire();
        wire.opaque_scalar = value;
        assert!(DesignConstructionOperandGroupFrame::try_from(wire).is_err());
    }
    for (field, value) in [
        ("opaque_index", json!(0)),
        ("opaque_scalar", json!(-1.0)),
        ("opaque_index_offset", json!(17)),
        ("opaque_index_offset", json!(u64::MAX)),
        ("opaque_scalar_offset", json!(123)),
    ] {
        let mut wire = serde_json::to_value(frame_wire()).unwrap();
        wire[field] = value;
        assert!(serde_json::from_value::<DesignConstructionOperandGroupFrame>(wire).is_err());
    }
    let mut wire = frame_wire();
    wire.trailing_record_indices.push(301);
    assert!(DesignConstructionOperandGroupFrame::try_from(wire.clone()).is_err());
    assert!(
        serde_json::from_value::<DesignConstructionOperandGroupFrame>(
            serde_json::to_value(wire).unwrap()
        )
        .is_err()
    );
}

#[test]
fn construction_frame_collections_reject_duplicates_and_keep_failed_edits_atomic() {
    let frame = DesignConstructionOperandGroupFrame::try_from(frame_wire()).unwrap();
    macro_rules! check_collection {
        ($field:ident, $setter:ident) => {{
            let mut wire = frame_wire();
            wire.$field.push(wire.$field[0].clone());
            assert!(DesignConstructionOperandGroupFrame::try_from(wire.clone()).is_err());
            assert!(
                serde_json::from_value::<DesignConstructionOperandGroupFrame>(
                    serde_json::to_value(&wire).unwrap()
                )
                .is_err()
            );
            let mut edited = frame.clone();
            assert!(edited.$setter(wire.$field).is_err());
            assert_eq!(edited, frame);
        }};
    }
    check_collection!(trailing_flags, try_set_trailing_flags);
    check_collection!(trailing_transforms, try_set_trailing_transforms);
    check_collection!(trailing_dual_transforms, try_set_trailing_dual_transforms);
    check_collection!(auxiliary_paths, try_set_auxiliary_paths);
    let wire = serde_json::to_value(frame_wire()).unwrap();
    assert_eq!(serde_json::to_value(frame).unwrap(), wire);
}

#[test]
fn construction_frame_borrowed_wire_matches_owned_wire_bytes() {
    for wire in [
        serde_json::json!({
            "opaque_index": 1, "opaque_scalar": 0.0, "variant": false
        }),
        serde_json::to_value(frame_wire()).unwrap(),
    ] {
        let frame: DesignConstructionOperandGroupFrame = serde_json::from_value(wire).unwrap();
        let owned = DesignConstructionOperandGroupFrameWire::from(frame.clone());
        assert_eq!(
            serde_json::to_vec(&frame).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_frame_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignConstructionOperandGroupFrame,
    }
    let frame = DesignConstructionOperandGroupFrame::try_from(frame_wire()).unwrap();
    let record = NestedRecord {
        id: "f3d:native:construction-frame#0",
        value: &frame,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_construction_operand_groups",
        || super::CONSTRUCTION_FRAME_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_FRAME_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn construction_group_borrowed_wire_matches_owned_wire_bytes() {
    let wire = json!({
        "id":"f3d:native:construction-group#0", "scope_record_index":7,
        "scope_reference_ordinal":0, "record_index":9,
        "class_tag":"277", "members":[10,11],
        "lost_edge_references":["edge#1"], "frame":frame_wire(),
        "role":0,"paired_class_tag":"278"
    });
    let mut group: DesignConstructionOperandGroup = serde_json::from_value(wire).unwrap();
    for role in [
        super::DesignConstructionOperandRole::Other(super::DesignOperandRole::from_raw(0)),
        super::DesignConstructionOperandRole::ExtrudeBodiesA,
        super::DesignConstructionOperandRole::ExtrudeProfile,
        super::DesignConstructionOperandRole::ExtrudeFaces {
            encoding: super::DesignExtrudeFaceEncoding::SelectedStart,
            usage: super::DesignExtrudeFaceRole::Start,
        },
    ] {
        group.operand_role = role;
        let owned = super::DesignConstructionOperandGroupSerde::from(group.clone());
        assert_eq!(
            serde_json::to_vec(&group).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_group_native_retained_limit_refuses_before_clone() {
    let wire = json!({
        "id":"f3d:native:construction-group#0", "scope_record_index":7,
        "scope_reference_ordinal":0, "record_index":9,
        "class_tag":"277", "members":[10,11],
        "frame":frame_wire(), "role":0,
        "paired_class_tag":"278"
    });
    let group: DesignConstructionOperandGroup = serde_json::from_value(wire).unwrap();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &group,
        "design_construction_operand_groups",
        || super::CONSTRUCTION_GROUP_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_GROUP_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn construction_group_owns_actual_member_stride_and_derives_source_role_offset() {
    let frame = super::DesignConstructionOperandGroupFrame::try_from(
        super::DesignConstructionOperandGroupFrameDraft {
            member_count_offset: 21,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 118,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 122,
            variant: false,
        },
    )
    .unwrap();
    let make = |offsets: [u64; 2], role_offset| {
        super::DesignConstructionOperandGroup::try_from(
            super::DesignConstructionOperandGroupDraft {
                id: "f3d:native:construction-group#0".into(),
                scope_record_index: 7,
                scope_reference_ordinal: 0,
                record_index: 9,
                byte_offset: 0,
                class_tag: "277".to_owned().try_into().unwrap(),
                members: [10, 11]
                    .into_iter()
                    .zip(offsets)
                    .map(|(value, offset)| Located { value, offset })
                    .collect(),
                lost_edge_references: Vec::new(),
                frame: frame.clone(),
                operand_role: super::DesignConstructionOperandRole::Other(
                    super::DesignOperandRole::from_raw(0),
                ),
                role_offset,
                paired_class_tag: "278".to_owned().try_into().unwrap(),
                paired_byte_offset: 200,
            },
        )
    };
    let mut group = make([26, 37], 100).unwrap();
    assert_eq!(group.role_offset(), Some(100));
    for offsets in [[26, 36], [u64::MAX, u64::MAX]] {
        assert!(make(offsets, 100).is_err());
    }
    assert!(make([26, 37], 101).is_err());
    let before = group.clone();
    assert!(group
        .try_set_members(vec![
            Located {
                value: 10,
                offset: 26
            },
            Located {
                value: 11,
                offset: 36
            }
        ])
        .is_err());
    assert_eq!(group, before);
    let wire = serde_json::to_value(&group).unwrap();
    let document: DesignConstructionOperandGroup = serde_json::from_value(wire.clone()).unwrap();
    assert!(document.byte_offset.is_none());
    assert!(document.role_offset().is_none());
    assert_eq!(serde_json::to_value(document).unwrap(), wire);
}

fn tracking_source(first: bool, second: bool) -> super::DesignConstructionTrackingPathDraft {
    super::DesignConstructionTrackingPathDraft {
        wrapper_record_index: 300,
        wrapper_byte_offset: 0,
        wrapper_class_tag: "361".to_owned().try_into().unwrap(),
        carrier_record_index: 301,
        carrier_byte_offset: 33,
        carrier_class_tag: "362".to_owned().try_into().unwrap(),
        primary_identity: 268,
        primary_identity_offset: 70,
        selector: 1,
        selector_offset: 90,
        kind: 3,
        kind_offset: 94,
        first_related_identity: first.then_some(Located {
            value: 0,
            offset: 110,
        }),
        second_related_identity: second.then_some(Located {
            value: 0,
            offset: 114 + u64::from(first) * 8,
        }),
        following_record_index: 302,
        following_byte_offset: 114 + u64::from(first) * 8 + u64::from(second) * 8,
        following_class_tag: "363".to_owned().try_into().unwrap(),
    }
}

#[test]
fn tracking_identities_preserve_optional_zero_values_without_source_locations() {
    for first in [false, true] {
        for second in [false, true] {
            let source =
                DesignConstructionTrackingPath::try_new(tracking_source(first, second)).unwrap();
            let wire = serde_json::to_value(source).unwrap();
            let document: DesignConstructionTrackingPath =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(
                document.first_related_identity().map(|value| value.value),
                first.then_some(0)
            );
            assert_eq!(
                document.second_related_identity().map(|value| value.value),
                second.then_some(0)
            );
            assert!(document.wrapper_byte_offset().is_none());
            assert!(document
                .first_related_identity()
                .is_none_or(|value| value.offset.is_none()));
            assert!(document.clone().into_draft().is_err());
            assert_eq!(serde_json::to_value(document).unwrap(), wire);
            for field in [
                "wrapper_byte_offset",
                "carrier_byte_offset",
                "first_related_identity_offset",
                "second_related_identity_offset",
            ] {
                let mut invalid = wire.clone();
                invalid[field] = 0.into();
                assert!(serde_json::from_value::<DesignConstructionTrackingPath>(invalid).is_err());
            }
        }
    }
}
#[test]
fn construction_tracking_path_borrowed_wire_matches_owned_wire_bytes() {
    for first in [false, true] {
        let path = DesignConstructionTrackingPath::try_new(tracking_source(first, first)).unwrap();
        let owned = super::DesignConstructionTrackingPathWire::from(path.clone());
        assert_eq!(
            serde_json::to_vec(&path).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}
#[test]
fn construction_tracking_path_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignConstructionTrackingPath,
    }
    let path = DesignConstructionTrackingPath::try_new(tracking_source(false, false)).unwrap();
    let record = NestedRecord {
        id: "f3d:native:tracking-path#0",
        value: &path,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::CONSTRUCTION_TRACKING_PATH_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_TRACKING_PATH_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
// Fixture fields are appended from the bounded table of explicit test cases.
#[allow(clippy::format_push_string)]
fn construction_path_preserves_layout_wire_and_rejects_mixed_forms() {
    let prefix = r#"{"record_index":100,"class_tag":"304","entity_ref":174"#;
    let suffix = r#","scope_record_index":90,"nested_record_index":102,"following_record_index":101,"following_class_tag":"390"}"#;
    let fields = [
        (
            "transform",
            "[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]",
        ),
        ("transform_offset", "33"),
        ("compact_variant", "false"),
    ];
    for mask in 0..8 {
        let mut wire = prefix.to_owned();
        for (index, (field, value)) in fields.iter().enumerate() {
            if mask & (1 << index) != 0 {
                wire.push_str(&format!(",\"{field}\":{value}"));
            }
        }
        wire.push_str(suffix);
        let result = serde_json::from_str::<super::DesignConstructionOperandPath>(&wire);
        if mask == 1 || mask == 4 {
            assert_eq!(
                serde_json::to_string(&result.expect("complete placement form"))
                    .expect("path wire"),
                wire
            );
        } else {
            let error = result.expect_err("invalid placement form").to_string();
            if mask & 2 != 0 {
                assert!(error.contains("transform_offset"), "{error}");
            } else {
                assert!(
                    error.contains("transform") && error.contains("compact_variant"),
                    "{error}"
                );
            }
        }
    }
}

#[test]
fn construction_path_borrowed_wire_matches_owned_wire_bytes() {
    for wire in [
        r#"{"record_index":100,"class_tag":"304","entity_ref":174,"compact_variant":false,"scope_record_index":90,"nested_record_index":102,"following_record_index":101,"following_class_tag":"390"}"#,
        r#"{"record_index":100,"class_tag":"304","entity_ref":174,"transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],"transform_offset":33,"scope_record_index":90,"nested_record_index":102,"following_record_index":101,"following_class_tag":"390"}"#,
    ] {
        let mut content: serde_json::Value = serde_json::from_str(wire).unwrap();
        let transform_offset = content.as_object_mut().unwrap().remove("transform_offset");
        let path: super::DesignConstructionOperandPath = serde_json::from_value(content).unwrap();
        let path = if let Some(transform_offset) = transform_offset {
            let transform_offset = transform_offset.as_u64().unwrap();
            // The stated matrix offset and the fixed 33-byte prefix locate
            // this source frame. No default source origin is introduced.
            let byte_offset = transform_offset.checked_sub(33).unwrap();
            let content = super::DesignConstructionOperandPathWire::from(path);
            let path = super::DesignConstructionOperandPath::try_new(
                super::DesignConstructionOperandPathDraft {
                    record_index: content.record_index,
                    byte_offset,
                    class_tag: content.class_tag.try_into().unwrap(),
                    entity_ref: content.entity_ref,
                    entity_ref_offset: byte_offset + 22,
                    placement: super::DesignConstructionPathPlacement::Transform(
                        content.transform.unwrap(),
                    ),
                    scope_record_index: content.scope_record_index,
                    scope_record_index_offset: byte_offset + 163,
                    nested_record_index: content.nested_record_index,
                    nested_record_index_offset: byte_offset + 174,
                    following_record_index: content.following_record_index,
                    following_byte_offset: byte_offset + 190,
                    following_class_tag: content.following_class_tag.try_into().unwrap(),
                },
            )
            .unwrap();
            assert_eq!(path.byte_offset().unwrap() + 33, transform_offset);
            path
        } else {
            path
        };
        let owned = super::DesignConstructionOperandPathWire::from(path.clone());
        assert_eq!(
            serde_json::to_vec(&path).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn construction_path_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignConstructionOperandPath,
    }
    let path: super::DesignConstructionOperandPath = serde_json::from_str(
        r#"{"record_index":100,"class_tag":"304","entity_ref":174,"compact_variant":false,"scope_record_index":90,"nested_record_index":102,"following_record_index":101,"following_class_tag":"390"}"#
    ).unwrap();
    let record = NestedRecord {
        id: "f3d:native:construction-path#0",
        value: &path,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_construction_operand_identities",
        || super::CONSTRUCTION_PATH_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_PATH_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

fn identity_wire(indices: &[u32], tags: &[&str]) -> serde_json::Value {
    json!({"id":"f3d:native:construction-identity#0","ordinal":0,"group_record_index":200,
        "wrapper_record_indices":indices,"wrapper_class_tags":tags,"following_record_index":310,"following_class_tag":"304"})
}
#[test]
fn identity_wrapper_rows_keep_order_and_reject_unequal_semantic_columns() {
    for count in 0..=2 {
        for tags in 0..=2 {
            let wire = identity_wire(&[300, 305][..count], &["384", "289"][..tags]);
            let result =
                serde_json::from_value::<super::DesignConstructionOperandIdentity>(wire.clone());
            if count == tags {
                let identity = result.unwrap();
                assert!(identity.following_byte_offset().is_none());
                assert!(identity
                    .wrappers()
                    .iter()
                    .all(|wrapper| wrapper.byte_offset.is_none()));
                assert_eq!(serde_json::to_value(identity).unwrap(), wire);
            } else {
                assert!(result.is_err());
            }
        }
    }
    let mut duplicate = identity_wire(&[300, 300], &["384", "289"]);
    assert!(
        serde_json::from_value::<super::DesignConstructionOperandIdentity>(duplicate.clone())
            .is_err()
    );
    duplicate["wrapper_record_indices"] = json!([300, 305]);
    duplicate["wrapper_byte_offsets"] = json!([0, 24]);
    assert!(serde_json::from_value::<super::DesignConstructionOperandIdentity>(duplicate).is_err());
}
#[test]
fn construction_identity_borrowed_wire_matches_owned_wire_bytes() {
    for count in 0..=2 {
        let identity: super::DesignConstructionOperandIdentity = serde_json::from_value(
            identity_wire(&[300, 305][..count], &["384", "289"][..count]),
        )
        .unwrap();
        let owned = super::DesignConstructionOperandIdentityWire::from(identity.clone());
        assert_eq!(
            serde_json::to_vec(&identity).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}
#[test]
fn construction_identity_native_retained_limit_refuses_before_clone() {
    let identity: super::DesignConstructionOperandIdentity =
        serde_json::from_value(identity_wire(&[300, 305], &["384", "289"])).unwrap();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &identity,
        "design_construction_operand_identities",
        || super::CONSTRUCTION_IDENTITY_CLONE_COUNT.with(|count| count.set(0)),
        || super::CONSTRUCTION_IDENTITY_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn construction_auxiliary_rows_preserve_semantic_order_without_location_columns() {
    for values in [Vec::<u32>::new(), vec![103, 106], vec![106, 103]] {
        let mut wire = json!({"opaque_index":1,"opaque_scalar":0.0,"variant":false});
        if !values.is_empty() {
            wire["auxiliary_record_indices"] = json!(values);
        }
        let frame: DesignConstructionOperandGroupFrame =
            serde_json::from_value(wire.clone()).unwrap();
        assert!(frame
            .auxiliary_records
            .iter()
            .all(|row| row.offset.is_none()));
        assert_eq!(
            frame
                .auxiliary_records
                .iter()
                .map(|row| row.value)
                .collect::<Vec<_>>(),
            values
        );
        assert_eq!(serde_json::to_value(frame).unwrap(), wire);
        wire["auxiliary_record_offsets"] = json!([37]);
        assert!(serde_json::from_value::<DesignConstructionOperandGroupFrame>(wire).is_err());
    }
}
#[test]
fn construction_trailing_rows_keep_optional_reference_and_reject_multiple_targets() {
    for values in [Vec::<u32>::new(), vec![300]] {
        let mut wire = json!({"opaque_index":1,"opaque_scalar":0.0,"variant":false});
        if !values.is_empty() {
            wire["trailing_record_indices"] = json!(values);
        }
        let frame: DesignConstructionOperandGroupFrame =
            serde_json::from_value(wire.clone()).unwrap();
        assert!(frame
            .trailing_records()
            .iter()
            .all(|row| row.offset.is_none()));
        assert_eq!(serde_json::to_value(frame).unwrap(), wire);
        wire["trailing_record_offsets"] = json!([1044]);
        assert!(serde_json::from_value::<DesignConstructionOperandGroupFrame>(wire).is_err());
    }
    assert!(
        serde_json::from_value::<DesignConstructionOperandGroupFrame>(json!({
            "trailing_record_indices":[300,301],"opaque_index":1,"opaque_scalar":0.0,"variant":false
        }))
        .is_err()
    );
}

#[test]
fn construction_member_rows_preserve_wire_and_reject_unequal_offsets() {
    for members in ["[]", "[10]", "[10,11]"] {
        let wire = format!(
            r#"{{"id":"group","scope_record_index":7,"scope_reference_ordinal":0,"record_index":9,"class_tag":"277","members":{members},"frame":{{"opaque_index":1,"opaque_scalar":0.0,"variant":false}},"role":0,"paired_class_tag":"278"}}"#
        );
        let group: super::DesignConstructionOperandGroup =
            serde_json::from_str(&wire).expect("construction group");
        assert_eq!(
            serde_json::to_string(&group).expect("construction wire"),
            wire
        );
        let invalid = wire.replacen("\"members\":", "\"member_offsets\":[1,2,3],\"members\":", 1);
        let error = serde_json::from_str::<super::DesignConstructionOperandGroup>(&invalid)
            .expect_err("unequal member arrays")
            .to_string();
        assert!(error.contains("member_offsets"));
        for (roles, valid) in [
            (r#""extrude_role":"bodies","#, true),
            (
                r#""extrude_role":"faces","extrude_face_role":"start","#,
                true,
            ),
            (r#""extrude_role":"faces","#, false),
            (r#""extrude_face_role":"start","#, false),
            (
                r#""extrude_role":"bodies","extrude_face_role":"start","#,
                false,
            ),
        ] {
            let role = if roles.contains("bodies") {
                super::super::extrude_selection::DesignOperandRole::BODIES_A
            } else {
                super::super::extrude_selection::DesignOperandRole::FACES
            };
            let tagged = wire.replace(r#""role":0,"#, &format!("\"role\":{},{roles}", role.raw()));
            let parsed = serde_json::from_str::<super::DesignConstructionOperandGroup>(&tagged);
            if valid {
                assert_eq!(
                    serde_json::to_string(&parsed.expect("tagged construction group"))
                        .expect("tagged construction wire"),
                    tagged
                );
            } else {
                assert!(parsed
                    .expect_err("unpaired extrude role")
                    .to_string()
                    .contains("extrude_face_role"));
            }
        }
    }
}

#[test]
fn construction_group_wire_requires_source_and_extrude_roles_to_agree() {
    use super::super::extrude_selection::DesignOperandRole;
    let wire = serde_json::json!({
        "id": "group", "scope_record_index": 1, "scope_reference_ordinal": 0,
        "record_index": 2, "class_tag": "256",
        "members": [],
        "frame": {"opaque_index": 1, "opaque_scalar": 0.0, "variant": false},
        "role": DesignOperandRole::PROFILE.raw(), "extrude_role": "profile",
        "paired_class_tag": "257"
    });
    let group: DesignConstructionOperandGroup = serde_json::from_value(wire.clone()).unwrap();
    let encoded = serde_json::to_value(&group).unwrap();
    assert_eq!(encoded["role"], wire["role"]);
    assert_eq!(encoded["extrude_role"], wire["extrude_role"]);
    assert_eq!(
        serde_json::from_value::<DesignConstructionOperandGroup>(encoded).unwrap(),
        group
    );
    for (role, extrude_role, face_role) in [
        (DesignOperandRole::PROFILE, "bodies", None),
        (DesignOperandRole::BODIES_A, "profile", None),
        (DesignOperandRole::PROFILE, "faces", Some("start")),
        (DesignOperandRole::FACES, "faces", None),
        (DesignOperandRole::PROFILE, "profile", Some("termination")),
    ] {
        let mut invalid = wire.clone();
        invalid["role"] = serde_json::json!(role.raw());
        invalid["extrude_role"] = serde_json::json!(extrude_role);
        if let Some(face_role) = face_role {
            invalid["extrude_face_role"] = serde_json::json!(face_role);
        }
        let error = serde_json::from_value::<DesignConstructionOperandGroup>(invalid).unwrap_err();
        assert!(error.to_string().contains("role"));
    }
}

#[test]
fn trailing_transform_scanner_checks_real_frames_and_document_keeps_only_content() {
    let wire = json!({"record_index":7,"byte_offset":100,"class_tag":"300",
        "transform":crate::records::identity::IDENTITY_MATRIX,"transform_offset":122,
        "following_record_index":8,"following_byte_offset":252,"following_class_tag":"301"});
    let admit = |wire| {
        DesignConstructionOperandTransform::try_new(
            serde_json::from_value::<super::DesignConstructionOperandTransformDraft>(wire).unwrap(),
        )
    };
    let source = admit(wire.clone()).unwrap();
    for field in [
        "record_index",
        "byte_offset",
        "transform_offset",
        "following_byte_offset",
    ] {
        let mut invalid = wire.clone();
        invalid[field] = (invalid[field].as_u64().unwrap() + 1).into();
        assert!(admit(invalid).is_err(), "{field}");
    }
    for index in [0, 6, 7] {
        let mut invalid = wire.clone();
        invalid["following_record_index"] = index.into();
        assert!(admit(invalid).is_err());
    }
    for index in [9, 10, 4000] {
        let mut independent = wire.clone();
        independent["following_record_index"] = index.into();
        assert_eq!(admit(independent).unwrap().following_record_index(), index);
    }
    let document_wire = serde_json::to_value(&source).unwrap();
    let document: DesignConstructionOperandTransform =
        serde_json::from_value(document_wire.clone()).unwrap();
    assert!(document.byte_offset().is_none());
    assert!(document.clone().into_draft().is_err());
    assert_eq!(document.transform, source.transform);
    assert_eq!(serde_json::to_value(document).unwrap(), document_wire);
    let mut boundary = wire;
    boundary["record_index"] = (u32::MAX - 1).into();
    boundary["following_record_index"] = u32::MAX.into();
    boundary["byte_offset"] = (u64::MAX - 152).into();
    boundary["transform_offset"] = (u64::MAX - 130).into();
    boundary["following_byte_offset"] = u64::MAX.into();
    let admitted = admit(boundary.clone()).unwrap();
    let mut draft = admitted.into_draft().unwrap();
    draft.record_index = u32::MAX;
    assert!(DesignConstructionOperandTransform::try_new(draft).is_err());
    boundary["byte_offset"] = (u64::MAX - 151).into();
    assert!(admit(boundary).is_err());
}

#[test]
fn tracking_optional_identities_select_and_validate_actual_source_extent() {
    for first in [false, true] {
        for second in [false, true] {
            let draft = tracking_source(first, second);
            let source = DesignConstructionTrackingPath::try_new(draft.clone()).unwrap();
            assert_eq!(
                source.following_byte_offset(),
                Some(draft.following_byte_offset)
            );
            assert_eq!(source.into_draft().unwrap(), draft);
            let mut invalid = draft.clone();
            invalid.carrier_record_index += 1;
            assert!(DesignConstructionTrackingPath::try_new(invalid).is_err());
            let mut invalid = draft.clone();
            invalid.carrier_byte_offset += 1;
            assert!(DesignConstructionTrackingPath::try_new(invalid).is_err());
            let mut invalid = draft.clone();
            invalid.primary_identity_offset += 1;
            assert!(DesignConstructionTrackingPath::try_new(invalid).is_err());
            let mut invalid = draft.clone();
            invalid.selector_offset += 1;
            assert!(DesignConstructionTrackingPath::try_new(invalid).is_err());
            let mut invalid = draft.clone();
            invalid.kind_offset += 1;
            assert!(DesignConstructionTrackingPath::try_new(invalid).is_err());
            let mut invalid = draft.clone();
            invalid.following_byte_offset += 1;
            assert!(DesignConstructionTrackingPath::try_new(invalid).is_err());
            if first {
                let mut invalid = draft.clone();
                invalid.first_related_identity.as_mut().unwrap().offset += 1;
                assert!(DesignConstructionTrackingPath::try_new(invalid).is_err());
            }
            if second {
                let mut invalid = draft;
                invalid.second_related_identity.as_mut().unwrap().offset += 1;
                assert!(DesignConstructionTrackingPath::try_new(invalid).is_err());
            }
        }
    }
}
fn persistent_source(tail: u64, next: u64) -> super::DesignConstructionPersistentIdentityDraft {
    super::DesignConstructionPersistentIdentityDraft {
        local_id: 1,
        local_id_offset: 21,
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 33,
        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e"
            .to_owned()
            .try_into()
            .unwrap(),
        context_id_offset: 109,
        tail_slot_present: false,
        tail_slot_offset: tail,
        next_record_index: 0,
        next_byte_offset: next,
    }
}
#[test]
fn persistent_identity_source_validates_both_actual_tail_extents() {
    for (tail, next) in [(0, 190), (185, 200)] {
        let draft = persistent_source(tail, next);
        let source = DesignConstructionPersistentIdentity::try_new(draft.clone()).unwrap();
        assert_eq!(source.clone().into_draft().unwrap(), draft);
        let document_wire = serde_json::to_value(source).unwrap();
        let document: DesignConstructionPersistentIdentity =
            serde_json::from_value(document_wire.clone()).unwrap();
        assert!(document.local_id_offset().is_none());
        assert!(document.into_draft().is_err());
        for field in [
            "local_id_offset",
            "asset_id_offset",
            "context_id_offset",
            "tail_slot_present",
            "tail_slot_offset",
            "next_byte_offset",
        ] {
            let mut invalid = document_wire.clone();
            invalid[field] = 0.into();
            assert!(
                serde_json::from_value::<DesignConstructionPersistentIdentity>(invalid).is_err()
            );
        }
        let mut invalid = draft.clone();
        invalid.local_id_offset += 1;
        assert!(DesignConstructionPersistentIdentity::try_new(invalid).is_err());
        let mut invalid = draft.clone();
        invalid.asset_id_offset += 1;
        assert!(DesignConstructionPersistentIdentity::try_new(invalid).is_err());
        let mut invalid = draft;
        invalid.next_byte_offset += 1;
        assert!(DesignConstructionPersistentIdentity::try_new(invalid).is_err());
    }
}
#[test]
fn persistent_identity_borrowed_wire_matches_owned_wire_bytes() {
    for (tail, next) in [(0, 190), (185, 200)] {
        let value =
            DesignConstructionPersistentIdentity::try_new(persistent_source(tail, next)).unwrap();
        let owned = super::DesignConstructionPersistentIdentityContent::from(value.clone());
        assert_eq!(
            serde_json::to_vec(&value).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}
#[test]
fn persistent_identity_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a DesignConstructionPersistentIdentity,
    }
    let value = DesignConstructionPersistentIdentity::try_new(persistent_source(0, 190)).unwrap();
    let record = NestedRecord {
        id: "f3d:native:persistent-identity#0",
        value: &value,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::PERSISTENT_IDENTITY_CLONE_COUNT.with(|count| count.set(0)),
        || super::PERSISTENT_IDENTITY_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

/// A top-level optional key on a topology record names itself in its refusal.
#[test]
fn a_top_level_topology_key_names_itself_in_its_refusal() {
    for key in ["transform", "compact_variant"] {
        states_the_key(
            key,
            &refusal::<super::DesignConstructionOperandPathWire>(key),
        );
    }
    let source_only = refusal::<super::DesignConstructionOperandPathWire>("transform_offset");
    assert!(
        source_only.contains("unknown field `transform_offset`"),
        "{source_only}"
    );
    for key in ["first_related_identity", "second_related_identity"] {
        states_the_key(
            key,
            &refusal::<super::DesignConstructionTrackingPathWire>(key),
        );
    }
}

#[test]
fn construction_path_source_admission_checks_actual_positions_and_reload_has_none() {
    use super::{
        DesignConstructionOperandPath as Path, DesignConstructionOperandPathDraft as Draft,
        DesignConstructionPathPlacement as Placement,
    };
    for placement in [
        Placement::Compact(false),
        Placement::Transform(crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY),
    ] {
        let scope_offset = if matches!(placement, Placement::Transform(_)) {
            163
        } else {
            35
        };
        let draft = Draft {
            record_index: 100,
            byte_offset: 0,
            class_tag: "304".to_owned().try_into().unwrap(),
            entity_ref: 174,
            entity_ref_offset: 22,
            placement,
            scope_record_index: 90,
            scope_record_index_offset: scope_offset,
            nested_record_index: 102,
            nested_record_index_offset: scope_offset + 11,
            following_record_index: 101,
            following_byte_offset: scope_offset + 27,
            following_class_tag: "390".to_owned().try_into().unwrap(),
        };
        let source = Path::try_new(draft.clone()).unwrap();
        assert_eq!(source.byte_offset(), Some(0));
        assert_eq!(source.clone().into_draft().unwrap(), draft);
        let wire = serde_json::to_value(&source).unwrap();
        let document: Path = serde_json::from_value(wire.clone()).unwrap();
        assert!(document.byte_offset().is_none());
        assert!(document.into_draft().is_err());
        for field in [
            "byte_offset",
            "entity_ref_offset",
            "scope_record_index_offset",
            "nested_record_index_offset",
            "following_byte_offset",
        ] {
            let mut invalid = wire.clone();
            invalid[field] = 0.into();
            assert!(serde_json::from_value::<Path>(invalid).is_err(), "{field}");
        }
        let mut invalid = draft.clone();
        invalid.entity_ref_offset += 1;
        assert!(Path::try_new(invalid).is_err());
        let mut invalid = draft.clone();
        invalid.scope_record_index_offset += 1;
        assert!(Path::try_new(invalid).is_err());
        let mut invalid = draft.clone();
        invalid.nested_record_index_offset += 1;
        assert!(Path::try_new(invalid).is_err());
        let mut invalid = draft.clone();
        invalid.following_byte_offset += 1;
        assert!(Path::try_new(invalid).is_err());
        let mut invalid = draft;
        invalid.byte_offset = u64::MAX;
        assert!(Path::try_new(invalid).is_err());
    }
}

#[test]
fn identity_source_wrapper_stride_is_checked_without_fabricating_document_locations() {
    let make = |offsets: [u64; 2], following| {
        super::DesignConstructionOperandIdentity::try_new(
            super::DesignConstructionOperandIdentityDraft {
                id: "f3d:native:construction-identity#0".into(),
                ordinal: 4,
                group_record_index: 200,
                wrappers: [300, 305]
                    .into_iter()
                    .zip(offsets)
                    .map(|(record_index, offset)| super::DesignIdentityWrapper {
                        record_index,
                        byte_offset: Some(offset),
                        class_tag: "384".to_owned().try_into().unwrap(),
                    })
                    .collect(),
                following_record_index: 310,
                following_byte_offset: following,
                following_class_tag: "304".to_owned().try_into().unwrap(),
                tracking_path: None,
                persistent_identity: None,
            },
        )
    };
    let source = make([0, 24], 48).unwrap();
    assert_eq!(source.ordinal, 4);
    assert!(make([0, 23], 48).is_err());
    assert!(make([0, 24], 49).is_err());
    let wire = serde_json::to_value(source).unwrap();
    let document: super::DesignConstructionOperandIdentity =
        serde_json::from_value(wire.clone()).unwrap();
    assert!(document.following_byte_offset().is_none());
    assert!(document.clone().into_draft().is_err());
    assert_eq!(serde_json::to_value(document).unwrap(), wire);
}

#[test]
fn tracking_fragment_domain_rejects_nonpositive_and_out_of_range_ordinals() {
    let base =
        serde_json::to_value(DesignConstructionTrackingPath::try_new(tracking_source(false, false)).unwrap())
            .unwrap();
    for (selector, kind) in [(1, 1), (1, 3), (3, 3), (i32::MAX, u32::MAX)] {
        let mut wire = base.clone();
        wire["selector"] = json!(selector);
        wire["kind"] = json!(kind);
        let value: DesignConstructionTrackingPath = serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), wire);
    }
    for (selector, kind) in [(-1, 3), (0, 3), (4, 3), (1, 0), (i32::MIN, u32::MAX)] {
        let mut wire = base.clone();
        wire["selector"] = json!(selector);
        wire["kind"] = json!(kind);
        let error = serde_json::from_value::<DesignConstructionTrackingPath>(wire).unwrap_err();
        assert!(error
            .to_string()
            .contains(if kind == 0 { "kind" } else { "selector" }));
        let mut draft = tracking_source(false, false);
        draft.selector = selector;
        draft.kind = kind;
        assert!(DesignConstructionTrackingPath::try_new(draft).is_err());
    }
}
