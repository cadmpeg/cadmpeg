// SPDX-License-Identifier: Apache-2.0
//! Evaluator arms whose evaluation leaves the finite range report the value
//! it reached; an arm with no value reports that.

use crate::eval::{
    model_curve_point_by_id, model_surface_point_by_id, pcurve_tangent, pcurve_uv, surface_point,
    surface_point_with_budget, EvaluationFailure,
};
use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::pcurve::{
    CirclePcurve, HyperbolaPcurve, LinePcurve, OffsetPcurve, ParabolaPcurve, PcurveGeometry,
    PcurveNurbs, PlacedPcurve, PolarHarmonicPcurve, PolarNurbsPole, PolarPcurveNurbs,
    SphericalGreatCirclePcurve,
};
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
    ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry, TolerantIntersectionConstruction, TolerantIntersectionParameterization,
};
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform2;
use crate::CadIr;
use cadmpeg_core::decode::WorkBudget;

fn line(origin: Point2, direction: Point2) -> PcurveGeometry {
    PcurveGeometry::Line(LinePcurve::try_new(origin, direction).expect("line pcurve fixture"))
}

/// The line through the origin along `(MAX, 0)`, placed by a transform that
/// doubles `u`: its point at `t = 0` is the origin and its tangent
/// `(2 MAX, 0)` overflows.
fn doubled_line() -> PcurveGeometry {
    PcurveGeometry::Transformed(
        PlacedPcurve::try_new(
            Box::new(line(Point2::new(0.0, 0.0), Point2::new(f64::MAX, 0.0))),
            Transform2::affine([[2.0, 0.0, 0.0], [0.0, 1.0, 0.0]]).expect("affine transform"),
        )
        .expect("placed pcurve fixture"),
    )
}

fn is_non_finite_point2(
    value: Result<crate::units::FinitePoint2, EvaluationFailure<Point2>>,
    u: impl Fn(f64) -> bool,
    v: impl Fn(f64) -> bool,
) -> bool {
    matches!(value, Err(EvaluationFailure::NonFinite(point)) if u(point.u) && v(point.v))
}

#[test]
fn a_circle_pcurve_whose_point_overflows_reports_the_point_and_tangent_it_reached() {
    let circle = PcurveGeometry::Circle(
        CirclePcurve::try_new(
            Point2::new(f64::MAX, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            f64::MAX,
        )
        .expect("circle pcurve fixture"),
    );
    // `MAX + MAX` has no finite value; the tangent `(0, MAX)` is finite, and
    // the evaluation that formed it left the finite range.
    assert!(is_non_finite_point2(
        pcurve_uv(&circle, 0.0),
        f64::is_nan,
        |v| v == 0.0
    ));
    assert_eq!(
        pcurve_tangent(&circle, 0.0),
        Err(EvaluationFailure::NonFinite(Point2::new(0.0, f64::MAX)))
    );
}

#[test]
fn a_parabola_pcurve_whose_axial_quotient_overflows_reports_the_point_it_reached() {
    let parabola = PcurveGeometry::Parabola(
        ParabolaPcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            0.25,
        )
        .expect("parabola pcurve fixture"),
    );
    // `t^2 / (4 * 0.25)` at `t = 1e200` overflows, so no coordinate is
    // reached.
    assert!(is_non_finite_point2(
        pcurve_uv(&parabola, 1.0e200),
        f64::is_nan,
        f64::is_nan
    ));
}

#[test]
fn a_hyperbola_pcurve_whose_scaled_cosh_overflows_reports_the_point_it_reached() {
    let hyperbola = PcurveGeometry::Hyperbola(
        HyperbolaPcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            f64::MAX,
            1.0,
        )
        .expect("hyperbola pcurve fixture"),
    );
    assert!(is_non_finite_point2(
        pcurve_uv(&hyperbola, 1.0),
        f64::is_nan,
        f64::is_nan
    ));
    assert!(is_non_finite_point2(
        pcurve_tangent(&hyperbola, 1.0),
        f64::is_nan,
        f64::is_nan
    ));
}

