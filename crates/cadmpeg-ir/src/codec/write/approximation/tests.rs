// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::{chart_ending_on_edge_vertices, Approximator};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::pcurve::{LinePcurve, PcurveGeometry, PlacedPcurve};
use crate::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, ProceduralCurve,
    ProceduralCurveDefinition, SolvedCurveGeometry, SupportPcurve, SurfaceCurveFamily,
    MAX_GEOMETRY_NESTING,
};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::math::{Point2, Point3};
use crate::transform::Transform2;

#[test]
fn chart_snapping_moves_only_an_end_pole_within_the_fit_tolerance() {
    let ir = crate::examples::unit_cube().unwrap();
    let index = crate::index::ModelIndex::new_model_only(&ir);
    let edge = &ir.model.edges[0];
    let curve = edge.curve().unwrap();
    let position = |id: &crate::ids::VertexId| {
        let vertex = ir.model.vertices.iter().find(|v| &v.id == id).unwrap();
        let point = ir
            .model
            .points
            .iter()
            .find(|p| p.id == vertex.point)
            .unwrap();
        point.position().get()
    };
    let start = position(&edge.start);
    let end = position(&edge.end);
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
    let cache = crate::geometry::LegacyCache::try_new(1e-3).unwrap();

    let snapped = chart_ending_on_edge_vertices(&ir, &index, curve, &chart, Some(&cache)).unwrap();

    let poles = snapped.pole_rows().raw_points();
    assert_eq!(poles[0].distance(start), 0.0);
    assert_eq!(poles[1].distance(near(end)), 0.0);
    assert_eq!(poles[2].distance(middle), 0.0);
}

#[test]
fn a_curve_without_a_chart_keeps_the_pcurves_of_its_stored_carrier() {
    let ir = crate::examples::unit_cube().unwrap();
    let curve = ir.model.edges[0].curve().unwrap().as_str().to_owned();
    let mut approximator = Approximator::new(&ir);
    assert!(!approximator.replaces_stored_chart(&curve));
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
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(spline)),
        source_object: None,
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
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
    });

    let mut approximator = Approximator::new(&ir);
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
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(spline)),
        source_object: None,
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
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
    });

    let mut approximator = Approximator::new(&ir);
    let Some(SolvedCurveGeometry::Nurbs(exact)) = approximator.curve(curve.as_str()) else {
        panic!("a constant-parameter curve on a spline surface has an exact spline form");
    };

    assert_eq!(exact.degree(), 2);
    assert_eq!(exact.knots().as_slice(), u_knots.as_slice());
    assert_eq!(exact.pole_rows().raw_points(), far_row);
}

#[test]
fn a_constant_parameter_curve_across_a_linear_sweep_becomes_the_moved_directrix() {
    use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
    use crate::geometry::surface_payloads::LinearSweepSurfaceConstruction;
    use crate::geometry::{
        ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
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
    ir.model.curves.push(Curve {
        id: directrix.clone(),
        geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            NurbsCurve::from_lanes(2, knots.clone(), poles.to_vec(), None, false).unwrap(),
        )),
        parameter_range: None,
        source_object: None,
    });
    let surface = SurfaceId::mint("test:approximation:surface#sweep").unwrap();
    let sweep = ProceduralSurfaceId::mint("test:approximation:construction#sweep").unwrap();
    ir.model.surfaces.push(Surface {
        id: surface.clone(),
        geometry: SurfaceGeometry::Procedural {
            construction: sweep.clone(),
            cache: None,
        },
        source_object: None,
    });
    ir.model.procedural_surfaces.push(ProceduralSurface::new(
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
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
    });

    let mut approximator = Approximator::new(&ir);
    let Some(SolvedCurveGeometry::Nurbs(exact)) = approximator.curve(curve.as_str()) else {
        panic!("a constant-parameter curve on a linear sweep has an exact spline form");
    };

    assert_eq!(exact.degree(), 2);
    assert_eq!(exact.knots().as_slice(), knots.as_slice());
    assert_eq!(
        exact.pole_rows().raw_points(),
        poles.map(|pole| Point3::new(pole.x, pole.y, 1.0))
    );
}

#[test]
fn a_kinked_curve_on_a_plane_becomes_the_exact_spline_through_the_plane() {
    use crate::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};
    use crate::units::FinitePoint2;

    let mut ir = crate::examples::unit_cube().unwrap();
    let plane = ir.model.faces[0].surface.clone();
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
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
    });

    let mut approximator = Approximator::new(&ir);
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
                surface: Some(ir.model.faces[0].surface.clone()),
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
    ir.model.procedural_curves.push(ProceduralCurve::new(
        construction.clone(),
        ProceduralCurveDefinition::SurfaceCurve {
            family: SurfaceCurveFamily::Parametric {
                context,
                tail: None,
            },
        },
    ));
    ir.model.curves.push(Curve {
        id: curve.clone(),
        geometry: CurveGeometry::Procedural {
            construction,
            cache: None,
        },
        parameter_range: None,
        source_object: None,
    });
    let mut approximator = Approximator::new(&ir);
    assert!(approximator.curve(curve.as_str()).is_none());
    let refusal = approximator
        .resource_limit()
        .expect("recursion refusal is retained");
    assert_eq!(
        refusal.dimension,
        cadmpeg_core::decode::ResourceDimension::RecursionDepth
    );
}
