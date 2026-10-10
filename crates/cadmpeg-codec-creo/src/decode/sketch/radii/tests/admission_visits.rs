// SPDX-License-Identifier: Apache-2.0

use crate::feature::definitions::{FeatureTrimEntity, FeatureTrimEntityTable, TrimEntityKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::mem::size_of;

fn assert_free_metadata_route(run: impl Fn(&DecodeContext<'_>) -> Result<bool, CodecError>) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    for refused in [false, true] {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let original = refused.then(|| ctx.charge_work_limit(1, "before radius metadata recovery").expect_err("zero work"));
        for _ in 0..2 {
            if let Some(original) = original {
                assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else { assert!(run(&ctx).expect("metadata requires no source work")); }
        }
        if let Some(original) = original {
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        } else { ctx.finish_session().expect("active free route"); }
    }
}

#[test]
fn absent_trim_ids_are_free_and_preserve_original_refusal() {
    let definition = super::arc_radius_definition([3.0, 3.0]);
    assert_free_metadata_route(|ctx| super::super::trim_segment_ids(ctx, &definition).map(|ids| ids.is_empty()));
}

#[test]
fn absent_arc_carrier_kind_is_free_and_preserves_original_refusal() {
    let mut segment = super::arc_carrier_segment();
    segment.kind = crate::feature::definitions::FeatureSegmentKind::Point(7);
    assert_free_metadata_route(|ctx| super::super::section_arc_carrier(
        ctx, &Default::default(), &Default::default(), &segment).map(|carrier| carrier.is_none()));
}

#[test]
fn absent_arc_carrier_fields_are_free_and_preserve_original_refusal() {
    let mut segment = super::arc_carrier_segment();
    segment.radius_ref = None;
    assert_free_metadata_route(|ctx| super::super::section_arc_carrier(
        ctx, &Default::default(), &Default::default(), &segment).map(|carrier| carrier.is_none()));
}

#[test]
fn absent_axis_carrier_kind_is_free_and_preserves_original_refusal() {
    let segment = super::arc_carrier_segment();
    assert_free_metadata_route(|ctx| super::super::section_axis_line_carrier_with_points(
        ctx, &Default::default(), &segment).map(|carrier| carrier.is_none()));
}

#[test]
fn absent_axis_carrier_selector_is_free_and_preserves_original_refusal() {
    let mut segment = super::arc_carrier_segment();
    segment.kind = crate::feature::definitions::FeatureSegmentKind::Line([1, 2]);
    assert_free_metadata_route(|ctx| super::super::section_axis_line_carrier_with_points(
        ctx, &Default::default(), &segment).map(|carrier| carrier.is_none()));
}

fn assert_absent_radius_relation(kind: u32) {
    let definition = super::arc_radius_definition([3.0, 3.0]);
    let relation = crate::feature::definitions::FeatureRelation {
        relation_id: 1, used: 0, operands: Vec::new(), operand_vectors: None,
        sign: 1, dimension_id: 0, relation_type: kind, body: Vec::new(), offset: 0,
    };
    assert_free_metadata_route(|ctx| super::super::section_radius_relation_arc(
        ctx, &definition, &relation).map(|carrier| carrier.is_none()));
}

#[test]
fn absent_radius_relation_kind_is_free_and_preserves_original_refusal() {
    assert_absent_radius_relation(0);
}

#[test]
fn absent_radius_relation_type5_dimension_is_free_and_preserves_original_refusal() {
    assert_absent_radius_relation(5);
}

#[test]
fn absent_radius_relation_type6_dimension_is_free_and_preserves_original_refusal() {
    assert_absent_radius_relation(6);
}

#[test]
fn trim_last_unmatched_row_keeps_its_unique_segment_without_an_empty_visit() {
    let mut definition = super::arc_radius_definition([3.0, 3.0]);
    definition.segments.as_mut().expect("segments").declared_count = 1;
    definition.trim_entities = Some(FeatureTrimEntityTable {
        declared_count: None, entity_ref: None, entry_ref: None, buckets: Vec::new(),
        rows: vec![FeatureTrimEntity {
            external_id: 99, mode: Some(0), vertices: [2, 3], kind: TrimEntityKind::Arc { center_vertex: 1 }, offset: 0,
        }], solved_external_ids: vec![99], offset: 0,
    });
    // Three singleton sorts each admit two roster visits and
    // (one value + two keys) * two levels * eight bytes of sort work.
    // Six other visits include the current core bucket/windows end probes;
    // these upstream generic semantics remain a separate shared request.
    // Two unique-candidate visits and three u32 comparisons complete the route.
    let scalar_bytes = u64::try_from(size_of::<u32>()).expect("u32 size");
    let sort_work = 2 + 3 * scalar_bytes * 2 * 8;
    let work = 3 * sort_work + 6 + 2 + 3 + 6 * scalar_bytes;
    for cap in [0, work - 1, work, work + 1] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let run = || super::super::trim_segment_ids(&ctx, &definition);
        let original = if cap >= work {
            assert_eq!(run().expect("unique missing partner admitted"), vec![Some(10)]);
            ctx.charge_work_limit(cap - work + 1, "after trim partner boundary").expect_err("exact work used")
        } else {
            let Err(CodecError::ResourceLimit(original)) = run() else { panic!("present work must refuse"); };
            if cap == work - 1 {
                assert_eq!((original.operation, original.used, original.additional),
                    ("creo trim segment IDs", work - scalar_bytes, scalar_bytes));
            }
            original
        };
        assert_eq!(original.dimension, ResourceDimension::WorkUnits);
        if cap >= work { assert_eq!(original.used, work); }
        for _ in 0..2 {
            assert!(matches!(run(), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == original));
    }
}