#[test]
fn a_polar_harmonic_pcurve_whose_radial_or_axial_value_overflows_reports_the_point_it_reached() {
    // The radial point `MAX + MAX` has no angle.
    let radial = PcurveGeometry::PolarHarmonic(
        PolarHarmonicPcurve::try_new(
            Point2::new(f64::MAX, 0.0),
            Point2::new(f64::MAX, 0.0),
            Point2::new(0.0, 1.0),
            0.0,
            0.0,
            0.0,
        )
        .expect("polar harmonic pcurve fixture"),
    );
    assert!(is_non_finite_point2(
        pcurve_uv(&radial, 0.0),
        f64::is_nan,
        |v| v == 0.0
    ));
    // The radial point `(2, 0)` has angle zero; the axial value overflows.
    let axial = PcurveGeometry::PolarHarmonic(
        PolarHarmonicPcurve::try_new(
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            f64::MAX,
            f64::MAX,
            0.0,
        )
        .expect("polar harmonic pcurve fixture"),
    );
    assert_eq!(
        pcurve_uv(&axial, 0.0),
        Err(EvaluationFailure::NonFinite(Point2::new(
            0.0,
            f64::INFINITY
        )))
    );
}

#[test]
fn a_polar_nurbs_pcurve_whose_axial_spline_overflows_reports_the_point_it_reached() {
    let pole = |axial| PolarNurbsPole {
        radial: Point2::new(1.0, 0.0),
        axial,
    };
    let pcurve = PcurveGeometry::PolarNurbs {
        nurbs: PolarPcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![pole(0.0), pole(f64::MAX)],
            None,
            false,
        )
        .expect("polar NURBS pcurve fixture"),
    };
    // At `t = 2` the axial spline reaches `2 MAX`; the radial spline stays at
    // `(1, 0)`.
    assert_eq!(
        pcurve_uv(&pcurve, 2.0),
        Err(EvaluationFailure::NonFinite(Point2::new(
            0.0,
            f64::INFINITY
        )))
    );
}

#[test]
fn a_spherical_great_circle_pcurve_whose_azimuth_or_phase_overflows_reports_the_point_it_reached() {
    let azimuth = PcurveGeometry::SphericalGreatCircle(
        SphericalGreatCirclePcurve::try_new(f64::MAX, 1.0, 0.0, 1.0)
            .expect("spherical pcurve fixture"),
    );
    assert!(is_non_finite_point2(
        pcurve_uv(&azimuth, f64::MAX),
        |u| u == f64::INFINITY,
        f64::is_nan
    ));
    let phase = PcurveGeometry::SphericalGreatCircle(
        SphericalGreatCirclePcurve::try_new(f64::MAX, 1.0, -f64::MAX, 1.0)
            .expect("spherical pcurve fixture"),
    );
    assert!(is_non_finite_point2(
        pcurve_uv(&phase, 0.0),
        |u| u == f64::MAX,
        f64::is_nan
    ));
}

#[test]
fn a_nurbs_pcurve_whose_point_or_basis_overflows_reports_the_point_it_reached() {
    let linear = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(0.0, 0.0), Point2::new(f64::MAX, 0.0)],
            None,
            false,
        )
        .expect("NURBS pcurve fixture"),
    };
    assert_eq!(
        pcurve_uv(&linear, 2.0),
        Err(EvaluationFailure::NonFinite(Point2::new(
            f64::INFINITY,
            0.0
        )))
    );
    // The derivatives read the finite point, so none is reached.
    assert!(is_non_finite_point2(
        pcurve_tangent(&linear, 2.0),
        f64::is_nan,
        f64::is_nan
    ));

    // A degree-20 basis at `t = 2^52` over the unit knot span grows past the
    // finite range before any pole weights it.
    let degree = 20;
    let knots = [vec![0.0; degree + 1], vec![1.0; degree + 1]].concat();
    let poles = (0..=degree)
        .map(|index| Point2::new(crate::scalar::FiniteReal::from_index(index).get(), 0.0))
        .collect();
    let high_degree = PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(20, knots, poles, None, false)
            .expect("degree-20 NURBS pcurve fixture"),
    };
    assert!(is_non_finite_point2(
        pcurve_uv(&high_degree, 2.0_f64.powi(52)),
        f64::is_nan,
        f64::is_nan
    ));
}

