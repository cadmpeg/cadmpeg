// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(unused_imports)]

use super::*;
use crate::examples::unit_cube;
use crate::geometry::{
    Curve, CurveGeometry, NurbsCurve, NurbsSurface, OffsetSupportExtension, PcurveGeometry,
    ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry, SurfaceParameterAxis,
};
use crate::ids::{CurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point2, Point3, Vector3};
use crate::report::Check;
use crate::transform::{Transform, Transform2};
use crate::validate::validate_neutral;
use crate::CadIr;
use cadmpeg_core::decode::WorkBudget;

fn bilinear_surface() -> NurbsSurface {
    NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    }
}

#[test]
fn nurbs_surface_inverse_distinguishes_closest_and_tolerance_contracts() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let closest =
        nurbs_surface_closest_parameter(&surface, point, None).expect("closest surface parameter");
    assert!((closest.u - 0.3).abs() < 1.0e-12);
    assert!((closest.v - 0.7).abs() < 1.0e-12);
    assert!(nurbs_surface_parameter_within_tolerance(&surface, point, None, 0.19).is_none());
    assert!(
        nurbs_surface_parameter_within_tolerance(&surface, point, None, 0.2 + 1.0e-12).is_some()
    );
}

#[test]
fn budgeted_nurbs_surface_inverse_stops_before_unbounded_patch_work() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let budget = WorkBudget::new(0);

    assert!(nurbs_surface_parameter_within_tolerance_with_budget(
        &surface, point, None, 1.0e-10, &budget,
    )
    .is_none());
    assert!(budget.exhausted());

    let budget = WorkBudget::new(10_000);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &surface, point, None, 1.0e-10, &budget,
    )
    .expect("a valid surface fits within a larger caller-owned budget");
    assert!((parameters.u - 0.3).abs() < 1.0e-12);
    assert!((parameters.v - 0.7).abs() < 1.0e-12);
    assert!(budget.consumed() > 0);
}

#[test]
fn budgeted_nurbs_surface_inverse_accepts_a_fit_qualified_seed_first() {
    const FIT_TOLERANCE: f64 = 1.0e-12;

    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let budget = WorkBudget::new(12);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &surface,
        point,
        Some(Point2::new(0.3, 0.7)),
        FIT_TOLERANCE,
        &budget,
    )
    .expect("a fit-qualified continuation seed does not need global search");

    assert_eq!(parameters, Point2::new(0.3, 0.7));
    assert_eq!(budget.consumed(), 12);
}

#[test]
fn budgeted_nurbs_surface_inverse_refines_an_approximate_seed_before_global_search() {
    const FIT_TOLERANCE: f64 = 1.0e-10;
    const PARAMETER_TOLERANCE: f64 = 1.0e-12;

    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.0);
    let budget = WorkBudget::new(256);
    let parameters = nurbs_surface_parameter_within_tolerance_with_budget(
        &surface,
        point,
        Some(Point2::new(0.29, 0.69)),
        FIT_TOLERANCE,
        &budget,
    )
    .expect("a nearby seed should be refined before global patch search");

    assert!((parameters.u - 0.3).abs() <= PARAMETER_TOLERANCE);
    assert!((parameters.v - 0.7).abs() <= PARAMETER_TOLERANCE);
    assert!(budget.consumed() > 0);
}

#[test]
fn budgeted_nurbs_surface_evaluation_charges_degree_work() {
    let surface = bilinear_surface();
    let budget = WorkBudget::new(3);
    assert!(nurbs_surface_point_with_budget(&surface, 0.25, 0.75, &budget).is_none());
    assert!(budget.exhausted());

    let budget = WorkBudget::new(12);
    assert!(nurbs_surface_point_with_budget(&surface, 0.25, 0.75, &budget).is_some());
    assert_eq!(budget.consumed(), 12);

    let budget = WorkBudget::new(27);
    assert!(nurbs_surface_partials_with_budget(&surface, 0.25, 0.75, &budget).is_none());
    assert!(budget.exhausted());

    let budget = WorkBudget::new(28);
    assert!(nurbs_surface_partials_with_budget(&surface, 0.25, 0.75, &budget).is_some());
    assert_eq!(budget.consumed(), 28);

    let transformed = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Nurbs(surface)),
        transform: Transform {
            rows: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 1.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        },
    };
    let budget = WorkBudget::new(12);
    assert!(surface_point_with_budget(&transformed, 0.25, 0.75, &budget).is_none());
    assert!(budget.exhausted());
    let budget = WorkBudget::new(13);
    assert_eq!(
        surface_point_with_budget(&transformed, 0.25, 0.75, &budget),
        Some(Point3::new(0.25, 0.75, 1.0))
    );
    assert_eq!(budget.consumed(), 13);
}

#[test]
fn budgeted_model_surface_charges_nurbs_directrix_work() {
    let directrix_id = CurveId("budgeted-directrix".into());
    let surface_id = SurfaceId("budgeted-sweep".into());
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            weights: None,
            periodic: false,
        }),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("budgeted-sweep-construction".into()),
        surface: surface_id.clone(),
        definition: ProceduralSurfaceDefinition::LinearSweep {
            directrix: directrix_id,
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let index = crate::index::ModelIndex::new(&ir);
    let budget = WorkBudget::new(5);
    assert!(
        model_surface_point_by_id_with_budget(&index, &surface_id, 0.25, 2.0, &budget).is_none()
    );
    assert!(budget.exhausted());
    let budget = WorkBudget::new(6);
    assert_eq!(
        model_surface_point_by_id_with_budget(&index, &surface_id, 0.25, 2.0, &budget),
        Some(Point3::new(0.25, 0.0, 2.0))
    );
    assert_eq!(budget.consumed(), 6);
}

