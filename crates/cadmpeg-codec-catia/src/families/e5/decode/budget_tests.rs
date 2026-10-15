// SPDX-License-Identifier: Apache-2.0
//! E5 geometry equality and scratch ownership.

use super::*;

fn curve(points: usize, rational: bool) -> CurveGeometry {
    let mut knots = vec![0.0, 0.0];
    knots
        .extend((1..points - 1).map(|index| f64::from(u32::try_from(index).expect("small index"))));
    let end = f64::from(u32::try_from(points - 1).expect("small index"));
    knots.extend([end, end]);
    let controls = (0..points)
        .map(|index| {
            Point3::new(
                f64::from(u32::try_from(index).expect("small index")),
                0.0,
                0.0,
            )
        })
        .collect();
    let weights = rational.then(|| vec![1.0; points]);
    let nurbs = crate::test_support::with_service_context(|ctx| {
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(ctx, 1, knots, controls, weights, false)
    })
    .expect("construction budget")
    .expect("valid open curve");
    CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))
}

#[test]
fn e5_equal_nurbs_carriers_charge_the_knot_and_pole_lanes() {
    for rational in [false, true] {
        let left = curve(256, rational);
        let right = left.clone();
        assert!(crate::test_support::with_service_context(|ctx| {
            equivalent_e5_curve_carriers(ctx, &left, &right)
        })
        .expect("equal carriers"));
        let refused =
            crate::test_support::with_work_refusal("catia_e5_curve_geometry_compare", |ctx| {
                equivalent_e5_curve_carriers(ctx, &left, &right)
            });
        assert!(
            matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_curve_geometry_compare")
        );
    }
}

