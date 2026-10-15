// SPDX-License-Identifier: Apache-2.0
use super::{read, CLASSES};
use crate::om::operation_core::{core_for_decode, OperationCore};
use crate::om::operation_record::OperationPayload;
use cadmpeg_ir::native::bytes::NativeBytes;

fn core() -> Vec<u8> {
    // Allocation references use f0/f1 payload tokens, not feature tokens.
    let mut b =
        b"\x01\0\0\x01\0\xf0\x11\xf0\x12\xf0\x13\x01\x05\x04\x01\x01\x01\0\x03\x02\xa0\x5e\x38\x02"
            .to_vec();
    for group in 0..2 {
        b.extend([1, 2, 3, 1, 2, 0xf0, 20]);
        if group == 0 {
            b.extend([1, 2]);
        }
        b.extend(b"\0\0\0\0\0\xff\x01\x02\xf0\x15");
        b.extend(if group == 0 {
            b"\0\x89\x5c".as_slice()
        } else {
            b"\0\x1d".as_slice()
        });
    }
    b.extend(b"\0\0\0\0\0\xff\0\xff\x01\0\0\x01\x05\x01\x01\x01\x01\x01\x05\x01\x01\x01\x01\0\x01\x05\xff\xff\xff\xff\x02");
    b.extend(
        b"\x01\x10\x04\x04\x02\x02\xa0\x5e\x38\x08\x08\x02\x02\x01\x01\x01\x01\x02\x02\x01\x05",
    );
    // Two ordinary binary64 encodings of 1.0, with an equality invariant.
    b.extend(b"\x2f\xf0\0\0\0\0\0\0\x03\x03\x2f\xf0\0\0\0\0\0\0\xf0\x16\xf0\x17\xf0\x18");
    b.extend(b"\x01\x01\0\x01\x05\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0\x01\x02\x0b\x19\x97\x75\x01\x02\x04\x1a\xff\0\0\0\x0b\0\xf0\x01\x02\x01\x1b\x01\0\0\0\0\0\0\x0d\x01\x02\x01\0\0\0\x1c\x01\x01\x01\x01\x02\x03\x04\x05\x03\x04\x05\x01\0\0\xff\0\xff\x06\xc0\x1f\xff\xff\x01\0\0\x1d\x1e\x01\x03\x02\x01\x02\x03\x04\x05\x06\x07\x08\x09\x1f\x1f\x20\0");
    b
}
fn wire(bytes: Vec<u8>) -> serde_json::Value {
    serde_json::json!({"kind":"curve_mesh", "bytes":NativeBytes::from(bytes)})
}
#[test]
fn curve_mesh_requires_two_nonempty_groups_and_exact_complete_wire() {
    let bytes = core();
    assert_eq!(read(&bytes), Some([1, 2, 3, 4]));
    let value = wire(bytes.clone());
    let decoded: OperationCore = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.as_bytes(), bytes);
    assert_eq!(serde_json::to_value(decoded).unwrap(), value);
    for end in 0..bytes.len() {
        assert!(read(&bytes[..end]).is_none(), "prefix {end}");
    }
    let groups: Vec<_> = bytes
        .windows(7)
        .enumerate()
        .filter(|(_, row)| *row == b"\x01\x02\x03\x01\x02\xf0\x14")
        .map(|(at, _)| at)
        .collect();
    assert_eq!(groups.len(), 2);
    for group in &groups {
        let mut empty = bytes.clone();
        empty[group + 1] = 1;
        assert!(read(&empty).is_none(), "empty group at {group}");
    }
    let mut direct_reference = bytes.clone();
    direct_reference[5] = 17;
    assert!(read(&direct_reference).is_none());
    for offset in [
        0,
        groups[0] - 1,
        groups[0] + 1,
        groups[0] + 2,
        groups[1] + 1,
        groups[1] + 2,
        bytes.len() - 3,
    ] {
        let mut changed = bytes.clone();
        changed[offset] = 0;
        assert!(read(&changed).is_none(), "offset {offset}");
    }
    let mut changed = bytes.clone();
    let atom = bytes
        .windows(8)
        .position(|w| w == b"\x2f\xf0\0\0\0\0\0\0")
        .unwrap();
    changed[atom + 17] = 1;
    assert!(read(&changed).is_none());
    let mut changed = bytes;
    changed.push(0);
    assert!(serde_json::from_value::<OperationCore>(wire(changed)).is_err());
}
#[test]
fn curve_mesh_requires_owning_profile_refit_and_endpoint_classes() {
    let bytes = core();
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
        let payload = OperationPayload::new(&bytes, 0, "THRU_CURVE_MESH").unwrap();
        assert!(core_for_decode(ctx, payload, &classes).unwrap().is_some());
        for index in 0..classes.len() {
            let name = classes[index].name;
            classes[index].name = "UGS::OTHER";
            assert!(core_for_decode(ctx, payload, &classes).unwrap().is_none());
            classes[index].name = name;
        }
    });
}
