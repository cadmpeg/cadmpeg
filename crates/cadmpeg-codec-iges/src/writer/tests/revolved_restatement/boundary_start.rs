// SPDX-License-Identifier: Apache-2.0
//! The start of a Type 142 boundary moves only to the seam cut of a restated
//! support. Parameter curves that meet within the topology tolerance keep
//! their junctions and their face.

use cadmpeg_ir::geometry::analytic::{LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{SolvedCurveGeometry, SolvedSurfaceGeometry};
use cadmpeg_ir::ids::CoedgeId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{LoopBoundaryRole, Sense};
use cadmpeg_ir::CadIr;

use super::{boundary, charged, id, origin, parameter_line, written_under, z_axis, Sheet, UNMOVED};

/// The side length of the square face.
const SIDE: f64 = 10.0;

/// A parameter-plane distance between parameter curves that is well inside
/// the topology tolerance of the square.
const SMALL_GAP: f64 = 1.0e-7;

/// The largest distance of a read-back boundary point from the square.
const ON_SQUARE: f64 = 1.0e-6;

/// The parameter lines of one side of the square: origin, direction and
/// length of each.
type SideLines = Vec<([f64; 2], [f64; 2], f64)>;

/// The planar square `[0, SIDE]²` as one sheet face with one classified loop
/// of four forward line coedges, each with the parameter lines `sides`.
fn square(sides: [SideLines; 4]) -> CadIr {
    let mut sheet = Sheet::new(SolvedSurfaceGeometry::Plane(
        PlaneSurface::try_new(origin(), z_axis(), Vector3::new(1.0, 0.0, 0.0)).expect("a plane"),
    ));
    let corners = [
        ("a", [0.0, 0.0]),
        ("b", [SIDE, 0.0]),
        ("c", [SIDE, SIDE]),
        ("d", [0.0, SIDE]),
    ];
    for (key, [x, y]) in corners {
        sheet.vertex(key, Point3::new(x, y, 0.0));
    }
    let edges = [
        ("s0", [1.0, 0.0]),
        ("s1", [0.0, 1.0]),
        ("s2", [-1.0, 0.0]),
        ("s3", [0.0, -1.0]),
    ];
    for (from, (key, [dx, dy])) in edges.into_iter().enumerate() {
        let [x, y] = corners[from].1;
        sheet.edge(
            key,
            SolvedCurveGeometry::Line(
                LineCurve::try_new(Point3::new(x, y, 0.0), Vector3::new(dx, dy, 0.0))
                    .expect("a line"),
            ),
            [0.0, SIDE],
            [corners[from].0, corners[(from + 1) % 4].0],
        );
    }
    let mut ir = sheet.finished(&edges.map(|(key, _)| (key, Sense::Forward)));
    for ((key, _), lines) in edges.into_iter().zip(sides) {
        let uses = lines
            .into_iter()
            .enumerate()
            .map(|(position, (origin, direction, length))| {
                parameter_line(
                    &mut ir,
                    &format!("{key}-{position}"),
                    origin,
                    direction,
                    length,
                )
            })
            .collect();
        ir.geometry
            .coedges
            .iter_mut()
            .find(|coedge| coedge.id == id(CoedgeId::mint, "coedge", key))
            .expect("the side coedge")
            .pcurves = uses;
    }
    ir.finalize();
    ir
}

/// The parameter lines of the square that meet exactly, each along its
/// coedge.
fn exact_sides() -> [SideLines; 4] {
    [
        vec![([0.0, 0.0], [1.0, 0.0], SIDE)],
        vec![([SIDE, 0.0], [0.0, 1.0], SIDE)],
        vec![([SIDE, SIDE], [-1.0, 0.0], SIDE)],
        vec![([0.0, SIDE], [0.0, -1.0], SIDE)],
    ]
}

/// Assert that `sides` write the square as one face with its outer loop,
/// and that its boundary reads back on the square.
fn assert_square_written(sides: [SideLines; 4]) {
    let (losses, read) = written_under(square(sides), UNMOVED);
    for code in [
        "iges/writer.face-omitted",
        "iges/writer.loop-classification-omitted",
    ] {
        assert!(charged(&losses, code).is_empty(), "{losses:#?}");
    }
    assert_eq!(read.geometry.faces.len(), 1, "{losses:#?}");
    let face = &read.geometry.faces[0];
    assert!(
        face.loops
            .stated()
            .any(|loop_| face.loop_role(loop_) == LoopBoundaryRole::Outer),
        "the square reads back with no outer loop"
    );
    let (points, _) = boundary(&read);
    for point in points {
        let off = [point.x, SIDE - point.x, point.y, SIDE - point.y]
            .into_iter()
            .map(f64::abs)
            .fold(f64::INFINITY, f64::min);
        assert!(
            off <= ON_SQUARE && point.z.abs() <= ON_SQUARE,
            "{point:?} lies off the square"
        );
    }
}

#[test]
fn parameter_lines_that_meet_exactly_write_the_square() {
    assert_square_written(exact_sides());
}

#[test]
fn parameter_lines_that_meet_within_the_topology_tolerance_write_the_square() {
    let mut sides = exact_sides();
    sides[1] = vec![([SIDE, SMALL_GAP], [0.0, 1.0], SIDE - SMALL_GAP)];
    sides[3] = vec![([SMALL_GAP, SIDE], [0.0, -1.0], SIDE)];
    assert_square_written(sides);
}

#[test]
fn parameter_lines_of_one_line_coedge_that_meet_within_the_topology_tolerance_write_the_square() {
    let mut sides = exact_sides();
    let half = SIDE / 2.0;
    sides[0] = vec![
        ([0.0, 0.0], [1.0, 0.0], half),
        ([half + SMALL_GAP, 0.0], [1.0, 0.0], half - SMALL_GAP),
    ];
    assert_square_written(sides);
}

#[test]
fn a_parameter_line_stated_against_a_forward_coedge_is_written_reversed() {
    let mut sides = exact_sides();
    sides[0] = vec![([SIDE, 0.0], [-1.0, 0.0], SIDE)];
    assert_square_written(sides);
}
