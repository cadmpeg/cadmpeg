// SPDX-License-Identifier: Apache-2.0
//! Codec-native validation composes placement frame admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::CodecBackend;
use cadmpeg_ir::codec::DecodeOptions;
use cadmpeg_ir::native::bytes::NativeBytes;
use std::io::Cursor;

#[test]
fn placement_native_validator_rejects_matrix_that_disagrees_with_source_bytes() {
    let mut bytes = vec![0xff, 0, 0x45, 1, 4, 1];
    bytes.extend([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]);
    bytes.extend([0, 0, 0, 1, 63, 0xf0, 0, 0, 0]);
    bytes.extend(b"\x03\x03A\0\x03\x2601234567-89ab-cdef-0123-456789abcdef\0");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut wire = serde_json::json!({
        "id":"nx:assembly:placement#123", "raw_frame":NativeBytes::new(bytes.as_slice()),
        "matrix":[1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0.],
        "label":"A", "uuid":"01234567-89ab-cdef-0123-456789abcdef",
        "source_entry":"/Root/UG_PART/UG_PART", "source_offset":123,
    });
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "assembly_placements", &[&wire])
        .unwrap();
    assert!(crate::NxCodec::validate_native(&ctx, &ir, None)
        .unwrap()
        .is_empty());
    wire["matrix"][3] = serde_json::json!(1.0);
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "assembly_placements", &[&wire])
        .unwrap();
    let findings = crate::NxCodec::validate_native(&ctx, &ir, None).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].check,
        cadmpeg_ir::report::check::Check::NativeLinks
    );
    assert_eq!(findings[0].severity, cadmpeg_ir::report::Severity::Error);
    assert!(findings[0].message.contains("fields must match"));
}

#[test]
fn placement_native_validator_checks_its_bounded_object_reference() {
    let mut bytes = vec![0xff, 0, 0x45, 1, 4, 1];
    bytes.extend([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0]);
    bytes.extend([0, 0, 0, 1, 63, 0xf0, 0, 0, 0]);
    bytes.extend(b"\x03\x03A\0\x03\x2601234567-89ab-cdef-0123-456789abcdef\0");
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let placement = serde_json::json!({
        "id":"nx:assembly:placement#123", "raw_frame":NativeBytes::new(bytes.as_slice()),
        "matrix":[1.,0.,0.,0., 0.,1.,0.,0., 0.,0.,1.,0.],
        "label":"A", "uuid":"01234567-89ab-cdef-0123-456789abcdef",
        "object_record":"nx:om-record:entry#1", "source_entry":"/Root/UG_PART/UG_PART", "source_offset":123,
    });
    let mut object = serde_json::json!({
        "id":"nx:om-record:entry#1", "object_id":17, "object_id_source_offset":16,
        "section_ordinal":0, "record_ordinal":0, "section_offset":0,
        "byte_len":bytes.len(), "sha256":"0000000000000000000000000000000000000000000000000000000000000000",
        "source_entry":"/Root/UG_PART/UG_PART", "source_offset":123,
    });
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "assembly_placements", &[&placement])
        .unwrap();
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "object_records", &[&object])
        .unwrap();
    assert!(crate::NxCodec::validate_native(&ctx, &ir, None)
        .unwrap()
        .is_empty());
    object["source_offset"] = serde_json::json!(124);
    ir.native
        .namespace_mut("nx")
        .set_arena(&ctx, "object_records", &[&object])
        .unwrap();
    assert_eq!(
        crate::NxCodec::validate_native(&ctx, &ir, None)
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn decode_binds_distinct_placements_to_their_bounded_object_records() {
    let frame = |x: u8| {
        let mut bytes = vec![0xff, 0, 0x45, 1, 4, 1];
        bytes.extend([1, 0, 0, x, 0, 1, 0, 0, 0, 0, 1, 0]);
        bytes.extend([0, 0, 0, 1, 63, 0xf0, 0, 0, 0]);
        bytes.extend(b"\x03\x03A\0\x03\x2601234567-89ab-cdef-0123-456789abcdef\0");
        bytes
    };
    let mut section = vec![
        0xff, 0xff, 0xff, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, b'O', b'M', 1, 1,
    ];
    let index = section.len();
    section.extend_from_slice(&[0; 20]);
    section.extend_from_slice(&4_u32.to_le_bytes());
    for id in [0_u32, 1234, 73, 42] {
        section.extend_from_slice(&id.to_le_bytes());
    }
    section.extend_from_slice(b"\x06\x01\x05NX \0");
    let first = section.len();
    section.extend(frame(0));
    let second = section.len();
    section.extend(frame(1));
    let end = section.len();
    for (slot, position) in [first, first, second, end].into_iter().enumerate() {
        let value = u32::try_from(position - 14).unwrap();
        let at = index + 4 * (slot + 1);
        section[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let payload_len = u32::try_from(section.len() - 16).unwrap();
    section[8..12].copy_from_slice(&payload_len.to_be_bytes());
    let file = crate::test_support::test_prt::prt_with_named_payloads(&[(
        "/Root/UG_PART/UG_PART",
        section,
    )]);
    let result = cadmpeg_ir::codec::Codec::decode(
        &crate::NxCodec,
        &mut Cursor::new(file),
        &DecodeOptions::default(),
    )
    .unwrap();
    let namespace = result.ir().native.namespace("nx").unwrap();
    let placements: Vec<serde_json::Value> = namespace.arena_as("assembly_placements").unwrap();
    let records: Vec<serde_json::Value> = namespace.arena_as("object_records").unwrap();
    assert_eq!(placements.len(), 2);
    for (expected_id, expected_x) in [(73_u64, 0.0), (42, 1.0)] {
        let record = records
            .iter()
            .find(|record| record["object_id"].as_u64() == Some(expected_id))
            .unwrap();
        let target = record["id"].as_str().unwrap();
        let placement = placements
            .iter()
            .find(|placement| placement["object_record"].as_str() == Some(target))
            .unwrap();
        assert!((placement["matrix"][3].as_f64().unwrap() - expected_x).abs() < 1e-12);
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(crate::NxCodec::validate_native(&ctx, result.ir(), None)
        .unwrap()
        .is_empty());
}
