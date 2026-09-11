// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::*;
use crate::examples::unit_cube;
use crate::geometry::{
    BlendCrossSection, BlendRadiusLaw, BlendSupport, Curve, CurveGeometry, LawExpression,
    LawFormula, LegacyExtensionFlags, NurbsCurve, NurbsSurface, OffsetExtension, PcurveGeometry,
    PolylineCurve, ProceduralCurve, ProceduralSurface, ProceduralSurfaceDefinition,
    RevisionCacheForm, RevisionSurfaceForm, RevisionSurfaceParameterization,
    RollingBallConstruction, RollingBallJetDerivative, RollingBallJetSite,
    RollingBallRadiusSelector, RollingBallSide, Surface, SurfaceGeometry, SurfaceParameterAxis,
    SweepRevisionForm, SweepSurfaceConstruction, SweepSurfaceLayout, VariableBlendConstruction,
    VariableBlendConvexity, VariableBlendCrossSection, VariableBlendRadii, VariableBlendRenderMode,
    VariableBlendSupportKind, VariableBlendSurfaceSubtype, VariableBlendValue,
    VariableBlendValuePayload,
};
use crate::ids::{CurveId, EdgeId, PointId, ProceduralSurfaceId, SurfaceId, VertexId};
use crate::math::{Point2, Point3, Vector3};
use crate::report::Check;
use crate::topology::{Edge, Point, Vertex};
use crate::transform::{Transform, Transform2};
use crate::validate::validate_neutral;
use crate::CadIr;
use cadmpeg_core::decode::WorkBudget;

macro_rules! procedural_surface {
    (
        id: $id:expr,
        definition: $definition:expr,
        cache_fit_tolerance: $cache_fit_tolerance:expr,
        record_bounds: $record_bounds:expr $(,)?
    ) => {
        ProceduralSurface::try_new($id, $definition, $cache_fit_tolerance, $record_bounds)
            .expect("valid procedural surface fixture")
    };
}

macro_rules! procedural_curve {
    (
        id: $id:expr,
        definition: $definition:expr,
        cache_fit_tolerance: $cache_fit_tolerance:expr $(,)?
    ) => {
        ProceduralCurve::try_new($id, $definition, $cache_fit_tolerance)
            .expect("valid procedural curve fixture")
    };
}

mod helix;
mod law_sweep;
mod pcurves;
mod procedural_curves;
mod ruled_sum;
mod variable_blend;

const EPS_DEGREE_ZERO_SURFACE_BOUND: f64 = 1.0e-12;

fn bilinear_surface() -> NurbsSurface {
    NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        None,
        false,
        false,
        false,
    )
    .unwrap()
}

#[test]
fn periodic_nurbs_surface_coordinates_reduce_into_the_knot_domain() {
    let mut surface = bilinear_surface();
    surface.set_u_periodic(true);
    let expected = nurbs_surface_point(&surface, 0.25, 0.75).expect("in-domain surface point");
    assert_eq!(nurbs_surface_point(&surface, 1.25, 0.75), Some(expected));
    assert_eq!(nurbs_surface_point(&surface, -0.75, 0.75), Some(expected));

    surface.set_u_periodic(false);
    assert_ne!(nurbs_surface_point(&surface, 1.25, 0.75), Some(expected));
}

#[test]
fn rolling_ball_jet_evaluation_interpolates_spine_and_sweeps_arc() {
    const TEST_TOLERANCE: f64 = 1e-12;
    let derivative = RollingBallJetDerivative {
        first_limit: Vector3::new(1.0 / 3.0, 0.0, 0.0),
        second_limit: Vector3::new(1.0 / 3.0, 0.0, 0.0),
        center: Vector3::new(1.0 / 3.0, 0.0, 0.0),
        angle: 0.0,
    };
    let definition = ProceduralSurfaceDefinition::RollingBallJet(
        crate::geometry::RollingBallJetStations::try_new(
            5,
            vec![
                crate::geometry::RollingBallJetStation {
                    knot: 2.0,
                    multiplicity: 6,
                    site: RollingBallJetSite {
                        first_limit: Point3::new(2.0, 0.0, 0.0),
                        second_limit: Point3::new(0.0, 2.0, 0.0),
                        center: Point3::new(0.0, 0.0, 0.0),
                        angle: std::f64::consts::FRAC_PI_2,
                        first_derivative: derivative.clone(),
                        second_derivative: RollingBallJetDerivative {
                            first_limit: Vector3::new(0.0, 0.0, 0.0),
                            second_limit: Vector3::new(0.0, 0.0, 0.0),
                            center: Vector3::new(0.0, 0.0, 0.0),
                            angle: 0.0,
                        },
                    },
                },
                crate::geometry::RollingBallJetStation {
                    knot: 5.0,
                    multiplicity: 6,
                    site: RollingBallJetSite {
                        first_limit: Point3::new(3.0, 0.0, 0.0),
                        second_limit: Point3::new(1.0, 2.0, 0.0),
                        center: Point3::new(1.0, 0.0, 0.0),
                        angle: std::f64::consts::FRAC_PI_2,
                        first_derivative: derivative,
                        second_derivative: RollingBallJetDerivative {
                            first_limit: Vector3::new(0.0, 0.0, 0.0),
                            second_limit: Vector3::new(0.0, 0.0, 0.0),
                            center: Vector3::new(0.0, 0.0, 0.0),
                            angle: 0.0,
                        },
                    },
                },
            ],
        )
        .unwrap(),
    );

    let point = rolling_ball_jet_point(&definition, 3.5, 0.5).expect("jet point");
    let expected = Point3::new(0.5 + 2.0_f64.sqrt(), 2.0_f64.sqrt(), 0.0);
    assert!((point.x - expected.x).abs() <= TEST_TOLERANCE);
    assert!((point.y - expected.y).abs() <= TEST_TOLERANCE);
    assert!((point.z - expected.z).abs() <= TEST_TOLERANCE);
    let start = rolling_ball_jet_point(&definition, 2.0, 0.0).expect("jet start");
    assert!((start.x - 2.0).abs() <= TEST_TOLERANCE);
    assert!(start.y.abs() <= TEST_TOLERANCE);
    assert!(start.z.abs() <= TEST_TOLERANCE);
    assert!(rolling_ball_jet_point(&definition, 5.0, 1.0).is_some());
    assert!(rolling_ball_jet_point(&definition, 1.0, 0.5).is_none());
}

