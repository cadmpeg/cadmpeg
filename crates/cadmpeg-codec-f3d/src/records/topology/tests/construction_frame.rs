use super::super::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandGroupFrameWire,
};
use crate::records::Located;
use serde_json::json;

fn frame_wire() -> DesignConstructionOperandGroupFrameWire {
    let matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    serde_json::from_value(json!({
        "member_count_offset": 21,
        "opaque_index": 1, "opaque_index_offset": 118,
        "opaque_scalar": 0.0, "opaque_scalar_offset": 122, "variant": false,
        "trailing_record_indices": [300], "trailing_record_offsets": [90],
        "trailing_flags": [{"record_index":300,"byte_offset":1000,"class_tag":"277","value":false,"value_offset":1022}],
        "trailing_transforms": [{"record_index":300,"byte_offset":1000,"class_tag":"277","transform":matrix,"transform_offset":1022,"following_record_index":301,"following_byte_offset":1152,"following_class_tag":"278"}],
        "trailing_dual_transforms": [{"record_index":300,"byte_offset":1000,"class_tag":"277","first_transform":matrix,"first_transform_offset":1021,"second_transform":matrix,"second_transform_offset":1149}],
        "auxiliary_paths": [{"record_index":400,"byte_offset":2000,"class_tag":"277","entity_ref":1,"entity_ref_offset":2022,"compact_variant":false,"scope_record_index":7,"scope_record_index_offset":2035,"nested_record_index":402,"nested_record_index_offset":2046,"following_record_index":401,"following_byte_offset":2062,"following_class_tag":"278"}]
    })).unwrap()
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
    wire.trailing_record_offsets.push(101);
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
    // The same identity is legal in different collections.
    let wire = serde_json::to_value(frame_wire()).unwrap();
    assert_eq!(serde_json::to_value(frame).unwrap(), wire);
}

#[test]
fn construction_group_owns_member_stride_and_derives_role_offset() {
    let wire = json!({
        "id":"stream:group", "scope_record_index":7, "scope_reference_ordinal":0,
        "record_index":9,"byte_offset":0,"class_tag":"277",
        "members":[10,11],"member_offsets":[26,37],"frame":frame_wire(),
        "role":0,"role_offset":100,"paired_class_tag":"278","paired_byte_offset":200
    });
    let mut group: DesignConstructionOperandGroup = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(group.role_offset(), 100);
    for offsets in [json!([26, 36]), json!([u64::MAX, u64::MAX])] {
        let mut invalid = wire.clone();
        invalid["member_offsets"] = offsets;
        assert!(serde_json::from_value::<DesignConstructionOperandGroup>(invalid).is_err());
    }
    let mut invalid = wire.clone();
    invalid["role_offset"] = json!(101);
    assert!(serde_json::from_value::<DesignConstructionOperandGroup>(invalid).is_err());
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
    assert_eq!(serde_json::to_value(group).unwrap(), wire);
}
