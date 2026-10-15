// SPDX-License-Identifier: Apache-2.0
use super::{CoreView, MoveFaceCore};

use crate::test_support::move_face::core;

#[test]
fn move_face_complete_lists_preserve_serialized_pairing_and_positions() {
    for count in [1, 2, 4] {
        let bytes = core(count);
        let view = CoreView::parse(&bytes).unwrap();
        assert_eq!(view.byte_len, bytes.len());
        let writes =
            crate::test_support::with_decode_context(|ctx| view.writes(ctx, 1000)).unwrap();
        assert_eq!(writes.len(), usize::from(count));
        for (i, write) in writes.iter().enumerate() {
            let i = u8::try_from(i).unwrap();
            assert_eq!(write.body_identity(), 11 + 2 * i);
            assert_eq!(write.group_node().value(), u32::from(20 + i));
            assert_eq!(write.body_image().value(), u32::from(40 + i));
            assert_eq!(write.endpoint_class_index().get(), 16);
            assert_eq!(bytes[write.group_node_offset() - 1000], 20 + i);
            assert_eq!(bytes[write.body_image_offset() - 1000], 40 + i);
            assert_eq!(write.end_offset(), write.body_image_offset() + 2);
            assert_eq!(
                write.relocated(2000).unwrap().body_image_offset(),
                u64::try_from(write.body_image_offset()).unwrap() + 2000
            );
        }
    }
}

#[test]
fn move_face_core_wire_rejects_incomplete_storage_and_trailing_lane() {
    let bytes = core(4);
    for len in 0..bytes.len() - 1 {
        assert!(CoreView::parse(&bytes[..len]).is_none(), "prefix {len}");
    }
    let wire =
        serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::from(bytes.clone())).unwrap();
    let decoded: MoveFaceCore = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(decoded.as_bytes().len(), bytes.len());
    assert_eq!(serde_json::to_value(decoded).unwrap(), wire);
    let mut extra = bytes.clone();
    extra.extend([1, 2]);
    assert!(serde_json::from_value::<MoveFaceCore>(
        serde_json::to_value(cadmpeg_ir::native::bytes::NativeBytes::from(extra)).unwrap()
    )
    .is_err());
    let view = CoreView::parse(&bytes).unwrap();
    for at in [
        0,
        view.relation_start + 1,
        view.groups_start + 2,
        view.images_start,
        view.images_start + 6,
        bytes.len() - 3,
    ] {
        let mut invalid = bytes.clone();
        invalid[at] = 0;
        assert!(CoreView::parse(&invalid).is_none(), "mutation {at}");
    }
}

#[test]
fn move_face_class_boundary_requires_the_owning_components() {
    use crate::om::operation_record::OperationPayload;
    let mut classes: Vec<_> = (0..68)
        .map(|i| crate::om::TypeDefinition {
            offset: i,
            name: "UGS::OTHER",
            registry_tail: &[],
        })
        .collect();
    for (index, name) in [
        (14, super::CLASSES[0]),
        (15, super::BODY_IMAGE_CLASS),
        (17, super::CLASSES[1]),
        (18, super::CLASSES[2]),
        (67, "UGS::CM_object"),
    ] {
        classes[index].name = name;
    }
    let bytes = core(1);
    assert!(CoreView::read(
        OperationPayload::new(&bytes, 0, "MOVE_FACE").unwrap(),
        &classes
    )
    .is_some());
    let mut following = bytes[..bytes.len() - 1].to_vec();
    following.extend([68, 48, 222]);
    let view = CoreView::read(
        OperationPayload::new(&following, 0, "MOVE_FACE").unwrap(),
        &classes,
    )
    .unwrap();
    assert_eq!(view.byte_len, bytes.len() - 1);
    classes[67].name = "UGS::OTHER";
    assert!(CoreView::read(
        OperationPayload::new(&following, 0, "MOVE_FACE").unwrap(),
        &classes
    )
    .is_none());
    classes[17].name = "UGS::OTHER";
    assert!(CoreView::read(
        OperationPayload::new(&bytes, 0, "MOVE_FACE").unwrap(),
        &classes
    )
    .is_none());
}

#[test]
fn move_face_empty_list_refuses_and_duplicate_native_entries_are_preserved() {
    assert!(CoreView::parse(&core(0)).is_none());
    let mut bytes = core(2);
    let view = CoreView::parse(&bytes).unwrap();
    let second_body = view.groups_start + 4;
    let second_image = view.images_start + 14;
    bytes[second_body] = 11;
    bytes[second_body + 1] = 20;
    bytes[second_image + 1] = 40;
    let view = CoreView::parse(&bytes).unwrap();
    let writes = crate::test_support::with_decode_context(|ctx| view.writes(ctx, 0)).unwrap();
    assert_eq!(writes.len(), 2);
    assert_eq!(writes[0].body_identity(), writes[1].body_identity());
    assert_eq!(writes[0].body_image(), writes[1].body_image());
    assert_ne!(writes[0].offset(), writes[1].offset());
}

#[test]
fn move_face_control_five_preserves_the_shifted_null_frame() {
    let mut bytes = core(1);
    bytes[18] = 5;
    bytes.remove(19);
    bytes.insert(22, 255);
    let view = CoreView::parse(&bytes).unwrap();
    let writes = crate::test_support::with_decode_context(|ctx| view.writes(ctx, 0)).unwrap();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].body_identity(), 11);
    bytes[22] = 0;
    assert!(CoreView::parse(&bytes).is_none());
}

#[test]
fn move_face_retained_core_honors_native_serialization_limits() {
    #[derive(serde::Serialize)]
    struct Record {
        id: &'static str,
        core: MoveFaceCore,
    }
    let bytes = cadmpeg_ir::native::bytes::NativeBytes::from(core(2));
    let value = serde_json::to_value(bytes).unwrap();
    let record = Record {
        id: "nx:feature:core#0",
        core: serde_json::from_value(value.clone()).unwrap(),
    };
    let wire = serde_json::json!({"id": record.id, "core": value});
    cadmpeg_test_support::native_serialization::assert_native_limit(&record, wire);
}