#[test]
fn rolling_ball_jet_evaluation_uses_fixed_radius_frame() {
    const TEST_TOLERANCE: f64 = 1e-12;
    let zero = RollingBallJetDerivative {
        first_limit: Vector3::new(0.0, 0.0, 0.0),
        second_limit: Vector3::new(0.0, 0.0, 0.0),
        center: Vector3::new(0.0, 0.0, 0.0),
        angle: 0.0,
    };
    let definition = ProceduralSurfaceDefinition::RollingBallJet(
        crate::geometry::RollingBallJetStations::try_new(
            5,
            vec![
                crate::geometry::RollingBallJetStation {
                    knot: 2.0,
                    multiplicity: 6,
                    site: RollingBallJetSite {
                        first_limit: Point3::new(2.0, 0.0, 0.0),
                        second_limit: Point3::new(0.0, 2.0, 0.0),
                        center: Point3::new(0.0, 0.0, 0.0),
                        angle: std::f64::consts::FRAC_PI_2,
                        first_derivative: zero.clone(),
                        second_derivative: zero.clone(),
                    },
                },
                crate::geometry::RollingBallJetStation {
                    knot: 5.0,
                    multiplicity: 6,
                    site: RollingBallJetSite {
                        first_limit: Point3::new(0.0, 2.0, 0.0),
                        second_limit: Point3::new(-2.0, 0.0, 0.0),
                        center: Point3::new(0.0, 0.0, 0.0),
                        angle: std::f64::consts::FRAC_PI_2,
                        first_derivative: zero.clone(),
                        second_derivative: zero,
                    },
                },
            ],
        )
        .unwrap(),
    );

    let point = rolling_ball_jet_point(&definition, 3.5, 0.5).expect("jet point");
    let root_two = 2.0_f64.sqrt();
    let expected = Point3::new(root_two / 2.0 - 1.0, root_two / 2.0 + 1.0, 0.0);
    assert!((point.x - expected.x).abs() <= TEST_TOLERANCE);
    assert!((point.y - expected.y).abs() <= TEST_TOLERANCE);
    assert!((point.z - expected.z).abs() <= TEST_TOLERANCE);
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
        transform: Transform::from_rows([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .expect("affine transform"),
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
    let directrix_id =
        CurveId::mint("test:model:entity#budgeted-directrix").expect("valid identity");
    let surface_id = SurfaceId::mint("test:model:entity#budgeted-sweep").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model
        .add_procedural_surface(
            surface_id.clone(),
            procedural_surface! {
                id: ProceduralSurfaceId::mint("test:model:entity#budgeted-sweep-construction").expect("valid identity"),
                definition: ProceduralSurfaceDefinition::LinearSweep(
                    crate::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                        directrix_id,
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .expect("valid linear sweep"),
                ),
                cache_fit_tolerance: None,
                record_bounds: None,
            },
        )
        .unwrap();

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
fn nurbs_surface_local_inverse_returns_a_forward_checked_candidate() {
    let surface = bilinear_surface();
    let point = Point3::new(0.3, 0.7, 0.2);
    let parameters = nurbs_surface_parameter_near_point(&surface, point, None)
        .expect("bounded local surface candidate");
    let mapped = nurbs_surface_point(&surface, parameters.u, parameters.v).expect("surface point");
    assert!(mapped.distance(point) <= 0.2 + f64::EPSILON * 1024.0);
    assert!((parameters.u - 0.3).abs() < f64::EPSILON * 1024.0);
    assert!((parameters.v - 0.7).abs() < f64::EPSILON * 1024.0);
}

#[test]
fn nurbs_surface_inverse_handles_rational_internal_spans() {
    let surface = NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        3,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.5, 0.0, 0.2),
            Point3::new(0.5, 1.0, 0.2),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        Some(vec![1.0, 1.0, 0.7, 0.7, 1.0, 1.0]),
        false,
        false,
        false,
    )
    .unwrap();
    let point = nurbs_surface_point(&surface, 0.75, 0.4).expect("surface point");
    let parameters = nurbs_surface_parameter_within_tolerance(&surface, point, None, 1.0e-10)
        .expect("rational multi-span inverse");
    assert!((parameters.u - 0.75).abs() < 1.0e-9);
    assert!((parameters.v - 0.4).abs() < 1.0e-9);
}

#[test]
fn nurbs_surface_parameter_segment_bound_contains_curved_diagonal() {
    let mut surface = bilinear_surface();
    surface
        .edit_control_points(|points| points[3].z = 1.0)
        .unwrap();
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
fn degree_zero_nurbs_surface_has_an_exact_parameter_segment_bound() {
    let surface = NurbsSurface::new(
        0,
        0,
        vec![0.0, 1.0],
        vec![0.0, 1.0],
        1,
        1,
        vec![Point3::new(1.0, 2.0, 3.0)],
        None,
        false,
        false,
        false,
    )
    .unwrap();
    let point = Point3::new(1.0, 2.0, 3.0);
    assert_eq!(nurbs_surface_point(&surface, 0.25, 0.75), Some(point));
    let bound = nurbs_surface_parameter_segment_chord_bound(
        &surface,
        [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)],
        [point, point],
    )
    .expect("degree-zero surface bound");
    assert!(bound <= EPS_DEGREE_ZERO_SURFACE_BOUND, "{bound}");
}

#[test]
fn degree_zero_nurbs_surface_patch_spans_use_their_matching_poles() {
    let surface = NurbsSurface::new(
        0,
        0,
        vec![0.0, 1.0, 2.0],
        vec![0.0, 1.0],
        2,
        1,
        vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
        None,
        false,
        false,
        false,
    )
    .unwrap();
    let poles = [Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)];
    for (range, pole) in [([0.0, 1.0], poles[0]), ([1.0, 2.0], poles[1])] {
        let bound = nurbs_surface_parameter_segment_chord_bound(
            &surface,
            [Point2::new(range[0], 0.0), Point2::new(range[1], 1.0)],
            [pole, pole],
        )
        .expect("degree-zero patch span bound");
        assert!(bound <= EPS_DEGREE_ZERO_SURFACE_BOUND, "{bound}");
    }
}

