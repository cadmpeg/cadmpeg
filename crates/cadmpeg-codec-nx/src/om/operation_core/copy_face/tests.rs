// SPDX-License-Identifier: Apache-2.0
use super::read;
use crate::om::operation_core::{core_for_decode, OperationCore};
use crate::om::operation_record::OperationPayload;
use cadmpeg_ir::native::bytes::NativeBytes;

fn core() -> Vec<u8> {
    let mut b=b"\x01\0\0\x01\0\x11\x01\x02\x02\x01\x02\x02\x12\xff\xff\x03\xf0\x13\0\xf0\x14\x04\x01\x01\x06\xff\xf0\x15\xff\0\0\x05".to_vec();
    b.extend(b"\x2f\x70\x62\x4d\xd2\xf1\xa9\xfc\x03\x50\x44\0\0\x81\x01\x46\x8b\xba\x01\xa3\xae\x30\x01\x01\x02\x04\x02\x01\x02\x01\0\0\0\0\x01\0");
    b.extend(b"\xf0\x16\0\0\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0\x01\x02\x0b\x17\x97\x75\x01\x02\x07\x18\xff\x06\0");
    // Ordinary signed binary64 -4.0, then one-byte zero, without a sign-bit mask.
    b.extend(b"\xb0\x10\0\0\0\0\0\0\0\x01\x02\x19\0\0\x0b\0\xf0\x01\x02\x01\x1a\x01\0\0\0\0\0\0\x0d\x01\x02\x01\0\0\0\x1b\x01\x01\x01\x01\x02\x03\x04\x05\x03\x04\x05\x01\0\0\xff\0\xff\x06\xc0\x1f\xff\xff\x01\x02\x01\x1c\x1d\x01\x03\x02\x01\x02\x03\x04\x05\x06\x07\x08\x09\x1e\x1e\x1f\0");
    b
}
fn wire(bytes: Vec<u8>) -> serde_json::Value {
    serde_json::json!({"kind":"copy_face","bytes":NativeBytes::from(bytes)})
}
#[test]
fn face_copy_admits_signed_recipe_and_complete_wire_with_exact_limits() {
    #[derive(serde::Serialize)]
    struct Record {
        id: &'static str,
        core: OperationCore,
    }
    let bytes = core();
    assert_eq!(read(&bytes), Some([1, 2, 3, 4, 5, 6, 7]));
    let value = wire(bytes.clone());
    let decoded: OperationCore = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.as_bytes(), bytes);
    assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
    let record = Record {
        id: "nx:feature:core#0",
        core: decoded,
    };
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id":"nx:feature:core#0","core":value}),
    );
    for end in 0..bytes.len() {
        assert!(read(&bytes[..end]).is_none(), "prefix {end}");
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(serde_json::from_value::<OperationCore>(wire(trailing)).is_err());
    for offset in [0, 7, 8, bytes.len() - 3] {
        let mut invalid = bytes.clone();
        invalid[offset] = 0;
        assert!(
            serde_json::from_value::<OperationCore>(wire(invalid)).is_err(),
            "offset {offset}"
        );
    }
}
#[test]
fn face_copy_requires_each_component_in_the_owning_registry() {
    let bytes = core();
    let mut classes: Vec<_> = super::CLASSES
        .into_iter()
        .enumerate()
        .map(|(offset, name)| crate::om::TypeDefinition {
            offset,
            name,
            registry_tail: &[],
        })
        .collect();
    crate::test_support::with_decode_context(|ctx| {
        let payload = OperationPayload::new(&bytes, 0, "COPY_FACE").unwrap();
        assert_eq!(
            core_for_decode(ctx, payload, &classes)
                .unwrap()
                .unwrap()
                .byte_len(),
            bytes.len()
        );
        for index in 0..classes.len() {
            let name = classes[index].name;
            classes[index].name = "UGS::OTHER";
            assert!(core_for_decode(ctx, payload, &classes).unwrap().is_none());
            classes[index].name = name;
        }
    });
}
