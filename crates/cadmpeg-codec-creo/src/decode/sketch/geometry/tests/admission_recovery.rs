// SPDX-License-Identifier: Apache-2.0

use super::super::{
    saved_profile_chains, saved_section_arc_record, saved_section_circle_values,
    saved_section_segment_point_coordinates, section_arc_geometry, section_line_geometry,
    section_point_geometry, section_reference_line_geometry,
};
use crate::feature::definitions::{FeatureCircleSegment, FeatureReferenceLineSegment, FeatureSegmentKind};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometry, SketchGeometryDefinition, SketchId};
use std::collections::BTreeMap;
use std::mem::size_of;

const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;

fn assert_free_recovery<T>(run: impl Fn(&DecodeContext<'_>) -> Result<Option<T>, CodecError>) {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(run(&ctx).expect("no source work").is_none());
    let original = ctx.charge_work_limit(1, "before absent section geometry")
        .expect_err("zero work");
    assert!(matches!(run(&ctx), Err(CodecError::ResourceLimit(actual)) if actual == original));
}

#[test]
fn absent_section_line_kind_is_free_and_keeps_original_refusal() {
    let (_, mut segment) = super::saved_arc_carrier_definition([None; 3], None);
    segment.kind = FeatureSegmentKind::Point(7);
    assert_free_recovery(|ctx| section_line_geometry(ctx, &BTreeMap::new(), &segment));
}

#[test]
fn absent_section_point_kind_is_free_and_keeps_original_refusal() {
    let (_, segment) = super::saved_arc_carrier_definition([None; 3], None);
    assert_free_recovery(|ctx| section_point_geometry(ctx, &BTreeMap::new(), &segment));
}

#[test]
fn absent_section_arc_orientation_is_free_and_keeps_original_refusal() {
    let (_, mut segment) = super::saved_arc_carrier_definition([None; 3], None);
    segment.arc_orientation = None;
    assert_free_recovery(|ctx| section_arc_geometry(ctx, &BTreeMap::new(), &segment));
}

#[test]
fn absent_section_reference_endpoints_are_free_and_keep_original_refusal() {
    let segment = FeatureReferenceLineSegment {
        directions: [None; 3], point_ids: [None; 2], vertical_horizontal: None,
        external_id: 7, offset: 0,
    };
    assert_free_recovery(|ctx| section_reference_line_geometry(ctx, &BTreeMap::new(), &segment));
}

#[test]
fn absent_saved_arc_order_is_free_and_keeps_original_refusal() {
    let (mut definition, segment) = super::saved_arc_carrier_definition([None; 3], None);
    definition.order_table = None;
    assert_free_recovery(|ctx| saved_section_arc_record(ctx, &definition, &segment));
}

#[test]
fn saved_point_coordinate_recovery_is_free_and_keeps_original_refusal() {
    let (definition, mut segment) = super::saved_arc_carrier_definition([None; 3], None);
    segment.kind = FeatureSegmentKind::Point(7);
    assert_free_recovery(|ctx| saved_section_segment_point_coordinates(ctx, &definition, &segment));
}

#[test]
fn absent_saved_circle_segments_are_free_and_keep_original_refusal() {
    let (definition, _) = super::saved_arc_carrier_definition([None; 3], None);
    let segment = FeatureCircleSegment {
        center_id: 1, radius_ref: 2, external_id: 3, offset: 0,
    };
    assert_free_recovery(|ctx| saved_section_circle_values(ctx, &definition, &segment));
}

#[test]
fn saved_profile_endpoint_candidates_admit_only_present_rows() {
    let sketch = SketchId::mint("creo:model:sketch#917").expect("sketch identity");
    for count in 0_u64..=2 {
        let geometries: Vec<_> = (0..count).map(|index| {
            let position = f64::from(u32::try_from(index).expect("two rows")) * 4.0;
            (u32::try_from(index).expect("two rows"),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: cadmpeg_ir::math::Point2::new(position, 0.0),
                    end: cadmpeg_ir::math::Point2::new(position + 1.0, 0.0),
                }).expect("isolated line"))
        }).collect();
        // Three admit_iter calls precharge each complete source bound.
        // Each outer row then visits count candidates for both fixed endpoints.
        let phase_work = 3 * count + 2 * count * count;
        let expected: Vec<_> = (0..count).flat_map(|row| {
            (0..2).flat_map(move |endpoint| {
                (0..count).map(move |candidate| {
                    3 * count + 2 * count * row + count * endpoint + candidate
                })
            })
        }).collect();
        let mut observed = Vec::new();
        for cap in 0..=phase_work {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = saved_profile_chains(&ctx, &sketch, &geometries);
            if let Err(CodecError::ResourceLimit(original)) = result {
                assert_eq!(ctx.resource_refusal(), Some(original));
                if original.operation == "creo saved profile endpoint candidates" {
                    assert_eq!((original.dimension, original.used, original.additional),
                        (ResourceDimension::WorkUnits, cap, 1));
                    observed.push(cap);
                }
                assert!(matches!(saved_profile_chains(&ctx, &sketch, &geometries),
                    Err(CodecError::ResourceLimit(actual)) if actual == original));
            } else {
                assert!(result.expect("empty phase").is_empty());
                assert_eq!(count, 0);
            }
        }
        assert_eq!(observed, expected);
        assert!(crate::decode::with_test_decode_ctx(|ctx| {
            saved_profile_chains(ctx, &sketch, &geometries)
        }).expect("service isolated rows").is_empty());
    }
}