#[test]
fn nurbs_surface_parameter_segment_bound_splits_internal_knots() {
    let surface = NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        3,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.5, 0.0, 0.25),
            Point3::new(0.5, 1.0, 0.25),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        Some(vec![1.0, 1.0, 0.5, 0.5, 1.0, 1.0]),
        false,
        false,
        false,
    )
    .unwrap();
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
        CurveGeometry::Line(
            crate::geometry::LineCurve::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        CurveGeometry::Circle(
            crate::geometry::CircleCurve::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                4.0,
            )
            .unwrap(),
        ),
        CurveGeometry::Ellipse(
            crate::geometry::EllipseCurve::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                4.0,
                2.0,
            )
            .unwrap(),
        ),
        CurveGeometry::Parabola(
            crate::geometry::ParabolaCurve::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .unwrap(),
        ),
        CurveGeometry::Hyperbola(
            crate::geometry::HyperbolaCurve::try_new(
                Point3::new(1.0, 2.0, 3.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                4.0,
                2.0,
            )
            .unwrap(),
        ),
    ];
    for (index, geometry) in geometries.into_iter().enumerate() {
        let parameter = if matches!(
            &geometry,
            CurveGeometry::Circle(_) | CurveGeometry::Ellipse(_)
        ) {
            0.7 + std::f64::consts::TAU
        } else {
            0.7
        };
        let point = curve_point(&geometry, parameter).expect("analytic curve evaluates");
        let id = CurveId::mint(format!("test:inverse:curve#{index}")).expect("valid identity");
        let mut ir = CadIr::empty();
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
            CurveGeometry::Polyline(
                PolylineCurve::new(
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 0.0, 0.0),
                        Point3::new(1.0, 1.0, 0.0),
                    ],
                    None,
                    0.0,
                )
                .unwrap(),
            ),
            Point3::new(0.5, 0.0, 0.0),
            0.5,
            0.5,
        ),
        (
            CurveGeometry::Polyline(
                PolylineCurve::new(
                    vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 0.0, 0.0),
                        Point3::new(1.0, 1.0, 0.0),
                    ],
                    Some(vec![4.0, 2.0, 0.0]),
                    0.0,
                )
                .unwrap(),
            ),
            Point3::new(1.0, 0.5, 0.0),
            1.0,
            1.0,
        ),
        (
            CurveGeometry::Polyline(
                PolylineCurve::new(
                    vec![
                        Point3::new(2.0, 3.0, 4.0),
                        Point3::new(2.0, 3.0, 4.0),
                        Point3::new(5.0, 3.0, 4.0),
                    ],
                    Some(vec![0.0, 1.0, 2.0]),
                    0.0,
                )
                .unwrap(),
            ),
            Point3::new(2.0, 3.0, 4.0),
            0.7,
            0.7,
        ),
    ];
    for (index, (geometry, point, seed, expected)) in cases.into_iter().enumerate() {
        let id =
            CurveId::mint(format!("test:polyline-inverse:curve#{index}")).expect("valid identity");
        let mut ir = CadIr::empty();
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
fn indexed_curve_inverse_uses_the_caller_tolerance() {
    let id = CurveId::mint("test:model:entity#test:inverse-tolerance").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Line(
            crate::geometry::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    let index = crate::index::ModelIndex::new(&ir);
    let point = Point3::new(0.5, 0.005, 0.0);
    assert!(super::model_curve_parameter_near_point_in_index(&index, &id, point, 0.5).is_none());
    let inverse = super::model_curve_parameter_near_point_in_index_with_tolerance(
        &index, &id, point, 0.5, 0.01,
    )
    .expect("caller tolerance admits the bounded residual");
    assert!((inverse - 0.5).abs() < 1.0e-12);
}

#[test]
fn transformed_curve_inverse_uses_the_basis_parameterization() {
    let basis = CurveGeometry::Circle(
        crate::geometry::CircleCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            4.0,
        )
        .unwrap(),
    );
    let transform = Transform::from_rows([
        [-2.0, 0.0, 0.0, 1.0e6],
        [0.0, 0.5, 0.0, -2.0e6],
        [0.0, 0.0, 3.0, 3.0e6],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .expect("affine transform");
    let geometry = CurveGeometry::Transformed {
        basis: Box::new(basis.clone()),
        transform,
    };
    let parameter = 0.7 + std::f64::consts::TAU;
    let point = curve_point(&geometry, parameter).expect("transformed curve evaluates");
    let id = CurveId::mint("test:model:entity#test:transformed-inverse").expect("valid identity");
    let mut ir = CadIr::empty();
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
        transform: Transform::from_rows([
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .expect("affine transform"),
    };
    assert!(
        super::model_curve_parameter_near_point(&ir, &id, Point3::new(0.0, 0.0, 0.0), 0.0,)
            .is_none()
    );
}

#[test]
fn degenerate_curve_inverse_preserves_the_selected_parameter() {
    let point = Point3::new(2.0, 3.0, 4.0);
    let id = CurveId::mint("test:model:entity#test:degenerate-inverse").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: id.clone(),
        geometry: CurveGeometry::Degenerate(
            crate::geometry::DegenerateCurve::try_new(point).unwrap(),
        ),
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
    let surface = NurbsSurface::new(
        2,
        1,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![-2.0, -2.0, 3.0, 3.0],
        3,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 4.0),
            Point3::new(1.0, 2.0, 0.5),
            Point3::new(1.0, 2.0, 4.5),
            Point3::new(3.0, -1.0, 1.0),
            Point3::new(3.0, -1.0, 5.0),
        ],
        Some(vec![1.0, 2.0, 0.5, 1.5, 3.0, 0.25]),
        false,
        false,
        false,
    )
    .unwrap();

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
                curve.degree(),
                curve.knots(),
                curve.control_points(),
                curve.weights(),
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
    let surface = NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 3.0, 0.0),
        ],
        None,
        false,
        false,
        false,
    )
    .unwrap();
    let partials = nurbs_surface_partials(&surface, 0.25, 0.75).expect("partials");
    assert_eq!(partials.point, Point3::new(0.5, 2.25, 0.0));
    assert_eq!(partials.du, Vector3::new(2.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 3.0, 0.0));
}

