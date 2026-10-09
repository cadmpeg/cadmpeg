// SPDX-License-Identifier: Apache-2.0

use super::super::super::{cluster_boundary_positions, split_homogeneous_pcurve,
    BoundaryVertexCreationError};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::PositiveReal;

fn separated_points() -> [FinitePoint3; 3] {
    [0.0, 10.0, 20.0].map(|x| FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap())
}

fn cluster_source_refusal(work: u64, operation: &'static str) {
    let points = separated_points();
    let tolerance = PositiveReal::ONE;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(first))) =
        cluster_boundary_positions(&points, tolerance, &ctx) else {
        panic!("expected one boundary-cluster source visit refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
    for replay in [points.as_slice(), &[]] {
        assert!(matches!(cluster_boundary_positions(replay, tolerance, &ctx),
            Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(last)))
                if last == first));
    }
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn cluster_position_source_refuses_one_visit_after_parent_and_size_initialization() {
    // Three parent indices and three filled size slots precede this source.
    cluster_source_refusal(3 + 3, "iges boundary clustering positions");
}

#[test]
fn cluster_comparison_source_refuses_one_pair_after_one_position() {
    cluster_source_refusal(3 + 3 + 1, "iges boundary clustering comparisons");
}

#[test]
fn cluster_membership_source_refuses_one_visit_after_all_separated_pairs() {
    // No close pair invokes a root walk: three position and three pair visits.
    cluster_source_refusal(3 + 3 + 3 + 3, "iges boundary cluster membership traversal");
}

#[test]
fn cluster_sources_preserve_separated_members_representatives_and_order() {
    let points = separated_points();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let clusters = cluster_boundary_positions(&points, PositiveReal::ONE, &ctx).unwrap();
    assert_eq!(clusters.len(), points.len());
    for (index, cluster) in clusters.iter().enumerate() {
        assert_eq!(cluster.members, [index]);
        assert_eq!(cluster.representative, points[index]);
    }
    assert!(cluster_boundary_positions(&[], PositiveReal::ONE, &ctx).unwrap().is_empty());
    ctx.finish_session().unwrap();
}

fn controls() -> [[f64; 4]; 4] {
    [[1.0, 0.0, 0.0, 0.0], [1.0, 1.0, 0.0, 0.0],
        [1.0, 2.0, 0.0, 0.0], [1.0, 3.0, 0.0, 0.0]]
}

fn split_source_refusal(work: u64, operation: &'static str, additional: u64) {
    let controls = controls();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = split_homogeneous_pcurve(&controls, 0.5, &ctx) else {
        panic!("expected pcurve source or interpolation refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, additional));
    assert!(matches!(split_homogeneous_pcurve(&controls, 0.5, &ctx),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn pcurve_split_source_refuses_one_visit_after_four_control_copies() {
    split_source_refusal(4, "iges pcurve split traversal", 1);
}

#[test]
fn pcurve_split_interpolation_refuses_after_one_source_visit_without_admitting_later_levels() {
    // The first interpolation level completes three infallible arithmetic slots.
    split_source_refusal(4 + 1, "iges pcurve split interpolation", 3);
}

#[test]
fn pcurve_split_source_preserves_both_sides_with_exact_current_operation_bounds() {
    let controls = controls();
    let reversal = controls.len()
        + 3 * std::mem::size_of_val(&controls);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // Four copies, three level visits, six interpolation slots, and the core
    // reversal bound of one visit plus three inline moves per returned slot.
    policy.limits.max_work_units = u64::try_from(4 + 3 + 3 + 2 + 1 + reversal).unwrap();
    policy.limits.max_materialized_bytes = u64::try_from(std::mem::size_of_val(&controls)).unwrap();
    policy.limits.max_retained_bytes = 2 * policy.limits.max_materialized_bytes;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (left, right) = split_homogeneous_pcurve(&controls, 0.5, &ctx).unwrap().unwrap();
    assert_eq!(left, [[1.0, 0.0, 0.0, 0.0], [1.0, 0.5, 0.0, 0.0],
        [1.0, 1.0, 0.0, 0.0], [1.0, 1.5, 0.0, 0.0]]);
    assert_eq!(right, [[1.0, 1.5, 0.0, 0.0], [1.0, 2.0, 0.0, 0.0],
        [1.0, 2.5, 0.0, 0.0], [1.0, 3.0, 0.0, 0.0]]);
    drop(left);
    drop(right);
    let released = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
        "test released split working controls").unwrap();
    drop(released);
    ctx.finish_session().unwrap();
}

#[test]
fn pcurve_split_empty_input_preserves_original_entry_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original empty split refusal") else {
        panic!("expected original work refusal");
    };
    assert!(matches!(split_homogeneous_pcurve(&[], 0.5, &ctx),
        Err(CodecError::ResourceLimit(last)) if last == first));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn pcurve_split_invalid_parameter_preserves_original_entry_refusal() {
    let controls = controls();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original invalid split refusal") else {
        panic!("expected original work refusal");
    };
    for parameter in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 2.0] {
        assert!(matches!(split_homogeneous_pcurve(&controls, parameter, &ctx),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn pcurve_split_empty_or_invalid_input_executes_no_work_or_allocation() {
    let controls = controls();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(split_homogeneous_pcurve(&[], 0.5, &ctx).unwrap().is_none());
    for parameter in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 2.0] {
        assert!(split_homogeneous_pcurve(&controls, parameter, &ctx).unwrap().is_none());
    }
    ctx.finish_session().unwrap();
}
