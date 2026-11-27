// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{chart_ending_on_edge_vertices, sample, Approximator, Axis, SEED_PATCHES};
use crate::eval::admission::EvaluationAdmission;
use crate::geometry::intersection_curves::{IntcurveSupportContext, IntcurveSupportSide};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry, PlacedPcurve};
use crate::geometry::procedural_curves::{ProceduralCurveDefinition, SupportPcurve};
use crate::geometry::surface_curves::SurfaceCurveFamily;
use crate::geometry::{
    Curve, CurveGeometry, ProceduralCurve, SolvedCurveGeometry, MAX_GEOMETRY_NESTING,
};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::math::{Point2, Point3};
use crate::transform::Transform2;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy};

#[test]
fn chart_snapping_moves_only_an_end_pole_within_the_fit_tolerance() {
    let ir = crate::examples::unit_cube().unwrap();
    let index = crate::index::ModelIndex::new_model_only(&ir);
    let edge = &ir.geometry.edges[0];
    let curve = edge.curve().unwrap();
    let position = |id: &crate::ids::VertexId| {
        let vertex = ir.geometry.vertices.iter().find(|v| &v.id == id).unwrap();
        let point = ir
            .geometry
            .points
            .iter()
            .find(|p| p.id == vertex.point)
            .unwrap();
        point.position().get()
    };
    let start = position(edge.start().unwrap());
    let end = position(edge.end().unwrap());
    let middle = Point3::new(
        f64::midpoint(start.x, end.x),
        f64::midpoint(start.y, end.y),
        f64::midpoint(start.z, end.z),
    );
    let near = |point: Point3| Point3::new(point.x + 1e-5, point.y, point.z);
    let chart = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 2.0],
        vec![near(start), near(end), middle],
        None,
        false,
    )
    .unwrap();
    let cache = crate::geometry::legacy_cache::LegacyCache::try_new(1e-3).unwrap();

    let snapped = chart_ending_on_edge_vertices(&ir, &index, curve, &chart, Some(&cache)).unwrap();

    let poles = snapped.pole_rows().raw_points();
    assert_eq!(poles[0].distance(start), 0.0);
    assert_eq!(poles[1].distance(near(end)), 0.0);
    assert_eq!(poles[2].distance(middle), 0.0);
}

#[test]
fn a_cornered_curve_on_a_spline_surface_is_fitted_with_a_pole_on_the_corner() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
    use crate::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use crate::ids::SurfaceId;
    use crate::units::FinitePoint2;

    let mut ir = crate::examples::unit_cube().unwrap();
    let rows = [(0.0, 0.0), (1.0, 2.0), (2.0, -1.0), (3.0, 0.0)];
    let spline = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(2, vec![0.0, 0.0, 0.0, 0.25, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            rows.iter()
                .map(|&(x, z)| vec![Point3::new(x, 0.0, z), Point3::new(x, 1.0, z)])
                .collect(),
            None,
        ),
        false,
    )
    .unwrap();
    let surface = SurfaceId::mint("test:approximation:surface#cornered").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(spline)),
        source_object: None,
        native_ref: None,
    });
    let corner = (0.4, 0.5);
    let uv = [
        (0.1, 0.1),
        (0.2, 0.2),
        (0.3, 0.3),
        corner,
        (0.5, 0.2),
        (0.6, 0.15),
        (0.7, 0.1),
    ];
    let corner_parameter = 3.0 / 7.0;
    let nurbs = PcurveNurbs::new(
        2,
        vec![
            0.0,
            0.0,
            0.0,
            1.0 / 7.0,
            corner_parameter,
            corner_parameter,
            5.0 / 7.0,
            1.0,
            1.0,
            1.0,
        ],
        PcurveNurbsPoles::Polynomial {
            points: uv
                .iter()
                .map(|&(u, v)| FinitePoint2::new(Point2::new(u, v)).unwrap())
                .collect(),
        },
        false,
    )
    .unwrap();
    let curve = CurveId::mint("test:approximation:curve#cornered").unwrap();
    let construction = ProceduralCurveId::mint("test:approximation:construction#cornered").unwrap();
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: Some(surface.clone()),
                pcurve: Some(SupportPcurve::new(PcurveGeometry::Nurbs { nurbs }, None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });

    let arena = DecodeArena::new();
    let context = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let mut approximator = Approximator::new(&ir, &context);
    let Some(SolvedCurveGeometry::Nurbs(fitted)) = approximator.curve(curve.as_str()) else {
        panic!("a cornered curve is fitted one smooth stretch at a time");
    };

    let index = crate::index::ModelIndex::new_model_only(&ir);
    let on_surface = crate::eval::model_surface_point_by_id(
        crate::eval::admission::EvaluationAdmission::Standard,
        &index,
        &surface,
        corner.0,
        corner.1,
    )
    .unwrap()
    .get();
    assert!(fitted
        .pole_rows()
        .raw_points()
        .iter()
        .any(|pole| pole.distance(on_surface) < 1e-9));
    assert!(fitted.knots().as_slice().contains(&corner_parameter));
}

