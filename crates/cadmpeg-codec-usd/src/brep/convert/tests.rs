// SPDX-License-Identifier: Apache-2.0

use super::{
    elliptic_arc, evaluate, hyperbolic_arc, nurbs_only, project, revolved, surface_point, traced,
    ParamMap,
};
use crate::brep::{Catalog, NurbsCurveRecord};

/// Distance within which an exact rational form reproduces its analytic
/// point.
const EXACT: f64 = 1e-12;

#[test]
fn an_arc_parameter_maps_to_its_angle() {
    let (arc, map) = elliptic_arc([0.0, 0.0], [2.0, 0.0], [0.0, 1.0], 0.3, 4.0);
    for theta in [0.3, 0.5, 1.0, 1.8, 2.5, 3.3, 4.0] {
        let point = evaluate(&arc, map.apply(theta)).expect("point");
        assert!((point[0] - 2.0 * theta.cos()).abs() < EXACT, "{theta}");
        assert!((point[1] - theta.sin()).abs() < EXACT, "{theta}");
    }
    assert_eq!(map.apply(0.3), 0.3);
    assert_eq!(map.apply(4.0), 4.0);
}

#[test]
fn a_hyperbolic_parameter_maps_to_its_parameter() {
    let (arc, map) = hyperbolic_arc([0.0; 3], [1.0, 0.0, 0.0], [0.0, 3.0, 0.0], -0.7, 1.2);
    for t in [-0.7, -0.2, 0.0, 0.6, 1.2] {
        let point = evaluate(&arc, map.apply(t)).expect("point");
        assert!((point[0] - t.cosh()).abs() < EXACT, "{t}");
        assert!((point[1] - 3.0 * t.sinh()).abs() < EXACT, "{t}");
    }
}

#[test]
fn a_revolved_arc_is_a_sphere() {
    let radius = 1.5;
    let (profile, latitude) =
        elliptic_arc([0.0; 3], [radius, 0.0, 0.0], [0.0, 0.0, radius], -1.0, 1.4);
    let (surface, longitude) = revolved(&profile, [0.0; 3], [0.0, 0.0, 1.0], -0.5, 5.0);
    for (u, v) in [
        (-0.5, -1.0),
        (0.0, 0.0),
        (2.0, 0.7),
        (4.4, 1.4),
        (5.0, -0.3),
    ] {
        let point =
            surface_point(&surface, [longitude.apply(u), latitude.apply(v)]).expect("point");
        let expected = [
            radius * v.cos() * u.cos(),
            radius * v.cos() * u.sin(),
            radius * v.sin(),
        ];
        for axis in 0..3 {
            assert!((point[axis] - expected[axis]).abs() < EXACT, "{u} {v}");
        }
    }
}

#[test]
fn a_traced_curve_is_within_its_tolerance() {
    let tolerance = 1e-9;
    let image = |t: f64| Some([t.cos(), 0.5 * t.sin()]);
    let curve = traced(&image, 0.0, 3.0, tolerance).expect("trace");
    assert_eq!(curve.order, 4);
    assert_eq!(curve.points.first(), image(0.0).as_ref());
    assert_eq!(curve.points.last(), image(3.0).as_ref());
    let domain = [curve.knots[3], curve.knots[curve.points.len()]];
    for step in 0..=200 {
        let s = domain[0] + (domain[1] - domain[0]) * f64::from(step) / 200.0;
        let point = evaluate(&curve, s).expect("point");
        let nearest = (0..=30_000)
            .map(|index| {
                let exact = image(3.0 * f64::from(index) / 30_000.0).expect("image");
                (exact[0] - point[0]).hypot(exact[1] - point[1])
            })
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 1e-7, "{s} {nearest}");
    }
}

#[test]
fn a_point_projects_to_its_surface_parameters() {
    let (profile, _) = elliptic_arc([0.0; 3], [2.0, 0.0, 0.0], [0.0, 0.0, 2.0], -1.2, 1.2);
    let (surface, _) = revolved(&profile, [0.0; 3], [0.0, 0.0, 1.0], 0.0, 3.0);
    let target = [0.7, 0.4];
    let point = surface_point(&surface, target).expect("point");
    let found = project(&surface, point, [0.5, 0.2]).expect("projection");
    let reached = surface_point(&surface, found).expect("point");
    for axis in 0..3 {
        assert!((reached[axis] - point[axis]).abs() < 1e-10);
    }
}

#[test]
fn a_cube_converts_with_exact_planar_trims() {
    let cube = crate::brep::primitive::cube(2.0);
    let converted = nurbs_only(&cube).expect("convert");
    assert!(converted.trimmed);
    let brep = converted.brep;
    assert_eq!(brep.nurbs_surfaces.len(), cube.faces.len());
    assert_eq!(brep.nurbs_curves.len(), cube.edges.len());
    assert!(brep.analytic_surfaces.is_empty() && brep.analytic_curves.is_empty());
    for (index, edge) in brep.edges.iter().enumerate() {
        assert_eq!(edge.curve, Some((Catalog::Nurbs, index)));
        let curve = &brep.nurbs_curves[index];
        let start = evaluate(curve, edge.range[0]).expect("start");
        let end = evaluate(curve, edge.range[1]).expect("end");
        assert_eq!(start, brep.vertices[edge.vertices[0]]);
        assert_eq!(end, brep.vertices[edge.vertices[1]]);
    }
    for (index, oriented) in brep.oriented_edges.iter().enumerate() {
        assert_eq!(oriented.pcurve, Some((Catalog::Nurbs, index)));
        let face = brep
            .faces
            .iter()
            .position(|face| {
                face.loops
                    .iter()
                    .any(|loop_| brep.loops[*loop_].oriented_edges.contains(&index))
            })
            .expect("face");
        let pcurve: &NurbsCurveRecord<2> = &brep.nurbs_pcurves[index];
        let edge = &brep.edges[oriented.edge];
        let end = if oriented.orientation {
            edge.vertices[1]
        } else {
            edge.vertices[0]
        };
        let last = *pcurve.points.last().expect("point");
        let point = surface_point(&brep.nurbs_surfaces[face], last).expect("point");
        for (value, expected) in point.iter().zip(brep.vertices[end]) {
            assert!((value - expected).abs() < EXACT);
        }
    }
    assert_eq!(ParamMap::Identity.apply(1.25), 1.25);
}