#[test]
fn a_placed_pcurve_reports_the_point_its_placement_reaches() {
    // The basis point `(MAX + MAX, 0)` is not finite, and the placement's
    // plain row products carry its infinity.
    let placed = PcurveGeometry::Transformed(
        PlacedPcurve::try_new(
            Box::new(line(Point2::new(f64::MAX, 0.0), Point2::new(1.0, 0.0))),
            Transform2::identity(),
        )
        .expect("placed pcurve fixture"),
    );
    assert!(is_non_finite_point2(
        pcurve_uv(&placed, f64::MAX),
        |u| u == f64::INFINITY,
        f64::is_nan
    ));
    // The placed tangent overflows while the point stays at the origin: the
    // evaluation that forms both left the finite range.
    assert_eq!(
        pcurve_uv(&doubled_line(), 0.0),
        Err(EvaluationFailure::NonFinite(Point2::new(0.0, 0.0)))
    );
    assert_eq!(
        pcurve_tangent(&doubled_line(), 0.0),
        Err(EvaluationFailure::NonFinite(Point2::new(
            f64::INFINITY,
            0.0
        )))
    );
}

#[test]
fn an_offset_pcurve_whose_basis_direction_is_not_reached_reports_no_coordinate() {
    let offset = |basis| {
        PcurveGeometry::Offset(
            OffsetPcurve::try_new(1.0, Box::new(basis)).expect("offset pcurve fixture"),
        )
    };
    // The basis tangent `(MAX, MAX)` has a length that overflows, and the
    // placed basis tangent overflows: neither states a unit normal.
    for pcurve in [
        offset(line(Point2::new(0.0, 0.0), Point2::new(f64::MAX, f64::MAX))),
        offset(doubled_line()),
    ] {
        assert!(is_non_finite_point2(
            pcurve_uv(&pcurve, 0.0),
            f64::is_nan,
            f64::is_nan
        ));
    }
}

#[test]
fn pcurve_tangents_whose_helper_quotient_overflows_report_the_tangent_they_reached() {
    // The polar chart `(1e-300, 0)` with chart tangent `(0, 1e300)` turns at
    // `1 / 1e-600`.
    let polar = PcurveGeometry::PolarHarmonic(
        PolarHarmonicPcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0e-300, 0.0),
            Point2::new(0.0, 1.0e300),
            0.0,
            0.0,
            0.0,
        )
        .expect("polar harmonic pcurve fixture"),
    );
    assert_eq!(
        pcurve_uv(&polar, 0.0).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(0.0, 0.0))
    );
    assert_eq!(
        pcurve_tangent(&polar, 0.0),
        Err(EvaluationFailure::NonFinite(Point2::new(
            f64::INFINITY,
            0.0
        )))
    );

    // The latitude turns at -2 per unit phase, and the phase at MAX per unit
    // parameter.
    let spherical = PcurveGeometry::SphericalGreatCircle(
        SphericalGreatCirclePcurve::try_new(std::f64::consts::FRAC_PI_2, f64::MAX, 0.0, 2.0)
            .expect("spherical pcurve fixture"),
    );
    assert!(pcurve_uv(&spherical, 0.0).is_ok());
    assert_eq!(
        pcurve_tangent(&spherical, 0.0),
        Err(EvaluationFailure::NonFinite(Point2::new(
            f64::MAX,
            f64::NEG_INFINITY
        )))
    );
}