#[test]
fn a_constant_parameter_curve_across_a_spline_surface_becomes_the_exact_isocurve() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
    use crate::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use crate::ids::SurfaceId;
    use crate::units::FinitePoint2;

    let mut ir = crate::examples::unit_cube().unwrap();
    let u_knots = vec![0.0, 0.0, 0.0, 0.25, 1.0, 1.0, 1.0];
    let far_row = [
        Point3::new(0.0, 1.0, 0.0),
        Point3::new(1.0, 1.0, 2.0),
        Point3::new(2.0, 1.0, -1.0),
        Point3::new(3.0, 1.0, 0.0),
    ];
    let spline = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(2, u_knots.clone(), false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            far_row
                .iter()
                .map(|far| vec![Point3::new(far.x, 0.0, far.z), *far])
                .collect(),
            None,
        ),
        false,
    )
    .unwrap();
    let surface = SurfaceId::mint("test:approximation:surface#spline").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(spline)),
        source_object: None,
        native_ref: None,
    });
    let nurbs = PcurveNurbs::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        PcurveNurbsPoles::Polynomial {
            points: vec![
                FinitePoint2::new(Point2::new(1.0, 1.0)).unwrap(),
                FinitePoint2::new(Point2::new(0.0, 1.0)).unwrap(),
            ],
        },
        false,
    )
    .unwrap();
    let curve = CurveId::mint("test:approximation:curve#isocurve").unwrap();
    let construction = ProceduralCurveId::mint("test:approximation:construction#isocurve").unwrap();
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: Some(surface),
                pcurve: Some(SupportPcurve::new(PcurveGeometry::Nurbs { nurbs }, None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });

    let arena = DecodeArena::new();
    let context = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let mut approximator = Approximator::new(&ir, &context);
    let Some(SolvedCurveGeometry::Nurbs(exact)) = approximator.curve(curve.as_str()) else {
        panic!("a constant-parameter curve on a spline surface has an exact spline form");
    };

    assert_eq!(exact.degree(), 2);
    // The line runs from u = 1 to u = 0, so the isocurve is reversed over the same interval.
    assert_eq!(
        exact.knots().as_slice(),
        [0.0, 0.0, 0.0, 0.75, 1.0, 1.0, 1.0]
    );
    assert_eq!(
        exact.pole_rows().raw_points(),
        far_row.iter().rev().copied().collect::<Vec<_>>()
    );
}

