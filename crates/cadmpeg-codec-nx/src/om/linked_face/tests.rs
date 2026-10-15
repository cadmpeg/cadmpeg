// SPDX-License-Identifier: Apache-2.0
use super::{payload_for_decode, read, LinkedFacePayload, CLASSES, SOURCE_KEYS, WAVE_KEYS};
use crate::om::operation_record::OperationPayload;
use cadmpeg_ir::native::bytes::NativeBytes;

fn text(bytes: &mut Vec<u8>, value: &str) {
    bytes.extend([3, u8::try_from(value.len() + 2).unwrap()]);
    bytes.extend(value.as_bytes());
    bytes.push(0);
}
fn payload() -> (Vec<u8>, usize) {
    let mut b = b"\x01\0\0\x01\0".to_vec();
    for _ in 0..4 {
        b.extend(b"\x01\0\0\0\0");
    }
    b.extend(b"\0\0\0\0\0\0\x01\x02\x02\xff\x11\x01\x02\x02\xff\x12\xff\x03");
    for count in [2, 2, 4] {
        b.extend([1, 3]);
        for _ in 0..count {
            text(&mut b, "");
        }
    }
    b.extend([0, 1, 3]);
    for (keys, kind) in [
        (SOURCE_KEYS, "Body"),
        (SOURCE_KEYS, "Face"),
        (WAVE_KEYS, ""),
    ] {
        b.extend([4, 1, 10]);
        for key in keys {
            text(&mut b, key);
        }
        b.extend([1, 10]);
        for index in 0..9 {
            text(&mut b, if index == 1 { kind } else { "" });
        }
    }
    text(&mut b, "");
    let prefix = b.len();
    // The named-cache contract assigns no grammar to this bounded tail.
    b.extend([255, 0, 17]);
    (b, prefix)
}

#[test]
fn linked_face_cache_validates_names_and_retains_the_unassigned_tail() {
    let (bytes, prefix) = payload();
    let wire = serde_json::to_value(NativeBytes::from(bytes.clone())).unwrap();
    let decoded: LinkedFacePayload = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(decoded.as_bytes(), bytes);
    assert_eq!(decoded.tables().unwrap()[1][1], "Face");
    assert_eq!(serde_json::to_value(&decoded).unwrap(), wire);
    for end in 0..prefix {
        assert!(read(&bytes[..end]).is_none(), "prefix {end}");
    }
    assert!(read(&bytes[..prefix]).is_some());
    for offset in [0, 33, 43] {
        let mut changed = bytes.clone();
        changed[offset] = 0;
        assert!(read(&changed).is_none(), "offset {offset}");
    }
    let name = bytes.windows(4).position(|w| w == b"Name").unwrap();
    let mut changed = bytes;
    changed[name] = b'X';
    assert!(serde_json::from_value::<LinkedFacePayload>(
        serde_json::to_value(NativeBytes::from(changed)).unwrap()
    )
    .is_err());
}

#[test]
fn linked_face_cache_requires_each_class_in_its_owning_registry() {
    let (bytes, _) = payload();
    let mut classes: Vec<_> = CLASSES
        .into_iter()
        .enumerate()
        .map(|(offset, name)| crate::om::TypeDefinition {
            offset,
            name,
            registry_tail: &[],
        })
        .collect();
    crate::test_support::with_decode_context(|ctx| {
        let payload = OperationPayload::new(&bytes, 0, "LINKED_FACE").unwrap();
        assert_eq!(
            payload_for_decode(ctx, payload, &classes)
                .unwrap()
                .unwrap()
                .as_bytes(),
            bytes
        );
        for index in 0..classes.len() {
            let name = classes[index].name;
            classes[index].name = "UGS::OTHER";
            assert!(payload_for_decode(ctx, payload, &classes)
                .unwrap()
                .is_none());
            classes[index].name = name;
        }
    });
}
