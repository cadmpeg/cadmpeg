// SPDX-License-Identifier: Apache-2.0
use crate::entities::annotation::{
    dimension_children_valid, general_note_text_valid_for_global_table,
    general_note_valid_for_global_table, general_symbol_note_valid, leader_valid_for_global_table,
    new_general_note_valid, pointer, sectioned_area_valid, witness_valid, AnnotationValidation,
    SectionedAreaContext, SectionedAreaGeometryCache,
};
use crate::global::GlobalTable;
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::CadIr;
use std::collections::BTreeMap;

fn empty_record() -> ParameterRecord {
    ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), 0, Vec::new(), Vec::new())
}

enum CachedRoute {
    Coplanarity,
    Symbol,
    Area,
}

// Fixed recovery leaves both cache owners empty.
fn cached_fixed_route_is_free(route: &CachedRoute) {
    let ir = CadIr::empty();
    let record = ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        2,
        vec![
            Token {
                value: TokenValue::Integer(228),
                span: 0..0,
            },
            Token {
                value: TokenValue::Integer(0),
                span: 0..0,
            },
        ],
        Vec::new(),
    );
    {
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
        let mut geometry =
            SectionedAreaGeometryCache::new(&ir, &ctx).expect("empty geometry owner");

        for _ in 0..64 {
            let result = match route {
                CachedRoute::Coplanarity => geometry.curve_coplanar(
                    1,
                    (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
                    -1.0,
                    &ctx,
                ),
                CachedRoute::Symbol => general_symbol_note_valid(
                    &record,
                    &BTreeMap::new(),
                    &BTreeMap::new(),
                    0,
                    GlobalTable::V5Later,
                    &mut validation,
                ),
                CachedRoute::Area => sectioned_area_valid(
                    &mut geometry,
                    &record,
                    &BTreeMap::new(),
                    2,
                    SectionedAreaContext {
                        global_table: GlobalTable::V5Later,
                        transform: Transform::identity(),
                        length_factor: 1.0,
                        resolution: 0.0,
                    },
                    &ctx,
                ),
            };
            assert_eq!(
                result.expect("fixed cached recovery is free"),
                matches!(route, CachedRoute::Symbol)
            );
            assert!(validation.primary.is_empty());
            assert!(validation.width_sums.is_empty());
            assert!(geometry.index.is_none());
            assert!(geometry.proofs.is_empty());
        }
        drop(geometry);
        drop(validation);
        ctx.finish_session().expect("fresh fixed routes finish");
    }
}

fn with_zero_context(mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>)) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    for _ in 0..64 {
        run(&ctx);
    }
    ctx.finish_session().unwrap();
}

#[test]
fn annotation_missing_pointer_has_no_reference_or_work() {
    let record = empty_record();
    with_zero_context(|ctx| {
        for index in [0, 1, usize::MAX] {
            let result = pointer(&record, index, &BTreeMap::new(), ctx);
            assert!(matches!(result, Ok(None)));
        }
    });
}

#[test]
fn annotation_default_symbol_note_is_valid_without_cache_storage() {
    cached_fixed_route_is_free(&CachedRoute::Symbol);
}

#[test]
fn annotation_empty_children_probe_once_and_are_invalid_without_work() {
    let parent = crate::test_support::directory_target(1, 202);
    with_zero_context(|ctx| {
        let mut probes = 0;
        let children = std::iter::from_fn(|| {
            probes += 1;
            None::<Result<Option<u32>, CodecError>>
        });
        let result = dimension_children_valid(&parent, children, &BTreeMap::new(), ctx);

        assert!(matches!(result, Ok(false)));
        assert_eq!(probes, 1);
    });
}

#[test]
fn annotation_empty_general_note_is_invalid_without_work() {
    let record = empty_record();
    with_zero_context(|ctx| {
        let result = general_note_valid_for_global_table(
            &record,
            &BTreeMap::new(),
            GlobalTable::V5Later,
            0,
            ctx,
        );
        assert!(matches!(result, Ok(false)));
    });
}

#[test]
fn annotation_empty_leader_is_invalid_without_work() {
    let record = empty_record();
    let entry = crate::test_support::directory_target(1, 214);
    with_zero_context(|ctx| {
        let result = leader_valid_for_global_table(&entry, &record, GlobalTable::V5Later, ctx);
        assert!(matches!(result, Ok(false)));
    });
}

#[test]
fn annotation_empty_new_note_is_invalid_without_work() {
    let record = empty_record();
    with_zero_context(|ctx| {
        let result = new_general_note_valid(&record, &BTreeMap::new(), ctx);
        assert!(matches!(result, Ok(false)));
    });
}

#[test]
fn annotation_empty_witness_is_invalid_without_work() {
    let record = empty_record();
    with_zero_context(|ctx| {
        let result = witness_valid(&record, ctx);
        assert!(matches!(result, Ok(false)));
    });
}

#[test]
fn annotation_fixed_text_recovery_returns_table_results_without_work() {
    with_zero_context(|ctx| {
        for (text, font, table, null, expected) in [
            (b"text".as_slice(), 1, GlobalTable::V5Later, false, true),
            (b"2121".as_slice(), 2001, GlobalTable::V4_0, false, false),
            (b" ".as_slice(), 2001, GlobalTable::V5_0, true, true),
            (b"bad".as_slice(), 2001, GlobalTable::V5Later, false, false),
        ] {
            let result = general_note_text_valid_for_global_table(text, font, table, null, ctx);
            assert_eq!(result.expect("fixed text recovery is free"), expected);
        }
    });
}

#[test]
fn annotation_invalid_resolution_rejects_without_work() {
    cached_fixed_route_is_free(&CachedRoute::Coplanarity);
}

#[test]
fn annotation_unsupported_area_is_invalid_without_cache_storage() {
    cached_fixed_route_is_free(&CachedRoute::Area);
}