#[test]
fn nurbs_pcurve_tangents_whose_derivative_quotient_overflows_report_the_tangent_they_reached() {
    let nurbs = |degree, knots: Vec<f64>, poles: Vec<Point2>| PcurveGeometry::Nurbs {
        nurbs: PcurveNurbs::from_lanes(degree, knots, poles, None, false)
            .expect("NURBS pcurve fixture"),
    };
    // The derivative projection `1e10 / 1e-300` overflows.
    let projected = nurbs(
        1,
        vec![0.0, 0.0, 1.0e-300, 1.0e-300],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0e10, 0.0)],
    );
    // The unit-span derivative `2` over the span `1e-308` overflows.
    let span = 1.0e-308;
    let unscaled = nurbs(
        2,
        vec![0.0, 0.0, 0.0, span, span, span],
        vec![
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(2.0, 0.0),
        ],
    );
    // The two-pole quotient `1 / 5e-324` overflows.
    let smallest = f64::from_bits(1);
    let linear = nurbs(
        1,
        vec![0.0, 0.0, smallest, smallest],
        vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
    );
    for (pcurve, parameter) in [(projected, 5.0e-301), (unscaled, span * 0.5), (linear, 0.0)] {
        assert!(pcurve_uv(&pcurve, parameter).is_ok(), "{pcurve:?}");
        assert!(
            is_non_finite_point2(
                pcurve_tangent(&pcurve, parameter),
                |u| u == f64::INFINITY,
                |v| v == 0.0
            ),
            "{:?}",
            pcurve_tangent(&pcurve, parameter)
        );
    }
}

/// The bilinear NURBS surface over the unit square with poles at `x = 0` and
/// `x = xs[1]`; at `u = 2` its `x` coordinate is `2 xs[1]`.
fn bilinear_nurbs(u_knots: [f64; 4], x: f64) -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, u_knots.to_vec(), false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
                vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)],
            ],
            None,
        ),
        false,
    )
    .expect("bilinear NURBS surface fixture")
}

fn solved(surface: SolvedSurfaceGeometry) -> SurfaceGeometry {
    SurfaceGeometry::Solved(surface)
}

#[test]
fn a_nurbs_surface_whose_point_overflows_reports_the_point_it_reached() {
    let surface = solved(SolvedSurfaceGeometry::Nurbs(bilinear_nurbs(
        [0.0, 0.0, 1.0, 1.0],
        f64::MAX,
    )));
    let reached = Err(EvaluationFailure::NonFinite(Point3::new(
        f64::INFINITY,
        0.5,
        0.0,
    )));
    let budget = WorkBudget::new(64);
    assert_eq!(surface_point(&surface, 2.0, 0.5), reached);
    assert_eq!(
        surface_point_with_budget(&surface, 2.0, 0.5, &budget),
        reached
    );

    let id = SurfaceId::mint("test:model:surface#overflow").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        geometry: surface,
        source_object: None,
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(model_surface_point_by_id(&index, &id, 2.0, 0.5), reached);
}

#[test]
fn a_nurbs_surface_whose_partial_overflows_has_its_finite_point_on_both_point_routes() {
    // Over the `u` span `1e-300` the partial `1e10 / 1e-300` overflows at the
    // finite point `(5e9, 0.5, 0)`. Both point routes evaluate the point
    // alone, so neither fails on the partial.
    let surface = solved(SolvedSurfaceGeometry::Nurbs(bilinear_nurbs(
        [0.0, 0.0, 1.0e-300, 1.0e-300],
        1.0e10,
    )));
    let budget = WorkBudget::new(64);
    assert_eq!(
        surface_point_with_budget(&surface, 5.0e-301, 0.5, &budget)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(5.0e9, 0.5, 0.0))
    );
    assert_eq!(
        surface_point(&surface, 5.0e-301, 0.5).map(crate::features::FinitePoint3::get),
        Ok(Point3::new(5.0e9, 0.5, 0.0))
    );
}

#[test]
fn a_placed_surface_whose_partial_overflows_has_its_finite_point_on_both_point_routes() {
    // The cylinder of radius 2 about the z axis, stretched by MAX in x: at
    // u = acos(1/4) its point x is MAX / 2 and its u partial x is
    // -2 sin(u) MAX, which overflows.
    let cylinder = SolvedSurfaceGeometry::Cylinder(
        crate::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            2.0,
        )
        .expect("cylinder fixture"),
    );
    let surface = solved(SolvedSurfaceGeometry::Transformed(
        crate::geometry::PlacedSurface::try_new(
            Box::new(cylinder),
            crate::transform::Transform::affine([
                [f64::MAX, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        )
        .expect("placed surface fixture"),
    ));
    let u = 0.25_f64.acos();
    assert!(crate::eval::surface_partials(&surface, u, 1.0).is_err());
    let budget = WorkBudget::new(64);
    let with_budget = surface_point_with_budget(&surface, u, 1.0, &budget);
    let point = surface_point(&surface, u, 1.0);
    assert!(point.is_ok(), "{point:?}");
    assert_eq!(point, with_budget);
}

fn procedural_surface_model(
    support: SurfaceGeometry,
    definition: impl FnOnce(SurfaceId) -> ProceduralSurfaceDefinition,
) -> (CadIr, SurfaceId) {
    let support_id = SurfaceId::mint("test:model:surface#support").expect("valid identity");
    let surface_id = SurfaceId::mint("test:model:surface#procedural").expect("valid identity");
    let construction =
        ProceduralSurfaceId::mint("test:model:procedural#surface").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([
        Surface {
            id: support_id.clone(),
            geometry: support,
            source_object: None,
        },
        Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Procedural {
                construction: construction.clone(),
                cache: None,
            },
            source_object: None,
        },
    ]);
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        definition(support_id),
        None,
    ));
    (ir, surface_id)
}

