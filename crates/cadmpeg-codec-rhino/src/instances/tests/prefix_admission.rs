// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::chunks::{ArchiveVersion, BoundedReader, FramingError};
use crate::loss::Diagnostics;

#[test]
fn hex_character_refusal_precedes_unvisited_byte_suffix() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = crate::instances::hex(&ctx, &[0xab; 8193], "instance hex character")
        .expect_err("first character cannot execute") else {
        panic!("original character refusal");
    };
    assert_eq!(limit.operation, "instance hex character");
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!((limit.used, limit.additional), (1, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn hex_admits_only_actual_bytes_and_characters() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Two byte visits and four emitted ASCII characters.
    policy.limits.max_work_units = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert_eq!(crate::instances::hex(&ctx, &[0xab, 0x01], "instance hex character").unwrap(), "ab01");
    ctx.finish_session().expect("exact executed work fits");
}

#[test]
fn empty_hex_has_no_visits_and_preserves_original_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    assert_eq!(crate::instances::hex(&ctx, &[], "instance hex character").unwrap(), "");
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "instance original fuse").unwrap_err() else {
        panic!("original work refusal");
    };
    assert!(matches!(crate::instances::hex(&ctx, &[], "instance hex character"), Err(CodecError::ResourceLimit(limit)) if limit == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn empty_definition_scan_has_no_exhaustion_visit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let parsed = crate::instances::parse_definitions(&ctx, &[], &[], ArchiveVersion::V5, 0x1000_0021).unwrap();
    assert!(parsed.scan.definitions().is_empty());
    assert!(parsed.scan.diagnostics().is_empty());
    assert!(parsed.opaque_records.is_empty());
    drop(parsed);
    ctx.finish_session().expect("empty scan has no work");
}

#[test]
fn empty_linked_userdata_scan_has_no_exhaustion_visit_and_preserves_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut definition = super::support::static_definition([7; 16], &[]);
    definition.kind = crate::instances::DefinitionKind::Linked;
    assert!(!crate::instances::apply_idef_alternative_path(&ctx, &[], &[], ArchiveVersion::V5, &mut definition, &mut Diagnostics::new()).unwrap());
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "instance original fuse").unwrap_err() else {
        panic!("original work refusal");
    };
    assert!(matches!(crate::instances::apply_idef_alternative_path(&ctx, &[], &[], ArchiveVersion::V5, &mut definition, &mut Diagnostics::new()), Err(FramingError::Resource(limit)) if limit == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn empty_object_array_has_no_visits_and_preserves_original_fuse() {
    let data = 0_i32.to_le_bytes();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&data, &arena, &policy).unwrap();
    let mut reader = BoundedReader::new(&data, 0, data.len()).unwrap();
    crate::instances::skip_object_array(&ctx, &data, &mut reader, ArchiveVersion::V8, &mut Vec::new()).unwrap();
    assert_eq!(reader.position(), 4);
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "instance original fuse").unwrap_err() else {
        panic!("original work refusal");
    };
    let mut reader = BoundedReader::new(&data, 0, data.len()).unwrap();
    assert!(matches!(crate::instances::skip_object_array(&ctx, &data, &mut reader, ArchiveVersion::V8, &mut Vec::new()), Err(FramingError::Resource(limit)) if limit == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