#[test]
fn a_constant_parameter_curve_across_a_linear_sweep_becomes_the_moved_directrix() {
    use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
    use crate::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
    use crate::geometry::surface_payloads::LinearSweepSurfaceConstruction;
    use crate::geometry::{ProceduralSurface, Surface, SurfaceGeometry};
    use crate::ids::{ProceduralSurfaceId, SurfaceId};
    use crate::math::Vector3;
    use crate::units::FinitePoint2;

    let mut ir = crate::examples::unit_cube().unwrap();
    let knots = vec![0.0, 0.0, 0.0, 0.25, 1.0, 1.0, 1.0];
    let poles = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 2.0, 0.0),
        Point3::new(2.0, -1.0, 0.0),
        Point3::new(3.0, 0.0, 0.0),
    ];
    let directrix = CurveId::mint("test:approximation:curve#directrix").unwrap();
    ir.geometry.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(2, knots.clone(), poles.to_vec(), None, false).unwrap(),
        )),
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });
    let surface = SurfaceId::mint("test:approximation:surface#sweep").unwrap();
    let sweep = ProceduralSurfaceId::mint("test:approximation:construction#sweep").unwrap();
    ir.geometry.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: sweep.clone(),
            cache: None,
        },
        source_object: None,
        native_ref: None,
    });
    ir.geometry.procedural_surfaces.push(ProceduralSurface::new(
        sweep,
        ProceduralSurfaceDefinition::LinearSweep(
            LinearSweepSurfaceConstruction::try_new(directrix, Vector3::new(0.0, 0.0, 2.0))
                .unwrap(),
        ),
        None,
    ));
    let nurbs = PcurveNurbs::new(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        PcurveNurbsPoles::Polynomial {
            points: vec![
                FinitePoint2::new(Point2::new(1.0, 0.5)).unwrap(),
                FinitePoint2::new(Point2::new(0.0, 0.5)).unwrap(),
            ],
        },
        false,
    )
    .unwrap();
    let curve = CurveId::mint("test:approximation:curve#across-sweep").unwrap();
    let construction =
        ProceduralCurveId::mint("test:approximation:construction#across-sweep").unwrap();
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: Some(surface),
                pcurve: Some(SupportPcurve::new(PcurveGeometry::Nurbs { nurbs }, None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });

    let arena = DecodeArena::new();
    let context = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let mut approximator = Approximator::new(&ir, &context);
    let Some(SolvedCurveGeometry::Nurbs(exact)) = approximator.curve(curve.as_str()) else {
        panic!("a constant-parameter curve on a linear sweep has an exact spline form");
    };

    assert_eq!(exact.degree(), 2);
    // The line runs from u = 1 to u = 0, so the directrix is reversed over the same interval.
    let mut moved = poles.map(|pole| Point3::new(pole.x, pole.y, 1.0));
    moved.reverse();
    assert_eq!(
        exact.knots().as_slice(),
        [0.0, 0.0, 0.0, 0.75, 1.0, 1.0, 1.0]
    );
    assert_eq!(exact.pole_rows().raw_points(), moved);
}

#[test]
fn a_kinked_curve_on_a_plane_becomes_the_exact_spline_through_the_plane() {
    use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
    use crate::units::FinitePoint2;

    let mut ir = crate::examples::unit_cube().unwrap();
    let plane = ir.geometry.faces[0].surface.clone();
    let uv = [
        (0.0, 0.0),
        (0.1, 0.05),
        (0.2, 0.1),
        (0.3, 0.3),
        (0.4, 0.5),
        (0.5, 0.45),
        (0.6, 0.4),
    ];
    let points = uv
        .iter()
        .map(|&(u, v)| FinitePoint2::new(Point2::new(u, v)).unwrap())
        .collect::<Vec<_>>();
    let nurbs = PcurveNurbs::new(
        2,
        vec![
            0.0,
            0.0,
            0.0,
            2.0 / 7.0,
            2.0 / 7.0,
            4.0 / 7.0,
            4.0 / 7.0,
            1.0,
            1.0,
            1.0,
        ],
        PcurveNurbsPoles::Polynomial { points },
        false,
    )
    .unwrap();
    let curve = CurveId::mint("test:approximation:curve#kinked").unwrap();
    let construction = ProceduralCurveId::mint("test:approximation:construction#kinked").unwrap();
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: Some(plane.clone()),
                pcurve: Some(SupportPcurve::new(PcurveGeometry::Nurbs { nurbs }, None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });

    let arena = DecodeArena::new();
    let context = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let mut approximator = Approximator::new(&ir, &context);
    let Some(SolvedCurveGeometry::Nurbs(exact)) = approximator.curve(curve.as_str()) else {
        panic!("a spline on a plane has an exact spline form");
    };

    assert_eq!(exact.degree(), 2);
    assert_eq!(exact.knots().as_slice()[3], 2.0 / 7.0);
    let index = crate::index::ModelIndex::new_model_only(&ir);
    for (pole, &(u, v)) in exact.pole_rows().raw_points().iter().zip(&uv) {
        let on_plane = crate::eval::model_surface_point_by_id(
            crate::eval::admission::EvaluationAdmission::Standard,
            &index,
            &plane,
            u,
            v,
        )
        .unwrap()
        .get();
        assert!(pole.distance(on_plane) < 1e-12);
    }
}