#[test]
fn a_replica_whose_placement_overflows_reports_the_point_it_reached() {
    let plane = solved(SolvedSurfaceGeometry::Plane(
        crate::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane fixture"),
    ));
    let (ir, replica) =
        procedural_surface_model(plane, |source| ProceduralSurfaceDefinition::Replica {
            source,
            transform: crate::transform::Transform::affine([
                [1.0, 0.0, 0.0, f64::MAX],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_surface_point_by_id(&index, &replica, f64::MAX, 2.0),
        Err(EvaluationFailure::NonFinite(Point3::new(
            f64::INFINITY,
            2.0,
            0.0
        )))
    );
    assert_eq!(
        model_surface_point_by_id(&index, &replica, -f64::MAX, 2.0)
            .map(crate::features::FinitePoint3::get),
        Ok(Point3::new(0.0, 2.0, 0.0))
    );
}

#[test]
fn an_offset_surface_whose_support_point_overflows_reaches_no_coordinate() {
    let support = solved(SolvedSurfaceGeometry::Nurbs(bilinear_nurbs(
        [0.0, 0.0, 1.0, 1.0],
        f64::MAX,
    )));
    let (ir, offset) = procedural_surface_model(support, |support| {
        ProceduralSurfaceDefinition::Offset(
            crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                support,
                1.0,
                None,
                None,
                false,
                crate::geometry::OffsetExtension::Legacy {
                    flags: crate::geometry::LegacyExtensionFlags::Absent {},
                    cache: None,
                },
            )
            .expect("offset construction fixture"),
        )
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert!(matches!(
        model_surface_point_by_id(&index, &offset, 2.0, 0.5),
        Err(EvaluationFailure::NonFinite(point))
            if point.x.is_nan() && point.y.is_nan() && point.z.is_nan()
    ));
}

/// A tolerant intersection of the planes `z = 0` and `y = 0` whose
/// parameterization evaluates `pcurve` on both.
fn tolerant_intersection_model(pcurve: PcurveGeometry) -> (CadIr, CurveId) {
    let plane = |normal: Vector3, reference: Vector3| {
        solved(SolvedSurfaceGeometry::Plane(
            crate::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                normal,
                reference,
            )
            .expect("plane fixture"),
        ))
    };
    let supports = ["test:model:surface#first", "test:model:surface#second"]
        .map(|id| SurfaceId::mint(id).expect("valid identity"));
    let curve = CurveId::mint("test:model:curve#intersection").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([
        Surface {
            id: supports[0].clone(),
            geometry: plane(Vector3::new(0.0, 0.0, 1.0), Vector3::new(1.0, 0.0, 0.0)),
            source_object: None,
        },
        Surface {
            id: supports[1].clone(),
            geometry: plane(Vector3::new(0.0, -1.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
            source_object: None,
        },
    ]);
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("line fixture"),
        )),
        source_object: None,
    });
    ir.model
        .add_procedural_curve(
            curve.clone(),
            ProceduralCurve::new(
                ProceduralCurveId::mint("test:model:procedural#intersection")
                    .expect("valid identity"),
                ProceduralCurveDefinition::TolerantIntersection {
                    construction: TolerantIntersectionConstruction::try_new(
                        supports,
                        [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
                        1.0e-9,
                    )
                    .expect("tolerant intersection fixture"),
                    parameterization: Some(
                        TolerantIntersectionParameterization::try_new(
                            [pcurve.clone(), pcurve],
                            [-1.0, 1.0],
                        )
                        .expect("tolerant parameterization fixture"),
                    ),
                    cache: None,
                },
            ),
        )
        .expect("procedural curve fixture");
    (ir, curve)
}

#[test]
fn a_tolerant_intersection_reads_the_point_of_a_pcurve_whose_placed_tangent_overflows() {
    // The placed pcurve's point at the origin is reached by an evaluation
    // that left the finite range; both supports lift it to the model origin.
    let (ir, curve) = tolerant_intersection_model(doubled_line());
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_curve_point_by_id(&index, &curve, 0.0).map(crate::features::FinitePoint3::get),
        Ok(Point3::new(0.0, 0.0, 0.0))
    );
}

#[test]
fn a_tolerant_intersection_has_no_point_where_its_offset_pcurve_point_overflows() {
    // The offset of the vertical line at u = MAX reaches u = +inf, which no
    // support lifts to a finite point.
    let offset = PcurveGeometry::Offset(
        OffsetPcurve::try_new(
            -f64::MAX,
            Box::new(line(Point2::new(f64::MAX, 0.0), Point2::new(0.0, 1.0))),
        )
        .expect("offset pcurve fixture"),
    );
    assert!(matches!(
        pcurve_uv(&offset, 0.0),
        Err(EvaluationFailure::NonFinite(_))
    ));
    let (ir, curve) = tolerant_intersection_model(offset);
    let index = crate::index::ModelIndex::new(&ir);
    assert!(matches!(
        model_curve_point_by_id(&index, &curve, 0.0),
        Err(EvaluationFailure::NonFinite(point))
            if point.x.is_nan() && point.y.is_nan() && point.z.is_nan()
    ));
}

#[test]
fn a_contact_track_evaluates_its_support_where_its_offset_pcurve_point_overflows() {
    use crate::geometry::{RollingBallSide, RollingBallSupportSurface, VariableBlendSupportKind};

    let plane = solved(SolvedSurfaceGeometry::Plane(
        crate::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane fixture"),
    ));
    let id = SurfaceId::mint("test:model:surface#contact").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        geometry: plane,
        source_object: None,
    });
    let index = crate::index::ModelIndex::new(&ir);
    let side = |pcurve| RollingBallSide {
        support_kind: VariableBlendSupportKind::Surface,
        surface: Some(RollingBallSupportSurface {
            surface: id.clone(),
            parameter_ranges: [[None, None], [None, None]],
        }),
        curve: None,
        pcurve: Some(pcurve),
        location: crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
            .expect("finite location"),
        secondary_pcurve: None,
        extension: None,
    };
    // The offset of the vertical line at u = MAX reaches u = +inf with the
    // finite tangent (0, 1); the plane maps that point to no finite
    // coordinate.
    let overflowing = side(PcurveGeometry::Offset(
        OffsetPcurve::try_new(
            -f64::MAX,
            Box::new(line(Point2::new(f64::MAX, 0.0), Point2::new(0.0, 1.0))),
        )
        .expect("offset pcurve fixture"),
    ));
    assert_eq!(
        super::super::variable_blend_contact_track(&index, &overflowing, 0.0).err(),
        Some(EvaluationFailure::NonFinite(()))
    );
    let finite = side(line(Point2::new(1.0, 2.0), Point2::new(0.0, 1.0)));
    let track = super::super::variable_blend_contact_track(&index, &finite, 0.0)
        .expect("a finite contact track");
    assert_eq!(track.point(), Point3::new(1.0, 2.0, 0.0));
}