#[test]
fn quadratic_surface_second_partials_follow_stored_parameterization() {
    let surface = NurbsSurface::new(
        2,
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        3,
        3,
        (0..3)
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
        None,
        false,
        false,
        false,
    )
    .unwrap();
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
    let support_id = SurfaceId::mint("test:model:entity#support").expect("valid identity");
    let first_id = SurfaceId::mint("test:model:entity#first-offset").expect("valid identity");
    let second_id = SurfaceId::mint("test:model:entity#second-offset").expect("valid identity");
    let first_construction =
        ProceduralSurfaceId::mint("test:model:entity#first-construction").expect("valid identity");
    let second_construction =
        ProceduralSurfaceId::mint("test:model:entity#second-construction").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces = vec![
        Surface {
            id: support_id.clone(),
            geometry: SurfaceGeometry::Plane(
                crate::geometry::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: first_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: first_construction.clone(),
                cache: None,
            },
            source_object: None,
        },
        Surface {
            id: second_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: second_construction.clone(),
                cache: None,
            },
            source_object: None,
        },
    ];
    ir.model.procedural_surfaces = vec![
        procedural_surface! {
            id: first_construction,
            definition: ProceduralSurfaceDefinition::Offset(crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(support_id, 2.0, None, None, false, OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent }).unwrap()),
            cache_fit_tolerance: None,
            record_bounds: None,
        },
        procedural_surface! {
            id: second_construction,
            definition: ProceduralSurfaceDefinition::Offset(crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(first_id, -5.0, None, None, false, OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent }).unwrap()),
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
    let support_id = SurfaceId::mint("test:model:entity#support").expect("valid identity");
    let offset_id = SurfaceId::mint("test:model:entity#offset").expect("valid identity");
    let construction =
        ProceduralSurfaceId::mint("test:model:entity#offset-construction").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces = vec![
        Surface {
            id: support_id.clone(),
            geometry: SurfaceGeometry::Nurbs(
                NurbsSurface::new(
                    1,
                    2,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    2,
                    3,
                    [0.0, 1.0]
                        .into_iter()
                        .flat_map(|u| {
                            [(0.0, 0.0), (0.5, 0.0), (1.0, 1.0)]
                                .into_iter()
                                .map(move |(v, z)| Point3::new(u, v, z))
                        })
                        .collect(),
                    None,
                    false,
                    false,
                    false,
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: offset_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        },
    ];
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: construction,
        definition: ProceduralSurfaceDefinition::Offset(crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(support_id, 0.0, None, None, true, OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent }).unwrap()),
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
fn offset_uses_the_nurbs_carrier_normal_orientation() {
    let support_id = SurfaceId::mint("test:model:entity#support").expect("valid identity");
    let offset_id = SurfaceId::mint("test:model:entity#offset").expect("valid identity");
    let construction =
        ProceduralSurfaceId::mint("test:model:entity#offset-construction").expect("valid identity");
    let mut support = bilinear_surface();
    support.set_normal_reversed(true);
    let mut ir = CadIr::empty();
    ir.model.surfaces = vec![
        Surface {
            id: support_id.clone(),
            geometry: SurfaceGeometry::Nurbs(support),
            source_object: None,
        },
        Surface {
            id: offset_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        },
    ];
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: construction,
        definition: ProceduralSurfaceDefinition::Offset(crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(support_id, 2.0, None, None, false, OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent }).unwrap()),
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let point =
        model_surface_point_by_id(&crate::index::ModelIndex::new(&ir), &offset_id, 0.2, 0.3)
            .expect("oriented offset point");

    let expected = Point3::new(0.2, 0.3, -2.0);
    let epsilon = 64.0 * f64::EPSILON;
    assert!((point.x - expected.x).abs() <= epsilon);
    assert!((point.y - expected.y).abs() <= epsilon);
    assert!((point.z - expected.z).abs() <= epsilon);
}