fn profile_backing_bytes() -> u64 {
    // collect_vec pushes the single circular use into four amortized slots.
    // The four-element line-use Vec and outer profiles Vec also retain four
    // slots each. Five identities retain their exact formatted text lengths.
    let text_bytes: usize = [30, 10, 11, 12, 13].into_iter()
        .map(|id| format!("creo:featdefs:sketch_entity#917:{id}").len()).sum();
    u64::try_from(text_bytes + 8 * size_of::<SketchEntityUse>()
        + 4 * size_of::<Vec<SketchEntityUse>>()).expect("profile backing bound")
}

fn assert_profile_identity(profiles: &[Vec<SketchEntityUse>]) {
    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 1);
    assert_eq!(profiles[0][0].entity.as_str(), "creo:featdefs:sketch_entity#917:30");
    assert!(!profiles[0][0].reversed);
    assert_eq!(profiles[1].len(), 4);
    for (usage, id) in profiles[1].iter().zip([10, 11, 12, 13]) {
        assert_eq!(usage.entity.as_str(), format!("creo:featdefs:sketch_entity#917:{id}"));
        assert!(!usage.reversed);
    }
}

#[test]
fn saved_profile_chain_transfer_keeps_only_surviving_use_backing() {
    let (sketch, geometries) = super::saved_profile_fixture();
    let bytes = profile_backing_bytes();
    for cap in 0..=bytes {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let result = saved_profile_chains(&ctx, &sketch, &geometries);
        if cap == bytes {
            assert_profile_identity(&result.expect("exact surviving backing"));
            let original = ctx.charge_retained_limit(1, "after retained profile")
                .expect_err("all retained backing consumed");
            assert_eq!((original.used, original.additional), (bytes, 1));
        } else {
            let original = ctx.resource_refusal().expect("smaller retained cap");
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
            assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
            assert!(matches!(saved_profile_chains(&ctx, &sketch, &geometries),
                Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
    }
}

#[test]
fn saved_profile_chain_transfer_preserves_actual_parent_overlap_and_cleanup() {
    let (sketch, geometries) = super::saved_profile_fixture();
    let bytes = profile_backing_bytes();
    for overflow in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        let mut parent = ctx.reserve_scoped(37, "saved profile parent").expect("parent");
        let profiles = parent.with_storage(|| saved_profile_chains(&ctx, &sketch, &geometries))
            .expect("profiles remain under actual parent");
        assert_profile_identity(&profiles);
        let available = EMPTY_ROOT_MATERIALIZED_BYTES - bytes - 37;
        let probe = ctx.reserve_scoped_limit(available + u64::from(overflow), "saved profile live backing");
        if overflow {
            let original = probe.expect_err("one byte above actual surviving backing");
            assert_eq!((original.dimension, original.used, original.additional, original.operation),
                (ResourceDimension::MaterializedBytes, bytes + 37, available + 1,
                    "saved profile live backing"));
            assert_eq!(ctx.resource_refusal(), Some(original));
        } else {
            drop(probe.expect("exact live backing"));
        }
        drop(profiles);
        drop(parent);
        if !overflow {
            drop(ctx.reserve_scoped(EMPTY_ROOT_MATERIALIZED_BYTES, "saved profile cleanup")
                .expect("all scratch and parent backing released"));
            let original = ctx.charge_retained_limit(1, "saved profile retained cleanup")
                .expect_err("retained zero policy");
            assert_eq!((original.used, original.additional), (0, 1));
        }
    }
}
