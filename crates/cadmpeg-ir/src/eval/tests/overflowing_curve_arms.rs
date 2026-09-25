// SPDX-License-Identifier: Apache-2.0
//! Curve evaluator arms, and the surface arms that read a model curve, report
//! the point an evaluation that leaves the finite range reached; an arm with
//! no value reports that.

use crate::eval::{
    curve_point, model_curve_point_by_id, model_surface_point, model_surface_point_by_id,
    model_surface_point_by_id_with_budget, pcurve_tangent, pcurve_uv, EvaluationFailure,
};
use crate::features::FinitePoint3;
use crate::geometry::analytic::LineCurve;
use crate::geometry::pcurve::{CirclePcurve, LinePcurve, OffsetPcurve, PcurveGeometry};
use crate::geometry::surface_payloads::{
    AxisRevolutionSurfaceConstruction, ExtrusionSurfaceConstruction,
    LinearSweepSurfaceConstruction, RevolutionSurfaceConstruction, SumSurfaceConstruction,
};
use crate::geometry::{
    CacheContract, Curve, CurveGeometry, HelixCurveConstruction, HelixFrame, PlacedCurve,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, Surface, SurfaceGeometry,
};
use crate::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId};
use crate::math::{Point2, Point3, Vector3};
use crate::transform::Transform;
use crate::CadIr;

/// The line through `(MAX, 0, 0)` along `x`: its point at `t = MAX` has no
/// finite `x`.
fn edge_line() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(f64::MAX, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("line fixture"),
    ))
}

/// A model holding `curves` and the procedural surface `definition` builds
/// from their ids, with the surface's geometry.
fn curve_surface_model(
    curves: Vec<CurveGeometry>,
    definition: impl FnOnce(&[CurveId]) -> ProceduralSurfaceDefinition,
) -> (CadIr, SurfaceId, SurfaceGeometry) {
    let ids = (0..curves.len())
        .map(|index| CurveId::mint(format!("test:model:curve#{index}")).expect("valid identity"))
        .collect::<Vec<_>>();
    let surface_id = SurfaceId::mint("test:model:surface#procedural").expect("valid identity");
    let construction =
        ProceduralSurfaceId::mint("test:model:procedural#surface").expect("valid identity");
    let geometry = SurfaceGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    };
    let mut ir = CadIr::empty();
    ir.model
        .curves
        .extend(ids.iter().zip(curves).map(|(id, geometry)| Curve {
            id: id.clone(),
            geometry,
            source_object: None,
        }));
    ir.model.surfaces.push(Surface {
        id: surface_id.clone(),
        geometry: geometry.clone(),
        source_object: None,
    });
    ir.model
        .procedural_surfaces
        .push(ProceduralSurface::new(construction, definition(&ids), None));
    (ir, surface_id, geometry)
}

/// Every surface point route's result at `(u, v)`: the geometry route, and
/// the arena route without and within a work budget.
fn surface_routes(
    ir: &CadIr,
    surface: &SurfaceId,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> [Result<FinitePoint3, EvaluationFailure<Point3>>; 3] {
    let index = crate::index::ModelIndex::new(ir);
    [
        model_surface_point(ir, geometry, u, v),
        model_surface_point_by_id(&index, surface, u, v),
        model_surface_point_by_id_with_budget(
            &index,
            surface,
            u,
            v,
            &cadmpeg_core::decode::WorkBudget::new(1_000_000),
        ),
    ]
}

#[test]
fn an_extrusion_whose_directrix_point_overflows_reports_the_point_its_extrusion_reaches() {
    // The directrix point at u = MAX has no finite x; its extrusion by v = 2
    // along z reaches (NaN, 0, 2).
    let (ir, surface, geometry) = curve_surface_model(vec![edge_line()], |curves| {
        ProceduralSurfaceDefinition::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                curves[0].clone(),
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                CacheContract::from_form(None),
            )
            .expect("extrusion fixture"),
        )
    });
    for route in surface_routes(&ir, &surface, &geometry, f64::MAX, 2.0) {
        assert!(
            matches!(
                route,
                Err(EvaluationFailure::NonFinite(point))
                    if point.x.is_nan() && point.y == 0.0 && point.z == 2.0
            ),
            "{route:?}"
        );
    }
    for route in surface_routes(&ir, &surface, &geometry, -f64::MAX, 2.0) {
        assert_eq!(route.map(FinitePoint3::get), Ok(Point3::new(0.0, 0.0, 2.0)));
    }
}