#[test]
fn fitting_retains_an_evaluation_resource_refusal() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let mut pcurve = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)).unwrap(),
    );
    for _ in 0..MAX_GEOMETRY_NESTING {
        pcurve = PcurveGeometry::Transformed(
            PlacedPcurve::try_new(Box::new(pcurve), Transform2::identity()).unwrap(),
        );
    }
    let curve = CurveId::mint("test:approximation:curve#deep").unwrap();
    let construction = ProceduralCurveId::mint("test:approximation:construction#deep").unwrap();
    let context = IntcurveSupportContext::try_new(
        [
            IntcurveSupportSide {
                surface: Some(ir.geometry.faces[0].surface.clone()),
                pcurve: Some(SupportPcurve::new(pcurve, None)),
            },
            IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });
    let arena = DecodeArena::new();
    let context = DecodeContext::new(&arena, &DecodePolicy::default(), DecodeDepth::Full);
    let mut approximator = Approximator::new(&ir, &context);
    assert!(approximator.curve(curve.as_str()).is_none());
    let refusal = approximator
        .resource_limit()
        .expect("recursion refusal is retained");
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::RecursionDepth
    );
}

/// The sampled residual limit of a model whose linear tolerance is `1e-6`.
const RESIDUAL: f64 = 2.5e-7;

#[test]
fn breaks_at_carrier_knots_fit_each_quadratic_span_at_the_seed() {
    let square = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0],
        vec![
            Point3::new(-10.0, -10.0, 0.0),
            Point3::new(10.0, -10.0, 0.0),
            Point3::new(10.0, 10.0, 0.0),
            Point3::new(-10.0, 10.0, 0.0),
            Point3::new(-10.0, -10.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    let mut evaluations = 0_usize;
    let mut extrusion = |u: f64, v: f64| {
        evaluations += 1;
        let point =
            crate::eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, &square, u)
                .expect("the square has a point across its domain")
                .get();
        Some(Point3::new(point.x, point.y, point.z - v))
    };
    let mut samples = usize::MAX;
    let grid = sample(
        [
            Axis::new([0.0, 3.0], &[1.0, 2.0], SEED_PATCHES).unwrap(),
            Axis::new([-5.0, 0.0], &[], SEED_PATCHES).unwrap(),
        ],
        false,
        RESIDUAL,
        &mut samples,
        usize::MAX,
        &mut extrusion,
    )
    .expect("a cubic patch holds each quadratic span");
    assert_eq!(
        grid.knots[0],
        [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 2.0, 2.0, 2.0, 3.0, 3.0, 3.0, 3.0]
    );
    assert_eq!(grid.rows.len(), 3 * 3 + 1);
    // One level: the (3 * 3 + 1) by (3 * 4 + 1) grid, and 25 probes in each
    // of the 3 by 4 patches.
    assert_eq!(evaluations, 10 * 13 + 3 * 4 * 25);
}

