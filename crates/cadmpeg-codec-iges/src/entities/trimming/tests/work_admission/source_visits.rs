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

fn cluster_size_storage_boundary(count: usize, exact: bool) {
    use std::mem::{align_of, size_of};
    let points: Vec<_> = (0..count).map(|index| FinitePoint3::new(
        Point3::new(f64::from(u32::try_from(index).unwrap()) * 2.0, 0.0, 0.0)).unwrap()).collect();
    let before = points.clone();
    let parent = u64::try_from(count * size_of::<usize>()).unwrap();
    let node = u64::try_from(11 * (size_of::<usize>() + size_of::<Vec<usize>>())
        + 16 * size_of::<usize>()
        + 2 * align_of::<usize>().max(align_of::<Vec<usize>>())).unwrap();
    let cap = parent + node - u64::from(!exact);
    // At 1 and 21 points, the two initial index lanes fit below the
    // first root node's legitimate peak. At 64 this witness is invalid:
    // initial 1024 exceeds the later first-node peak 1008 on this target.
    assert!(2 * parent < cap);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cap;
    policy.limits.max_retained_bytes = 0;
    // Both initialized index lanes and the first root entry. Exact node
    // backing then reaches the first member's independent slot admission.
    policy.limits.max_collection_items = 2 * u64::try_from(count).unwrap() + 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = cluster_boundary_positions(&points, PositiveReal::ONE, &ctx)
        .expect_err("expected root node or member slot refusal");
    let first = match error {
        BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(first)) => first,
        _ => panic!("expected original resource refusal"),
    };
    if exact {
        let items = policy.limits.max_collection_items;
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "iges boundary cluster members");
        assert_eq!((first.limit, first.used, first.additional), (items, items, 1));
    } else {
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.operation, "iges boundary cluster roots");
        assert_eq!((first.limit, first.used, first.additional), (cap, parent, node));
    }
    for _ in 0..64 {
        for source in [points.as_slice(), &[]] {
            assert!(matches!(cluster_boundary_positions(source, PositiveReal::ONE, &ctx),
                Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(last)))
                    if last == first));
        }
    }
    assert_eq!(points, before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn cluster_sizes_release_before_one_short_root_node_storage() {
    for count in [1, 21] { cluster_size_storage_boundary(count, false); }
}

#[test]
fn cluster_sizes_exact_root_node_peak_reaches_first_member_slot() {
    for count in [1, 21] { cluster_size_storage_boundary(count, true); }
}

fn zero_work_trimming_entry(run: impl Fn(&DecodeContext<'_>) -> Result<bool, CodecError>) {
    for refused in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = if refused {
            match ctx.charge_work(1, "test original trimming entry refusal") {
                Err(CodecError::ResourceLimit(first)) => Some(first),
                _ => panic!("expected original refusal"),
            }
        } else { None };
        for _ in 0..64 {
            match original {
                Some(first) => assert!(matches!(run(&ctx),
                    Err(CodecError::ResourceLimit(last)) if last == first)),
                None => assert!(run(&ctx).unwrap()),
            }
        }
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(),
                Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().unwrap(),
        }
    }
}

#[test]
fn trimming_native_identity_wrong_prefix_preserves_original_entry_refusal() {
    for id in ["", "test:model:curve#0"] {
        zero_work_trimming_entry(|ctx| super::super::super::native_sequence_from_id(
            id, "iges:model:curve#D", ctx).map(|value| value.is_none()));
    }
}

#[test]
fn trimming_absent_support_bounds_preserve_original_entry_refusal() {
    let id = cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#0").unwrap();
    zero_work_trimming_entry(|ctx| super::super::super::surface_parameter_bound_intervals(
        None, &id, &[], &[], crate::global::RealPrecision {
            single_significance: 7, double_significance: 15,
        }, ctx).map(|value| value.is_none()));
}

#[test]
fn trimming_non_nurbs_pcurve_preserves_original_entry_refusal() {
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
    use cadmpeg_ir::math::Point2;
    let geometry = PcurveGeometry::Line(LinePcurve::try_new(
        Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap());
    zero_work_trimming_entry(|ctx| super::super::super::linear_pcurve_points(
        &geometry, [0.0, 1.0], ctx).map(|value| value.is_none()));
}

#[test]
fn trimming_invalid_simple_ring_preserves_original_entry_refusal() {
    for points in [Vec::new(), vec![[0.0, 0.0]; 3],
        vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]] {
        zero_work_trimming_entry(|ctx| super::super::super::SimpleRing::new(
            points.clone(), ctx).map(|value| value.is_err()));
    }
}

#[test]
fn trimming_constant_ring_relationships_preserve_original_entry_refusal() {
    use super::super::super::{BoundarySurfaceKind, NonSimpleRing};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::geometry::analytic::PlaneSurface;
    use cadmpeg_ir::math::Vector3;
    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)).unwrap()));
    for (non_simple, kind, outer, periodic, bounds, expected) in [
        (true, BoundarySurfaceKind::Trimmed, true, [false; 2], None, Some(false)),
        (false, BoundarySurfaceKind::Bounded, false, [false; 2], None, Some(true)),
        (false, BoundarySurfaceKind::Trimmed, true, [false; 2], None, None),
        (false, BoundarySurfaceKind::Trimmed, false, [true, false], None, None),
        (false, BoundarySurfaceKind::Trimmed, false, [false; 2], Some([None; 4]), None),
    ] {
        zero_work_trimming_entry(|ctx| super::super::super::linear_boundary_relationship_is_valid(
            if non_simple { Err(&NonSimpleRing) } else { Ok(&[]) },
            kind, outer, &plane, bounds, periodic, ctx).map(|value| value == expected));
    }
}

