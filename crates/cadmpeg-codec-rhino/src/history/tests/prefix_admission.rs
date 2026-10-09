// SPDX-License-Identifier: Apache-2.0
//! History source traversal and empty-route refusal controls.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::{record, ArchiveVersion, Diagnostics};

#[test]
fn empty_history_record_scan_has_no_terminal_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut warnings = Diagnostics::new();
    let scan = super::super::parse_records(&ctx, b"", &[], ArchiveVersion::V8,
        &mut warnings, 0x1000_0026).expect("empty source has no visit");
    assert!(scan.records.is_empty());
    assert!(scan.opaque_records.is_empty());
    assert!(warnings.is_empty());
    drop(scan);
    ctx.finish_session().expect("zero work fits");
}

#[test]
fn empty_history_record_scan_keeps_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let original = ctx.charge_work(1, "history original refusal").expect_err("original fuse");
    let error = super::super::parse_records(&ctx, b"", &[], ArchiveVersion::V8,
        &mut Diagnostics::new(), 0x1000_0026).expect_err("empty source keeps the fuse");
    assert_eq!(error.to_string(), original.to_string());
    assert_eq!(ctx.finish_session().expect_err("original fuse").to_string(), original.to_string());
}

#[test]
fn history_projection_refuses_before_the_first_record_visit() {
    let records = vec![record(0, 1, &[], &[]); 8193];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let error = super::super::project(&ctx, &records, None,
        &mut ir, &mut Diagnostics::new()).expect_err("first source visit refuses");
    let super::super::ProjectionError::Codec(CodecError::ResourceLimit(limit)) = error else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "Rhino project traversal");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert!(ir.model.features.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn history_projection_first_key_refusal_leaves_later_records_unvisited() {
    // Nil UUIDs use the source offset. One visit precedes formatting the
    // seven-byte literal "offset-"; no later record has been visited.
    let records = vec![record(0, 1, &[], &[]); 8193];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).expect("context");
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let error = super::super::project(&ctx, &records, None,
        &mut ir, &mut Diagnostics::new()).expect_err("first identity key refuses");
    let super::super::ProjectionError::Codec(CodecError::ResourceLimit(limit)) = error else {
        panic!("work refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.operation, "Rhino history identity key");
    assert_eq!((limit.used, limit.additional), (1, 7));
    assert!(ir.model.features.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}
