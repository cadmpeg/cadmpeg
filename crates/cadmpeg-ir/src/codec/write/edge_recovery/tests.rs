// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::recover_edge_curves;
use crate::eval::{admission::EvaluationAdmission, decode};
use crate::examples::unit_cube;
use crate::geometry::pcurve::{
    LinePcurve, Pcurve, PcurveGeometry, PcurveMetadata, PcurveNurbs, PcurveNurbsPoles,
};
use crate::geometry::{DirectedParameterRange, SolvedSurfaceGeometry};
use crate::ids::PcurveId;
use crate::math::{Point2, Point3, Vector3};
use crate::topology::{CoedgeUseCurve, EdgeCarrier, ParameterInterval, PcurveUse};
use crate::CadIr;

fn attach_pcurve(ir: &mut CadIr, coedge_index: usize, reverse: bool, bow: f64) {
    let coedge = &ir.model.coedges[coedge_index];
    let edge = ir
        .model
        .edges
        .iter()
        .find(|edge| edge.id == coedge.edge)
        .unwrap();
    let at_vertex = |id: &crate::ids::VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|vertex| &vertex.id == id)
            .unwrap();
        ir.model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .unwrap()
            .position()
            .get()
    };
    let first = at_vertex(&edge.start);
    let last = at_vertex(&edge.end);
    let lp = ir
        .model
        .loops
        .iter()
        .find(|lp| lp.id == coedge.owner_loop)
        .unwrap();
    let face = ir
        .model
        .faces
        .iter()
        .find(|face| face.id == lp.face)
        .unwrap();
    let surface = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .unwrap();
    let Some(SolvedSurfaceGeometry::Plane(plane)) = surface.geometry.solved() else {
        panic!("cube plane")
    };
    let origin = plane.origin().get();
    let u = *plane.frame().reference().as_raw();
    let v = plane.frame().axis().as_raw().cross(u);
    let uv = |point: Point3| {
        let d = Vector3::new(point.x - origin.x, point.y - origin.y, point.z - origin.z);
        Point2::new(d.dot(u), d.dot(v))
    };
    let (a, b) = if reverse {
        (uv(last), uv(first))
    } else {
        (uv(first), uv(last))
    };
    let geometry = if bow == 0.0 {
        PcurveGeometry::Line(LinePcurve::try_new(a, Point2::new(b.u - a.u, b.v - a.v)).unwrap())
    } else {
        PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                PcurveNurbsPoles::Polynomial {
                    points: vec![
                        a,
                        Point2::new(f64::midpoint(a.u, b.u), f64::midpoint(a.v, b.v) + bow),
                        b,
                    ],
                },
                false,
            )
            .unwrap(),
        }
    };
    let id = PcurveId::mint(format!("test:recovery:pcurve#{coedge_index}")).unwrap();
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry,
        metadata: PcurveMetadata::default(),
    });
    ir.model.coedges[coedge_index].pcurves.push(PcurveUse {
        pcurve: id,
        isoparametric: None,
        parameter_range: Some(DirectedParameterRange::new([0.0, 1.0]).unwrap()),
    });
}

fn coedges_for_first_edge(ir: &CadIr) -> Vec<usize> {
    ir.model
        .coedges
        .iter()
        .enumerate()
        .filter_map(|(i, coedge)| (coedge.edge == ir.model.edges[0].id).then_some(i))
        .collect()
}

#[test]
fn recovers_an_existing_coedge_curve_without_changing_the_source() {
    let mut ir = unit_cube().unwrap();
    let curve = ir.model.edges[0].curve().unwrap().clone();
    let coedge = coedges_for_first_edge(&ir)[0];
    ir.model.edges[0].carrier = EdgeCarrier::Free;
    ir.model.coedges[coedge].use_curve = Some(CoedgeUseCurve {
        curve: curve.clone(),
        parameter_range: ParameterInterval::new([0.0, 10.0]).unwrap(),
    });
    let recovered = recover_edge_curves(&ir).unwrap();
    assert_eq!(recovered.document.model.edges[0].curve(), Some(&curve));
    assert!(recovered.recovered_edges.contains(&ir.model.edges[0].id));
    assert!(ir.model.edges[0].curve().is_none());
}

