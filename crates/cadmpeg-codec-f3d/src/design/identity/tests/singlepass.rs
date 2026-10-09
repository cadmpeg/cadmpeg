// SPDX-License-Identifier: Apache-2.0

use crate::design::identity::{encode_component, neutral_configuration_id};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn identity_singlepass_preserves_literal_unicode_and_ascii_lowercase_bytes() {
    for (source, lowercase, expected) in [
        ("", false, ""),
        ("plain/ABç", false, "plain/ABç"),
        ("plain/ABÇç", true, "plain/abÇç"),
        (":#%", false, "%3A%23%25"),
        (" \t\n\u{a0}\u{2003}", false, "%20%09%0A%C2%A0%E2%80%83"),
        ("AZÇç:#%\t\n\u{a0}\u{2003}🧪", true,
         "azÇç%3A%23%25%09%0A%C2%A0%E2%80%83🧪"),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let encoded = encode_component(&ctx, source, lowercase, "encode identity control").unwrap();
        assert_eq!(encoded.text, expected);
        assert!(ctx.resource_refusal().is_none());
    }
}

#[test]
fn identity_singlepass_executes_only_reached_character_and_utf8_writes() {
    // First growth copies no existing bytes. Each source scalar costs one
    // step; each emitted UTF-8 byte costs one write. Known end costs nothing.
    for (source, expected, work) in [
        ("", "", 0),
        ("A", "A", 2),
        ("ç", "ç", 3),
        ("\u{2003}", "%E2%80%83", 10),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let encoded = encode_component(&ctx, source, false, "encode exact identity work").unwrap();
        assert_eq!(encoded.text, expected);
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "observe exact identity work") else {
            panic!("the exact work ceiling must be exhausted");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!((original.used, original.additional), (work, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
        drop(encoded);
    }
}

#[test]
fn identity_singlepass_refuses_before_first_write_and_unvisited_suffix() {
    let source = format!("A{}", ":#% \u{2003}".repeat(1024));
    for (work, used) in [(0, 0), (1, 1)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = work;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = if work == 0 { 0 } else { 1 };
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = neutral_configuration_id(&ctx, &source, "B") else {
            panic!("reached identity source/write must refuse");
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        assert_eq!(original.operation, "f3d configuration identifier");
        assert_eq!((original.used, original.additional), (used, 1));
        assert_eq!(ctx.resource_refusal(), Some(original));
        let Err(CodecError::ResourceLimit(later)) = ctx.charge_work(0, "after identity refusal") else {
            panic!("the original refusal must stay fused");
        };
        assert_eq!(later, original);
    }
}

#[test]
fn identity_singlepass_geometric_growth_has_real_overlap_and_move_work() {
    let source = "A".repeat(256);
    // Actual capacities are 1,2,4,...,256. Relocations copy 1+2+...+128
    // bytes. The largest real overlap is old128 plus new256.
    let moved: u64 = (0..8).map(|shift| 1u64 << shift).sum();
    let total_work = 256 * 2 + moved;
    assert_eq!(moved, 255);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = total_work;
    policy.limits.max_materialized_bytes = 128 + 256;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let encoded = encode_component(&ctx, &source, false, "identity geometric growth").unwrap();
    assert_eq!(encoded.text, source);
    assert_eq!(encoded.text.capacity(), 256);
    drop(encoded);
    drop(ctx.reserve_scoped(384, "released identity backing").unwrap());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "observe identity move work") else {
        panic!("exact source/write/move work is exhausted");
    };
    assert_eq!(original.dimension, ResourceDimension::WorkUnits);
    assert_eq!((original.used, original.additional), (total_work, 1));

    for (work, materialized, dimension, used, additional) in [
        (total_work - 1, 384, ResourceDimension::WorkUnits, total_work - 1, 1),
        (total_work, 383, ResourceDimension::MaterializedBytes, 256, 128),
    ] {
        let arena = DecodeArena::new();
        policy.limits.max_work_units = work;
        policy.limits.max_materialized_bytes = materialized;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = encode_component(&ctx, &source, false, "identity geometric growth") else {
            panic!("one below actual work or overlap must refuse");
        };
        assert_eq!(original.dimension, dimension);
        assert_eq!(original.operation, "identity geometric growth");
        assert_eq!((original.used, original.additional), (used, additional));
        assert_eq!(ctx.resource_refusal(), Some(original));
        let Err(CodecError::ResourceLimit(later)) = ctx.charge_work(0, "after identity growth refusal") else {
            panic!("growth refusal must remain fused");
        };
        assert_eq!(later, original);
    }
}

#[test]
fn identity_singlepass_component_owners_live_through_final_identity_copy() {
    const EXPECTED: &str = "f3d:configuration:variant#1:A1:B";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 2;
    policy.limits.max_retained_bytes = u64::try_from(EXPECTED.len()).unwrap();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let id = neutral_configuration_id(&ctx, "A", "B").unwrap();
    assert_eq!(id.as_str(), EXPECTED);
    drop(ctx.reserve_scoped(2, "both encoded components released").unwrap());
    let Err(CodecError::ResourceLimit(retained)) = ctx.charge_retained(1, "observe final identity retention") else {
        panic!("only the exact final identity consumes retained storage");
    };
    assert_eq!(retained.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((retained.used, retained.additional), (u64::try_from(EXPECTED.len()).unwrap(), 1));
    assert_eq!(id.as_str(), EXPECTED);

    let arena = DecodeArena::new();
    policy.limits.max_materialized_bytes = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(original)) = neutral_configuration_id(&ctx, "A", "B") else {
        panic!("second component cannot fit while first backing remains live");
    };
    assert_eq!(original.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(original.operation, "f3d configuration identifier");
    assert_eq!((original.used, original.additional), (1, 1));
    assert_eq!(ctx.resource_refusal(), Some(original));
}