/// The line along `y` through the origin, placed by a transform that adds
/// `f64::MAX` to `x` twice: every point it reaches has `x = +inf`.
fn overflowing_placed_line() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(f64::MAX, 0.0, 0.0), Vector3::new(0.0, 1.0, 0.0))
                    .expect("line fixture"),
            )),
            Transform::affine([
                [1.0, 0.0, 0.0, f64::MAX],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        )
        .expect("placed curve fixture"),
    ))
}

#[test]
fn a_nurbs_curve_whose_projection_overflows_reports_the_point_it_reached() {
    // The weights 1 and -1 + 2^-40 nearly cancel at t = 0.5, so the
    // projected x coordinate, about 2e300 over 2^-41, overflows; y and z stay
    // zero.
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        crate::geometry::nurbs::NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![
                Point3::new(1.0e300, 0.0, 0.0),
                Point3::new(-1.0e300, 0.0, 0.0),
            ],
            Some(vec![1.0, -1.0 + 2.0_f64.powi(-40)]),
            false,
        )
        .expect("NURBS curve fixture"),
    ));
    assert_eq!(
        curve_point(&curve, 0.5),
        Err(EvaluationFailure::NonFinite(Point3::new(
            f64::INFINITY,
            0.0,
            0.0
        )))
    );
    assert_eq!(curve_point(&curve, 2.0), Err(EvaluationFailure::NoValue));
}