#[test]
fn recovers_a_reversed_coedge_curve_in_the_edge_direction() {
    let mut ir = unit_cube().unwrap();
    let curve = ir.model.edges[0].curve().unwrap().clone();
    let coedge = coedges_for_first_edge(&ir)[0];
    let carrier = ir
        .model
        .curves
        .iter_mut()
        .find(|carrier| carrier.id == curve)
        .unwrap();
    let crate::geometry::CurveGeometry::Solved(crate::geometry::SolvedCurveGeometry::Line(line)) =
        &mut carrier.geometry
    else {
        panic!("cube line")
    };
    line.reverse_parameterization();
    ir.model.edges[0].carrier = EdgeCarrier::Free;
    ir.model.coedges[coedge].use_curve = Some(CoedgeUseCurve {
        curve,
        parameter_range: ParameterInterval::new([-10.0, 0.0]).unwrap(),
    });
    let recovered = recover_edge_curves(&ir).unwrap();
    let edge = &recovered.document.model.edges[0];
    let curve = recovered
        .document
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == edge.curve())
        .unwrap();
    let range = edge.param_range().unwrap().get();
    let start = decode::curve_point(EvaluationAdmission::Standard, &curve.geometry, range[0])
        .unwrap()
        .get();
    let end = decode::curve_point(EvaluationAdmission::Standard, &curve.geometry, range[1])
        .unwrap()
        .get();
    assert!(start.distance(Point3::new(0.0, 0.0, 0.0)) < 1e-10);
    assert!(end.distance(Point3::new(10.0, 0.0, 0.0)) < 1e-10);
}

#[test]
fn lifts_a_planar_pcurve_in_both_parameter_directions() {
    for (reverse, explicit_range) in [(false, true), (true, true), (false, false), (true, false)] {
        let mut ir = unit_cube().unwrap();
        let old_curve = ir.model.edges[0].curve().unwrap().clone();
        ir.model.edges[0].carrier = EdgeCarrier::Free;
        ir.model.curves.retain(|curve| curve.id != old_curve);
        let coedge = coedges_for_first_edge(&ir)[0];
        attach_pcurve(&mut ir, coedge, reverse, 0.0);
        if !explicit_range {
            ir.model.coedges[coedge].pcurves[0].parameter_range = None;
        }
        let recovered = recover_edge_curves(&ir).unwrap();
        if !explicit_range {
            assert!(ir.model.coedges[coedge].pcurves[0]
                .parameter_range
                .is_none());
            assert!(recovered.document.model.coedges[coedge].pcurves[0]
                .parameter_range
                .is_some());
        }
        let edge = &recovered.document.model.edges[0];
        let curve = recovered
            .document
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())
            .unwrap();
        let range = edge.param_range().unwrap().get();
        for (t, expected) in [
            (range[0], Point3::new(0.0, 0.0, 0.0)),
            (range[1], Point3::new(10.0, 0.0, 0.0)),
        ] {
            let actual = decode::curve_point(EvaluationAdmission::Standard, &curve.geometry, t)
                .unwrap()
                .get();
            assert!(actual.distance(expected) < 1e-10);
        }
        assert!(ir.model.edges[0].curve().is_none());
    }
}

#[test]
fn conflicting_coedge_pcurves_do_not_define_a_shared_curve() {
    let mut ir = unit_cube().unwrap();
    ir.model.edges[0].carrier = EdgeCarrier::Free;
    let coedges = coedges_for_first_edge(&ir);
    attach_pcurve(&mut ir, coedges[0], false, 0.0);
    attach_pcurve(&mut ir, coedges[1], false, 2.0);
    let recovered = recover_edge_curves(&ir).unwrap();
    assert!(recovered.document.model.edges[0].curve().is_none());
}