#[test]
fn trimming_invalid_knot_insertion_preserves_original_entry_refusal() {
    for insertion in [(0.5, 0, 0), (0.5, 1, 2)] {
        zero_work_trimming_entry(|ctx| super::super::super::insert_homogeneous_pcurve_knot(
            1, &mut Vec::new(), &mut Vec::new(), insertion, ctx).map(|value| value.is_none()));
    }
}

#[test]
fn trimming_invalid_homogeneous_span_layout_preserves_original_entry_refusal() {
    for (degree, knots, controls) in [
        (0, vec![0.0, 1.0], vec![[1.0, 0.0, 0.0, 0.0]]),
        (1, Vec::new(), Vec::new()),
        (1, vec![0.0, 1.0], vec![[1.0, 0.0, 0.0, 0.0]; 2]),
    ] {
        zero_work_trimming_entry(|ctx| super::super::super::homogeneous_pcurve_spans(
            degree, &knots, controls.clone(), ctx).map(|value| value.is_none()));
    }
}

#[test]
fn trimming_free_edge_range_preserves_original_entry_refusal() {
    use cadmpeg_ir::ids::{EdgeId, VertexId};
    use cadmpeg_ir::topology::{Edge, EdgeCarrier};
    let edge = Edge {
        id: EdgeId::mint("test:model:edge#0").unwrap(),
        carrier: EdgeCarrier::unbounded(None),
        start: VertexId::mint("test:model:vertex#0").unwrap(),
        end: VertexId::mint("test:model:vertex#1").unwrap(), tolerance: None,
    };
    let ir = cadmpeg_ir::CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    zero_work_trimming_entry(|ctx| super::super::super::edge_range_matches_curve(
        ctx, &edge, &index, Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0), 0.0).map(|value| !value).map_err(CodecError::from));
}

#[test]
fn trimming_invalid_procedural_bounds_preserve_original_entry_refusal() {
    use cadmpeg_ir::geometry::{ProceduralSurface, ProceduralSurfaceDefinition, RecordBounds};
    use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};
    let ir = cadmpeg_ir::CadIr::empty();
    let index = cadmpeg_ir::index::ModelIndex::build(&ir, cadmpeg_ir::index::StandardIndex);
    for bounds in [None, Some([None; 4]),
        Some([Some(1.0), Some(0.0), None, None]),
        Some([Some(1.0), Some(1.0), None, None])] {
        let procedural = ProceduralSurface::new(
            ProceduralSurfaceId::mint("test:model:procedural-surface#0").unwrap(),
            ProceduralSurfaceDefinition::CurveBounded {
                support: SurfaceId::mint("test:model:surface#0").unwrap(),
                boundaries: Vec::new(), boundary_pcurves: Vec::new(), implicit_outer: false,
            }, bounds.map(|raw| RecordBounds::try_new(raw).unwrap()));
        zero_work_trimming_entry(|ctx| super::super::super::procedural_pcurve_parameter_map(
            &index, &procedural, ctx).map(|value| value.is_none()));
    }
}

#[test]
fn trimming_empty_or_discontinuous_path_preserves_original_entry_refusal() {
    for path in [&[][..], &[2_u8][..]] {
        zero_work_trimming_entry(|ctx| {
            let mut target = vec![1_u8];
            let result = super::super::super::append_path(&mut target, path, ctx);
            assert_eq!(target, [1]);
            result.map(|value| !value)
        });
    }
}

fn split_level_boundary(count: usize, completed: usize, interpolation: bool) {
    let controls: Vec<_> = (0..count).map(|index|
        [1.0, f64::from(u32::try_from(index).unwrap()), 0.0, 0.0]).collect();
    let before = controls.clone();
    // One copy per control, one visit per completed level, then the
    // completed interpolation populations N-1 down to N-completed.
    let slots = completed * (2 * count - completed - 1) / 2;
    let work = u64::try_from(count + completed + slots + usize::from(interpolation)).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = u64::try_from(
        count * std::mem::size_of::<[f64; 4]>()).unwrap();
    policy.limits.max_retained_bytes = 2 * policy.limits.max_materialized_bytes;
    policy.limits.max_collection_items = u64::try_from(3 * count).unwrap();
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = split_homogeneous_pcurve(&controls, 0.5, &ctx) else {
        panic!("expected the next split level operation to refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, if interpolation {
        "iges pcurve split interpolation"
    } else { "iges pcurve split traversal" });
    let additional = if interpolation { count - completed - 1 } else { 1 };
    assert_eq!((first.limit, first.used, first.additional),
        (work, work, u64::try_from(additional).unwrap()));
    for _ in 0..64 {
        for source in [controls.as_slice(), &[]] {
            assert!(matches!(split_homogeneous_pcurve(source, 0.5, &ctx),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
    assert_eq!(controls, before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn pcurve_split_first_level_refuses_after_exact_source_copy_population() {
    for count in [4, 64] { split_level_boundary(count, 0, false); }
}

#[test]
fn pcurve_split_last_level_refuses_after_completed_interpolation_populations() {
    for count in [4, 64] { split_level_boundary(count, count - 2, false); }
}

#[test]
fn pcurve_split_last_interpolation_refuses_after_exact_last_level_visit() {
    for count in [4, 64] { split_level_boundary(count, count - 2, true); }
}