/// A model with `curves` and the procedural curve `definition` builds on
/// them, attached to one more curve; the id of that curve.
fn procedural_curve_model(
    curves: Vec<CurveGeometry>,
    definition: impl FnOnce(&[CurveId]) -> ProceduralCurveDefinition,
) -> (CadIr, CurveId) {
    let ids = (0..curves.len())
        .map(|index| CurveId::mint(format!("test:model:curve#{index}")).expect("valid identity"))
        .collect::<Vec<_>>();
    let owner = CurveId::mint("test:model:curve#procedural").expect("valid identity");
    let construction =
        ProceduralCurveId::mint("test:model:procedural#curve").expect("valid identity");
    let mut ir = CadIr::empty();
    ir.model
        .curves
        .extend(ids.iter().zip(curves).map(|(id, geometry)| Curve {
            id: id.clone(),
            geometry,
            source_object: None,
        }));
    ir.model.curves.push(Curve {
        id: owner.clone(),
        geometry: CurveGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model
        .add_procedural_curve(
            owner.clone(),
            ProceduralCurve::new(construction, definition(&ids)),
        )
        .expect("procedural curve fixture");
    (ir, owner)
}

#[test]
fn a_replica_curve_whose_placement_overflows_reports_the_point_it_reached() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("line fixture"),
    ));
    let (ir, replica) =
        procedural_curve_model(vec![line], |curves| ProceduralCurveDefinition::Replica {
            source: curves[0].clone(),
            transform: Transform::affine([
                [1.0, 0.0, 0.0, f64::MAX],
                [0.0, 1.0, 0.0, 2.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_curve_point_by_id(&index, &replica, f64::MAX),
        Err(EvaluationFailure::NonFinite(Point3::new(
            f64::INFINITY,
            2.0,
            0.0
        )))
    );
    assert_eq!(
        model_curve_point_by_id(&index, &replica, -f64::MAX).map(FinitePoint3::get),
        Ok(Point3::new(0.0, 2.0, 0.0))
    );
}

#[test]
fn a_subset_curve_whose_span_overflows_keeps_its_finite_local_point() {
    let line = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .expect("line fixture"),
    ));
    let (ir, subset) = procedural_curve_model(vec![line], |curves| {
        ProceduralCurveDefinition::Subset(
            crate::geometry::curve_payloads::SubsetCurveConstruction::try_new(
                curves[0].clone(),
                [-f64::MAX, f64::MAX],
                true,
                None,
            )
            .expect("subset fixture"),
        )
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_curve_point_by_id(&index, &subset, 1.0).map(FinitePoint3::get),
        Ok(Point3::new(-f64::MAX, 0.0, 0.0))
    );
    assert_eq!(
        model_curve_point_by_id(&index, &subset, f64::MAX).map(FinitePoint3::get),
        Ok(Point3::new(0.0, 0.0, 0.0))
    );
    assert_eq!(
        model_curve_point_by_id(&index, &subset, -1.0),
        Err(EvaluationFailure::NoValue)
    );
}

/// A helix over the angles `[0, 1]` whose major and minor vectors reach
/// `0.9 MAX` and whose `apex_factor` scales its radius: at angle 0 the point
/// `(0.9 MAX, 0, 0)` is finite, the tangent's radial term is
/// `apex_factor / TAU * 0.9 MAX` along `x` and the acceleration's
/// `2 apex_factor / TAU * 0.9 MAX` along `y`.
fn large_helix(apex_factor: f64) -> ProceduralCurveDefinition {
    ProceduralCurveDefinition::Helix(
        HelixCurveConstruction::try_new(
            [0.0, 1.0],
            HelixFrame {
                center: Point3::new(0.0, 0.0, 0.0),
                major: Vector3::new(0.9 * f64::MAX, 0.0, 0.0),
                minor: Vector3::new(0.0, 0.9 * f64::MAX, 0.0),
                pitch: Vector3::new(0.0, 0.0, 1.0),
                axis: Vector3::new(0.0, 0.0, 1.0),
            },
            apex_factor,
            None,
        )
        .expect("helix fixture"),
    )
}

#[test]
fn a_helix_whose_tangent_overflows_has_its_finite_point() {
    // The tangent at angle 0 leaves the finite range; the point reads no
    // tangent.
    let (ir, helix) = procedural_curve_model(Vec::new(), |_| large_helix(10.0));
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_curve_point_by_id(&index, &helix, 0.0).map(FinitePoint3::get),
        Ok(Point3::new(0.9 * f64::MAX, 0.0, 0.0))
    );
    assert_eq!(
        model_curve_point_by_id(&index, &helix, 2.0),
        Err(EvaluationFailure::NoValue)
    );
}

#[test]
fn an_extrusion_over_a_helix_whose_tangent_overflows_has_its_finite_point() {
    // The helix directrix is finite at angle 0 and its tangent overflows; the
    // extrusion point reads the directrix point only.
    let (mut ir, helix) = procedural_curve_model(Vec::new(), |_| large_helix(10.0));
    let surface = SurfaceId::mint("test:model:surface#extrusion").expect("valid identity");
    let construction =
        ProceduralSurfaceId::mint("test:model:procedural#extrusion").expect("valid identity");
    let geometry = SurfaceGeometry::Procedural {
        construction: construction.clone(),
        cache: None,
    };
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: geometry.clone(),
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                helix,
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                CacheContract::from_form(None),
            )
            .expect("extrusion fixture"),
        ),
        None,
    ));
    for route in surface_routes(&ir, &surface, &geometry, 0.0, 2.0) {
        assert_eq!(
            route.map(FinitePoint3::get),
            Ok(Point3::new(0.9 * f64::MAX, 0.0, 2.0))
        );
    }
}