#[test]
fn nurbs_surface_inverse_handles_rational_internal_spans() {
    let surface = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 3,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.5, 0.0, 0.2),
            Point3::new(0.5, 1.0, 0.2),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        weights: Some(vec![1.0, 1.0, 0.7, 0.7, 1.0, 1.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let point = nurbs_surface_point(&surface, 0.75, 0.4).expect("surface point");
    let parameters = nurbs_surface_parameter_within_tolerance(&surface, point, None, 1.0e-10)
        .expect("rational multi-span inverse");
    assert!((parameters.u - 0.75).abs() < 1.0e-9);
    assert!((parameters.v - 0.4).abs() < 1.0e-9);
}

#[test]
fn nurbs_surface_parameter_segment_bound_contains_curved_diagonal() {
    let mut surface = bilinear_surface();
    surface.control_points[3].z = 1.0;
    let parameters = [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)];
    let chord = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0)];
    let bound = nurbs_surface_parameter_segment_chord_bound(&surface, parameters, chord)
        .expect("rational Bézier residual bound");

    assert!(bound >= 1.0 / 3.0);
    assert!(bound < 1.0 / 3.0 + 1.0e-12);
    let reverse_bound = nurbs_surface_parameter_segment_chord_bound(
        &surface,
        [parameters[1], parameters[0]],
        [chord[1], chord[0]],
    )
    .expect("reversed rational Bézier residual bound");
    assert!((reverse_bound - bound).abs() < 1.0e-12);
    for index in 0..=100 {
        let parameter = f64::from(index) / 100.0;
        let point = nurbs_surface_point(&surface, parameter, parameter).expect("surface point");
        let target = Point3::new(parameter, parameter, parameter);
        let distance = (point.x - target.x)
            .hypot(point.y - target.y)
            .hypot(point.z - target.z);
        assert!(distance <= bound);
    }
}

#[test]
fn nurbs_surface_parameter_segment_bound_splits_internal_knots() {
    let surface = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 3,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.5, 0.0, 0.25),
            Point3::new(0.5, 1.0, 0.25),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        weights: Some(vec![1.0, 1.0, 0.5, 0.5, 1.0, 1.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let parameters = [Point2::new(0.1, 0.2), Point2::new(0.9, 0.8)];
    let endpoints = parameters
        .map(|point| nurbs_surface_point(&surface, point.u, point.v).expect("surface endpoint"));
    let bound = nurbs_surface_parameter_segment_chord_bound(&surface, parameters, endpoints)
        .expect("multi-span rational Bézier residual bound");

    for index in 0..=100 {
        let parameter = f64::from(index) / 100.0;
        let uv = Point2::new(
            parameters[0].u + parameter * (parameters[1].u - parameters[0].u),
            parameters[0].v + parameter * (parameters[1].v - parameters[0].v),
        );
        let point = nurbs_surface_point(&surface, uv.u, uv.v).expect("surface point");
        let target = Point3::new(
            endpoints[0].x + parameter * (endpoints[1].x - endpoints[0].x),
            endpoints[0].y + parameter * (endpoints[1].y - endpoints[0].y),
            endpoints[0].z + parameter * (endpoints[1].z - endpoints[0].z),
        );
        let distance = (point.x - target.x)
            .hypot(point.y - target.y)
            .hypot(point.z - target.z);
        assert!(distance <= bound);
    }
}

#[test]
fn direct_analytic_curve_inverses_preserve_native_parameters() {
    let geometries = [
        CurveGeometry::Line {
            origin: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        },
        CurveGeometry::Circle {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            ref_direction: Vector3::new(1.0, 0.0, 0.0),
            radius: 4.0,
        },
        CurveGeometry::Ellipse {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 4.0,
            minor_radius: 2.0,
        },
        CurveGeometry::Parabola {
            vertex: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            focal_distance: 2.0,
        },
        CurveGeometry::Hyperbola {
            center: Point3::new(1.0, 2.0, 3.0),
            axis: Vector3::new(0.0, 0.0, 1.0),
            major_direction: Vector3::new(1.0, 0.0, 0.0),
            major_radius: 4.0,
            minor_radius: 2.0,
        },
    ];
    for (index, geometry) in geometries.into_iter().enumerate() {
        let parameter = if matches!(
            &geometry,
            CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. }
        ) {
            0.7 + std::f64::consts::TAU
        } else {
            0.7
        };
        let point = curve_point(&geometry, parameter).expect("analytic curve evaluates");
        let id = CurveId(format!("test:inverse:{index}"));
        let mut ir = CadIr::empty(crate::units::Units::default());
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        let inverse = super::model_curve_parameter_near_point(&ir, &id, point, parameter)
            .expect("direct analytic inverse");
        assert!((inverse - parameter).abs() < 1.0e-12);
    }
}

#[test]
fn polyline_inverse_searches_every_segment_in_native_parameter_space() {
    let cases = [
        (
            CurveGeometry::Polyline {
                points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ],
                parameters: None,
                chordal_deflection: 0.0,
            },
            Point3::new(0.5, 0.0, 0.0),
            0.5,
            0.5,
        ),
        (
            CurveGeometry::Polyline {
                points: vec![
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1.0, 0.0, 0.0),
                    Point3::new(1.0, 1.0, 0.0),
                ],
                parameters: Some(vec![4.0, 2.0, 0.0]),
                chordal_deflection: 0.0,
            },
            Point3::new(1.0, 0.5, 0.0),
            1.0,
            1.0,
        ),
        (
            CurveGeometry::Polyline {
                points: vec![
                    Point3::new(2.0, 3.0, 4.0),
                    Point3::new(2.0, 3.0, 4.0),
                    Point3::new(5.0, 3.0, 4.0),
                ],
                parameters: Some(vec![0.0, 1.0, 2.0]),
                chordal_deflection: 0.0,
            },
            Point3::new(2.0, 3.0, 4.0),
            0.7,
            0.7,
        ),
    ];
    for (index, (geometry, point, seed, expected)) in cases.into_iter().enumerate() {
        let id = CurveId(format!("test:polyline-inverse:{index}"));
        let mut ir = CadIr::empty(crate::units::Units::default());
        ir.model.curves.push(Curve {
            id: id.clone(),
            geometry,
            source_object: None,
        });
        let inverse = super::model_curve_parameter_near_point(&ir, &id, point, seed)
            .expect("polyline inverse");
        assert!((inverse - expected).abs() < 1.0e-12);
    }
}