#[test]
fn a_difference_quotient_over_an_empty_domain_has_no_value_and_one_that_overflows_is_infinite() {
    use crate::scalar::FiniteReal;
    let real = |value| FiniteReal::new(value).expect("finite test value");
    assert_eq!(
        super::super::difference_quotient(real(1.0), real(0.0), real(2.0), real(2.0)),
        Err(EvaluationFailure::NoValue)
    );
    assert_eq!(
        super::super::difference_quotient(real(f64::MAX), real(-f64::MAX), real(1.0), real(0.0)),
        Err(EvaluationFailure::NonFinite(f64::INFINITY))
    );
    assert_eq!(
        super::super::difference_quotient(real(3.0), real(1.0), real(5.0), real(1.0))
            .map(FiniteReal::get),
        Ok(0.5)
    );
}

/// The degree-2 by degree-1 NURBS surface over `u` in `[0, 1e-200]` whose
/// middle pole row sits at `x = 1e-190`: at `u = 0` its point is on the `x = 0`
/// edge and its `u` partial `2e10` along `x`, and its second `u` partial of
/// order `1e-190 / 1e-400` overflows.
fn narrow_quadratic_nurbs() -> NurbsSurface {
    NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 1.0e-200, 1.0e-200, 1.0e-200], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            [0.0, 1.0e-190, 0.0]
                .map(|x| vec![Point3::new(x, 0.0, 0.0), Point3::new(x, 1.0, 0.0)])
                .to_vec(),
            None,
        ),
        false,
    )
    .expect("quadratic NURBS surface fixture")
}

