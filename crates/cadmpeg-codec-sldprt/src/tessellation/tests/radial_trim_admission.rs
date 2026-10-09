// SPDX-License-Identifier: Apache-2.0
//! Temporary angle backing and borrowed conical pole checks.

use super::{add_cylindrical_patch_face, model_with_body};
use crate::tessellation::{conical_trim, cylindrical_trim, TrimTopology};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::Model;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::Face;

fn with_fixture(
    conical: bool,
    edit: impl FnOnce(&mut Model),
    check: impl FnOnce(&Face, &SurfaceGeometry, &TrimTopology<'_>),
) {
    let mut model = model_with_body();
    let face_id = add_cylindrical_patch_face(&mut model, "radial-trim-admission", 0., 2.);
    if conical {
        model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
            cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                Point3::new(0., 0., 0.), Vector3::new(0., 0., 1.),
                Vector3::new(1., 0., 0.), 5., 1., 0.,
            ).unwrap(),
        ));
    }
    edit(&mut model);
    let face = model.faces.iter().find(|face| face.id == face_id).unwrap();
    let surface = model.surfaces.iter().find(|surface| surface.id == face.surface).unwrap();
    // Fixture maps are retained in this outer owner, outside measured execution.
    let loops = model.loops.iter().map(|value| (&value.id, value)).collect();
    let coedges = model.coedges.iter().map(|value| (&value.id, value)).collect();
    let edges = model.edges.iter().map(|value| (&value.id, value)).collect();
    let vertices = model.vertices.iter().map(|value| (&value.id, value)).collect();
    let points = model.points.iter().map(|value| (&value.id, value.position().get())).collect();
    let curves = model.curves.iter().map(|value| (&value.id, &value.geometry)).collect();
    check(face, &surface.geometry, &TrimTopology {
        loops: &loops, coedges: &coedges, edges: &edges, vertices: &vertices,
        points: &points, curves: &curves, coordinate_scale: 5.,
    });
}

fn evaluate(
    ctx: &DecodeContext<'_>,
    conical: bool,
    face: &Face,
    surface: &SurfaceGeometry,
    topology: &TrimTopology<'_>,
) -> Result<Option<[f64; 4]>, CodecError> {
    if conical {
        Ok(conical_trim(ctx, face, surface, topology)?.map(|trim| [
            trim.min_axial, trim.max_axial, trim.angular_start, trim.angular_span,
        ]))
    } else {
        Ok(cylindrical_trim(ctx, face, surface, topology)?.map(|trim| [
            trim.min_axial, trim.max_axial, trim.angular_start, trim.angular_span,
        ]))
    }
}

fn policy(bytes: u64) -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_collection_items = 8;
    policy
}

fn angle_bytes(count: u64) -> u64 {
    count * u64::try_from(std::mem::size_of::<f64>()).unwrap()
}

fn operation(conical: bool) -> &'static str {
    if conical { "collect SLDPRT conical trim angles" }
    else { "collect SLDPRT cylindrical trim angles" }
}

fn two_pole_curve(points: [Point3; 2], rational: bool) -> cadmpeg_ir::geometry::CurveGeometry {
    use cadmpeg_ir::geometry::{nurbs::{NurbsCurve, NurbsPoles3}, CurveGeometry, SolvedCurveGeometry};
    let source_ctx = cadmpeg_test_support::service_decode_context();
    let poles = NurbsPoles3::from_lanes(&source_ctx, Vec::from(points),
        rational.then(|| vec![1., 2.])).unwrap().unwrap();
    let curve = NurbsCurve::new(&source_ctx, 1, vec![0., 0., 1., 1.], poles, false).unwrap().unwrap();
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))
}

#[test]
fn radial_trim_releases_angles_at_the_exact_growth_peak() {
    // Eight endpoints grow capacity 4 to 8. Old backing overlaps the new
    // backing during reallocation; the eight-value stable sort uses no scratch.
    let peak = angle_bytes(8 + 4);
    for conical in [false, true] {
        with_fixture(conical, |_| {}, |face, surface, topology| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak)).unwrap();
            assert_eq!(evaluate(&ctx, conical, face, surface, topology).unwrap(),
                Some([0., 2., 0., std::f64::consts::FRAC_PI_2]));
            drop(ctx.reserve_scoped(peak, "test complete radial angle release").unwrap());
            ctx.finish_session().unwrap();
        });
    }
}

#[test]
fn radial_trim_refuses_first_backing_before_allocation() {
    let need = angle_bytes(4);
    for conical in [false, true] {
        with_fixture(conical, |_| {}, |face, surface, topology| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(need - 1)).unwrap();
            let (result, allocations) = crate::test_support::allocation::count_allocations(||
                evaluate(&ctx, conical, face, surface, topology));
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("first backing refusal"); };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.operation, operation(conical));
            assert_eq!((limit.used, limit.additional, limit.limit), (0, need, need - 1));
            assert_eq!(allocations, 0);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        });
    }
}