#[test]
fn e5_boundary_pcurve_equality_charges_nurbs_lanes() {
    let left = crate::test_support::with_service_context(|ctx| {
        PcurveNurbs::from_lanes(
            ctx,
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
    })
    .expect("construction budget")
    .expect("valid rational pcurve");
    let left = PcurveGeometry::Nurbs { nurbs: left };
    let right = left.clone();
    assert!(crate::test_support::with_service_context(|ctx| {
        equal_e5_pcurve_geometry(ctx, &left, &right)
    })
    .expect("equal pcurves"));
    assert!(
        matches!(crate::test_support::with_work_refusal("catia_e5_pcurve_geometry_compare", |ctx| {
        equal_e5_pcurve_geometry(ctx, &left, &right)
    }), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_e5_pcurve_geometry_compare")
    );
}

#[test]
fn e5_duplicate_body_face_does_not_admit_the_face_suffix() {
    let topology = crate::families::e5::graph::E5Topology {
        bodies: Vec::new(),
        faces: Vec::new(),
        edges: BTreeMap::new(),
        pcurves: BTreeMap::new(),
        bounds: BTreeMap::new(),
        curve_supports: BTreeMap::new(),
        vertex_refs: Vec::new(),
    };
    let mut faces = vec![1, 1];
    faces.extend(2..10_002);
    assert!(crate::test_support::with_work_limit(512, |ctx| {
        e5_ownership_plan(ctx, &topology, &[(None, faces)])
    })
    .expect("the second face rejects ownership")
    .is_none());
}

#[test]
fn e5_boundary_geometry_is_scoped_through_plan_readers() {
    use crate::families::e5::graph::{
        E5CurveSupport, E5CurveSupportKind, E5Edge, E5Face, E5Loop, E5OrientedMember, E5Pcurve,
        E5Topology, Sign,
    };
    use crate::families::e5::records::E5Surface;
    use crate::families::e5::tests::e5_loop_members;
    use crate::test_support::test_b5::{finite_lane, finite_pair};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::ids::SurfaceId;
    use cadmpeg_ir::math::Vector3;
    use std::collections::{BTreeMap, HashMap};

    let surface = E5Surface {
        pos: 0,
        record_id: 100,
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("plane"),
        )),
        uv_scale: finite_pair([1.0, 1.0]),
    };
    for intersection in [false, true] {
        for reversed in [false, true] {
            let pcurve = || E5Pcurve::Nurbs {
                surface: 100,
                degree: 1,
                knots: finite_lane(&[0.0, 0.0, 1.0, 1.0]),
                control_points: vec![finite_pair([0.0, 0.0]), finite_pair([1.0, 0.0])],
                range: finite_pair([0.0, 1.0]),
            };
            let topology = E5Topology {
                bodies: Vec::new(),
                faces: vec![E5Face {
                    record_id: 1,
                    surface: 100,
                    trailer_sign: Sign::Positive,
                    loops: vec![E5Loop {
                        record_id: 2,
                        surface: 100,
                        members: e5_loop_members(&[20], &[200], &[false]),
                        oriented_members: Some(vec![E5OrientedMember {
                            serialized_index: 0,
                            reversed: false,
                        }]),
                        outer: Some(true),
                        orientation_hint: None,
                    }],
                }],
                edges: BTreeMap::from([(
                    200,
                    E5Edge {
                        support: 300,
                        start_vertex: 400,
                        end_vertex: 401,
                        parameter_start: 500,
                        parameter_end: 501,
                        tail: Vec::new(),
                    },
                )]),
                pcurves: BTreeMap::from([(20, pcurve()), (21, pcurve())]),
                bounds: BTreeMap::new(),
                curve_supports: BTreeMap::from([(
                    300,
                    E5CurveSupport {
                        kind: if intersection {
                            E5CurveSupportKind::Intersection([20, 21])
                        } else {
                            E5CurveSupportKind::Boundary(20)
                        },
                        mode: 0,
                        range: finite_pair([0.0, 1.0]),
                        tail: Vec::new(),
                    },
                )]),
                vertex_refs: vec![400, 401],
            };
            let surfaces = HashMap::from([(
                100,
                (
                    SurfaceId::mint("catia:test:surface#scoped-boundary".to_owned())
                        .expect("identity"),
                    &surface,
                ),
            )]);
            let endpoints = if reversed { [1.0, 0.0] } else { [0.0, 1.0] };
            let points = HashMap::from([
                (400, Point3::new(endpoints[0], 0.0, 0.0)),
                (401, Point3::new(endpoints[1], 0.0, 0.0)),
            ]);
            crate::test_support::with_retained_limit(0, |ctx| {
                for _ in 0..3 {
                    let mut storage = ctx
                        .reserve_scoped(0, "test_boundary_storage")
                        .expect("boundary owner");
                    let plan = super::plan_boundary_with_points(
                        ctx,
                        &mut storage,
                        &topology,
                        &surfaces,
                        &points,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                    .expect("boundary geometry uses temporary storage")
                    .expect("boundary plan");
                    assert_eq!(plan.pcurves.len(), 1);
                    assert_eq!(plan.edge_curves.len(), 1);
                    assert_eq!(plan.pcurve_use_reversed[&(2, 0)], reversed);
                    assert_eq!(plan.intersections.len(), usize::from(intersection));
                    let geometry = &plan.edge_curves[&200].0;
                    let start = cadmpeg_ir::eval::decode::curve_point(ctx, geometry, 0.0)
                        .expect("start evaluation");
                    let end = cadmpeg_ir::eval::decode::curve_point(ctx, geometry, 1.0)
                        .expect("end evaluation");
                    assert_eq!(start.get(), points[&400]);
                    assert_eq!(end.get(), points[&401]);
                }
            });
            if !intersection && !reversed {
                crate::test_support::with_work_limit(u64::MAX, |ctx| {
                    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                        cadmpeg_core::decode::ResourceDimension::WorkUnits,
                        "catia_e5_oriented_pcurve_copy",
                        None,
                    );
                    let mut storage = ctx
                        .reserve_scoped(0, "test_boundary_storage")
                        .expect("boundary owner");
                    let plan = super::plan_boundary_with_points(
                        ctx,
                        &mut storage,
                        &topology,
                        &surfaces,
                        &points,
                        &mut crate::nurbs::LaneRefusals::new(),
                    )
                    .expect("a lifted boundary needs no oriented pcurve copy")
                    .expect("boundary plan");
                    assert_eq!(plan.pcurves.len(), 1);
                    assert_eq!(plan.edge_curves.len(), 1);
                });
            }
        }
    }
}
