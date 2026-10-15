// SPDX-License-Identifier: Apache-2.0
use super::{core_for_decode, inherited, wire, OperationCore, OperationPayload};

fn body(b: &mut Vec<u8>, endpoint: u8) {
    b.extend([
        1, 2, 11, 0xa0, 0x5e, 0x38, 0x97, 0x75, 1, 2, endpoint, 0xf1, 1, 0, 0xff,
    ]);
}
fn delete_face(branch: u8) -> Vec<u8> {
    let mut b = b"\x01\0\0\x01\0\0\x03\xf0\x10\x02\x2f\xf0\0\0\0\0\0\0\x03\x50\x44\0\0\x01\x46\x85\x1a\x01\x8f\xff\x01\x01\x02\x04\x02\x01\x02\x01\0\0\0\0\xf0\x11\0\0".to_vec();
    for _ in 0..2 {
        b.extend([1, 8, 1, 2, 3, 4, 5, 6, 7]);
    }
    b.extend(b"\x01\xf0\x12\xff\x01\x01\x01\0\0\x01\0\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0");
    body(&mut b, 3);
    b.extend(b"\0\0\0\x0b\0\xf0\x01\x02\x02\x01\x01\0\0\0\0\0\0\x10\x01\x02\x01");
    b.push(branch);
    if branch == 0 {
        b.extend([0, 0]);
    } else {
        b.extend([2, 17, 0, 9, 1, 1, 0, 0]);
    }
    b.extend(b"\x01\x02\x01\x01\x01\x03\x03\x04\x05\x06\x07\x08\x03\x04\x05\x01\0\0\xff\x1e\xc0\x1f\xff\xff\x01\0\0\x02\x03\x01\x03\x01\x01\x01\x01\0\x01\0\0\0\x04\x04\x05\0");
    b
}
fn thread(family: u8, secondary: bool) -> Vec<u8> {
    let mut b = b"\x01\0\0\x01\0\0\x01\x03\x01\x02".to_vec();
    for (prefix, count) in [
        (b"".as_slice(), 1),
        (b"\x01\x02", 1),
        (b"\x01\x03", 3),
        (b"\x01\x07", 6),
        (b"\x01\x07", 6),
    ] {
        b.extend(prefix);
        for _ in 0..count {
            b.extend([0xf0, 20]);
        }
    }
    b.extend(b"\0\x03\x02\0\x03\x03X\0\0\xf0\x15\x03\x05Cut\0\xff\xff\xf0\x16\xff\xff");
    b.extend([0; 10]);
    b.extend(b"\xff\xff\xff\xff\xff\xff\xff\xff\xff\0\0\0\0");
    body(&mut b, 2);
    b.extend(b"\0\0\0\x0b\0\xf0\x01\x02\x02\x01\x01\0\0\0\0\0");
    if secondary {
        b.extend(b"\x01\x02\x0b\x01\x97\x75");
    } else {
        b.push(0);
    }
    b.extend(inherited());
    b.extend([1, 2, 1, 3, 2, 1, family]);
    if secondary {
        b.extend([3, 0, 1, 0, 0, 0]);
    } else {
        b.extend([3, 0, 0, 1, 0, 0, 0]);
    }
    b.extend([4, 4, 5, 0]);
    b
}

#[test]
fn direct_and_physical_thread_cores_require_complete_storage_and_owning_classes() {
    #[derive(serde::Serialize)]
    struct Record {
        id: &'static str,
        core: OperationCore,
    }
    let mut cases = Vec::new();
    for branch in [0, 1] {
        cases.push((
            "delete_face",
            "DELETE FACE",
            delete_face(branch),
            vec![
                "UGS::ModlDirect::DeleteFaceParms",
                "UGS::ModlDirect::Constrain::ManagerOptions",
                "UGS::CM_BREF_DATA",
            ],
        ));
    }
    for secondary in [false, true] {
        for (family, kind, label) in [
            (2, "threads", "THREADS"),
            (1, "detailed_thread", "DETAILED_THREAD"),
        ] {
            cases.push((
                kind,
                label,
                thread(family, secondary),
                vec!["UGS::THD_PARMS", "UGS::CM_BREF_DATA"],
            ));
        }
    }
    for (kind, label, bytes, names) in cases {
        let value = wire(kind, bytes.clone());
        let core: OperationCore = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(core.as_bytes(), bytes);
        assert_eq!(core.boolean_property(), None);
        assert_eq!(serde_json::to_value(&core).unwrap(), value);
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &Record {
                id: "nx:feature:core#0",
                core,
            },
            serde_json::json!({"id":"nx:feature:core#0", "core":value}),
        );
        for end in 0..bytes.len() {
            assert!(
                serde_json::from_value::<OperationCore>(wire(kind, bytes[..end].to_vec())).is_err(),
                "{kind}: prefix {end}"
            );
        }
        for offset in [0, bytes.len() - 3] {
            let mut invalid = bytes.clone();
            invalid[offset] = 0;
            assert!(serde_json::from_value::<OperationCore>(wire(kind, invalid)).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(serde_json::from_value::<OperationCore>(wire(kind, extra)).is_err());
        let mut classes: Vec<_> = names
            .into_iter()
            .enumerate()
            .map(|(offset, name)| crate::om::TypeDefinition {
                offset,
                name,
                registry_tail: &[],
            })
            .collect();
        crate::test_support::with_decode_context(|ctx| {
            let payload = OperationPayload::new(&bytes, 0, label).unwrap();
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
}