#[test]
fn matching_coedges_record_that_their_agreement_was_sampled() {
    let mut ir = unit_cube().unwrap();
    ir.model.edges[0].carrier = EdgeCarrier::Free;
    let coedges = coedges_for_first_edge(&ir);
    attach_pcurve(&mut ir, coedges[0], false, 0.0);
    attach_pcurve(&mut ir, coedges[1], true, 0.0);
    let recovered = recover_edge_curves(&ir).unwrap();
    assert!(recovered.document.model.edges[0].curve().is_some());
    assert_eq!(
        recovered.sampled_agreements,
        vec![ir.model.edges[0].id.clone()]
    );
}

#[test]
fn recovers_a_deeply_placed_pcurve_on_the_normal_test_stack() {
    use crate::geometry::pcurve::PlacedPcurve;
    use crate::geometry::MAX_GEOMETRY_NESTING;

    let mut ir = unit_cube().unwrap();
    let coedge = coedges_for_first_edge(&ir)[0];
    attach_pcurve(&mut ir, coedge, false, 0.0);
    let pcurve = ir.model.pcurves.last_mut().unwrap();
    let mut geometry = pcurve.geometry.clone();
    for _ in 0..MAX_GEOMETRY_NESTING - 1 {
        geometry = PcurveGeometry::Transformed(
            PlacedPcurve::try_new(Box::new(geometry), crate::transform::Transform2::identity())
                .unwrap(),
        );
    }
    pcurve.geometry = geometry;
    ir.model.edges[0].carrier = EdgeCarrier::Free;
    let recovered = recover_edge_curves(&ir).unwrap();
    assert_eq!(recovered.sampled_fits, vec![ir.model.edges[0].id.clone()]);
    assert!(recovered.document.model.edges[0].curve().is_some());
}

fn cylinder_pcurve(direction: Point2, end: Point3) -> CadIr {
    use crate::geometry::analytic::CylinderSurface;
    use crate::geometry::SurfaceGeometry;

    let mut ir = unit_cube().unwrap();
    let coedge = coedges_for_first_edge(&ir)[0];
    let lp = ir
        .model
        .loops
        .iter()
        .find(|lp| lp.id == ir.model.coedges[coedge].owner_loop)
        .unwrap();
    let face = ir
        .model
        .faces
        .iter()
        .find(|face| face.id == lp.face)
        .unwrap();
    let surface_id = face.surface.clone();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .unwrap(),
    ));
    ir.model.points[0]
        .set_position(crate::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0)).unwrap());
    ir.model.points[1].set_position(crate::features::FinitePoint3::new(end).unwrap());
    let id = PcurveId::mint("test:recovery:pcurve#helix").unwrap();
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry: PcurveGeometry::Line(
            LinePcurve::try_new(Point2::new(0.0, 0.0), direction).unwrap(),
        ),
        metadata: PcurveMetadata::default(),
    });
    ir.model.coedges[coedge].pcurves.push(PcurveUse {
        pcurve: id,
        isoparametric: None,
        parameter_range: Some(DirectedParameterRange::new([0.0, 1.0]).unwrap()),
    });
    ir.model.edges[0].carrier = EdgeCarrier::Free;
    ir
}

#[test]
fn a_nonplanar_pcurve_is_fitted_with_reported_sampled_residual() {
    use crate::geometry::CurveGeometry;
    use crate::index::{ModelIndex, StandardIndex};

    let ir = cylinder_pcurve(
        Point2::new(std::f64::consts::FRAC_PI_2, 1.0),
        Point3::new(0.0, 1.0, 1.0),
    );
    let recovered = recover_edge_curves(&ir).unwrap();
    let curve = recovered.document.model.edges[0].curve().unwrap();
    let index = ModelIndex::new_model_only(&recovered.document);
    assert!(matches!(
        index
            .curves(curve.as_str(), StandardIndex)
            .unwrap()
            .geometry,
        CurveGeometry::Solved(crate::geometry::SolvedCurveGeometry::Nurbs(_))
    ));
    assert_eq!(recovered.sampled_fits, vec![ir.model.edges[0].id.clone()]);
    let midpoint =
        crate::eval::model_curve_point_by_id(EvaluationAdmission::Standard, &index, curve, 0.5)
            .unwrap()
            .get();
    assert!(
        midpoint.distance(Point3::new(
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.5
        )) <= recovered.residual_limit
    );
}