#[test]
fn a_ruled_surface_whose_rail_point_overflows_reports_the_point_its_rule_reaches() {
    // The first rail reaches x = +inf; the rule toward the finite second
    // rail at v = 0.5 reaches no finite x.
    let second = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 1.0), Vector3::new(0.0, 1.0, 0.0))
            .expect("line fixture"),
    ));
    let (ir, surface, geometry) =
        curve_surface_model(vec![overflowing_placed_line(), second], |curves| {
            ProceduralSurfaceDefinition::Ruled {
                first: curves[0].clone(),
                second: curves[1].clone(),
                cache: None,
            }
        });
    for route in surface_routes(&ir, &surface, &geometry, 3.0, 0.5) {
        assert!(
            matches!(
                route,
                Err(EvaluationFailure::NonFinite(point))
                    if point.x.is_nan() && point.y == 3.0 && point.z == 0.5
            ),
            "{route:?}"
        );
    }
}

#[test]
fn a_sum_surface_whose_curve_point_overflows_reports_the_point_its_sum_reaches() {
    let second = CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0))
            .expect("line fixture"),
    ));
    let (ir, surface, geometry) =
        curve_surface_model(vec![overflowing_placed_line(), second], |curves| {
            ProceduralSurfaceDefinition::Sum(
                SumSurfaceConstruction::try_new(
                    curves[0].clone(),
                    curves[1].clone(),
                    Vector3::new(0.0, 0.0, 0.0),
                    CacheContract::from_form(None),
                )
                .expect("sum fixture"),
            )
        });
    for route in surface_routes(&ir, &surface, &geometry, 3.0, 2.0) {
        assert_eq!(
            route,
            Err(EvaluationFailure::NonFinite(Point3::new(
                f64::INFINITY,
                3.0,
                2.0
            )))
        );
    }
}

#[test]
fn a_linear_sweep_whose_directrix_point_overflows_reports_the_point_its_sweep_reaches() {
    let (ir, surface, geometry) = curve_surface_model(vec![edge_line()], |curves| {
        ProceduralSurfaceDefinition::LinearSweep(
            LinearSweepSurfaceConstruction::try_new(curves[0].clone(), Vector3::new(0.0, 0.0, 1.0))
                .expect("linear sweep fixture"),
        )
    });
    for route in surface_routes(&ir, &surface, &geometry, f64::MAX, 2.0) {
        assert!(
            matches!(
                route,
                Err(EvaluationFailure::NonFinite(point))
                    if point.x.is_nan() && point.y == 0.0 && point.z == 2.0
            ),
            "{route:?}"
        );
    }
    // A sweep distance that is not finite has no value.
    for route in surface_routes(&ir, &surface, &geometry, 0.0, f64::NAN) {
        assert_eq!(route, Err(EvaluationFailure::NoValue));
    }
}

#[test]
fn a_revolution_whose_directrix_point_overflows_reports_the_point_its_revolution_reaches() {
    // The directrix reaches x = +inf; its revolution by 0 about the z axis
    // keeps no finite x.
    let (ir, surface, geometry) = curve_surface_model(vec![overflowing_placed_line()], |curves| {
        ProceduralSurfaceDefinition::Revolution(
            RevolutionSurfaceConstruction::try_new(
                curves[0].clone(),
                (
                    FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).expect("finite origin"),
                    crate::units::UnitVector3::new(Vector3::new(0.0, 0.0, 1.0)).expect("unit axis"),
                ),
                [0.0, std::f64::consts::TAU],
                None,
                None,
                false,
                CacheContract::from_form(None),
            )
            .expect("revolution fixture"),
        )
    });
    for route in surface_routes(&ir, &surface, &geometry, 3.0, 0.0) {
        assert!(
            matches!(route, Err(EvaluationFailure::NonFinite(point)) if !point.x.is_finite()),
            "{route:?}"
        );
    }
}

