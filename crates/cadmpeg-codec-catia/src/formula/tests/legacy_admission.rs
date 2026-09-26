// SPDX-License-Identifier: Apache-2.0
//! Entity admission for the three legacy parameter packet forms.

use cadmpeg_ir::document::CadIr;

fn legacy_values() -> crate::native::CatiaNative {
    let mut bytes = Vec::new();
    for entity_id in [1_u32, 4, 9, 12, 13] {
        bytes.push(0xea);
        bytes.extend(entity_id.to_le_bytes());
        bytes.extend([0x81, 0xfd, 0x8c]);
        if entity_id == 4 {
            for (role, selector, value) in [
                ("body", vec![0x80, 4, 0, 0, 0], "#1_ + 2"),
                ("param", vec![0xd1, 8], "(#1_ : #In Real) : Real\n"),
            ] {
                bytes.push(u8::try_from(role.len() + 1).expect("short role"));
                bytes.extend(role.as_bytes());
                bytes.extend(selector);
                bytes.extend(b"\xe8\x00\x12\x01");
                bytes.push(u8::try_from(value.len() + 1).expect("short text"));
                bytes.extend(value.as_bytes());
                bytes.push(0xfe);
            }
        } else if entity_id == 9 {
            bytes.extend([8, b'p', b'a', b'r', b'a', b'm', b'i', b'n', 0x80]);
            bytes.extend(4134_u32.to_le_bytes());
            bytes.extend([0xe8, 0xe4, 0x0b, 0x01]);
            bytes.extend(b"\xfe\x84\x92\x82\x05Real\x83");
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 9]);
            bytes.extend(b"\xe8\x00\x12\x01\x07Result\xfe");
            bytes.extend(b"\xfe\x84\x88\x82\xfe\xe6");
            bytes.extend(3.5_f64.to_bits().to_le_bytes());
        } else if entity_id == 12 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 11]);
            bytes.extend(b"\xe8\x00\x12\x01\x0cResponsible\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x07String\x83");
            bytes.extend(b"\xfe\x85\x93\x82\xfe\x0cCilas Evans");
        } else if entity_id == 13 {
            bytes.extend([5, b'n', b'a', b'm', b'e', 0xd1, 12]);
            bytes.extend(b"\xe8\x00\x12\x01\x06Count\xfe");
            bytes.extend(b"\xfe\x84\x92\x82\x08Integer\x83");
            bytes.extend(b"\xfe\x85\x9d\x82\xfe\x8c");
        }
    }
    bytes.extend(b"\xde\x04\xfe\xfe\x12CATCatalogManager");
    bytes.extend(b"\xfe\xfe\xfe");
    bytes.extend([0x81, 0x04, b'F', b'o', b'o', 0x84, 0xfe]);
    bytes.extend(b"\x4e\x11\x00\x00\x00DASSAULT-SYSTEMES\x05\x00\x00\x00CATIA");
    let native = crate::native::CatiaNative::decode(&bytes);
    let [run] = native.legacy_entity_runs.as_slice() else {
        panic!("one complete legacy entity run");
    };
    assert_eq!(run.scalar_values.len(), 1);
    assert_eq!(run.string_values.len(), 1);
    assert_eq!(run.integer_values.len(), 1);
    native
}

fn assert_first_candidate_refused(native: &crate::native::CatiaNative) {
    crate::test_support::with_entity_limit(0, |ctx| {
        let mut ir = CadIr::empty();
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            crate::formula::transfer_parameters(
                ctx,
                &mut ir,
                native,
                &mut cadmpeg_ir::Annotations::default(),
                &crate::decode::ModelingGraphScope::Unscoped,
            )
        else {
            panic!("legacy candidate must exceed the zero entity limit");
        };
        assert_eq!(
            limit.dimension,
            cadmpeg_core::decode::ResourceDimension::Entities
        );
        assert_eq!(limit.used, 0);
        assert_eq!(limit.operation, "admit CATIA formula candidate");
        assert!(ir.model.parameters.is_empty());
    });
}

#[test]
fn formula_legacy_scalar_limit_refuses_before_candidate_creation() {
    let mut native = legacy_values();
    let run = &mut native.legacy_entity_runs[0];
    run.string_values.clear();
    run.integer_values.clear();
    assert_first_candidate_refused(&native);
}

#[test]
fn formula_legacy_string_limit_refuses_before_candidate_creation() {
    let mut native = legacy_values();
    let run = &mut native.legacy_entity_runs[0];
    run.scalar_values.clear();
    run.integer_values.clear();
    assert_first_candidate_refused(&native);
}

#[test]
fn formula_legacy_integer_limit_refuses_before_candidate_creation() {
    let mut native = legacy_values();
    let run = &mut native.legacy_entity_runs[0];
    run.scalar_values.clear();
    run.string_values.clear();
    assert_first_candidate_refused(&native);
}

#[test]
fn formula_legacy_candidates_transfer_under_service_profile() {
    let native = legacy_values();
    let mut ir = CadIr::empty();
    crate::test_support::with_service_context(|ctx| {
        crate::formula::transfer_parameters(
            ctx,
            &mut ir,
            &native,
            &mut cadmpeg_ir::Annotations::default(),
            &crate::decode::ModelingGraphScope::Unscoped,
        )
    })
    .expect("service profile admits three legacy candidates");
    assert_eq!(ir.model.parameters.len(), 3);
}
