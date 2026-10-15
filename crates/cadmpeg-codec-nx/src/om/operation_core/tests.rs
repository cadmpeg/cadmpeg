// SPDX-License-Identifier: Apache-2.0
use super::{core_for_decode, OperationCore};
use crate::om::operation_record::OperationPayload;
use cadmpeg_ir::native::bytes::NativeBytes;

fn inherited() -> Vec<u8> {
    b"\x0d\x01\x02\x01\0\0\0\x01\x01\x01\x01\x01\x02\x03\x04\x05\x03\x04\x05\x01\0\0\xff\0\xff\x09\xc0\x1f\xff\xff\x01\0\0".to_vec()
}
fn result(bytes: &mut Vec<u8>, association: u8) {
    bytes.extend(b"\x01\x02\x07\x08\x97\x75\x01\x02\x03\x09\xff\0\0\0\x0b\0\xf0\x01\x02");
    bytes.extend([association, 10, 1]);
}
fn cylinder(mode: [u8; 3], null: bool) -> Vec<u8> {
    let mut bytes = b"\x01\0\0\x01\0\x05".to_vec();
    // Six atoms: signed cardinals and both finite shifted widths.
    bytes.extend(b"\0\x80\x01\x81\x50\x40\0\0\x30\x10\0\0\0\0\0\0");
    for i in 0..9 {
        bytes.extend([0xf0, i]);
    }
    bytes.extend(b"\xff\x01\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0");
    let create = mode == [2, 0, 0];
    result(&mut bytes, if create { 1 } else { 2 });
    if create {
        bytes.extend([0; 6]);
    } else {
        bytes.extend(b"\0\0\0\0\0\x01\x02\x07\x08\x97\x75");
    }
    bytes.extend(inherited());
    bytes.extend(b"\x81\x73\x80\xb4\x01\x03\x02\x01\x02");
    bytes.extend(mode);
    bytes.extend(b"\x01\0\0\0\x06\x06");
    bytes.extend([if null { 255 } else { 8 }, 0]);
    bytes
}
fn motion(null: bool) -> Vec<u8> {
    let mut bytes =
        b"\x01\0\0\x01\0\x02\xf1\x01\0\xff\x02\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0"
            .to_vec();
    result(&mut bytes, 2);
    bytes.extend([0; 6]);
    bytes.extend(inherited());
    bytes.extend(b"\x01\x02\x01\x03\x02\x01\x02\x03\x04\x05\x06\x07\x08\x09\x06\x06");
    if null {
        bytes.push(255);
    } else {
        bytes.extend([8, 0]);
    }
    bytes
}
fn wire(kind: &str, bytes: Vec<u8>) -> serde_json::Value {
    serde_json::json!({"kind": kind, "bytes": NativeBytes::from(bytes)})
}

#[test]
fn complete_native_cores_admit_each_boolean_mode_and_exact_scalar_width() {
    #[derive(serde::Serialize)]
    struct Record {
        id: &'static str,
        core: OperationCore,
    }
    for (tuple, expected) in [
        ([2, 0, 0], "create"),
        ([2, 0, 1], "unite"),
        ([3, 0, 0], "subtract"),
    ] {
        for null in [false, true] {
            let bytes = cylinder(tuple, null);
            let value = wire("cylinder", bytes.clone());
            let core: OperationCore = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(core.as_bytes(), bytes);
            assert_eq!(core.cylinder_boolean_mode(), Some(expected));
            assert_eq!(serde_json::to_value(&core).unwrap(), value);
            let record = Record {
                id: "nx:feature:core#0",
                core,
            };
            cadmpeg_test_support::native_serialization::assert_native_limit(
                &record,
                serde_json::json!({"id":"nx:feature:core#0","core":value}),
            );
            for end in 0..bytes.len() {
                assert!(
                    serde_json::from_value::<OperationCore>(wire(
                        "cylinder",
                        bytes[..end].to_vec()
                    ))
                    .is_err(),
                    "prefix {end}"
                );
            }
            let mut extra = bytes.clone();
            extra.push(0);
            assert!(serde_json::from_value::<OperationCore>(wire("cylinder", extra)).is_err());
            let mut broken = bytes;
            broken[6 + 4] = 0xff;
            assert!(serde_json::from_value::<OperationCore>(wire("cylinder", broken)).is_err());
        }
    }
    let mut disagreement = cylinder([2, 0, 0], false);
    let at = disagreement.len() - 11; // mode precedes four zeros and four terminal bytes.
    disagreement[at..at + 3].copy_from_slice(&[3, 0, 0]);
    assert!(serde_json::from_value::<OperationCore>(wire("cylinder", disagreement)).is_err());
}

#[test]
fn object_motion_null_terminal_bounds_the_following_lane_and_checks_registry() {
    for null in [false, true] {
        let bytes = motion(null);
        let value = wire("move_object", bytes.clone());
        let core: OperationCore = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(core.as_bytes(), bytes);
        assert_eq!(core.cylinder_boolean_mode(), None);
        assert_eq!(serde_json::to_value(core).unwrap(), value);
        let mut classes: Vec<_> = [
            "UGS::ModlFeature::MoveObjectParms",
            "UGS::Modl::MotionOptions",
            "UGS::CM_BREF_DATA",
            "UGS::OM::MethodicObject",
        ]
        .into_iter()
        .enumerate()
        .map(|(offset, name)| crate::om::TypeDefinition {
            offset,
            name,
            registry_tail: &[],
        })
        .collect();
        let mut following = bytes.clone();
        following.extend([4, 48, 222]);
        crate::test_support::with_decode_context(|ctx| {
            let read = |classes: &[_]| {
                core_for_decode(
                    ctx,
                    OperationPayload::new(&following, 0, "MOVE_OBJECT").unwrap(),
                    classes,
                )
                .unwrap()
            };
            assert_eq!(
                read(&classes).map(|v| v.byte_len()),
                null.then_some(bytes.len())
            );
            classes[3].name = "UGS::OTHER";
            assert!(read(&classes).is_none());
            assert!(
                serde_json::from_value::<OperationCore>(wire("move_object", following)).is_err()
            );
            classes[3].name = "UGS::OM::MethodicObject";
            classes[1].name = "UGS::OTHER";
            assert!(core_for_decode(
                ctx,
                OperationPayload::new(&bytes, 0, "MOVE_OBJECT").unwrap(),
                &classes
            )
            .unwrap()
            .is_none());
        });
        for end in 0..bytes.len() {
            assert!(
                serde_json::from_value::<OperationCore>(wire("move_object", bytes[..end].to_vec()))
                    .is_err(),
                "prefix {end}"
            );
        }
    }
}
