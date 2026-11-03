// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::examples::unit_cube;
use crate::geometry::{nurbs::NurbsCurve, CurveGeometry, SolvedCurveGeometry};
use crate::math::Point3;
use crate::report::check::Check;
use crate::validate::validate_neutral;

const AGAINST_CURVE: &str = "edge runs against its curve";

/// The unit cube with its first edge on a range-less degree-one NURBS curve
/// through `poles`, given as positions of the edge's start and end vertices.
fn cube_with_spline_edge(poles: impl FnOnce(Point3, Point3) -> Vec<Point3>) -> CadIr {
    let mut ir = unit_cube().expect("valid unit cube fixture");
    let position = |ir: &CadIr, vertex: &crate::ids::VertexId| {
        let vertex = ir
            .model
            .vertices
            .iter()
            .find(|candidate| &candidate.id == vertex)
            .expect("edge vertex");
        ir.model
            .points
            .iter()
            .find(|point| point.id == vertex.point)
            .expect("vertex point")
            .position()
            .get()
    };
    let edge = &ir.model.edges[0];
    let (start, end) = (position(&ir, &edge.start), position(&ir, &edge.end));
    let curve_id = edge.curve().cloned().expect("cube edge curve");
    assert_eq!(
        ir.model
            .edges
            .iter()
            .filter(|edge| edge.curve() == Some(&curve_id))
            .count(),
        1
    );
    let controls = poles(start, end);
    let mut knots = vec![0.0];
    knots.extend((0..controls.len()).map(|index| f64::from(u32::try_from(index).unwrap())));
    knots.push(knots[knots.len() - 1]);
    let curve = ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .expect("edge curve");
    curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(1, knots, controls, None, false).unwrap(),
    ));
    curve.parameter_range = None;
    ir.model.edges[0].set_param_range(None);
    ir
}

fn against_curve_findings(ir: &CadIr) -> usize {
    let report = validate_neutral(ir, Vec::new()).expect("validate the neutral model");
    report
        .findings
        .iter()
        .filter(|finding| {
            finding.check == Check::GeometricConsistency
                && finding.entity.as_deref() == Some(ir.model.edges[0].id.as_str())
                && finding.message.contains(AGAINST_CURVE)
        })
        .count()
}

#[test]
fn a_rangeless_edge_along_its_curve_has_no_direction_finding() {
    let ir = cube_with_spline_edge(|start, end| vec![start, end]);
    assert_eq!(against_curve_findings(&ir), 0);
}

#[test]
fn a_rangeless_edge_against_its_curve_is_flagged() {
    let ir = cube_with_spline_edge(|start, end| vec![end, start]);
    assert_eq!(against_curve_findings(&ir), 1);
}

#[test]
fn a_rangeless_edge_on_a_closed_curve_has_no_direction_finding() {
    let ir = cube_with_spline_edge(|start, end| vec![end, start, end]);
    assert_eq!(against_curve_findings(&ir), 0);
}

#[test]
fn a_rangeless_edge_on_part_of_its_curve_has_no_direction_finding() {
    let ir = cube_with_spline_edge(|start, end| {
        let beyond = |from: Point3, to: Point3| {
            Point3::new(
                2.0 * to.x - from.x,
                2.0 * to.y - from.y,
                2.0 * to.z - from.z,
            )
        };
        vec![beyond(start, end), beyond(end, start)]
    });
    assert_eq!(against_curve_findings(&ir), 0);
}
