// SPDX-License-Identifier: Apache-2.0
//! Rejected parameter orderings release candidate reference lists.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use super::super::{ordered_native_parameter_face_loops, NativeCurveEvidence};

#[test]
fn rejected_parameter_ordering_retains_no_loop_references() {
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).expect("plane fixture"),
    ));
    let lp = crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        (10..13).map(|curve_id| crate::topology::HalfEdgeId {
            curve_id, side: crate::topology::Side::Zero,
        }).collect(),
    );
    let polygon = [[0.0, 0.0], [2.0, 0.0], [0.0, 2.0]];
    let bindings: Vec<_> = lp.half_edges().iter().copied().enumerate().map(|(index, half_edge)| {
        crate::topology::HalfEdgeVertexIncidence {
            half_edge,
            start_vertex_id: std::num::NonZeroU32::new(u32::try_from(index + 1).expect("three vertices"))
                .expect("one-based vertex"),
            end_vertex_id: std::num::NonZeroU32::new(u32::try_from((index + 1) % 3 + 1)
                .expect("three vertices")),
        }
    }).collect();
    let incidence: BTreeMap<_, _> = bindings.iter().map(|binding| (binding.half_edge, binding)).collect();
    let solved_vertices: BTreeMap<_, _> = polygon.iter().enumerate().map(|(index, point)| {
        (u32::try_from(index + 1).expect("three vertices"), [point[0], point[1], 0.0])
    }).collect();
    let pcurves = lp.half_edges().iter().enumerate().map(|(index, half_edge)| {
        ((half_edge.curve_id, 5), vec![([polygon[index], polygon[(index + 1) % 3]], 0)])
    }).collect();
    let typed = BTreeSet::new();
    let source_carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    // An empty root has the core's 16 MiB materialized base allowance.
    policy.limits.max_materialized_bytes = 16 * 1024 * 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root");
    for _ in 0..2 {
        // Coincident polygons have no strict outer loop. Three-edge loops
        // cannot use the two-edge circle fallback.
        assert!(ordered_native_parameter_face_loops(
            &ctx, &[&lp, &lp], (5, &surface), &incidence, &solved_vertices, &pcurves,
            NativeCurveEvidence {
                typed_nonlinear_curve_ids: &typed, model_curves: &[],
                source_carriers: &source_carriers,
            },
        ).expect("rejected ordering uses scratch only").is_none());
    }
    ctx.reserve_scoped(ctx.policy().limits.max_materialized_bytes, "released parameter ordering scratch")
        .expect("candidate lists and polygons are released");
}

