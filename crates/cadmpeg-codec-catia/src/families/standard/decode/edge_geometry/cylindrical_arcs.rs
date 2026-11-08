// SPDX-License-Identifier: Apache-2.0
//! Match an unbounded circular edge to the opposite arc of a cylindrical strip.

use std::collections::BTreeMap;

use cadmpeg_ir::geometry::analytic::CircleCurve;

use crate::families::standard::records::{StandardCurveGeometry, StandardCurveSupport};
use crate::families::standard::topology::admitted::StandardTopology;

use super::{
    standard_circle_axis_from_carrier, unwrap_angle, witness_arc_end, CadIr, CurveGeometry,
    CurveId, HashMap, Point3, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    SurfaceId, ANALYTIC_CURVE_ENDPOINT_TOLERANCE, EPS_STANDARD_DECODE_GEOMETRY,
};

pub(in crate::families::standard::decode) fn ranges(
    ir: &CadIr,
    bindings: &[(SurfaceId, bool, usize)],
    surface_indices: &HashMap<SurfaceId, usize>,
    curve_indices: &HashMap<CurveId, usize>,
    supports: &[StandardCurveSupport],
    topology: &StandardTopology,
    endpoints: impl Fn(usize) -> [Point3; 2],
) -> BTreeMap<usize, Option<[f64; 2]>> {
    let mut recovered = BTreeMap::<usize, Option<[f64; 2]>>::new();
    for (face, topology) in topology.faces().iter().enumerate() {
        let surface = &ir.model.surfaces[surface_indices[&bindings[face].0]].geometry;
        let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder)) = surface else {
            continue;
        };
        let [boundary] = topology.boundaries() else {
            continue;
        };
        let [a, b, c, d] = boundary.coedges() else {
            continue;
        };
        let is_line = |edge: usize| matches!(supports[edge].geometry, StandardCurveGeometry::Line);
        let ([first, second], generators) = if is_line(b.edge_row) && is_line(d.edge_row) {
            ([a.edge_row, c.edge_row], [b.edge_row, d.edge_row])
        } else if is_line(a.edge_row) && is_line(c.edge_row) {
            ([b.edge_row, d.edge_row], [a.edge_row, c.edge_row])
        } else {
            continue;
        };
        if generators.into_iter().any(|edge| {
            let [start, end] = endpoints(edge);
            end.vector_from(start)
                .cross(*cylinder.frame().axis().as_raw())
                .norm()
                > ANALYTIC_CURVE_ENDPOINT_TOLERANCE
        }) {
            continue;
        }
        let (target, witness, range) = match [
            ir.model.edges[first].param_range(),
            ir.model.edges[second].param_range(),
        ] {
            [None, Some(range)] => (first, second, range.get()),
            [Some(range), None] => (second, first, range.get()),
            _ => continue,
        };
        if !matches!(supports[target].geometry, StandardCurveGeometry::Bspline) {
            continue;
        }
        let circle = |edge: usize| {
            let curve = ir.model.edges[edge].curve()?;
            let geometry = &ir.model.curves[*curve_indices.get(curve)?].geometry;
            let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) = geometry else {
                return None;
            };
            Some(circle)
        };
        let (Some(target_circle), Some(witness_circle)) = (circle(target), circle(witness)) else {
            continue;
        };
        let Some(range) = translated_range(
            surface,
            target_circle,
            endpoints(target),
            witness_circle,
            endpoints(witness),
            range,
        ) else {
            continue;
        };
        recovered
            .entry(target)
            .and_modify(|prior| {
                if prior.is_some_and(|prior| {
                    prior
                        .into_iter()
                        .zip(range)
                        .any(|(a, b)| (a - b).abs() > EPS_STANDARD_DECODE_GEOMETRY)
                }) {
                    *prior = None;
                }
            })
            .or_insert(Some(range));
    }
    recovered
}

fn translated_range(
    surface: &SurfaceGeometry,
    target: &CircleCurve,
    endpoints: [Point3; 2],
    witness: &CircleCurve,
    witness_endpoints: [Point3; 2],
    range: [f64; 2],
) -> Option<[f64; 2]> {
    let tolerance = ANALYTIC_CURVE_ENDPOINT_TOLERANCE;
    for circle in [target, witness] {
        let axis = standard_circle_axis_from_carrier(
            circle.center().get(),
            circle.radius().get(),
            surface,
        )?;
        if axis.as_raw().cross(*circle.frame().axis().as_raw()).norm()
            > EPS_STANDARD_DECODE_GEOMETRY
        {
            return None;
        }
    }
    if target.center().get().distance(witness.center().get()) <= tolerance {
        return None;
    }
    let target_radials = endpoints.map(|point| point.vector_from(target.center().get()));
    let witness_radials = witness_endpoints.map(|point| point.vector_from(witness.center().get()));
    let matches = |other| {
        target_radials
            .into_iter()
            .zip(other)
            .all(|(a, b)| (a - b).norm() <= tolerance)
    };
    if !matches(witness_radials) && !matches([witness_radials[1], witness_radials[0]]) {
        return None;
    }
    let witness_reference = *witness.frame().reference().as_raw();
    let witness_tangent = witness.frame().axis().as_raw().cross(witness_reference);
    let midpoint = range[0].midpoint(range[1]);
    let radial = witness_reference.scale(midpoint.cos()) + witness_tangent.scale(midpoint.sin());
    let reference = *target.frame().reference().as_raw();
    let tangent = target.frame().axis().as_raw().cross(reference);
    let angle = |v: cadmpeg_ir::math::Vector3| v.dot(tangent).atan2(v.dot(reference));
    let [start, end] = target_radials.map(angle);
    let end = witness_arc_end(start, unwrap_angle(end, start), angle(radial))?;
    if start < end {
        crate::nurbs::canonical_periodic_range([start, end])
    } else {
        crate::nurbs::canonical_periodic_range([end, start]).map(|[lo, hi]| [hi, lo])
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::{PI, TAU};

    use cadmpeg_ir::geometry::analytic::CylinderSurface;
    use cadmpeg_ir::math::Vector3;

    use super::*;

    #[test]
    fn opposite_cylinder_arc_selects_the_same_semicircle() {
        let origin = Point3::new(0.0, 0.0, 0.0);
        let axis = Vector3::new(0.0, 0.0, 1.0);
        let reference = Vector3::new(1.0, 0.0, 0.0);
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            CylinderSurface::try_new(origin, axis, reference, 2.0).expect("cylinder"),
        ));
        let witness = CircleCurve::try_new(origin, axis, reference, 2.0).expect("circle");
        let target = CircleCurve::try_new(Point3::new(0.0, 0.0, 3.0), axis, reference, 2.0)
            .expect("translated circle");
        let ends = [Point3::new(2.0, 0.0, 0.0), Point3::new(-2.0, 0.0, 0.0)];
        let translated = ends.map(|point| point.translated(axis, 3.0));

        assert_eq!(
            translated_range(&surface, &target, translated, &witness, ends, [0.0, PI]),
            Some([0.0, PI])
        );
        assert_eq!(
            translated_range(&surface, &target, translated, &witness, ends, [PI, TAU]),
            Some([TAU, PI])
        );
        assert_eq!(
            translated_range(
                &surface,
                &target,
                [translated[0], Point3::new(0.0, 2.0, 3.0)],
                &witness,
                ends,
                [0.0, PI],
            ),
            None
        );
    }
}