#[test]
fn an_axis_revolution_whose_directrix_point_overflows_reports_the_point_its_revolution_reaches() {
    let (ir, surface, geometry) = curve_surface_model(vec![overflowing_placed_line()], |curves| {
        ProceduralSurfaceDefinition::AxisRevolution(
            AxisRevolutionSurfaceConstruction::try_new(
                curves[0].clone(),
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("axis revolution fixture"),
        )
    });
    for route in surface_routes(&ir, &surface, &geometry, 0.0, 3.0) {
        assert!(
            matches!(route, Err(EvaluationFailure::NonFinite(point)) if !point.x.is_finite()),
            "{route:?}"
        );
    }
}

#[test]
fn an_offset_pcurve_whose_basis_acceleration_overflows_reports_its_tangent_as_left_the_finite_range(
) {
    // The circle of radius MAX about (-MAX, 0) on the axes (1, 0) and (1, 1)
    // has at t = pi / 4 the finite point (0.41 MAX, 0.71 MAX) and tangent
    // (0, 0.71 MAX); its acceleration -0.71 MAX (2, 1) overflows in u.
    let basis = PcurveGeometry::Circle(
        CirclePcurve::try_new(
            Point2::new(-f64::MAX, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(1.0, 1.0),
            f64::MAX,
        )
        .expect("circle pcurve fixture"),
    );
    let parameter = std::f64::consts::FRAC_PI_4;
    assert!(pcurve_tangent(&basis, parameter).is_ok());
    let offset = PcurveGeometry::Offset(
        OffsetPcurve::try_new(1.0, Box::new(basis)).expect("offset pcurve fixture"),
    );
    assert!(pcurve_uv(&offset, parameter).is_ok());
    assert!(matches!(
        pcurve_tangent(&offset, parameter),
        Err(EvaluationFailure::NonFinite(tangent)) if tangent.u.is_nan() && tangent.v.is_nan()
    ));
}

#[test]
fn an_offset_pcurve_over_an_offset_basis_has_no_tangent() {
    // An offset states no second derivative, so an offset over it forms no
    // tangent.
    let inner = PcurveGeometry::Offset(
        OffsetPcurve::try_new(
            1.0,
            Box::new(PcurveGeometry::Line(
                LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
                    .expect("line pcurve fixture"),
            )),
        )
        .expect("offset pcurve fixture"),
    );
    let outer =
        PcurveGeometry::Offset(OffsetPcurve::try_new(1.0, Box::new(inner)).expect("offset pcurve"));
    assert_eq!(
        pcurve_uv(&outer, 0.5).map(crate::units::FinitePoint2::get),
        Ok(Point2::new(0.5, 2.0))
    );
    assert_eq!(pcurve_tangent(&outer, 0.5), Err(EvaluationFailure::NoValue));
}

#[test]
fn an_extrusion_whose_directrix_tangent_overflows_has_its_finite_point_on_every_route() {
    // The line through the origin along (1, 1, 0) / sqrt(2), placed by rows
    // whose first sums the two lanes at the largest finite scale: its point
    // at t = 0 is the origin, and its tangent reaches x = MAX * sqrt(2),
    // outside the finite range. The extrusion point by v = 2 along z reads no
    // tangent.
    let directrix = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Line(
                LineCurve::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(
                        std::f64::consts::FRAC_1_SQRT_2,
                        std::f64::consts::FRAC_1_SQRT_2,
                        0.0,
                    ),
                )
                .expect("line fixture"),
            )),
            Transform::affine([
                [f64::MAX, f64::MAX, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        )
        .expect("placed curve fixture"),
    ));
    assert!(crate::eval::curve_tangent(&directrix, 0.0).is_err());
    let (ir, surface, geometry) = curve_surface_model(vec![directrix], |curves| {
        ProceduralSurfaceDefinition::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                curves[0].clone(),
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                CacheContract::from_form(None),
            )
            .expect("extrusion fixture"),
        )
    });
    for route in surface_routes(&ir, &surface, &geometry, 0.0, 2.0) {
        assert_eq!(route.map(FinitePoint3::get), Ok(Point3::new(0.0, 0.0, 2.0)));
    }
}