#[test]
fn transformed_curve_inverse_uses_the_basis_parameterization() {
    let basis = CurveGeometry::Circle {
        center: Point3::new(1.0, 2.0, 3.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 4.0,
    };
    let transform = Transform {
        rows: [
            [-2.0, 0.0, 0.0, 1.0e6],
            [0.0, 0.5, 0.0, -2.0e6],
            [0.0, 0.0, 3.0, 3.0e6],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let geometry = CurveGeometry::Transformed {
        basis: Box::new(basis.clone()),
        transform,
    };
    let parameter = 0.7 + std::f64::consts::TAU;
    let point = curve_point(&geometry, parameter).expect("transformed curve evaluates");
    let id = CurveId("test:transformed-inverse".into());
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry,
        source_object: None,
    });
    let inverse = super::model_curve_parameter_near_point(&ir, &id, point, parameter)
        .expect("transformed inverse");
    assert!((inverse - parameter).abs() < 1.0e-10);

    ir.model.curves[0].geometry = CurveGeometry::Transformed {
        basis: Box::new(basis),
        transform: Transform {
            rows: [
                [0.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        },
    };
    assert!(
        super::model_curve_parameter_near_point(&ir, &id, Point3::new(0.0, 0.0, 0.0), 0.0,)
            .is_none()
    );
}

#[test]
fn degenerate_curve_inverse_preserves_the_selected_parameter() {
    let point = Point3::new(2.0, 3.0, 4.0);
    let id = CurveId("test:degenerate-inverse".into());
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Degenerate { point },
        source_object: None,
    });
    let seed = 123.5;
    assert_eq!(
        super::model_curve_parameter_near_point(&ir, &id, point, seed),
        Some(seed)
    );
    assert!(
        super::model_curve_parameter_near_point(&ir, &id, Point3::new(2.0, 3.0, 5.0), seed,)
            .is_none()
    );
}

#[test]
fn a_surface_isoline_reproduces_the_surface_along_its_free_parameter() {
    // Rational, quadratic in u and linear in v, so the blend across the
    // fixed direction has to carry weights to stay exact.
    let surface = NurbsSurface {
        u_degree: 2,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![-2.0, -2.0, 3.0, 3.0],
        u_count: 3,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 4.0),
            Point3::new(1.0, 2.0, 0.5),
            Point3::new(1.0, 2.0, 4.5),
            Point3::new(3.0, -1.0, 1.0),
            Point3::new(3.0, -1.0, 5.0),
        ],
        weights: Some(vec![1.0, 2.0, 0.5, 1.5, 3.0, 0.25]),
        u_periodic: false,
        v_periodic: false,
    };

    for (direction, at, samples) in [
        (IsolineDirection::ConstantU, 0.4, [-2.0, 0.75, 3.0]),
        (IsolineDirection::ConstantV, 1.25, [0.0, 0.6, 1.0]),
    ] {
        let curve = nurbs_surface_isoline(&surface, direction, at).expect("isoline");
        for sample in samples {
            let (u, v) = match direction {
                IsolineDirection::ConstantU => (at, sample),
                IsolineDirection::ConstantV => (sample, at),
            };
            let expected = nurbs_surface_point(&surface, u, v).expect("surface point");
            let actual = nurbs_curve_point(
                curve.degree,
                &curve.knots,
                &curve.control_points,
                curve.weights.as_deref(),
                sample,
            )
            .expect("curve point");
            for (left, right) in [
                (actual.x, expected.x),
                (actual.y, expected.y),
                (actual.z, expected.z),
            ] {
                assert!((left - right).abs() <= 1.0e-12, "{left} vs {right}");
            }
        }
    }
}

#[test]
fn bilinear_surface_partials_follow_stored_parameterization() {
    let surface = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 3.0, 0.0),
        ],
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    let partials = nurbs_surface_partials(&surface, 0.25, 0.75).expect("partials");
    assert_eq!(partials.point, Point3::new(0.5, 2.25, 0.0));
    assert_eq!(partials.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 3.0, 0.0));
}

#[test]
fn quadratic_surface_second_partials_follow_stored_parameterization() {
    let surface = NurbsSurface {
        u_degree: 2,
        v_degree: 2,
        u_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        u_count: 3,
        v_count: 3,
        control_points: (0..3)
            .flat_map(|i| {
                (0..3).map(move |j| {
                    Point3::new(
                        f64::from(i) / 2.0,
                        f64::from(j) / 2.0,
                        f64::from(u8::from(i == 2)) + f64::from(u8::from(j == 2)),
                    )
                })
            })
            .collect(),
        weights: None,
        u_periodic: false,
        v_periodic: false,
    };
    let partials = nurbs_surface_second_partials(&surface, 0.25, 0.75).expect("second partials");
    assert_eq!(partials.point, Point3::new(0.25, 0.75, 0.625));
    assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.5));
    assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 1.5));
    assert_eq!(partials.duu, Vector3::new(0.0, 0.0, 2.0));
    assert_eq!(partials.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(partials.dvv, Vector3::new(0.0, 0.0, 2.0));
}