#[test]
fn accepted_parameter_ordering_transfers_only_the_original_reference_vector() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    use std::mem::size_of;

    const EMPTY_ROOT_MATERIALIZED_BYTES: u64 = 16 * 1024 * 1024;
    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        ).expect("plane fixture"),
    ));
    let loops = [10_u32, 20].map(|first| crate::test_support::closed_loop(
        std::num::NonZeroU32::new(5),
        (first..first + 3).map(|curve_id| crate::topology::HalfEdgeId {
            curve_id, side: crate::topology::Side::Zero,
        }).collect(),
    ));
    let polygons = [
        [[0.0, 0.0], [8.0, 0.0], [0.0, 8.0]],
        [[1.0, 1.0], [2.0, 1.0], [1.0, 2.0]],
    ];
    let mut bindings = Vec::new();
    let mut solved_vertices = BTreeMap::new();
    let mut pcurves = super::super::NativePcurveCandidates::new();
    for (loop_index, lp) in loops.iter().enumerate() {
        for (index, half_edge) in lp.half_edges().iter().copied().enumerate() {
            let vertex = u32::try_from(loop_index * 3 + index + 1).expect("six vertices");
            let next = u32::try_from(loop_index * 3 + (index + 1) % 3 + 1).expect("six vertices");
            bindings.push(crate::topology::HalfEdgeVertexIncidence {
                half_edge,
                start_vertex_id: std::num::NonZeroU32::new(vertex).expect("one-based vertex"),
                end_vertex_id: std::num::NonZeroU32::new(next),
            });
            let point = polygons[loop_index][index];
            solved_vertices.insert(vertex, [point[0], point[1], 0.0]);
            pcurves.insert((half_edge.curve_id, 5),
                vec![([point, polygons[loop_index][(index + 1) % 3]], 0)]);
        }
    }
    let incidence: BTreeMap<_, _> = bindings.iter().map(|binding| (binding.half_edge, binding)).collect();
    let typed = BTreeSet::new();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    // extend_from_slice reserves four reference slots for one or two loops.
    // Containment rotates that same Vec. Segments, polygons and their outer
    // row slots belong to separate scratch leases and do not survive.
    let bytes = u64::try_from(4 * size_of::<&crate::topology::Loop>()).expect("reference slots");
    for count in [1, 2] {
        let inputs = if count == 1 { vec![&loops[0]] } else { vec![&loops[1], &loops[0]] };
        for cap in 0..=bytes {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let result = ordered_native_parameter_face_loops(
                &ctx, &inputs, (5, &surface), &incidence, &solved_vertices, &pcurves,
                NativeCurveEvidence { typed_nonlinear_curve_ids: &typed,
                    model_curves: &[], source_carriers: &carriers },
            );
            let original = if cap == bytes {
                let ordered = result.expect("exact reference backing").expect("proven ordering");
                assert_eq!(ordered.len(), count);
                assert!(std::ptr::eq(ordered[0], &loops[0]));
                if count == 2 { assert!(std::ptr::eq(ordered[1], &loops[1])); }
                let original = ctx.charge_retained_limit(1, "after retained face references")
                    .expect_err("exact backing consumed");
                assert_eq!((original.dimension, original.used, original.additional),
                    (ResourceDimension::RetainedBytes, bytes, 1));
                original
            } else {
                let original = ctx.resource_refusal().expect("smaller retained cap");
                assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == original));
                assert_eq!((original.dimension, original.used, original.additional, original.operation),
                    (ResourceDimension::RetainedBytes, 0, bytes,
                        "creo native face ordering candidate references"));
                original
            };
            assert!(matches!(ordered_native_parameter_face_loops(
                &ctx, &inputs, (5, &surface), &incidence, &solved_vertices, &pcurves,
                NativeCurveEvidence { typed_nonlinear_curve_ids: &typed,
                    model_curves: &[], source_carriers: &carriers },
            ), Err(CodecError::ResourceLimit(actual)) if actual == original));
        }
        for overflow in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_materialized_bytes = EMPTY_ROOT_MATERIALIZED_BYTES;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            let mut parent = ctx.reserve_scoped(37, "parameter ordering parent").expect("parent");
            let ordered = parent.with_storage(|| ordered_native_parameter_face_loops(
                &ctx, &inputs, (5, &surface), &incidence, &solved_vertices, &pcurves,
                NativeCurveEvidence { typed_nonlinear_curve_ids: &typed,
                    model_curves: &[], source_carriers: &carriers },
            )).expect("parent transfer").expect("proven ordering");
            assert_eq!(ordered.len(), count);
            assert!(std::ptr::eq(ordered[0], &loops[0]));
            let available = EMPTY_ROOT_MATERIALIZED_BYTES - 37 - bytes;
            let probe = ctx.reserve_scoped_limit(available + u64::from(overflow), "live face references");
            if overflow {
                let original = probe.expect_err("one byte beyond actual overlap");
                assert_eq!((original.dimension, original.used, original.additional),
                    (ResourceDimension::MaterializedBytes, 37 + bytes, available + 1));
                assert_eq!(ctx.resource_refusal(), Some(original));
            } else {
                drop(probe.expect("exact live overlap"));
            }
            drop(ordered);
            drop(parent);
            if !overflow {
                drop(ctx.reserve_scoped(EMPTY_ROOT_MATERIALIZED_BYTES, "parameter ordering cleanup")
                    .expect("all scratch and parent storage released"));
                let original = ctx.charge_retained_limit(1, "parameter ordering retained cleanup")
                    .expect_err("zero retained policy");
                assert_eq!((original.used, original.additional), (0, 1));
            }
        }
    }
}
