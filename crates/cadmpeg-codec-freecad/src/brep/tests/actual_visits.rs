// SPDX-License-Identifier: Apache-2.0
//! BREP prefix visits, row allocation order and known token exhaustion.

use super::super::{grid_rows, parse_reference_suffix, TokenCursor};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn grid_row_allocation_refuses_before_unmoved_values() {
    for (collection_limit, work_limit, used) in [(3, 0, 2), (5, 2, 4)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = collection_limit;
        policy.limits.max_work_units = work_limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = grid_rows(&ctx, vec![1, 2, 3, 4], 2)
            else { panic!("row storage must refuse before its values move") };
        assert_eq!(original.dimension, ResourceDimension::CollectionItems);
        assert_eq!(original.operation, "FreeCAD B-rep surface row values");
        assert_eq!(original.used, used);
        assert_eq!(original.additional, 2);
        assert!(matches!(grid_rows::<u8>(&ctx, Vec::new(), 0),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn grid_rows_charge_each_moved_value_once_and_keep_order() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert_eq!(grid_rows(&ctx, vec![1, 2, 3, 4], 2).unwrap(), [[1, 2], [3, 4]]);
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "after row moves")
        else { panic!("four moves exhaust the work budget") };
    assert_eq!(limit.used, 4);
}

#[test]
fn empty_and_invalid_grid_dimensions_need_no_work_and_keep_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(grid_rows::<u8>(&ctx, Vec::new(), 2).unwrap().is_empty());
    assert!(matches!(grid_rows(&ctx, vec![1_u8], 2), Err(CodecError::Malformed(message))
        if message == "surface grid dimensions do not match pole count"));
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "prior grid refusal")
        else { panic!("work refusal") };
    for width in [0, 2] {
        assert!(matches!(grid_rows::<u8>(&ctx, Vec::new(), width),
            Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}

#[test]
fn invalid_reference_prefix_does_not_scan_or_copy_suffix() {
    for token in ["0a".to_owned(), format!("0{}", "a".repeat(4096))] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // One token visit, the digit and first suffix byte, then one parsed digit.
        policy.limits.max_work_units = 4;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let tokens = [token.as_str()];
        let mut cursor = TokenCursor::new(&ctx, &tokens);
        assert!(matches!(parse_reference_suffix(&mut cursor, "test suffix", 1),
            Err(CodecError::Malformed(message)) if message == "test suffix limit exceeded"));
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "after reference prefix")
            else { panic!("only the visited prefix exhausts work") };
        assert_eq!(limit.used, 4);
    }
}

#[test]
fn reference_prefix_keeps_utf8_suffix_and_digit_only_exhaustion() {
    for (token, maximum, work, expected) in [
        ("1α", 1, 6, (1, Some("α".to_owned()))),
        ("12", 12, 5, (12, None)),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Token visit + visited prefix bytes + parsed digits + retained suffix bytes.
        policy.limits.max_work_units = work;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let tokens = [token];
        let mut cursor = TokenCursor::new(&ctx, &tokens);
        assert_eq!(parse_reference_suffix(&mut cursor, "test suffix", maximum).unwrap(), expected);
        assert_eq!(ctx.resource_refusal(), None);
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "after valid reference")
            else { panic!("the executed scans and copy exhaust work") };
        assert_eq!(limit.used, work);
    }
}

#[test]
fn empty_token_cursor_returns_truncation_without_work_and_preserves_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cursor = TokenCursor::new(&ctx, &[]);
    assert!(matches!(cursor.next("test token"), Err(CodecError::Malformed(message))
        if message == "truncated test token in text B-rep Curves table"));
    assert_eq!(ctx.resource_refusal(), None);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "prior token refusal")
        else { panic!("work refusal") };
    assert!(matches!(cursor.next("test token"),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn token_cursor_charges_before_each_actual_visit_and_keeps_order() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let tokens = ["first", "second"];
    let mut cursor = TokenCursor::new(&ctx, &tokens);
    assert_eq!(cursor.next("test token").unwrap(), "first");
    let Err(CodecError::ResourceLimit(original)) = cursor.next("test token")
        else { panic!("the second token visit must refuse") };
    assert_eq!(original.operation, "FreeCAD text B-rep token");
    assert_eq!(original.used, 1);
    assert_eq!(original.additional, 1);
    assert_eq!(cursor.peek(), Some("second"));
    assert!(matches!(cursor.next("test token"),
        Err(CodecError::ResourceLimit(actual)) if actual == original));
}