/// The circle of radius 2 about `(-2, 0, 0)` in the `xy` plane, placed by a
/// transform that scales `x` by `f64::MAX`: at `t = 0` its point is the
/// origin and its tangent `(0, 2, 0)`, and its acceleration `(-2, 0, 0)`
/// reaches `x = -inf`.
fn overflowing_acceleration_circle() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Circle(
                crate::geometry::analytic::CircleCurve::try_new(
                    Point3::new(-2.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    2.0,
                )
                .expect("circle fixture"),
            )),
            Transform::affine([
                [f64::MAX, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ])
            .expect("affine transform"),
        )
        .expect("placed curve fixture"),
    ))
}

/// The line through `origin` along `direction`.
fn line(origin: Point3, direction: Vector3) -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Line(
        LineCurve::try_new(origin, direction).expect("line fixture"),
    ))
}

/// A surface over [`overflowing_acceleration_circle`] has its point on every
/// route and its first partials `[du, dv]` there; its second partials leave
/// the finite range at the point.
fn assert_second_order_alone_overflows(
    (ir, surface, geometry): (CadIr, SurfaceId, SurfaceGeometry),
    (u, v): (f64, f64),
    point: Point3,
    [du, dv]: [Vector3; 2],
) {
    assert!(crate::eval::curve_second_derivative(&overflowing_acceleration_circle(), 0.0).is_err());
    for route in surface_routes(&ir, &surface, &geometry, u, v) {
        assert_eq!(route.map(FinitePoint3::get), Ok(point));
    }
    let index = crate::index::ModelIndex::new(&ir);
    let budget = cadmpeg_core::decode::WorkBudget::new(1_000_000);
    let expected = Ok(crate::eval::SurfacePartials { point, du, dv });
    assert_eq!(
        crate::eval::model_surface_partials_by_id(&index, &surface, u, v)
            .map(crate::eval::SurfacePartials::into_raw),
        expected
    );
    assert_eq!(
        crate::eval::model_surface_partials_by_id_with_budget(&index, &surface, u, v, &budget)
            .map(crate::eval::SurfacePartials::into_raw),
        expected
    );
    assert_eq!(
        crate::eval::model_surface_second_partials_by_id(&index, &surface, u, v)
            .map(crate::eval::SurfaceSecondPartials::into_raw),
        Err(EvaluationFailure::NonFinite(point))
    );
}

