// SPDX-License-Identifier: Apache-2.0
use crate::entities::annotation::{dimension_children_valid, general_note_text_valid_for_global_table,
    general_note_valid_for_global_table, leader_valid_for_global_table, new_general_note_valid,
    pointer, witness_valid, general_symbol_note_valid, sectioned_area_valid,
    AnnotationValidation, SectionedAreaContext, SectionedAreaGeometryCache};
use crate::global::GlobalTable;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

fn empty_record() -> ParameterRecord {
    ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), 0, Vec::new(), Vec::new())
}

#[test]
fn annotation_fixed_text_recovery_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        for (text, font, table, null, expected) in [
            (b"text".as_slice(), 1, GlobalTable::V5Later, false, true),
            (b"2121".as_slice(), 2001, GlobalTable::V4_0, false, false),
            (b" ".as_slice(), 2001, GlobalTable::V5_0, true, true),
            (b"bad".as_slice(), 2001, GlobalTable::V5Later, false, false),
        ] {
            let result = general_note_text_valid_for_global_table(text, font, table, null, ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert_eq!(result.expect("fixed text recovery is free"), expected),
            }
        }
    });
}

#[test]
fn annotation_empty_general_note_preserves_original_refusal() {
    let record = empty_record();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = general_note_valid_for_global_table(&record, &BTreeMap::new(), GlobalTable::V5Later, 0, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(false))),
        }
    });
}

#[test]
fn annotation_empty_new_note_preserves_original_refusal() {
    let record = empty_record();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = new_general_note_valid(&record, &BTreeMap::new(), ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(false))),
        }
    });
}

#[test]
fn annotation_empty_leader_preserves_original_refusal() {
    let record = empty_record();
    let entry = crate::test_support::directory_target(1, 214);
    crate::test_support::with_entry_context(|ctx, original| {
        let result = leader_valid_for_global_table(&entry, &record, GlobalTable::V5Later, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(false))),
        }
    });
}

#[test]
fn annotation_empty_witness_preserves_original_refusal() {
    let record = empty_record();
    crate::test_support::with_entry_context(|ctx, original| {
        let result = witness_valid(&record, ctx);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert!(matches!(result, Ok(false))),
        }
    });
}

#[test]
fn annotation_absent_pointer_preserves_original_refusal() {
    let record = empty_record();
    crate::test_support::with_entry_context(|ctx, original| {
        for index in [0, 1, usize::MAX] {
            let result = pointer(&record, index, &BTreeMap::new(), ctx);
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(matches!(result, Ok(None))),
            }
        }
    });
}

#[test]
fn annotation_empty_children_preserve_original_refusal_before_probe() {
    let parent = crate::test_support::directory_target(1, 202);
    crate::test_support::with_entry_context(|ctx, original| {
        let mut probes = 0;
        let children = std::iter::from_fn(|| {
            probes += 1;
            None::<Result<Option<u32>, CodecError>>
        });
        let result = dimension_children_valid(&parent, children, &BTreeMap::new(), ctx);
        match original {
            Some(first) => {
                assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(probes, 0);
            }
            None => {
                assert!(matches!(result, Ok(false)));
                assert_eq!(probes, 1);
            }
        }
    });
}

enum CachedRoute {
    Coplanarity,
    Symbol,
    Area,
}

// Construct each owner in the caller session before inducing its refusal.
fn cached_route_preserves_original_refusal(route: CachedRoute) {
    let ir = CadIr::empty();
    let record = ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), 2,
        vec![Token { value: TokenValue::Integer(228), span: 0..0 },
            Token { value: TokenValue::Integer(0), span: 0..0 }], Vec::new());
    for dimension in [None, Some(ResourceDimension::WorkUnits), Some(ResourceDimension::CollectionItems),
        Some(ResourceDimension::MaterializedBytes), Some(ResourceDimension::RetainedBytes),
        Some(ResourceDimension::Entities), Some(ResourceDimension::RecursionDepth)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
        let mut validation = AnnotationValidation::new(&ctx).expect("empty validation owner");
        let mut geometry = SectionedAreaGeometryCache::new(&ir, &ctx).expect("empty geometry owner");
        let original = dimension.map(|dimension| {
            let result = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "annotation original refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "annotation original refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "annotation original refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "annotation original refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "annotation original refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("annotation original refusal").map(|_| ()),
                _ => panic!("test dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = result else { panic!("original refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        for _ in 0..64 {
            let result = match route {
                CachedRoute::Coplanarity => geometry.curve_coplanar(1,
                    (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)), -1.0, &ctx),
                CachedRoute::Symbol => general_symbol_note_valid(&record, &BTreeMap::new(),
                    &BTreeMap::new(), 0, GlobalTable::V5Later, &mut validation),
                CachedRoute::Area => sectioned_area_valid(&mut geometry, &record, &BTreeMap::new(),
                    2, SectionedAreaContext { global_table: GlobalTable::V5Later,
                        transform: Transform::identity(), length_factor: 1.0, resolution: 0.0 }, &ctx),
            };
            match original {
                Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert_eq!(result.expect("fixed cached recovery is free"), matches!(route, CachedRoute::Symbol)),
            }
            assert!(validation.primary.is_empty());
            assert!(validation.width_sums.is_empty());
            assert!(geometry.index.is_none());
            assert!(geometry.proofs.is_empty());
        }
        drop(geometry);
        drop(validation);
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().expect("fresh fixed routes finish"),
        }
    }
}

#[test]
fn annotation_invalid_resolution_preserves_original_refusal() {
    cached_route_preserves_original_refusal(CachedRoute::Coplanarity);
}

#[test]
fn annotation_default_symbol_note_preserves_original_refusal() {
    cached_route_preserves_original_refusal(CachedRoute::Symbol);
}

#[test]
fn annotation_unsupported_area_preserves_original_refusal() {
    cached_route_preserves_original_refusal(CachedRoute::Area);
}