#[test]
fn cylinder_isoparametric_lines_recover_exact_curves() {
    for (direction, end, midpoint) in [
        (
            Point2::new(0.0, 2.0),
            Point3::new(1.0, 0.0, 2.0),
            Point3::new(1.0, 0.0, 1.0),
        ),
        (
            Point2::new(std::f64::consts::FRAC_PI_2, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
                0.0,
            ),
        ),
        (
            Point2::new(-std::f64::consts::FRAC_PI_2, 0.0),
            Point3::new(0.0, -1.0, 0.0),
            Point3::new(
                std::f64::consts::FRAC_1_SQRT_2,
                -std::f64::consts::FRAC_1_SQRT_2,
                0.0,
            ),
        ),
    ] {
        let ir = cylinder_pcurve(direction, end);
        let recovered = recover_edge_curves(&ir).unwrap();
        assert!(recovered.sampled_fits.is_empty());
        let edge = &recovered.document.model.edges[0];
        let curve = recovered
            .document
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())
            .unwrap();
        let [start, finish] = edge.param_range().unwrap().get();
        let actual = decode::curve_point(
            EvaluationAdmission::Standard,
            &curve.geometry,
            f64::midpoint(start, finish),
        )
        .unwrap()
        .get();
        assert!(actual.distance(midpoint) < 1.0e-10);
    }
}

#[test]
fn planar_circles_recover_in_both_edge_directions() {
    use crate::geometry::analytic::PlaneSurface;
    use crate::geometry::pcurve::CirclePcurve;
    use crate::geometry::{SolvedCurveGeometry, SurfaceGeometry};

    for reversed in [false, true] {
        let mut ir = unit_cube().unwrap();
        let coedge = coedges_for_first_edge(&ir)[0];
        let lp = ir
            .model
            .loops
            .iter()
            .find(|lp| lp.id == ir.model.coedges[coedge].owner_loop)
            .unwrap();
        let face = ir
            .model
            .faces
            .iter()
            .find(|face| face.id == lp.face)
            .unwrap();
        let surface_id = face.surface.clone();
        ir.model
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == surface_id)
            .unwrap()
            .geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        ));
        let (first, last) = if reversed {
            (Point3::new(0.0, 1.0, 0.0), Point3::new(1.0, 0.0, 0.0))
        } else {
            (Point3::new(1.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0))
        };
        ir.model.points[0].set_position(crate::features::FinitePoint3::new(first).unwrap());
        ir.model.points[1].set_position(crate::features::FinitePoint3::new(last).unwrap());
        let id = PcurveId::mint("test:recovery:pcurve#circle").unwrap();
        ir.model.pcurves.push(Pcurve {
            id: id.clone(),
            geometry: PcurveGeometry::Circle(
                CirclePcurve::try_new(
                    Point2::new(0.0, 0.0),
                    Point2::new(1.0, 0.0),
                    Point2::new(0.0, 1.0),
                    1.0,
                )
                .unwrap(),
            ),
            metadata: PcurveMetadata::default(),
        });
        ir.model.coedges[coedge].pcurves.push(PcurveUse {
            pcurve: id,
            isoparametric: None,
            parameter_range: Some(
                DirectedParameterRange::new([0.0, std::f64::consts::FRAC_PI_2]).unwrap(),
            ),
        });
        ir.model.edges[0].carrier = EdgeCarrier::Free;
        let recovered = recover_edge_curves(&ir).unwrap();
        assert!(recovered.sampled_fits.is_empty());
        let edge = &recovered.document.model.edges[0];
        let curve = recovered
            .document
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())
            .unwrap();
        assert!(matches!(
            curve.geometry.solved(),
            Some(SolvedCurveGeometry::Circle(_))
        ));
        let [start, end] = edge.param_range().unwrap().get();
        for (parameter, expected) in [(start, first), (end, last)] {
            let actual =
                decode::curve_point(EvaluationAdmission::Standard, &curve.geometry, parameter)
                    .unwrap()
                    .get();
            assert!(actual.distance(expected) < 1.0e-10);
        }
    }
}