/// Document with a unit cylinder about the Z axis and a bicubic spline surface `z = 0.3 x y`
/// over `[-2, 2]²`, and a curve whose degree-one chart samples their intersection
/// coarsely. Returns the document and the curve identity.
fn cylinder_on_spline_intersection(linear: f64) -> (crate::CadIr, CurveId) {
    use crate::geometry::analytic::CylinderSurface;
    use crate::geometry::intersection_curves::IntcurveSupportSide;
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    use crate::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
    use crate::ids::SurfaceId;

    let mut ir = crate::examples::unit_cube().unwrap();
    ir.tolerances.linear = crate::scalar::PositiveLength::new(linear).unwrap();
    let grid = [-2.0, -2.0 / 3.0, 2.0 / 3.0, 2.0];
    let spline = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(3, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(
            grid.iter()
                .map(|&x| {
                    grid.iter()
                        .map(|&y| Point3::new(x, y, 0.3 * x * y))
                        .collect()
                })
                .collect(),
            None,
        ),
        false,
    )
    .unwrap();
    let cylinder = CylinderSurface::try_new(
        Point3::new(0.0, 0.0, 0.0),
        crate::math::Vector3::new(0.0, 0.0, 1.0),
        crate::math::Vector3::new(1.0, 0.0, 0.0),
        1.0,
    )
    .unwrap();
    let mut add = |name: &str, geometry: SolvedSurfaceGeometry| {
        let id = SurfaceId::mint(name).unwrap();
        ir.geometry.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Solved(geometry),
            source_object: None,
            native_ref: None,
        });
        id
    };
    let cylinder = add(
        "test:approximation:surface#cylinder",
        SolvedSurfaceGeometry::Cylinder(cylinder),
    );
    let spline = add(
        "test:approximation:surface#free-form",
        SolvedSurfaceGeometry::Nurbs(spline),
    );
    let segments = 8_u32;
    let points: Vec<Point3> = (0..=segments)
        .map(|i| {
            let t = 0.3 + 1.6 * f64::from(i) / f64::from(segments);
            Point3::new(t.cos(), t.sin(), 0.15 * (2.0 * t).sin())
        })
        .collect();
    let mut knots = vec![0.0];
    knots.extend((0..=segments).map(f64::from));
    knots.push(f64::from(segments));
    let chart = NurbsCurve::from_lanes(1, knots, points, None, false).unwrap();
    let curve = CurveId::mint("test:approximation:curve#cylinder-on-spline").unwrap();
    let construction =
        ProceduralCurveId::mint("test:approximation:construction#cylinder-on-spline").unwrap();
    let side = |surface| IntcurveSupportSide {
        surface: Some(surface),
        pcurve: None,
    };
    let context = IntcurveSupportContext::try_new(
        [side(cylinder), side(spline)],
        [0.0, f64::from(segments)],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .unwrap();
    ir.geometry.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::Intersection {
            context,
            discontinuity_flag: false,
            cache: None,
        },
    ));
    ir.geometry.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: Some(SolvedCurveGeometry::Nurbs(chart)),
        },
        parameter_range: None,
        source_object: None,
        native_ref: None,
    });
    (ir, curve)
}

/// The largest departure of `fitted` from the cylinder and from the spline surface of
/// [`cylinder_on_spline_intersection`], over the sampled parameters.
fn departure_from_supports(fitted: &NurbsCurve, steps: u32) -> f64 {
    let domain = crate::eval::nurbs_curve_parameter_domain(fitted)
        .map(crate::topology::IncreasingParameterInterval::endpoints)
        .unwrap();
    (0..=steps)
        .map(|i| {
            let u = domain[0] + (domain[1] - domain[0]) * f64::from(i) / f64::from(steps);
            let p =
                crate::eval::decode::nurbs_curve_point_at(EvaluationAdmission::Standard, fitted, u)
                    .unwrap()
                    .get();
            let on_cylinder = (p.x.hypot(p.y) - 1.0).abs();
            let on_spline =
                (p.z - 0.3 * p.x * p.y).abs() / (1.0 + 0.09 * (p.x * p.x + p.y * p.y)).sqrt();
            on_cylinder.max(on_spline)
        })
        .fold(0.0, f64::max)
}

#[test]
fn a_cylinder_spline_intersection_is_fitted_within_the_document_tolerance() {
    let linear = 1.0e-5;
    let (ir, curve) = cylinder_on_spline_intersection(linear);
    let mut approximator = Approximator::new(&ir);
    let Some(SolvedCurveGeometry::Nurbs(fitted)) = approximator.exact_intersection(curve.as_str())
    else {
        panic!("a cylinder and a spline surface meet in a fitted carrier");
    };
    assert!(departure_from_supports(&fitted, 4000) <= linear);
    assert_eq!(approximator.free_form_curve_fits().0, 0);
}
