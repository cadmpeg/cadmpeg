// SPDX-License-Identifier: Apache-2.0
use super::{core_for_decode, inherited, result, wire, OperationCore, OperationPayload};

fn prefix() -> Vec<u8> {
    b"\x01\0\0\x01\0\x05\0\x80\x01\x81\x50\x40\0\0\x30\x10\0\0\0\0\0\0".to_vec()
}
fn cone() -> Vec<u8> {
    let mut bytes = prefix();
    for value in 0..13 {
        bytes.extend([0xf0, value]);
    }
    bytes.extend(b"\x01\xf0\x0d\xff\xff\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01\x01");
    bytes.extend(b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0");
    result(&mut bytes, 1);
    bytes.extend([0; 6]);
    bytes.extend(inherited());
    bytes.extend(b"\x81\x73\x80\xb4\x01\x03\x02\x01\x02\x02\0\0\x01\0\0\0\x06\x06\x08\0");
    bytes
}
fn sphere(newer: bool) -> Vec<u8> {
    let mut bytes = prefix();
    for value in 0..4 {
        bytes.extend([0xf0, value]);
    }
    bytes.extend(b"\xff\xf0\x04\x01\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0");
    bytes.extend(b"\x01\x02\x07\x08\x97\x75\x01\x02\x03\x09\xff\0\0\0\x0b\0\xf0\x01\x02\x02");
    bytes.extend(if newer {
        b"\xa0\x18\x07".as_slice()
    } else {
        b"\x0a".as_slice()
    });
    bytes.extend(b"\x01\0\0\0\0\0\x01\x02\x07\x08\x97\x75\x0d\x01\x02\x01");
    bytes.extend(if newer {
        b"\x01\x02\x11\0\x05\x01\x01\0\0".as_slice()
    } else {
        b"\0\0\0".as_slice()
    });
    bytes.extend(b"\x05\x01\x01\x01\x01");
    if newer {
        bytes.extend(b"\x03\x03\x04\x05\x06\x07\x08\x03\x04\x05");
    } else {
        bytes.extend(b"\x02\x03\x04\x05\x03\x04\x05");
    }
    bytes.extend(b"\x01\0\0\xff\x0d\xc0\x1f\xff\xfd\x01\0\0\x04\x05\x01\x03\x01\x01\x01\x03\0\x01\0\0\0\x06\x06\x08\0");
    bytes
}

#[test]
fn primitive_cores_require_complete_wire_and_owning_classes() {
    for (kind, label, owner, key, mode, bytes) in [
        (
            "cone",
            "CONE",
            "UGS::CONE_PARMS",
            "cone.boolean_mode",
            "create",
            cone(),
        ),
        (
            "sphere",
            "SPHERE",
            "UGS::SPHERE_PARMS",
            "sphere.boolean_mode",
            "subtract",
            sphere(false),
        ),
        (
            "sphere",
            "SPHERE",
            "UGS::SPHERE_PARMS",
            "sphere.boolean_mode",
            "subtract",
            sphere(true),
        ),
    ] {
        let value = wire(kind, bytes.clone());
        let core: OperationCore = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(core.as_bytes(), bytes);
        assert_eq!(core.boolean_property(), Some((key, mode)));
        assert_eq!(serde_json::to_value(core).unwrap(), value);
        for end in 0..bytes.len() {
            assert!(
                serde_json::from_value::<OperationCore>(wire(kind, bytes[..end].to_vec())).is_err(),
                "{kind} prefix {end}"
            );
        }
        for (offset, replacement) in [(0, 0), (bytes.len() - 3, 7), (bytes.len() - 2, 255)] {
            let mut malformed = bytes.clone();
            malformed[offset] = replacement;
            assert!(serde_json::from_value::<OperationCore>(wire(kind, malformed)).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(serde_json::from_value::<OperationCore>(wire(kind, extra)).is_err());
        let mut classes: Vec<_> = [owner, "UGS::OM::MethodicObject", "UGS::CM_BREF_DATA"]
            .into_iter()
            .enumerate()
            .map(|(offset, name)| crate::om::TypeDefinition {
                offset,
                name,
                registry_tail: &[],
            })
            .collect();
        crate::test_support::with_decode_context(|ctx| {
            let record = OperationPayload::new(&bytes, 0, label).unwrap();
            assert!(core_for_decode(ctx, record, &classes).unwrap().is_some());
            for index in [0, 2] {
                let name = classes[index].name;
                classes[index].name = "UGS::OTHER";
                assert!(core_for_decode(ctx, record, &classes).unwrap().is_none());
                classes[index].name = name;
            }
        });
    }
}

#[test]
fn sphere_inherited_states_require_nonempty_endpoints_and_duplicate_agreement() {
    for newer in [false, true] {
        let bytes = sphere(newer);
        let at = bytes
            .windows(5)
            .position(|w| w == [1, 1, 1, 1, if newer { 3 } else { 2 }])
            .unwrap();
        let mut short = bytes.clone();
        short[at + 4] = 1;
        assert!(serde_json::from_value::<OperationCore>(wire("sphere", short)).is_err());
        let mut disagreement = bytes;
        disagreement[at + 5] = 9;
        assert!(serde_json::from_value::<OperationCore>(wire("sphere", disagreement)).is_err());
    }
}
