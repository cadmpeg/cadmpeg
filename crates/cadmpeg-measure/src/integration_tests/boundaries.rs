// SPDX-License-Identifier: Apache-2.0
//! Boundaries of 3D-curve edges: tolerant edges that meet their vertex
//! apart, with expectations from geometry.

use cadmpeg_ir::geometry::analytic::LineCurve;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::PositiveReal;
use cadmpeg_ir::topology::{BodyKind, Sense};

use crate::select::Selection;
use crate::test_support::builders::{measure, value, Builder};

/// The square `[0, 40]²` whose two sides at the corner `(40, 0)` each miss
/// it by 0.8 of their tolerance, outward and perpendicular to each other:
/// their ends lie about 1.13 tolerances apart, more than either tolerance
/// and within their sum. Each shifted side adds `40 × miss` of area.
#[test]
fn edges_missing_a_vertex_within_their_summed_tolerances_close_the_loop() {
    let tolerance = 0.01;
    let miss = 0.8 * tolerance;
    let mut builder = Builder::new();
    let corners = [
        builder.vertex([0.0, 0.0, 0.0]),
        builder.vertex([40.0, 0.0, 0.0]),
        builder.vertex([40.0, 40.0, 0.0]),
        builder.vertex([0.0, 40.0, 0.0]),
    ];
    let sides: Vec<_> = (0..4)
        .map(|index| {
            (
                builder.line(&corners[index], &corners[(index + 1) % 4]),
                Sense::Forward,
            )
        })
        .collect();
    let surface = builder.plane([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    builder.face(&surface, Sense::Forward, std::slice::from_ref(&sides), true);
    let mut ir = builder.finish(BodyKind::Sheet);
    for (side, shift) in [(0, [0.0, -miss]), (1, [miss, 0.0])] {
        let edge = ir
            .model
            .edges
            .iter_mut()
            .find(|edge| edge.id == sides[side].0)
            .expect("edge");
        edge.tolerance = PositiveReal::new(tolerance);
        let curve_id = edge.carrier.curve().cloned().expect("curve");
        let curve = ir
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == curve_id)
            .expect("curve");
        let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = &curve.geometry else {
            panic!("a line");
        };
        let origin = line.origin().get();
        let direction = *line.direction().as_raw();
        curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Line(
            LineCurve::try_new(
                Point3::new(origin.x + shift[0], origin.y + shift[1], origin.z),
                direction,
            )
            .expect("line"),
        ));
    }
    let faces = Selection {
        all_faces: true,
        ..Selection::default()
    };
    let report = measure(&ir, &faces);
    let area = value(report.faces()[0].measures().area());
    let expected = 1600.0 + 2.0 * 40.0 * miss;
    assert!((area - expected).abs() <= miss * miss, "{area}");
}