#[test]
fn offset_of_reversed_subset_uses_the_local_surface_normal() {
    let base_id = SurfaceId::mint("test:model:entity#base").expect("valid identity");
    let subset_id = SurfaceId::mint("test:model:entity#subset").expect("valid identity");
    let offset_id = SurfaceId::mint("test:model:entity#offset").expect("valid identity");
    let subset_construction =
        ProceduralSurfaceId::mint("test:model:entity#subset-construction").expect("valid identity");
    let offset_construction =
        ProceduralSurfaceId::mint("test:model:entity#offset-construction").expect("valid identity");
    let plane = SurfaceGeometry::Plane(
        crate::geometry::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    );
    let mut ir = CadIr::empty();
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
    ir.model
        .add_procedural_surface(
            subset_id.clone(),
            procedural_surface! {
                id: subset_construction,
                definition: ProceduralSurfaceDefinition::Subset(crate::geometry::surface_payloads::SubsetSurfaceConstruction::try_new(base_id, [[0.0, 1.0], [0.0, 1.0]], Some(false), Some(true)).unwrap()),
                cache_fit_tolerance: None,
                record_bounds: None,
            },
        )
        .expect("subset surface exists and has no procedural construction");
    ir.model
        .add_procedural_surface(
            offset_id.clone(),
            procedural_surface! {
                id: offset_construction,
                definition: ProceduralSurfaceDefinition::Offset(crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(subset_id, 2.0, None, None, false, OffsetExtension::Legacy { flags: LegacyExtensionFlags::Absent }).unwrap()),
                cache_fit_tolerance: None,
                record_bounds: None,
            },
        )
        .expect("offset surface exists and has no procedural construction");

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
fn curve_bounded_surface_delegates_evaluation_to_its_support() {
    let support_id =
        SurfaceId::mint("test:model:entity#curve-bounded-support").expect("valid identity");
    let bounded_id = SurfaceId::mint("test:model:entity#curve-bounded").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces = vec![
        Surface {
            id: support_id.clone(),
            geometry: SurfaceGeometry::Plane(
                crate::geometry::PlaneSurface::try_new(
                    Point3::new(1.0, 2.0, 3.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Surface {
            id: bounded_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
    ];
    ir.model
        .add_procedural_surface(
            bounded_id.clone(),
            procedural_surface! {
                id: ProceduralSurfaceId::mint("test:model:entity#curve-bounded-construction").expect("valid identity"),
                definition: ProceduralSurfaceDefinition::CurveBounded {
                    support: support_id,
                    boundaries: Vec::new(),
                    boundary_pcurves: Vec::new(),
                    implicit_outer: true,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            },
        )
        .unwrap();

    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &bounded_id, 0.25, 0.75),
        Some(Point3::new(1.25, 2.75, 3.0))
    );
    let partials = model_surface_partials_by_id(&index, &bounded_id, 0.25, 0.75)
        .expect("curve-bounded support evaluates");
    assert_eq!(partials.point, Point3::new(1.25, 2.75, 3.0));
    assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn linear_sweep_surface_evaluation_uses_directrix_and_sweep_parameters() {
    let directrix_id = CurveId::mint("test:model:entity#directrix").expect("valid identity");
    let surface_id = SurfaceId::mint("test:model:entity#sweep").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Transformed {
            basis: Box::new(CurveGeometry::Line(
                crate::geometry::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            transform: crate::transform::Transform::from_rows([
                [2.0, 0.0, 0.0, 1.0],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 3.0],
                [0.0, 0.0, 0.0, 1.0],
            ])
            .unwrap(),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model
        .add_procedural_surface(
            surface_id.clone(),
            procedural_surface! {
                id: ProceduralSurfaceId::mint("test:model:entity#sweep-construction").expect("valid identity"),
                definition: ProceduralSurfaceDefinition::LinearSweep(
                    crate::geometry::surface_payloads::LinearSweepSurfaceConstruction::try_new(
                        directrix_id,
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .expect("valid linear sweep"),
                ),
                cache_fit_tolerance: None,
                record_bounds: None,
            },
        )
        .unwrap();

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
fn cacheless_revision_extrusion_uses_the_directrix_sense_chart() {
    let directrix_id =
        CurveId::mint("test:model:entity#reversed-directrix").expect("valid identity");
    let surface_id =
        SurfaceId::mint("test:model:entity#cacheless-extrusion").expect("valid identity");
    let construction_id =
        ProceduralSurfaceId::mint("test:model:entity#cacheless-extrusion-construction")
            .expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Nurbs(
            NurbsCurve::new(
                1,
                vec![0.0, 0.0, 2.0, 2.0],
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
                None,
                false,
            )
            .unwrap(),
        ),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction_id.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: construction_id,
        definition: ProceduralSurfaceDefinition::Extrusion(crate::geometry::surface_payloads::ExtrusionSurfaceConstruction::try_new(directrix_id, Some([-2.0, 0.0]), Vector3::new(0.0, 0.0, 1.0), Some(Point3::new(0.0, 0.0, 0.0)), Some(RevisionSurfaceForm {
                revision: 1,
                support_bounds: [None; 4],
                reference_endpoints: [None; 2],
                second_endpoints: [None; 2],
                flags: vec![true],
                cache: RevisionCacheForm::Parameterization(
                    RevisionSurfaceParameterization::default(),
                ),
                discontinuities: Default::default(),
                tail_flag: false,
                trailing_flags: Vec::new(),
            })).unwrap()),
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let index = crate::index::ModelIndex::new(&ir);
    let partials = model_surface_partials_by_id(&index, &surface_id, -0.5, 3.0)
        .expect("cacheless reversed extrusion point");
    assert_eq!(partials.point, Point3::new(0.5, 0.0, 3.0));
    assert_eq!(partials.du, Vector3::new(-1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, 0.0, 1.0));
}

#[test]
fn cacheless_law_sweep_evaluation_uses_text_law_and_identity_rail() {
    let profile_id = CurveId::mint("test:model:entity#profile").expect("valid identity");
    let spine_id = CurveId::mint("test:model:entity#spine").expect("valid identity");
    let surface_id = SurfaceId::mint("test:model:entity#cacheless-sweep").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves = vec![
        Curve {
            id: profile_id.clone(),
            geometry: CurveGeometry::Line(
                crate::geometry::LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
        Curve {
            id: spine_id.clone(),
            geometry: CurveGeometry::Line(
                crate::geometry::LineCurve::try_new(
                    Point3::new(7.0, 11.0, 13.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            source_object: None,
        },
    ];
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: ProceduralSurfaceId::mint(
                "test:model:entity#cacheless-sweep-construction",
            )
            .expect("valid identity"),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(procedural_surface! {
        id: ProceduralSurfaceId::mint("test:model:entity#cacheless-sweep-construction").expect("valid identity"),
        definition: ProceduralSurfaceDefinition::Sweep(crate::geometry::surface_payloads::SweepSurfacePayload::try_new(profile_id, spine_id, Some(Box::new(SweepSurfaceConstruction {
                primary_kind: 0,
                revision_form: Some(SweepRevisionForm {
                    revision: 23100,
                    primary_flag: false,
                    profile_endpoints: [Some(0.0), Some(1.0)],
                    path_endpoints: [Some(0.0), Some(1.0)],
                    cache: RevisionCacheForm::Parameterization(
                        RevisionSurfaceParameterization::default(),
                    ),
                }),
                layout: SweepSurfaceLayout::LawDriven {
                    mode: 10,
                    profile_range: [0.0, 1.0],
                    profile_frame: None,
                    origin: Point3::new(0.0, 0.0, 0.0),
                    directions: [
                        Vector3::new(1.0, 0.0, 0.0),
                        Vector3::new(0.0, 1.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    ],
                    first_law: Box::new(LawExpression::Text {
                        value: "2.0*X".into(),
                    }),
                    first_mode: 21,
                    first_range: [0.0, 1.0],
                    law_direction: Vector3::new(0.0, 0.0, 1.0),
                    path_mode: 1,
                    path_flag: false,
                    path_range: [0.0, 1.0],
                    path_parameter: 0.0,
                    second_law_flag: false,
                    second_law: Box::new(LawExpression::Text {
                        value: "VEC(1,1,1)".into(),
                    }),
                    formula_mode: 0,
                    formula: LawFormula::Null {},
                    trailing_flag: false,
                },
                discontinuities: std::array::from_fn(|_| Vec::new()),
                discontinuity_flag: false,
            }))).unwrap()),
        cache_fit_tolerance: None,
        record_bounds: None,
    });

    let index = crate::index::ModelIndex::new(&ir);
    let expected = Point3::new(0.5, -0.5, 0.25);
    assert_eq!(
        model_surface_point_by_id(&index, &surface_id, 0.5, 0.25),
        Some(expected)
    );
    assert_eq!(
        model_surface_point(&ir, &ir.model.surfaces[0].geometry, 0.5, 0.25),
        Some(expected)
    );
    let partials = model_surface_partials_by_id(&index, &surface_id, 0.5, 0.25)
        .expect("cacheless sweep partials");
    assert_eq!(partials.point, expected);
    assert_eq!(partials.du, Vector3::new(1.0, 0.0, 0.0));
    assert_eq!(partials.dv, Vector3::new(0.0, -2.0, 1.0));
}

#[test]
fn axis_revolution_surface_evaluation_rotates_the_profile_parameterization() {
    let directrix_id = CurveId::mint("test:model:entity#profile").expect("valid identity");
    let surface_id = SurfaceId::mint("test:model:entity#revolution").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Transformed {
            basis: Box::new(CurveGeometry::Line(
                crate::geometry::LineCurve::try_new(
                    Point3::new(2.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            )),
            transform: Transform::identity(),
        },
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model
        .add_procedural_surface(
            surface_id.clone(),
            procedural_surface! {
                id: ProceduralSurfaceId::mint("test:model:entity#revolution-construction").expect("valid identity"),
                definition: ProceduralSurfaceDefinition::AxisRevolution(
                    crate::geometry::surface_payloads::AxisRevolutionSurfaceConstruction::try_new(
                        directrix_id,
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                    )
                    .expect("valid axis revolution"),
                ),
                cache_fit_tolerance: None,
                record_bounds: None,
            },
        )
        .unwrap();

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
    let directrix_id = CurveId::mint("test:model:entity#mapped-profile").expect("valid identity");
    let surface_id =
        SurfaceId::mint("test:model:entity#mapped-revolution").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Line(
            crate::geometry::LineCurve::try_new(
                Point3::new(2.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model
        .add_procedural_surface(
            surface_id.clone(),
            procedural_surface! {
                id: ProceduralSurfaceId::mint("test:model:entity#mapped-revolution-construction").expect("valid identity"),
                definition: ProceduralSurfaceDefinition::Revolution(crate::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(directrix_id, (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)), [0.0, std::f64::consts::PI], Some([10.0, 14.0]), None, false, None).unwrap()),
                cache_fit_tolerance: None,
                record_bounds: None,
            },
        )
        .unwrap();

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
fn revolution_surface_maps_a_normalized_line_domain_to_its_distance_carrier() {
    let directrix_id =
        CurveId::mint("test:model:entity#normalized-profile").expect("valid identity");
    let surface_id =
        SurfaceId::mint("test:model:entity#normalized-revolution").expect("valid identity");
    let start_point_id =
        PointId::mint("test:model:entity#normalized-profile-start-point").expect("valid identity");
    let end_point_id =
        PointId::mint("test:model:entity#normalized-profile-end-point").expect("valid identity");
    let start_vertex_id = VertexId::mint("test:model:entity#normalized-profile-start-vertex")
        .expect("valid identity");
    let end_vertex_id =
        VertexId::mint("test:model:entity#normalized-profile-end-vertex").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.curves.push(Curve {
        id: directrix_id.clone(),
        geometry: CurveGeometry::Line(
            crate::geometry::LineCurve::try_new(
                Point3::new(2.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .unwrap(),
        ),
        source_object: None,
    });
    ir.model.points.extend([
        Point {
            id: start_point_id.clone(),
            position: Point3::new(2.0, 0.0, 0.0),
            source_object: None,
        },
        Point {
            id: end_point_id.clone(),
            position: Point3::new(2.0, 0.0, 10.0),
            source_object: None,
        },
    ]);
    ir.model.vertices.extend([
        Vertex {
            id: start_vertex_id.clone(),
            point: start_point_id,
            tolerance: None,
        },
        Vertex {
            id: end_vertex_id.clone(),
            point: end_point_id,
            tolerance: None,
        },
    ]);
    ir.model.edges.push(Edge {
        id: EdgeId::mint("test:model:entity#normalized-profile-edge").expect("valid identity"),
        carrier: crate::topology::EdgeCarrier::new(Some(directrix_id.clone()), Some([0.0, 10.0]))
            .unwrap(),
        start: start_vertex_id,
        end: end_vertex_id,
        tolerance: None,
    });
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    });
    ir.model
        .add_procedural_surface(
            surface_id.clone(),
            procedural_surface! {
                id: ProceduralSurfaceId::mint("test:model:entity#normalized-revolution-construction").expect("valid identity"),
                definition: ProceduralSurfaceDefinition::Revolution(crate::geometry::surface_payloads::RevolutionSurfaceConstruction::try_new(directrix_id, (Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)), [0.0, std::f64::consts::TAU], None, Some([0.0, 1.0]), false, None).unwrap()),
                cache_fit_tolerance: None,
                record_bounds: Some([Some(0.0), Some(10.0), None, None]),
            },
        )
        .unwrap();

    let index = crate::index::ModelIndex::new(&ir);
    let point = model_surface_point_by_id(&index, &surface_id, 5.0, 0.0)
        .expect("normalized line domain maps to distance carrier");
    assert_eq!(point, Point3::new(2.0, 0.0, 5.0));
    let partials = model_surface_partials_by_id(&index, &surface_id, 5.0, 0.0)
        .expect("normalized line domain partials");
    assert_eq!(partials.du, Vector3::new(0.0, 0.0, 1.0));
}

#[test]
fn analytic_and_transformed_surface_partials_follow_parameterization() {
    let cylinder = SurfaceGeometry::Cylinder(
        crate::geometry::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .unwrap(),
    );
    let cone = SurfaceGeometry::Cone(
        crate::geometry::ConeSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .unwrap(),
    );
    let sphere = SurfaceGeometry::Sphere(
        crate::geometry::SphereSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
        )
        .unwrap(),
    );
    let torus = SurfaceGeometry::Torus(
        crate::geometry::TorusSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            5.0,
            2.0,
        )
        .unwrap(),
    );
    let transformed = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Plane(
            crate::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        transform: Transform::from_rows([
            [2.0, 0.0, 0.0, 7.0],
            [0.0, 3.0, 0.0, 11.0],
            [0.0, 0.0, 4.0, 13.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
        .expect("affine transform"),
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
    assert!((cone.point.x - 5.0).abs() < 1.0e-12);
    assert!((cone.du.y - 5.0).abs() < 1.0e-12);
    assert!((cone.dv.x - 1.0).abs() < 1.0e-12);
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
    let circle = CurveGeometry::Circle(
        crate::geometry::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            3.0,
        )
        .unwrap(),
    );
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

    let arc = CurveGeometry::Nurbs(
        NurbsCurve::new(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            false,
        )
        .unwrap(),
    );
    for parameter in [0.0, 0.5, 1.0] {
        let point = curve_point(&arc, parameter).expect("rational arc point");
        let tangent = curve_tangent(&arc, parameter).expect("rational arc tangent");
        let second = curve_second_derivative(&arc, parameter).expect("rational arc acceleration");
        let radial_dot = point.x * tangent.x + point.y * tangent.y;
        assert!(radial_dot.abs() < 1.0e-12);
        assert!((point.x * second.x + point.y * second.y + tangent.dot(tangent)).abs() < 1.0e-11);
        assert!(tangent.norm() > 0.0);
    }

    let corner = CurveGeometry::Polyline(
        PolylineCurve::new(
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 1.0, 0.0),
            ],
            Some(vec![0.0, 1.0, 2.0]),
            0.0,
        )
        .unwrap(),
    );
    assert_eq!(
        curve_tangent(&corner, 0.5),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(curve_tangent(&corner, 1.0), None);
}

#[test]
fn rational_surface_partials_apply_the_weight_quotient_rule() {
    let surface = NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(2.0, 3.0, 0.0),
        ],
        Some(vec![1.0, 1.0, 2.0, 2.0]),
        false,
        false,
        false,
    )
    .unwrap();
    let partials = nurbs_surface_partials(&surface, 0.5, 0.25).expect("partials");
    assert!((partials.point.x - 4.0 / 3.0).abs() < 1.0e-12);
    assert!((partials.point.y - 0.75).abs() < 1.0e-12);
    assert!((partials.du.x - 16.0 / 9.0).abs() < 1.0e-12);
    assert!(partials.du.y.abs() < 1.0e-12);
    assert!((partials.dv.y - 3.0).abs() < 1.0e-12);
    let second = nurbs_surface_second_partials(&surface, 0.5, 0.25).expect("second partials");
    assert!((second.duu.x + 64.0 / 27.0).abs() < 1.0e-12);
    assert!(second.duu.y.abs() < 1.0e-12);
    assert_eq!(second.duv, Vector3::new(0.0, 0.0, 0.0));
    assert_eq!(second.dvv, Vector3::new(0.0, 0.0, 0.0));
}

#[test]
fn rational_surface_isocurves_preserve_the_tensor_product_parameterization() {
    let surface = NurbsSurface::new(
        1,
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 1.0, 1.0],
        2,
        2,
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 3.0, 0.0),
            Point3::new(2.0, 0.0, 1.0),
            Point3::new(2.0, 3.0, 1.0),
        ],
        Some(vec![1.0, 2.0, 3.0, 4.0]),
        false,
        false,
        false,
    )
    .unwrap();
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
            assert!((actual.x - expected.x).abs() < 1.0e-12);
            assert!((actual.y - expected.y).abs() < 1.0e-12);
            assert!((actual.z - expected.z).abs() < 1.0e-12);
        }
    }
}

#[test]
fn nurbs_curve_inverse_uses_the_seed_to_select_an_ambiguous_witness() {
    let curve = NurbsCurve::new(
        1,
        vec![0.0, 0.0, 0.5, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
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

mod periodic_and_analytic;