#[test]
fn recursive_offsets_use_exact_support_normals_at_large_parameters() {
    let support_id = SurfaceId("support".into());
    let first_id = SurfaceId("first-offset".into());
    let second_id = SurfaceId("second-offset".into());
    let first_construction = ProceduralSurfaceId("first-construction".into());
    let second_construction = ProceduralSurfaceId("second-construction".into());
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.surfaces = vec![
        Surface {
            id: support_id.clone(),
            geometry: SurfaceGeometry::Plane {
                origin: Point3::new(0.0, 0.0, 0.0),
                normal: Vector3::new(0.0, 0.0, 1.0),
                u_axis: Vector3::new(1.0, 0.0, 0.0),
            },
            source_object: None,
        },
        Surface {
            id: first_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: first_construction.clone(),
            },
            source_object: None,
        },
        Surface {
            id: second_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: second_construction.clone(),
            },
            source_object: None,
        },
    ];
    ir.model.procedural_surfaces = vec![
        ProceduralSurface {
            id: first_construction,
            surface: first_id.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: support_id,
                distance: 2.0,
                u_sense: None,
                v_sense: None,
                support_extension: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        },
        ProceduralSurface {
            id: second_construction,
            surface: second_id.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: first_id,
                distance: -5.0,
                u_sense: None,
                v_sense: None,
                support_extension: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        },
    ];

    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &second_id, 1.0e16, -1.0e16),
        Some(Point3::new(1.0e16, -1.0e16, -3.0))
    );
    let budget = WorkBudget::new(2);
    assert!(
        model_surface_point_by_id_with_budget(&index, &second_id, 1.0e16, -1.0e16, &budget,)
            .is_none()
    );
    assert!(budget.exhausted());
    let budget = WorkBudget::new(3);
    assert_eq!(
        model_surface_point_by_id_with_budget(&index, &second_id, 1.0e16, -1.0e16, &budget,),
        Some(Point3::new(1.0e16, -1.0e16, -3.0))
    );
    assert_eq!(budget.consumed(), 3);
    let partials = model_surface_partials_by_id(&index, &second_id, 1.0e16, -1.0e16)
        .expect("transformed plane evaluates");
    assert_eq!(partials.point, Point3::new(1.0e16, -1.0e16, -3.0));
    assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn linear_offset_support_extension_uses_the_boundary_tangent_plane() {
    let support_id = SurfaceId("support".into());
    let offset_id = SurfaceId("offset".into());
    let construction = ProceduralSurfaceId("offset-construction".into());
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.surfaces = vec![
        Surface {
            id: support_id.clone(),
            geometry: SurfaceGeometry::Nurbs(NurbsSurface {
                u_degree: 1,
                v_degree: 2,
                u_knots: vec![0.0, 0.0, 1.0, 1.0],
                v_knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                u_count: 2,
                v_count: 3,
                control_points: [0.0, 1.0]
                    .into_iter()
                    .flat_map(|u| {
                        [(0.0, 0.0), (0.5, 0.0), (1.0, 1.0)]
                            .into_iter()
                            .map(move |(v, z)| Point3::new(u, v, z))
                    })
                    .collect(),
                weights: None,
                u_periodic: false,
                v_periodic: false,
            }),
            source_object: None,
        },
        Surface {
            id: offset_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
            },
            source_object: None,
        },
    ];
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: construction,
        surface: offset_id.clone(),
        definition: ProceduralSurfaceDefinition::Offset {
            support: support_id,
            distance: 0.0,
            u_sense: None,
            v_sense: None,
            support_extension: Some(OffsetSupportExtension::Linear),
            extension_flags: Vec::new(),
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });
    let index = crate::index::ModelIndex::new(&ir);

    let point =
        model_surface_point_by_id(&index, &offset_id, 0.25, 1.2).expect("linearly extended offset");

    let epsilon = 64.0 * f64::EPSILON;
    assert!((point.x - 0.25).abs() <= epsilon);
    assert!((point.y - 1.2).abs() <= epsilon);
    assert!((point.z - 1.4).abs() <= epsilon);
}

