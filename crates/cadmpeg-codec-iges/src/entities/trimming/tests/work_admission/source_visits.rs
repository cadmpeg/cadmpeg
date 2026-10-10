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

fn cluster_source_refusal(operation: &'static str) {
    let points = if operation == "iges boundary clustering comparisons" {
        [0.0, 0.5, 20.0].map(|x| FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap())
    } else { separated_points() };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = cluster_boundary_positions(&points, PositiveReal::ONE, &ctx);
            match result {
                Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(first))) => {
                    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
                    assert_eq!(first.operation, operation);
                    assert_eq!(first.additional, 1);
                    for replay in [points.as_slice(), &[]] {
                        assert!(matches!(cluster_boundary_positions(replay, PositiveReal::ONE, &ctx),
                            Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(last)))
                                if last == first));
                    }
                    assert!(matches!(ctx.finish_session(),
                        Err(CodecError::ResourceLimit(last)) if last == first));
                    Err(CodecError::ResourceLimit(first))
                }
                Err(error) => panic!("unexpected cluster refusal: {error:?}"),
                Ok(_) => { ctx.finish_session().unwrap(); Ok(()) }
            }
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.additional == 1));
}

#[test]
fn cluster_position_source_refuses_one_visit_after_parent_and_size_initialization() {
    cluster_source_refusal("iges boundary clustering positions");
}

#[test]
fn cluster_comparison_source_refuses_one_candidate_after_cell_lookup() {
    cluster_source_refusal("iges boundary clustering comparisons");
}

#[test]
fn cluster_membership_source_refuses_one_visit_after_grid_construction() {
    cluster_source_refusal("iges boundary cluster membership traversal");
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
    let points: Vec<_> = (0..count).map(|index| FinitePoint3::new(
        Point3::new(f64::from(u32::try_from(index).unwrap()) * 2.0, 0.0, 0.0)).unwrap()).collect();
    let before = points.clone();
    let operation = if exact { "iges boundary cluster members" } else { "iges boundary cluster roots" };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test cluster membership storage").unwrap();
            let result = storage.with_storage(|| cluster_boundary_positions(&points, PositiveReal::ONE, &ctx));
            let first = match result {
                Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(first))) => first,
                error => panic!("expected root node or member slot refusal: {error:?}"),
            };
            // Parent, size, link and cell slots precede the first root entry.
            let used = 4 * u64::try_from(count).unwrap() + u64::from(exact);
            assert_eq!(first.dimension, ResourceDimension::CollectionItems);
            assert_eq!(first.operation, operation);
            assert_eq!((first.used, first.additional), (used, 1));
            for _ in 0..64 {
                for source in [points.as_slice(), &[]] {
                    assert!(matches!(cluster_boundary_positions(source, PositiveReal::ONE, &ctx),
                        Err(BoundaryVertexCreationError::Resource(CodecError::ResourceLimit(last)))
                            if last == first));
                }
            }
            assert_eq!(points, before);
            drop(storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
            Err::<(), _>(CodecError::ResourceLimit(first))
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.operation == operation && limit.additional == 1));
}

#[test]
fn cluster_root_node_slot_refuses_after_grid_slots() {
    for count in [1, 21] { cluster_size_storage_boundary(count, false); }
}

#[test]
fn cluster_first_member_slot_refuses_after_root_slot() {
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

fn knot_interpolation_boundary(degree: usize, last: bool, exact: bool) {
    let mut knots = Vec::with_capacity(2 * degree + 4);
    knots.extend((0..=2 * degree + 2).map(|index| {
        if index <= degree {
            if !last || index <= 2 { -f64::MAX } else { 0.0 }
        } else if index == degree + 1 { f64::MAX / 2.0 } else { f64::MAX }
    }));
    let before_knots = knots.clone();
    let mut controls = Vec::with_capacity(degree + 3);
    controls.extend((0..degree + 2).map(|index|
        [1.0, f64::from(u32::try_from(index).unwrap()), 0.0, 0.0]));
    let completed = if last { degree - 2 } else { 0 };
    // Insertion shifts one existing inline row: one slot plus its bytes.
    // The bad denominator is visited after the completed descending indices.
    let shift = 1 + std::mem::size_of::<[f64; 4]>();
    let work = u64::try_from(shift + completed + usize::from(exact)).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 1;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::super::insert_homogeneous_pcurve_knot(
        degree, &mut knots, &mut controls, (f64::MAX / 2.0, degree + 1, 1), &ctx);
    let first = if exact {
        assert!(result.unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
        None
    } else {
        let Err(CodecError::ResourceLimit(first)) = result else {
            panic!("expected the next knot interpolation visit to refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges pcurve knot interpolation");
        assert_eq!((first.limit, first.used, first.additional), (work, work, 1));
        Some(first)
    };
    assert_eq!(knots, before_knots);
    assert_eq!(controls.len(), degree + 3);
    for (index, control) in controls.iter().enumerate() {
        let coordinate = if index > degree { index - 1 } else { index };
        let expected = f64::from(u32::try_from(coordinate).unwrap())
            - if degree - completed < index && index <= degree { 0.5 } else { 0.0 };
        assert_eq!(*control, [1.0, expected, 0.0, 0.0]);
    }
    if let Some(first) = first {
        for _ in 0..64 {
            assert!(matches!(super::super::super::insert_homogeneous_pcurve_knot(
                degree, &mut knots, &mut controls, (0.5, degree + 1, 1), &ctx),
                Err(CodecError::ResourceLimit(last)) if last == first));
            assert!(matches!(super::super::super::insert_homogeneous_pcurve_knot(
                degree, &mut Vec::new(), &mut Vec::new(), (0.5, 0, 0), &ctx),
                Err(CodecError::ResourceLimit(last)) if last == first));
        }
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    } else { ctx.finish_session().unwrap(); }
}

#[test]
fn pcurve_knot_first_overflow_visit_refuses_before_recovery() {
    for degree in [4, 64] { knot_interpolation_boundary(degree, false, false); }
}

#[test]
fn pcurve_knot_last_overflow_visit_refuses_after_completed_interpolation() {
    for degree in [4, 64] { knot_interpolation_boundary(degree, true, false); }
}

#[test]
fn pcurve_knot_first_overflow_recovers_with_exact_visit_admission() {
    for degree in [4, 64] { knot_interpolation_boundary(degree, false, true); }
}

#[test]
fn pcurve_knot_last_overflow_recovers_with_exact_visit_admission() {
    for degree in [4, 64] { knot_interpolation_boundary(degree, true, true); }
}
