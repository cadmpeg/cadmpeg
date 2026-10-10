// SPDX-License-Identifier: Apache-2.0

use super::super::{
    admitted_scope_variant, bind_scope_constructions, bind_sketch_entity,
    named_parameter_scope_tail_is_valid, parse_parameter_scope, parse_scope_at,
    parse_scope_frame, reference_table,
};
use crate::design::decode::sketch::IndexedRecordOffsets;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::references::DesignClassTag;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ResourceLimit};
use cadmpeg_core::CodecError;

fn zero_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

fn original(ctx: &DecodeContext<'_>) -> ResourceLimit {
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "original scope entry refusal") else {
        panic!("the real zero-work context must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
    assert_eq!(limit.additional, 1);
    assert_eq!(ctx.resource_refusal(), Some(limit.clone()));
    limit
}

#[test]
fn scope_parse_entry_keeps_original_refusal_before_offset_gate() {
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let records = IndexedRecordOffsets::build(&ctx, &[]).unwrap();
    let class = DesignClassTag::try_from("378".to_owned()).unwrap();
    assert!(parse_parameter_scope(&ctx, &[], &records, 12, &class, u64::MAX).unwrap().is_none());
    assert!(ctx.resource_refusal().is_none());
    let original = original(&ctx);
    assert!(matches!(parse_parameter_scope(&ctx, &[], &records, 12, &class, u64::MAX), Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn scope_pair_entry_keeps_original_refusal_before_overflow_gate() {
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let records = IndexedRecordOffsets::build(&ctx, &[]).unwrap();
    assert!(parse_scope_at(&ctx, &[], &records, 12, *b"378", usize::MAX).unwrap().is_none());
    assert!(ctx.resource_refusal().is_none());
    let original = original(&ctx);
    assert!(matches!(parse_scope_at(&ctx, &[], &records, 12, *b"378", usize::MAX), Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn scope_frame_entry_keeps_original_refusal_before_missing_header_gate() {
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let records = IndexedRecordOffsets::build(&ctx, &[]).unwrap();
    assert!(parse_scope_frame(&ctx, &[], &records, 12, *b"378", 0, 0).unwrap().is_none());
    assert!(ctx.resource_refusal().is_none());
    let original = original(&ctx);
    assert!(matches!(parse_scope_frame(&ctx, &[], &records, 12, *b"378", 0, 0), Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn scope_reference_table_entry_keeps_original_refusal_before_incomplete_extent() {
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for (start, end) in [(usize::MAX, 0), (0, 14), (0, 40)] {
        assert!(reference_table(&ctx, &[], start, end).unwrap().is_none());
    }
    assert!(ctx.resource_refusal().is_none());
    let original = original(&ctx);
    for (start, end) in [(usize::MAX, 0), (0, 14), (0, 40)] {
        assert!(matches!(reference_table(&ctx, &[], start, end), Err(CodecError::ResourceLimit(later)) if later == original));
    }
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn scope_named_tail_entry_keeps_original_refusal_before_label_offset_overflow() {
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(named_parameter_scope_tail_is_valid(&ctx, &[], usize::MAX, 0, 0).unwrap().is_none());
    assert!(ctx.resource_refusal().is_none());
    let original = original(&ctx);
    assert!(matches!(named_parameter_scope_tail_is_valid(&ctx, &[], usize::MAX, 0, 0), Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn scope_sketch_binding_entry_keeps_original_refusal_before_family_or_frame_gate() {
    for kind in [DesignFeatureKind::Chamfer, DesignFeatureKind::Sketch] {
        let mut scope = DesignParameterScope::empty("f3d:stream:design-parameter-scope#100", kind, 42);
        let native = scope.clone();
        let arena = DecodeArena::new();
        let policy = zero_policy();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut index = None;
        bind_sketch_entity(&ctx, &[], &mut scope, "f3d:stream", &[], &mut index).unwrap();
        assert_eq!(scope, native);
        assert!(index.is_none());
        assert!(ctx.resource_refusal().is_none());
        let original = original(&ctx);
        assert!(matches!(bind_sketch_entity(&ctx, &[], &mut scope, "f3d:stream", &[], &mut index), Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(scope, native);
        assert!(index.is_none());
        assert_eq!(ctx.resource_refusal(), Some(original));
    }
}

#[test]
fn scope_construction_entry_keeps_original_refusal_before_unbound_family() {
    let mut scope = DesignParameterScope::empty("f3d:stream:design-parameter-scope#100", DesignFeatureKind::SurfaceTrim, 42);
    let native = scope.clone();
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let records = IndexedRecordOffsets::build(&ctx, &[]).unwrap();
    let types = std::collections::HashMap::new();
    let source = crate::native::F3dNative::default();
    bind_scope_constructions(&ctx, &[], &records, &mut scope, &types, &source).unwrap();
    assert_eq!(scope, native);
    assert!(ctx.resource_refusal().is_none());
    let original = original(&ctx);
    assert!(matches!(bind_scope_constructions(&ctx, &[], &records, &mut scope, &types, &source), Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(scope, native);
    assert_eq!(ctx.resource_refusal(), Some(original));
}

#[test]
fn scope_variant_entry_keeps_original_refusal_before_empty_or_single_group() {
    let scopes = [DesignParameterScope::empty("f3d:stream:design-parameter-scope#100", DesignFeatureKind::Chamfer, 42)];
    let native = scopes.clone();
    let arena = DecodeArena::new();
    let policy = zero_policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for group in [&[][..], &[("f3d:stream", 42, 0)][..]] {
        assert!(admitted_scope_variant(&ctx, &scopes, group, &[]).unwrap().is_none());
    }
    assert!(ctx.resource_refusal().is_none());
    let original = original(&ctx);
    for group in [&[][..], &[("f3d:stream", 42, 0)][..]] {
        assert!(matches!(admitted_scope_variant(&ctx, &scopes, group, &[]), Err(CodecError::ResourceLimit(later)) if later == original));
    }
    assert_eq!(scopes, native);
    assert_eq!(ctx.resource_refusal(), Some(original));
}