#[test]
fn offset_of_reversed_subset_uses_the_local_surface_normal() {
    let base_id = SurfaceId("base".into());
    let subset_id = SurfaceId("subset".into());
    let offset_id = SurfaceId("offset".into());
    let subset_construction = ProceduralSurfaceId("subset-construction".into());
    let offset_construction = ProceduralSurfaceId("offset-construction".into());
    let plane = SurfaceGeometry::Plane {
        origin: Point3::new(0.0, 0.0, 0.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        u_axis: Vector3::new(1.0, 0.0, 0.0),
    };
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.surfaces = vec![
        Surface {
            id: base_id.clone(),
            geometry: plane.clone(),
            source_object: None,
        },
        Surface {
            id: subset_id.clone(),
            geometry: plane,
            source_object: None,
        },
        Surface {
            id: offset_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
    ];
    ir.model.procedural_surfaces = vec![
        ProceduralSurface {
            id: subset_construction,
            surface: subset_id.clone(),
            definition: ProceduralSurfaceDefinition::Subset {
                support: base_id,
                parameter_ranges: [[0.0, 1.0], [0.0, 1.0]],
                u_sense: Some(false),
                v_sense: Some(true),
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        },
        ProceduralSurface {
            id: offset_construction,
            surface: offset_id.clone(),
            definition: ProceduralSurfaceDefinition::Offset {
                support: subset_id,
                distance: 2.0,
                u_sense: None,
                v_sense: None,
                support_extension: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        },
    ];

    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &offset_id, 0.25, 0.5),
        Some(Point3::new(-0.25, 0.5, -2.0))
    );
    let partials = model_surface_partials_by_id(&index, &offset_id, 0.25, 0.5)
        .expect("offset of a reversed subset evaluates");
    assert_eq!(partials.point, Point3::new(-0.25, 0.5, -2.0));
    assert_eq!(partials.du, Vector3::new(-1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn linear_sweep_surface_evaluation_uses_directrix_and_sweep_parameters() {
    let directrix_id = CurveId("directrix".into());
    let surface_id = SurfaceId("sweep".into());
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(2.0, 0.0, 0.0),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("sweep-construction".into()),
        surface: surface_id.clone(),
        definition: ProceduralSurfaceDefinition::LinearSweep {
            directrix: directrix_id,
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let index = crate::index::ModelIndex::new(&ir);
    let point =
        model_surface_point_by_id(&index, &surface_id, 0.5, 4.0).expect("linear sweep point");
    assert_eq!(point, Point3::new(2.0, 2.0, 7.0));
    let partials =
        model_surface_partials_by_id(&index, &surface_id, 0.5, 4.0).expect("linear sweep partials");
    assert_eq!(partials.point, point);
    assert_eq!(partials.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 0.0, 1.0));
    let second_partials = model_surface_second_partials_by_id(&index, &surface_id, 0.5, 4.0)
        .expect("linear sweep second partials");
    assert_eq!(second_partials.duu, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(second_partials.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(second_partials.dvv, Vector3::new(0.0, 0.0, 0.0));
}

#[test]
fn axis_revolution_surface_evaluation_rotates_the_profile_parameterization() {
    let directrix_id = CurveId("profile".into());
    let surface_id = SurfaceId("revolution".into());
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("revolution-construction".into()),
        surface: surface_id.clone(),
        definition: ProceduralSurfaceDefinition::AxisRevolution {
            directrix: directrix_id,
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let index = crate::index::ModelIndex::new(&ir);
    let point = model_surface_point_by_id(&index, &surface_id, std::f64::consts::FRAC_PI_2, 1.5)
        .expect("axis revolution point");
    assert!(point.x.abs() < 1.0e-12);
    assert!((point.y - 2.0).abs() < 1.0e-12);
    assert!((point.z - 1.5).abs() < 1.0e-12);
    let partials =
        model_surface_partials_by_id(&index, &surface_id, std::f64::consts::FRAC_PI_2, 1.5)
            .expect("axis revolution partials");
    assert!((partials.du.x + 2.0).abs() < 1.0e-12);
    assert!(partials.du.y.abs() < 1.0e-12);
    assert_eq!(partials.dv, Vector3::new(0.0, 0.0, 1.0));
    let second_partials =
        model_surface_second_partials_by_id(&index, &surface_id, std::f64::consts::FRAC_PI_2, 1.5)
            .expect("axis revolution second partials");
    assert!((second_partials.duu.y + 2.0).abs() < 1.0e-12);
    assert!(second_partials.duv.norm() < 1.0e-12);
    assert!(second_partials.dvv.norm() < 1.0e-12);
}

#[test]
fn revolution_surface_maps_its_angular_parameter_interval() {
    let directrix_id = CurveId("mapped-profile".into());
    let surface_id = SurfaceId("mapped-revolution".into());
    let mut ir = CadIr::empty(crate::units::Units::default());
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Line {
            origin: Point3::new(2.0, 0.0, 0.0),
            direction: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface {
        id: ProceduralSurfaceId("mapped-revolution-construction".into()),
        surface: surface_id.clone(),
        definition: ProceduralSurfaceDefinition::Revolution {
            directrix: directrix_id,
            axis_origin: Point3::new(0.0, 0.0, 0.0),
            axis_direction: Vector3::new(0.0, 0.0, 1.0),
            angular_interval: [0.0, std::f64::consts::PI],
            angular_parameter_interval: Some([10.0, 14.0]),
            parameter_interval: None,
            transposed: false,
            revision_form: None,
        },
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let index = crate::index::ModelIndex::new(&ir);
    let partials = model_surface_second_partials_by_id(&index, &surface_id, 1.5, 12.0)
        .expect("mapped revolution partials");
    assert!(partials.point.x.abs() < 1.0e-12);
    assert!((partials.point.y - 2.0).abs() < 1.0e-12);
    assert!((partials.point.z - 1.5).abs() < 1.0e-12);
    assert_eq!(partials.du, Vector3::new(0.0, 0.0, 1.0));
    assert!((partials.dv.x + std::f64::consts::FRAC_PI_2).abs() < 1.0e-12);
    assert!(partials.dv.y.abs() < 1.0e-12);
    assert!(partials.dvv.x.abs() < 1.0e-12);
    assert!((partials.dvv.y + std::f64::consts::PI.powi(2) / 8.0).abs() < 1.0e-12);
    assert!(partials.duv.norm() < 1.0e-12);
}

#[test]
fn analytic_and_transformed_surface_partials_follow_parameterization() {
    let cylinder = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
    };
    let cone = SurfaceGeometry::Cone {
        origin: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 2.0,
        ratio: 1.0,
        half_angle: std::f64::consts::FRAC_PI_4,
    };
    let sphere = SurfaceGeometry::Sphere {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.0,
    };
    let torus = SurfaceGeometry::Torus {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        major_radius: 5.0,
        minor_radius: 2.0,
    };
    let transformed = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform: Transform {
            rows: [
                [2.0, 0.0, 0.0, 7.0],
                [0.0, 3.0, 0.0, 11.0],
                [0.0, 0.0, 4.0, 13.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        },
    };

    let cylinder_second =
        surface_second_partials(&cylinder, 0.0, 4.0).expect("cylinder second partials evaluate");
    let cylinder = surface_partials(&cylinder, 0.0, 4.0).expect("cylinder partials evaluate");
    assert_eq!(cylinder.point, Point3::new(2.0, 0.0, 4.0));
    assert_eq!(cylinder.du, Vector3::new(0.0, 2.0, 0.0));
    assert_eq!(cylinder.dv, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(cylinder_second.duu, Vector3::new(-2.0, 0.0, 0.0));
    assert_eq!(cylinder_second.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(cylinder_second.dvv, Vector3::new(0.0, 0.0, 0.0));
    let cone = surface_partials(&cone, 0.0, 3.0).expect("cone partials evaluate");
    assert!((cone.point.x - 5.0).abs() < 1e-12);
    assert!((cone.du.y - 5.0).abs() < 1e-12);
    assert!((cone.dv.x - 1.0).abs() < 1e-12);
    assert_eq!(cone.dv.z, 1.0);
    let sphere = surface_partials(&sphere, 0.0, 0.0).expect("sphere partials evaluate");
    assert_eq!(sphere.point, Point3::new(3.0, 0.0, 0.0));
    assert_eq!(sphere.du, Vector3::new(0.0, 3.0, 0.0));
    assert_eq!(sphere.dv, Vector3::new(0.0, 0.0, 3.0));
    let torus = surface_partials(&torus, 0.0, 0.0).expect("torus partials evaluate");
    assert_eq!(torus.point, Point3::new(7.0, 0.0, 0.0));
    assert_eq!(torus.du, Vector3::new(0.0, 7.0, 0.0));
    assert_eq!(torus.dv, Vector3::new(0.0, 0.0, 2.0));
    let transformed =
        surface_partials(&transformed, 2.0, 3.0).expect("transformed partials evaluate");
    assert_eq!(transformed.point, Point3::new(11.0, 20.0, 13.0));
    assert_eq!(transformed.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(transformed.dv, Vector3::new(0.0, 3.0, 0.0));
}

#[test]
fn analytic_and_rational_curve_derivatives_are_exact() {
    let parameter = 1.0e16;
    let circle = CurveGeometry::Circle {
        center: Point3::new(0.0, 0.0, 0.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 3.0,
    };
    let tangent = curve_tangent(&circle, parameter).expect("analytic tangent");
    assert_eq!(
        tangent,
        Vector3::new(-3.0 * parameter.sin(), 3.0 * parameter.cos(), 0.0)
    );
    assert_eq!(
        curve_second_derivative(&circle, parameter),
        Some(Vector3::new(
            -3.0 * parameter.cos(),
            -3.0 * parameter.sin(),
            0.0,
        ))
    );
    assert_eq!(curve_tangent(&circle, f64::NAN), None);

    let arc = CurveGeometry::Nurbs(NurbsCurve {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        control_points: vec![
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
        ],
        weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
        periodic: false,
    });
    for parameter in [0.0, 0.5, 1.0] {
        let point = curve_point(&arc, parameter).expect("rational arc point");
        let tangent = curve_tangent(&arc, parameter).expect("rational arc tangent");
        let second = curve_second_derivative(&arc, parameter).expect("rational arc acceleration");
        let radial_dot = point.x * tangent.x + point.y * tangent.y;
        assert!(radial_dot.abs() < 1e-12);
        assert!((point.x * second.x + point.y * second.y + tangent.dot(tangent)).abs() < 1e-11);
        assert!(tangent.norm() > 0.0);
    }

    let corner = CurveGeometry::Polyline {
        points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        parameters: Some(vec![0.0, 1.0, 2.0]),
        chordal_deflection: 0.0,
    };
    assert_eq!(
        curve_tangent(&corner, 0.5),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(curve_tangent(&corner, 1.0), None);
}

#[test]
fn rational_surface_partials_apply_the_weight_quotient_rule() {
    let surface = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 3.0, 0.0),
        ],
        weights: Some(vec![1.0, 1.0, 2.0, 2.0]),
        u_periodic: false,
        v_periodic: false,
    };
    let partials = nurbs_surface_partials(&surface, 0.5, 0.25).expect("partials");
    assert!((partials.point.x - 4.0 / 3.0).abs() < 1e-12);
    assert!((partials.point.y - 0.75).abs() < 1e-12);
    assert!((partials.du.x - 16.0 / 9.0).abs() < 1e-12);
    assert!(partials.du.y.abs() < 1e-12);
    assert!((partials.dv.y - 3.0).abs() < 1e-12);
    let second = nurbs_surface_second_partials(&surface, 0.5, 0.25).expect("second partials");
    assert!((second.duu.x + 64.0 / 27.0).abs() < 1e-12);
    assert!(second.duu.y.abs() < 1e-12);
    assert_eq!(second.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(second.dvv, Vector3::new(0.0, 0.0, 0.0));
}

#[test]
fn rational_surface_isocurves_preserve_the_tensor_product_parameterization() {
    let surface = NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(2.0, 0.0, 1.0),
            Point3::new(2.0, 3.0, 1.0),
        ],
        weights: Some(vec![1.0, 2.0, 3.0, 4.0]),
        u_periodic: false,
        v_periodic: false,
    };
    for (axis, fixed) in [
        (SurfaceParameterAxis::U, 0.25),
        (SurfaceParameterAxis::V, 0.75),
    ] {
        let isocurve = nurbs_surface_isocurve(&surface, axis, fixed).expect("exact isocurve");
        let geometry = CurveGeometry::Nurbs(isocurve);
        for varying in [0.0, 0.2, 0.7, 1.0] {
            let expected = match axis {
                SurfaceParameterAxis::U => {
                    nurbs_surface_point(&surface, fixed, varying).expect("surface point")
                }
                SurfaceParameterAxis::V => {
                    nurbs_surface_point(&surface, varying, fixed).expect("surface point")
                }
            };
            let actual = curve_point(&geometry, varying).expect("isocurve point");
            assert!((actual.x - expected.x).abs() < 1e-12);
            assert!((actual.y - expected.y).abs() < 1e-12);
            assert!((actual.z - expected.z).abs() < 1e-12);
        }
    }
}

#[test]
fn nurbs_curve_inverse_uses_the_seed_to_select_an_ambiguous_witness() {
    let curve = NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 0.5, 1.0, 1.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: false,
    };
    let point = Point3::new(0.5, 0.0, 0.0);
    assert_eq!(
        nurbs_curve_parameter_near_point(&curve, point, 1.0e-12, 0.1),
        Some(0.25)
    );
    assert_eq!(
        nurbs_curve_parameter_near_point(&curve, point, 1.0e-12, 0.9),
        Some(0.75)
    );
    assert_eq!(
        nurbs_curve_parameter_near_point(&curve, Point3::new(0.5, 1.0, 0.0), 1.0e-12, 0.5,),
        None
    );
    assert!(nurbs_curve_speed_bound(&curve).is_some_and(|bound| bound >= 2.0));
}

#[test]
fn bounded_nurbs_interval_search_keeps_a_fixed_working_set() {
    let boundaries = (0..=10_000).map(f64::from).collect::<Vec<_>>();
    let intervals = super::bounded_nearest_intervals(&boundaries, 5_000.5);

    assert_eq!(intervals.len(), 512);
    assert!(intervals.contains(&[5_000.0, 5_001.0]));
}

#[test]
fn bounded_nurbs_containment_search_keeps_the_final_valid_spans() {
    let boundaries = [0.0, 1.0, 1.0, 2.0, 3.0];

    assert_eq!(
        super::bounded_tail_intervals(&boundaries),
        (vec![[0.0, 1.0], [1.0, 2.0], [2.0, 3.0]], false)
    );

    let many_boundaries = (0..=10_000).map(f64::from).collect::<Vec<_>>();
    let (intervals, truncated) = super::bounded_tail_intervals(&many_boundaries);
    assert_eq!(intervals.len(), 512);
    assert!(truncated);
}

#[test]
fn bounded_nurbs_boundary_witness_preserves_seed_priority() {
    let boundaries = [0.0, 1.0, 2.0];

    assert_eq!(
        super::nearest_boundary_witness(&boundaries, 1.4, 0.0, |_| Some(0.0)),
        super::BoundaryWitness::Found(1.0)
    );
}

#[test]
fn analytic_pcurves_preserve_angular_parameterization() {
    let circle = PcurveGeometry::Circle {
        center: Point2::new(2.0, 3.0),
        x_axis: Point2::new(1.0, 0.0),
        y_axis: Point2::new(0.0, -1.0),
        radius: 4.0,
    };
    let ellipse = PcurveGeometry::Ellipse {
        center: Point2::new(2.0, 3.0),
        x_axis: Point2::new(0.0, 1.0),
        y_axis: Point2::new(-1.0, 0.0),
        major_radius: 4.0,
        minor_radius: 2.0,
    };
    let polar = PcurveGeometry::PolarHarmonic {
        radial_center: Point2::new(0.0, 0.0),
        radial_cos: Point2::new(2.0, 0.0),
        radial_sin: Point2::new(0.0, 2.0),
        axial_origin: 3.0,
        axial_cos: 4.0,
        axial_sin: 0.0,
    };
    let polar_nurbs = PcurveGeometry::PolarNurbs {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        radial_control_points: vec![
            Point2::new(2.0, 0.0),
            Point2::new(2.0, 2.0),
            Point2::new(0.0, 2.0),
        ],
        axial_control_points: vec![3.0, 4.0, 5.0],
        weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
        periodic: false,
    };

    let circle_tangent =
        pcurve_tangent(&circle, std::f64::consts::FRAC_PI_2).expect("circle tangent");
    let circle = pcurve_uv(&circle, std::f64::consts::FRAC_PI_2).expect("circle evaluates");
    let ellipse = pcurve_uv(&ellipse, std::f64::consts::FRAC_PI_2).expect("ellipse evaluates");
    let polar = pcurve_uv(&polar, std::f64::consts::FRAC_PI_2).expect("polar curve evaluates");
    let polar_nurbs = pcurve_uv(&polar_nurbs, 0.5).expect("polar NURBS evaluates");
    assert!((circle.u - 2.0).abs() < 1e-12 && (circle.v + 1.0).abs() < 1e-12);
    assert!((circle_tangent.u + 4.0).abs() < 1e-12 && circle_tangent.v.abs() < 1e-12);
    assert!(ellipse.u.abs() < 1e-12 && (ellipse.v - 3.0).abs() < 1e-12);
    assert!((polar.u - std::f64::consts::FRAC_PI_2).abs() < 1e-12);
    assert!((polar.v - 3.0).abs() < 1e-12);
    assert!((polar_nurbs.u - std::f64::consts::FRAC_PI_4).abs() < 1e-12);
    assert!((polar_nurbs.v - 4.0).abs() < 1e-12);
}

#[test]
fn spherical_great_circle_pcurve_preserves_affine_source_parameterization() {
    let geometry = PcurveGeometry::SphericalGreatCircle {
        azimuth_origin: 0.25,
        azimuth_rate: 0.5,
        plane_phase: 1.0,
        plane_slope: -0.75,
    };
    let point = pcurve_uv(&geometry, 1.5).expect("great-circle pcurve evaluates");
    assert_eq!(point.u, 1.0);
    assert_eq!(point.v, (-0.75_f64).atan());
}

#[test]
fn general_harmonic_pcurves_evaluate_their_vector_coefficients() {
    let harmonic = PcurveGeometry::Harmonic {
        center: Point2::new(2.0, 3.0),
        cosine: Point2::new(4.0, -1.0),
        sine: Point2::new(2.0, 5.0),
    };
    let hyperbolic = PcurveGeometry::Hyperbolic {
        center: Point2::new(-3.0, 7.0),
        cosine: Point2::new(2.5, -4.0),
        sine: Point2::new(1.5, 0.75),
    };
    let angle = std::f64::consts::FRAC_PI_3;
    assert_eq!(
        pcurve_uv(&harmonic, angle),
        Some(Point2::new(
            2.0 + 4.0 * angle.cos() + 2.0 * angle.sin(),
            3.0 - angle.cos() + 5.0 * angle.sin(),
        ))
    );
    let parameter = 0.75_f64;
    assert_eq!(
        pcurve_uv(&hyperbolic, parameter),
        Some(Point2::new(
            -3.0 + 2.5 * parameter.cosh() + 1.5 * parameter.sinh(),
            7.0 - 4.0 * parameter.cosh() + 0.75 * parameter.sinh(),
        ))
    );
}

#[test]
fn transformed_pcurves_apply_the_map_to_all_differential_orders() {
    let geometry = PcurveGeometry::Transformed {
        basis: Box::new(PcurveGeometry::Parabola {
            vertex: Point2::new(1.0, 2.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            focal_distance: 0.5,
        }),
        transform: Transform2 {
            rows: [[0.0, -2.0, 10.0], [2.0, 0.0, 20.0], [0.0, 0.0, 1.0]],
        },
    };

    assert_eq!(pcurve_uv(&geometry, 2.0), Some(Point2::new(2.0, 26.0)));
    assert_eq!(pcurve_tangent(&geometry, 2.0), Some(Point2::new(-2.0, 4.0)));
    let differential =
        pcurve_uv_differential_inner(&geometry, 2.0, 0).expect("transformed pcurve differential");
    assert_eq!(differential.acceleration, Some(Point2::new(0.0, 2.0)));
}

#[test]
fn signed_offset_pcurves_use_the_exact_left_normal() {
    let line = PcurveGeometry::Offset {
        distance: 2.0,
        basis: Box::new(PcurveGeometry::Line {
            origin: Point2::new(1.0, 2.0),
            direction: Point2::new(3.0, 4.0),
        }),
    };
    let circle = PcurveGeometry::Offset {
        distance: 1.0,
        basis: Box::new(PcurveGeometry::Circle {
            center: Point2::new(0.0, 0.0),
            x_axis: Point2::new(1.0, 0.0),
            y_axis: Point2::new(0.0, 1.0),
            radius: 4.0,
        }),
    };
    let point = pcurve_uv(&line, 0.5).expect("regular line offset evaluates");
    assert!((point.u - 0.9).abs() < 1e-12);
    assert!((point.v - 5.2).abs() < 1e-12);
    assert_eq!(pcurve_uv(&circle, 0.0), Some(Point2::new(3.0, 0.0)));
    assert_eq!(pcurve_tangent(&line, 0.5), Some(Point2::new(3.0, 4.0)));
    assert_eq!(pcurve_tangent(&circle, 0.0), Some(Point2::new(0.0, 3.0)));

    let rational_arc = PcurveGeometry::Offset {
        distance: 0.25,
        basis: Box::new(PcurveGeometry::Nurbs {
            degree: 2,
            knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            control_points: vec![
                Point2::new(1.0, 0.0),
                Point2::new(1.0, 1.0),
                Point2::new(0.0, 1.0),
            ],
            weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            periodic: false,
        }),
    };
    for parameter in [0.0, 0.5, 1.0] {
        let point =
            pcurve_uv(&rational_arc, parameter).expect("regular rational NURBS offset evaluates");
        let tangent = pcurve_tangent(&rational_arc, parameter).expect("rational offset tangent");
        assert!((point.u.hypot(point.v) - 0.75).abs() < 1e-12);
        assert!((point.u * tangent.u + point.v * tangent.v).abs() < 1e-12);
    }

    let nested = PcurveGeometry::Offset {
        distance: 1.0,
        basis: Box::new(line),
    };
    let nested_point = pcurve_uv(&nested, 0.5).expect("nested offset point");
    assert!((nested_point.u - 0.1).abs() < 1e-12);
    assert!((nested_point.v - 5.8).abs() < 1e-12);
    assert_eq!(pcurve_tangent(&nested, 0.5), None);
}

#[test]
fn periodic_nurbs_parameters_preserve_phase_and_wrap_for_evaluation() {
    let nurbs = crate::geometry::NurbsCurve {
        degree: 1,
        knots: vec![0.0, 0.0, 1.0, 2.0, 2.0],
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        weights: None,
        periodic: true,
    };
    let geometry = CurveGeometry::Nurbs(nurbs.clone());
    assert_eq!(
        crate::eval::curve_point(&geometry, 0.5),
        crate::eval::curve_point(&geometry, 2.5)
    );

    let mut ir = unit_cube();
    let curve_id = ir.model.edges[0].curve.clone().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = geometry;
    ir.model.edges[0].param_range = Some([0.5, 2.5]);
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    ir.model.edges[0].param_range = Some([0.5, 2.500_001]);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    let CurveGeometry::Nurbs(nurbs) = &mut ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry
    else {
        unreachable!()
    };
    nurbs.periodic = false;
    ir.model.edges[0].param_range = Some([0.5, 2.5]);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}

#[test]
fn rational_quadratic_arc_evaluates_on_the_circle() {
    // Quarter circle of radius 5 as a rational quadratic Bezier.
    let weight = 0.5_f64.sqrt();
    let point = crate::eval::nurbs_curve_point(
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(5.0, 5.0, 0.0),
            Point3::new(0.0, 5.0, 0.0),
        ],
        Some(&[1.0, weight, 1.0]),
        0.5,
    )
    .unwrap();
    let radius = (point.x * point.x + point.y * point.y).sqrt();
    assert!((radius - 5.0).abs() < 1e-12, "mid-span radius {radius}");
}

#[test]
fn rational_pcurve_membership_finds_interior_points_without_sampling() {
    use crate::math::Point2;

    let weight = 0.5_f64.sqrt();
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let controls = [
        Point2::new(5.0, 0.0),
        Point2::new(5.0, 5.0),
        Point2::new(0.0, 5.0),
    ];
    let weights = [1.0, weight, 1.0];
    let interior =
        crate::eval::nurbs_pcurve_uv(2, &knots, &controls, Some(&weights), 0.375).unwrap();
    assert_eq!(
        crate::eval::nurbs_pcurve_contains_point(
            2,
            &knots,
            &controls,
            Some(&weights),
            interior,
            1.0e-9,
        ),
        Some(true)
    );
    assert_eq!(
        crate::eval::nurbs_pcurve_contains_point(
            2,
            &knots,
            &controls,
            Some(&weights),
            Point2::new(4.0, 4.0),
            1.0e-6,
        ),
        Some(false)
    );
}

#[test]
fn analytic_parabola_and_hyperbola_use_step_parameterization() {
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let major = Vector3::new(1.0, 0.0, 0.0);
    let parabola = CurveGeometry::Parabola {
        vertex: Point3::new(0.0, 0.0, 0.0),
        axis,
        major_direction: major,
        focal_distance: 2.0,
    };
    assert_eq!(
        crate::eval::curve_point(&parabola, 1.5),
        Some(Point3::new(4.5, 6.0, 0.0))
    );

    let hyperbola = CurveGeometry::Hyperbola {
        center: Point3::new(1.0, 2.0, 3.0),
        axis,
        major_direction: major,
        major_radius: 2.0,
        minor_radius: 3.0,
    };
    let point = crate::eval::curve_point(&hyperbola, 0.5).unwrap();
    assert_eq!(point.x, 1.0 + 2.0 * 0.5_f64.cosh());
    assert_eq!(point.y, 2.0 + 3.0 * 0.5_f64.sinh());
    assert_eq!(point.z, 3.0);
}

#[test]
fn transformed_carriers_preserve_basis_parameters() {
    let transform = crate::transform::Transform {
        rows: [
            [-2.0, 0.0, 0.0, 4.0],
            [0.0, 2.0, 0.0, 5.0],
            [0.0, 0.0, 2.0, 6.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let curve = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Line {
            origin: Point3::new(1.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    assert_eq!(
        crate::eval::curve_point(&curve, 3.0),
        Some(Point3::new(-4.0, 5.0, 6.0))
    );

    let surface = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    assert_eq!(
        crate::eval::surface_point(&surface, 2.0, 3.0),
        Some(Point3::new(0.0, 11.0, 6.0))
    );
}

#[test]
fn polyline_carriers_evaluate_in_both_parameter_directions() {
    let increasing = CurveGeometry::Polyline {
        points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        parameters: Some(vec![1.0, 3.0]),
        chordal_deflection: 0.01,
    };
    assert_eq!(
        crate::eval::curve_point(&increasing, 2.0),
        Some(Point3::new(1.0, 0.0, 0.0))
    );

    let decreasing = CurveGeometry::Polyline {
        points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        parameters: Some(vec![3.0, 1.0]),
        chordal_deflection: 0.01,
    };
    assert_eq!(
        crate::eval::curve_point(&decreasing, 2.5),
        Some(Point3::new(0.5, 0.0, 0.0))
    );
}
