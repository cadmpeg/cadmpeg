// SPDX-License-Identifier: Apache-2.0
//! NURBS boundary regressions.

use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::quadratic::Coefficient;
use crate::decode::surfaces::nurbs_boundaries::{
    cubic_extrusion_plane_generator_curve, cubic_unit_interval_roots,
};
use crate::decode::surfaces::nurbs_boundaries::{
    nurbs_plane_boundary_curve as decode_nurbs_plane_boundary_curve,
    shared_extrusion_generator_curve as decode_shared_extrusion_generator_curve,
};
use crate::decode::tests::with_decode_ctx;
use cadmpeg_ir::geometry::nurbs::{
    NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes,
};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_test_support::edit;

fn nurbs_plane_boundary_curve(
    nurbs: &NurbsSurface,
    surface_id: u32,
    plane: PlaneEquation,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<CurveGeometry> {
    with_decode_ctx(|ctx| decode_nurbs_plane_boundary_curve(ctx, nurbs, surface_id, plane, refusal))
        .expect("service profile admits the boundary")
}

fn shared_extrusion_generator_curve(
    first: &NurbsSurface,
    first_surface_id: u32,
    second: &NurbsSurface,
    second_surface_id: u32,
    refusal: &mut crate::lane_refusal::LaneRefusals,
) -> Option<CurveGeometry> {
    with_decode_ctx(|ctx| {
        decode_shared_extrusion_generator_curve(
            ctx,
            first,
            first_surface_id,
            second,
            second_surface_id,
            refusal,
        )
    })
    .expect("service profile admits the shared boundary")
}

#[test]
fn extrusion_nurbs_boundary_requires_one_plane_supported_control_edge() {
    let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            (0..4)
                .flat_map(|u| {
                    [
                        Point3::new(f64::from(u), 0.0, f64::from(u * u)),
                        Point3::new(f64::from(u), 1.0, f64::from(u * u)),
                    ]
                })
                .collect::<Vec<_>>()
                .chunks(2_usize)
                .map(<[_]>::to_vec)
                .collect(),
            Some(vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    ).expect("fixture constructor admission")
    .expect("valid extrusion surface");
    let boundary = nurbs_plane_boundary_curve(
        &surface,
        7,
        PlaneEquation {
            origin: [0.0, 1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("v1 boundary");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(boundary)) = boundary else {
        panic!("extrusion boundary must retain its NURBS parameterization");
    };
    assert_eq!(boundary.degree(), 3);
    assert_eq!(boundary.knots(), surface.u_knots());
    assert_eq!(
        boundary.control_points(),
        [
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 1.0, 1.0),
            Point3::new(2.0, 1.0, 4.0),
            Point3::new(3.0, 1.0, 9.0),
        ]
    );
    assert_eq!(
        boundary.pole_rows().weights(),
        Some(vec![1.0, 2.0, 3.0, 4.0])
    );

    let generator = nurbs_plane_boundary_curve(
        &surface,
        7,
        PlaneEquation {
            origin: [3.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("u1 boundary");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(generator)) = generator else {
        panic!("extrusion generator must retain its NURBS parameterization");
    };
    assert_eq!(generator.degree(), 1);
    assert_eq!(generator.knots(), surface.v_knots());
    assert_eq!(
        generator.control_points(),
        [Point3::new(3.0, 0.0, 9.0), Point3::new(3.0, 1.0, 9.0)]
    );
    assert_eq!(generator.pole_rows().weights(), Some(vec![4.0, 4.0]));

    assert!(nurbs_plane_boundary_curve(
        &surface,
        7,
        PlaneEquation {
            origin: [0.0, 0.5, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .is_none());
    let mut coplanar = surface.clone();
    coplanar
        .try_map_control_points(|_, point| {
            let mut point = point.get();
            point.z = 0.0;
            cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
                cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".into(),
                )
            })
        }, &cadmpeg_test_support::service_decode_context()).expect("pole edit admission")
        .expect("finite fixture geometry preserves NURBS invariants");
    assert!(nurbs_plane_boundary_curve(
        &coplanar,
        7,
        PlaneEquation {
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 0.0, 1.0],
        },
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .is_none());
    let restored = surface.poles();
    coplanar
        .try_map_control_points(|index, point| {
            let mut point = point.get();
            if let Some(value) = restored.get(index) {
                point = value.get();
            }
            cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
                cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".into(),
                )
            })
        }, &cadmpeg_test_support::service_decode_context()).expect("pole edit admission")
        .expect("finite fixture geometry preserves NURBS invariants");
    let mut zero_weights = coplanar.pole_grid().weights().expect("rational fixture");
    zero_weights[0][0] = 0.0;
    assert!(
        NurbsPoleGrid::from_lanes(&cadmpeg_test_support::service_decode_context(), coplanar.pole_grid().raw_points(), Some(zero_weights)).expect("fixture pole pairing admission").is_err()
    );
}

#[test]
fn shared_extrusion_generator_requires_equivalent_boundaries_and_separated_nets() {
    let first = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(-1.0, 0.0, 0.0), Point3::new(-1.0, 0.0, 1.0)],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0)],
            ],
            Some(vec![2.0, 2.0, 3.0, 4.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    ).expect("fixture constructor admission")
    .expect("valid first extrusion surface");
    let second = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![4.0, 4.0, 8.0, 8.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0)],
                vec![Point3::new(0.0, 1.0, 0.0), Point3::new(0.0, 1.0, 1.0)],
            ],
            Some(vec![6.0, 8.0, 8.0, 8.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    ).expect("fixture constructor admission")
    .expect("valid second extrusion surface");
    let shared = shared_extrusion_generator_curve(
        &first,
        7,
        &second,
        9,
        &mut crate::lane_refusal::LaneRefusals::new(),
    )
    .expect("shared generator boundary");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(shared)) = shared else {
        panic!("shared extrusion generator must retain its NURBS representation");
    };
    assert_eq!(shared.degree(), 1);
    assert_eq!(shared.knots(), first.v_knots());
    assert_eq!(
        shared.control_points(),
        [Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 1.0)]
    );
    assert_eq!(shared.pole_rows().weights(), Some(vec![3.0, 4.0]));

    let mut reversed = second.clone();
    let mut reversed_grid = reversed.pole_grid().raw_points();
    reversed_grid[0].swap(0, 1);
    reversed_grid[1].swap(0, 1);
    let mut reversed_weights = reversed.pole_grid().weights();
    if let Some(rows) = &mut reversed_weights {
        rows[0].swap(0, 1);
        rows[1].swap(0, 1);
    }
    {
        let replacement = NurbsPoleGrid::from_lanes(&cadmpeg_test_support::service_decode_context(), reversed_grid, reversed_weights).expect("fixture pole pairing admission")
            .expect("finite fixture geometry preserves NURBS invariants");
        edit::replace(&mut reversed, |previous| {
            NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), 
                NurbsSurfaceAxis::new(
                    previous.u_degree(),
                    previous.u_knots().to_vec(),
                    previous.u_periodic(),
                ),
                NurbsSurfaceAxis::new(
                    previous.v_degree(),
                    previous.v_knots().to_vec(),
                    previous.v_periodic(),
                ),
                replacement,
                previous.normal_reversed(),
            ).expect("fixture final NURBS admission")
        })
    }
    .expect("finite fixture geometry preserves NURBS invariants");
    assert!(shared_extrusion_generator_curve(
        &first,
        7,
        &reversed,
        9,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .is_some());

    let mut same_side = second.clone();
    let mut same_side_grid = same_side.pole_grid().raw_points();
    same_side_grid[1][0] = Point3::new(-2.0, 0.0, 0.0);
    same_side_grid[1][1] = Point3::new(-2.0, 0.0, 1.0);
    let same_side_weights = same_side.pole_grid().weights();
    {
        let replacement = NurbsPoleGrid::from_lanes(&cadmpeg_test_support::service_decode_context(), same_side_grid, same_side_weights).expect("fixture pole pairing admission")
            .expect("finite fixture geometry preserves NURBS invariants");
        edit::replace(&mut same_side, |previous| {
            NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), 
                NurbsSurfaceAxis::new(
                    previous.u_degree(),
                    previous.u_knots().to_vec(),
                    previous.u_periodic(),
                ),
                NurbsSurfaceAxis::new(
                    previous.v_degree(),
                    previous.v_knots().to_vec(),
                    previous.v_periodic(),
                ),
                replacement,
                previous.normal_reversed(),
            ).expect("fixture final NURBS admission")
        })
    }
    .expect("finite fixture geometry preserves NURBS invariants");
    assert!(shared_extrusion_generator_curve(
        &first,
        7,
        &same_side,
        9,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .is_none());

    let mut periodic_transverse = second.clone();
    {
        let replacement = true;
        edit::replace(&mut periodic_transverse, |previous| {
            NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), 
                NurbsSurfaceAxis::new(
                    previous.u_degree(),
                    previous.u_knots().to_vec(),
                    replacement,
                ),
                NurbsSurfaceAxis::new(
                    previous.v_degree(),
                    previous.v_knots().to_vec(),
                    previous.v_periodic(),
                ),
                previous.pole_grid().clone(),
                previous.normal_reversed(),
            ).expect("fixture final NURBS admission")
        })
        .expect("admitted periodic fixture");
    };
    assert!(shared_extrusion_generator_curve(
        &first,
        7,
        &periodic_transverse,
        9,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .is_none());

    let mut different_boundary = second;
    let mut different_grid = different_boundary.pole_grid().raw_points();
    different_grid[0][1].x = 0.1;
    let different_weights = different_boundary.pole_grid().weights();
    {
        let replacement = NurbsPoleGrid::from_lanes(&cadmpeg_test_support::service_decode_context(), different_grid, different_weights).expect("fixture pole pairing admission")
            .expect("finite fixture geometry preserves NURBS invariants");
        edit::replace(&mut different_boundary, |previous| {
            NurbsSurface::new(&cadmpeg_test_support::service_decode_context(), 
                NurbsSurfaceAxis::new(
                    previous.u_degree(),
                    previous.u_knots().to_vec(),
                    previous.u_periodic(),
                ),
                NurbsSurfaceAxis::new(
                    previous.v_degree(),
                    previous.v_knots().to_vec(),
                    previous.v_periodic(),
                ),
                replacement,
                previous.normal_reversed(),
            ).expect("fixture final NURBS admission")
        })
    }
    .expect("finite fixture geometry preserves NURBS invariants");
    assert!(shared_extrusion_generator_curve(
        &first,
        7,
        &different_boundary,
        9,
        &mut crate::lane_refusal::LaneRefusals::new()
    )
    .is_none());
}