#[test]
fn an_extrusion_whose_directrix_acceleration_overflows_keeps_its_point_and_first_partials() {
    assert_second_order_alone_overflows(
        curve_surface_model(vec![overflowing_acceleration_circle()], |curves| {
            ProceduralSurfaceDefinition::Extrusion(
                ExtrusionSurfaceConstruction::try_new(
                    curves[0].clone(),
                    None,
                    Vector3::new(0.0, 0.0, 1.0),
                    None,
                    CacheContract::from_form(None),
                )
                .expect("extrusion fixture"),
            )
        }),
        (0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        [Vector3::new(0.0, 2.0, 0.0), Vector3::new(0.0, 0.0, 1.0)],
    );
}

#[test]
fn a_linear_sweep_whose_directrix_acceleration_overflows_keeps_its_point_and_first_partials() {
    assert_second_order_alone_overflows(
        curve_surface_model(vec![overflowing_acceleration_circle()], |curves| {
            ProceduralSurfaceDefinition::LinearSweep(
                LinearSweepSurfaceConstruction::try_new(
                    curves[0].clone(),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .expect("linear sweep fixture"),
            )
        }),
        (0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        [Vector3::new(0.0, 2.0, 0.0), Vector3::new(0.0, 0.0, 1.0)],
    );
}

#[test]
fn a_ruled_surface_whose_rail_acceleration_overflows_keeps_its_point_and_first_partials() {
    // Halfway from the circle's origin to the line's (0, 0, 2) along y.
    assert_second_order_alone_overflows(
        curve_surface_model(
            vec![
                overflowing_acceleration_circle(),
                line(Point3::new(0.0, 0.0, 2.0), Vector3::new(0.0, 1.0, 0.0)),
            ],
            |curves| ProceduralSurfaceDefinition::Ruled {
                first: curves[0].clone(),
                second: curves[1].clone(),
                cache: None,
            },
        ),
        (0.0, 0.5),
        Point3::new(0.0, 0.0, 1.0),
        [Vector3::new(0.0, 1.5, 0.0), Vector3::new(0.0, 0.0, 2.0)],
    );
}

#[test]
fn a_sum_surface_whose_curve_acceleration_overflows_keeps_its_point_and_first_partials() {
    assert_second_order_alone_overflows(
        curve_surface_model(
            vec![
                overflowing_acceleration_circle(),
                line(Point3::new(0.0, 0.0, 0.0), Vector3::new(0.0, 0.0, 1.0)),
            ],
            |curves| {
                ProceduralSurfaceDefinition::Sum(
                    SumSurfaceConstruction::try_new(
                        curves[0].clone(),
                        curves[1].clone(),
                        Vector3::new(0.0, 0.0, 0.0),
                        CacheContract::from_form(None),
                    )
                    .expect("sum fixture"),
                )
            },
        ),
        (0.0, 2.0),
        Point3::new(0.0, 0.0, 2.0),
        [Vector3::new(0.0, 2.0, 0.0), Vector3::new(0.0, 0.0, 1.0)],
    );
}

#[test]
fn a_revolution_whose_directrix_acceleration_overflows_keeps_its_point_and_first_partials() {
    // The origin revolved by angle 0 about the z axis through (1, 0, 0).
    assert_second_order_alone_overflows(
        curve_surface_model(vec![overflowing_acceleration_circle()], |curves| {
            ProceduralSurfaceDefinition::Revolution(
                RevolutionSurfaceConstruction::try_new(
                    curves[0].clone(),
                    (
                        FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).expect("finite origin"),
                        crate::units::UnitVector3::new(Vector3::new(0.0, 0.0, 1.0))
                            .expect("unit axis"),
                    ),
                    [0.0, std::f64::consts::TAU],
                    None,
                    None,
                    false,
                    CacheContract::from_form(None),
                )
                .expect("revolution fixture"),
            )
        }),
        (0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
        [Vector3::new(0.0, 2.0, 0.0), Vector3::new(0.0, -1.0, 0.0)],
    );
}

#[test]
fn an_axis_revolution_whose_directrix_acceleration_overflows_keeps_its_point_and_first_partials() {
    // The angle is u and the directrix parameter v.
    assert_second_order_alone_overflows(
        curve_surface_model(vec![overflowing_acceleration_circle()], |curves| {
            ProceduralSurfaceDefinition::AxisRevolution(
                AxisRevolutionSurfaceConstruction::try_new(
                    curves[0].clone(),
                    Point3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .expect("axis revolution fixture"),
            )
        }),
        (0.0, 0.0),
        Point3::new(0.0, 0.0, 0.0),
        [Vector3::new(0.0, -1.0, 0.0), Vector3::new(0.0, 2.0, 0.0)],
    );
}

#[test]
fn a_hyperbola_whose_scaled_cosh_alone_overflows_keeps_its_point_and_second_derivative() {
    // With major radius 1 and minor radius MAX, the point and second
    // derivative read cosh(t) and MAX sinh(t), both finite at t = 1e-7; the
    // tangent reads MAX cosh(t), which overflows.
    let hyperbola = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
        crate::geometry::analytic::HyperbolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
            f64::MAX,
        )
        .expect("hyperbola fixture"),
    ));
    let t = 1.0e-7_f64;
    let lanes = |value: Vector3| {
        assert_eq!(value.x, t.cosh());
        assert!((value.y / (f64::MAX * t.sinh()) - 1.0).abs() <= 4.0 * f64::EPSILON);
        assert_eq!(value.z, 0.0);
    };
    lanes(
        curve_point(&hyperbola, t)
            .expect("point")
            .get()
            .vector_from(Point3::new(0.0, 0.0, 0.0)),
    );
    lanes(
        crate::eval::curve_second_derivative(&hyperbola, t)
            .expect("second derivative")
            .get(),
    );
    assert_eq!(
        crate::eval::curve_tangent(&hyperbola, t),
        Err(EvaluationFailure::NonFinite(()))
    );
}

#[test]
fn an_extrusion_over_a_helix_whose_acceleration_overflows_keeps_its_first_partials() {
    // With apex factor 5 the tangent's radial term 5 / TAU * 0.9 MAX is
    // finite and the acceleration's 10 / TAU * 0.9 MAX overflows.
    let (mut ir, helix) = procedural_curve_model(Vec::new(), |_| large_helix(5.0));
    let surface = SurfaceId::mint("test:model:surface#extrusion").expect("valid identity");
    let construction =
        ProceduralSurfaceId::mint("test:model:procedural#extrusion").expect("valid identity");
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: construction.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
        construction,
        ProceduralSurfaceDefinition::Extrusion(
            ExtrusionSurfaceConstruction::try_new(
                helix,
                None,
                Vector3::new(0.0, 0.0, 1.0),
                None,
                CacheContract::from_form(None),
            )
            .expect("extrusion fixture"),
        ),
        None,
    ));
    let index = crate::index::ModelIndex::new(&ir);
    let point = Point3::new(0.9 * f64::MAX, 0.0, 2.0);
    let partials = crate::eval::model_surface_partials_by_id(&index, &surface, 0.0, 2.0)
        .expect("first partials")
        .into_raw();
    assert_eq!(partials.point, point);
    let radial = 5.0 / std::f64::consts::TAU * 0.9 * f64::MAX;
    assert!((partials.du.x / radial - 1.0).abs() <= 8.0 * f64::EPSILON);
    assert!((partials.du.y / (0.9 * f64::MAX) - 1.0).abs() <= 8.0 * f64::EPSILON);
    assert!((partials.du.z * std::f64::consts::TAU - 1.0).abs() <= 8.0 * f64::EPSILON);
    assert_eq!(partials.dv, Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        crate::eval::model_surface_second_partials_by_id(&index, &surface, 0.0, 2.0)
            .map(crate::eval::SurfaceSecondPartials::into_raw),
        Err(EvaluationFailure::NonFinite(point))
    );
}

#[test]
fn a_helix_whose_axis_length_overflows_has_its_point() {
    // The helix reads its pitch, not its axis; an axis whose length
    // overflows leaves the point as it is.
    let (ir, helix) = procedural_curve_model(Vec::new(), |_| {
        ProceduralCurveDefinition::Helix(
            HelixCurveConstruction::try_new(
                [0.0, 1.0],
                HelixFrame {
                    center: Point3::new(0.0, 0.0, 0.0),
                    major: Vector3::new(1.0, 0.0, 0.0),
                    minor: Vector3::new(0.0, 1.0, 0.0),
                    pitch: Vector3::new(0.0, 0.0, 1.0),
                    axis: Vector3::new(f64::MAX, f64::MAX, 0.0),
                },
                0.0,
                None,
            )
            .expect("helix fixture"),
        )
    });
    let index = crate::index::ModelIndex::new(&ir);
    assert_eq!(
        model_curve_point_by_id(&index, &helix, 0.0).map(FinitePoint3::get),
        Ok(Point3::new(1.0, 0.0, 0.0))
    );
}