#[test]
fn radial_trim_refuses_reallocation_overlap_one_below_peak() {
    let peak = angle_bytes(8 + 4);
    for conical in [false, true] {
        with_fixture(conical, |_| {}, |face, surface, topology| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak - 1)).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = evaluate(&ctx, conical, face, surface, topology)
            else { panic!("overlap refusal"); };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.operation, operation(conical));
            assert_eq!((limit.used, limit.additional, limit.limit),
                (angle_bytes(8), angle_bytes(4), peak - 1));
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        });
    }
}

#[test]
fn radial_trim_releases_rejected_angle_prefix() {
    for conical in [false, true] {
        with_fixture(conical, |model| {
            let point = &mut model.points[2];
            *point = cadmpeg_ir::topology::Point::new(point.id.clone(),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0., 0., 2.)).unwrap(), None);
        }, |face, surface, topology| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(angle_bytes(4))).unwrap();
            assert!(evaluate(&ctx, conical, face, surface, topology).unwrap().is_none());
            drop(ctx.reserve_scoped(angle_bytes(4), "test rejected radial prefix release").unwrap());
            ctx.finish_session().unwrap();
        });
    }
}

#[test]
fn conical_two_pole_rejection_borrows_both_pole_forms_without_allocation() {
    for rational in [false, true] {
        with_fixture(true, |model| {
            model.curves[0].geometry = two_pole_curve(
                [Point3::new(0., 0., 0.), Point3::new(0., 0., 2.)], rational);
        }, |face, surface, topology| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(0)).unwrap();
            let (result, allocations) = crate::test_support::allocation::count_allocations(||
                conical_trim(&ctx, face, surface, topology));
            assert!(result.unwrap().is_none());
            assert_eq!(allocations, 0);
            ctx.finish_session().unwrap();
        });
    }
}

#[test]
fn conical_two_pole_acceptance_allocates_only_angle_backing() {
    for rational in [false, true] {
        with_fixture(true, |model| {
            model.curves[1].geometry = two_pole_curve(
                [Point3::new(0., 5., 0.), Point3::new(0., 5., 2.)], rational);
            model.curves[3].geometry = two_pole_curve(
                [Point3::new(5., 0., 2.), Point3::new(5., 0., 0.)], rational);
        }, |face, surface, topology| {
            let arena = DecodeArena::new();
            let peak = angle_bytes(12);
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(peak)).unwrap();
            let (result, allocations) = crate::test_support::allocation::count_allocations(||
                evaluate(&ctx, true, face, surface, topology));
            assert_eq!(result.unwrap(), Some([0., 2., 0., std::f64::consts::FRAC_PI_2]));
            // The only heap operations are the angle vector's first backing
            // and its capacity4-to-capacity8 reallocation.
            assert_eq!(allocations, 2);
            drop(ctx.reserve_scoped(peak, "test accepted pole trim release").unwrap());
            ctx.finish_session().unwrap();
        });
    }
}

#[test]
fn classified_single_loop_borrows_the_same_radial_boundary() {
    for conical in [false, true] {
        with_fixture(conical, |model| {
            model.faces[0].loops = cadmpeg_ir::topology::FaceLoops::classified(
                model.loops[0].id.clone(), Vec::new());
        }, |face, surface, topology| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(angle_bytes(12))).unwrap();
            assert_eq!(evaluate(&ctx, conical, face, surface, topology).unwrap(),
                Some([0., 2., 0., std::f64::consts::FRAC_PI_2]));
            ctx.finish_session().unwrap();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(angle_bytes(4) - 1)).unwrap();
            let (result, allocations) = crate::test_support::allocation::count_allocations(||
                evaluate(&ctx, conical, face, surface, topology));
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("classified backing refusal"); };
            assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
            assert_eq!(limit.operation, operation(conical));
            assert_eq!((limit.used, limit.additional, limit.limit), (0, angle_bytes(4), angle_bytes(4) - 1));
            assert_eq!(allocations, 0);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        });
    }
}

#[test]
fn radial_trim_preserves_original_refusal_before_geometry_or_allocation() {
    for conical in [false, true] {
        with_fixture(conical, |_| {}, |face, surface, topology| {
            let arena = DecodeArena::new();
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy(angle_bytes(12))).unwrap();
            let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(9, "test original radial refusal")
            else { panic!("original refusal"); };
            let (result, allocations) = crate::test_support::allocation::count_allocations(||
                evaluate(&ctx, conical, face, surface, topology));
            assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
            assert_eq!(allocations, 0);
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        });
    }
}