#[test]
fn cubic_extrusion_plane_generator_requires_one_directrix_root() {
    let surface = NurbsSurface::from_lanes(&cadmpeg_test_support::service_decode_context(), 
        NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            [-1.0, -0.5, 0.5, 1.0]
                .into_iter()
                .flat_map(|x| [Point3::new(x, 0.0, 0.0), Point3::new(x, 0.0, 2.0)])
                .collect::<Vec<_>>()
                .chunks(2_usize)
                .map(<[_]>::to_vec)
                .collect(),
            Some(vec![1.0, 1.0, 2.0, 2.0, 3.0, 3.0, 4.0, 4.0])
                .map(|values| values.chunks(2_usize).map(<[_]>::to_vec).collect()),
        ),
        false,
    ).expect("fixture constructor admission")
    .expect("valid cubic extrusion surface");
    let generator = with_decode_ctx(|ctx| {
        cubic_extrusion_plane_generator_curve(
            ctx,
            &surface,
            7,
            PlaneEquation {
                origin: [0.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            &mut Vec::new(),
        )
    })
    .expect("resource limits")
    .expect("unique directrix-plane root");
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(generator)) = generator else {
        panic!("plane section generator must retain its NURBS representation");
    };
    assert_eq!(generator.degree(), 1);
    assert_eq!(generator.knots(), surface.v_knots());
    assert_eq!(generator.control_points().len(), 2);
    assert!(generator
        .control_points()
        .iter()
        .all(|point| point.x.abs() <= 1.0e-8));
    assert_eq!(generator.control_points()[0].z, 0.0);
    assert_eq!(generator.control_points()[1].z, 2.0);
    let weights = generator.weights().expect("rational generator");
    assert_eq!(weights.len(), 2);
    assert!((weights[0].get() - weights[1].get()).abs() <= 1.0e-12);

    assert!(with_decode_ctx(|ctx| cubic_extrusion_plane_generator_curve(
        ctx,
        &surface,
        7,
        PlaneEquation {
            origin: [2.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
        &mut Vec::new(),
    ))
    .expect("resource limits")
    .is_none());
    assert!(with_decode_ctx(|ctx| cubic_extrusion_plane_generator_curve(
        ctx,
        &surface,
        7,
        PlaneEquation {
            origin: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
        },
        &mut Vec::new(),
    ))
    .expect("resource limits")
    .is_none());
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| cubic_unit_interval_roots(
            ctx,
            Coefficient::single(1.0),
            Coefficient::single(-1.5),
            Coefficient::single(0.66),
            Coefficient::single(-0.08),
            1.0e-12
        ))
        .expect("roots are admitted")
        .len(),
        3
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| cubic_unit_interval_roots(
            ctx,
            Coefficient::single(1.0),
            Coefficient::single(-1.8),
            Coefficient::single(1.05),
            Coefficient::single(-0.2),
            1.0e-12
        ))
        .expect("roots are admitted")
        .len(),
        2
    );
}