#[test]
fn a_nurbs_surface_whose_second_partial_overflows_keeps_its_first_partials() {
    let surface = narrow_quadratic_nurbs();
    let point = Point3::new(0.0, 0.5, 0.0);
    let budget = WorkBudget::new(1_000);
    for partials in [
        crate::eval::nurbs_surface_partials(&surface, 0.0, 0.5),
        crate::eval::nurbs_surface_partials_with_budget(&surface, 0.0, 0.5, &budget),
    ] {
        let partials = partials.expect("first partials").into_raw();
        assert_eq!(partials.point, point);
        assert!((partials.du.x / 2.0e10 - 1.0).abs() <= 8.0 * f64::EPSILON);
        assert_eq!([partials.du.y, partials.du.z], [0.0, 0.0]);
        assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 0.0));
    }
    for second in [
        crate::eval::nurbs_surface_second_partials(&surface, 0.0, 0.5),
        crate::eval::nurbs_surface_second_partials_with_budget(&surface, 0.0, 0.5, &budget),
    ] {
        assert_eq!(
            second.map(crate::eval::SurfaceSecondPartials::into_raw),
            Err(EvaluationFailure::NonFinite(point))
        );
    }
}

#[test]
fn an_analytic_surface_whose_point_overflows_has_no_partials() {
    // The plane through (MAX, 0, 0) leaves the finite range in x at u = MAX,
    // where x has no value; the partials report the point it reached.
    let plane = solved(SolvedSurfaceGeometry::Plane(
        crate::geometry::analytic::PlaneSurface::try_new(
            Point3::new(f64::MAX, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("plane fixture"),
    ));
    let reached = |failure: Option<EvaluationFailure<Point3>>| {
        matches!(
            failure,
            Some(EvaluationFailure::NonFinite(point))
                if point.x.is_nan() && point.y == 2.0 && point.z == 0.0
        )
    };
    assert!(reached(surface_point(&plane, f64::MAX, 2.0).err()));
    assert!(reached(
        crate::eval::surface_partials(&plane, f64::MAX, 2.0).err()
    ));
    assert!(reached(
        crate::eval::surface_second_partials(&plane, f64::MAX, 2.0).err()
    ));
    let id = SurfaceId::mint("test:model:surface#plane").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: id.clone(),
        geometry: plane,
        source_object: None,
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert!(reached(
        crate::eval::model_surface_partials_by_id(&index, &id, f64::MAX, 2.0).err()
    ));
    assert!(reached(
        crate::eval::model_surface_partials_by_id_with_budget(
            &index,
            &id,
            f64::MAX,
            2.0,
            &WorkBudget::new(64)
        )
        .err()
    ));
}

#[test]
fn a_placed_surface_whose_second_partial_overflows_keeps_its_first_partials() {
    // The cylinder of radius 2 about (-2, 0, 0), stretched by MAX in x: at
    // u = 0 its point is the origin shifted by v along z and its u partial
    // (0, 2, 0); its second u partial -2 MAX along x overflows.
    let surface = solved(SolvedSurfaceGeometry::Transformed(
        crate::geometry::PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Cylinder(
                crate::geometry::analytic::CylinderSurface::try_new(
                    Point3::new(-2.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .expect("cylinder fixture"),
            )),
            crate::transform::Transform::affine([
                [f64::MAX, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        )
        .expect("placed surface fixture"),
    ));
    let point = Point3::new(0.0, 0.0, 1.0);
    assert_eq!(
        crate::eval::surface_partials(&surface, 0.0, 1.0)
            .map(crate::eval::SurfacePartials::into_raw),
        Ok(crate::eval::SurfacePartials {
            point,
            du: Vector3::new(0.0, 2.0, 0.0),
            dv: Vector3::new(0.0, 0.0, 1.0),
        })
    );
    assert_eq!(
        crate::eval::surface_second_partials(&surface, 0.0, 1.0)
            .map(crate::eval::SurfaceSecondPartials::into_raw),
        Err(EvaluationFailure::NonFinite(point))
    );
}

#[test]
fn an_arena_nurbs_surface_whose_partial_overflows_has_its_finite_point_on_both_id_routes() {
    // The surface of the both-point-routes case above, stored in the arena
    // directly and under a subset over its whole domain.
    let support = solved(SolvedSurfaceGeometry::Nurbs(bilinear_nurbs(
        [0.0, 0.0, 1.0e-300, 1.0e-300],
        1.0e10,
    )));
    let (ir, subset) = procedural_surface_model(support, |support| {
        ProceduralSurfaceDefinition::Subset(
            crate::geometry::surface_payloads::SubsetSurfaceConstruction::try_new(
                support,
                [[0.0, 1.0e-300], [0.0, 1.0]],
                None,
                None,
                None,
            )
            .expect("subset fixture"),
        )
    });
    let index = crate::index::ModelIndex::new(&ir);
    let support = SurfaceId::mint("test:model:surface#support").expect("valid identity");
    for surface in [&support, &subset] {
        assert_eq!(
            model_surface_point_by_id(&index, surface, 5.0e-301, 0.5)
                .map(crate::features::FinitePoint3::get),
            Ok(Point3::new(5.0e9, 0.5, 0.0))
        );
        assert_eq!(
            crate::eval::model_surface_point_by_id_with_budget(
                &index,
                surface,
                5.0e-301,
                0.5,
                &WorkBudget::new(64)
            )
            .map(crate::features::FinitePoint3::get),
            Ok(Point3::new(5.0e9, 0.5, 0.0))
        );
        assert!(matches!(
            crate::eval::model_surface_partials_by_id(&index, surface, 5.0e-301, 0.5),
            Err(EvaluationFailure::NonFinite(point)) if point == Point3::new(5.0e9, 0.5, 0.0)
        ));
    }
}

#[test]
fn an_offset_surface_whose_support_partial_overflows_reaches_no_coordinate() {
    // The offset point reads the support normal, which the overflowing `u`
    // partial of the both-point-routes surface leaves without a value.
    let support = solved(SolvedSurfaceGeometry::Nurbs(bilinear_nurbs(
        [0.0, 0.0, 1.0e-300, 1.0e-300],
        1.0e10,
    )));
    let (ir, offset) = procedural_surface_model(support, |support| {
        ProceduralSurfaceDefinition::Offset(
            crate::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                support,
                1.0,
                None,
                None,
                false,
                crate::geometry::OffsetExtension::Legacy {
                    flags: crate::geometry::LegacyExtensionFlags::Absent {},
                    cache: None,
                },
            )
            .expect("offset construction fixture"),
        )
    });
    let index = crate::index::ModelIndex::new(&ir);
    for route in [
        model_surface_point_by_id(&index, &offset, 5.0e-301, 0.5),
        crate::eval::model_surface_point_by_id_with_budget(
            &index,
            &offset,
            5.0e-301,
            0.5,
            &WorkBudget::new(64),
        ),
    ] {
        assert!(
            matches!(
                route,
                Err(EvaluationFailure::NonFinite(point))
                    if point.x.is_nan() && point.y.is_nan() && point.z.is_nan()
            ),
            "{route:?}"
        );
    }
}
