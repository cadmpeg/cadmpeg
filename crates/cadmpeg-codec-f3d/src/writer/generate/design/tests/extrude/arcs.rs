// SPDX-License-Identifier: Apache-2.0
//! The `Extrude` of a profile whose loops hold arcs, or whose holes are
//! loops of lines and arcs (f3d.md §3.1, "Side-face walk", "Walk of a loop
//! of lines" and the curve record of a side face).

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI, TAU};

use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::design::selection::PrincipalEntity;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{CircleCurve, CylinderSurface, LineCurve, PlaneSurface};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry as ModelCurve, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PointId, RegionId, ShellId, SurfaceId,
    VertexId,
};
use cadmpeg_ir::math::portable::Portable;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{Angle, PositiveLength};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, EdgeCarrier, EdgeEnds, Face, FaceLoops, Loop, LoopBoundary,
    LoopRing, Point, Region, Sense, Shell, Vertex,
};

use super::super::super::extrude::profile::{profile, LoopEntry, ProfileLoop};
use super::super::super::sketch::{CurveGeometry, PlannedCurve, PlannedSketch};
use super::super::roster::segment;
use super::super::sketch::{feature_id, frame, picked, point2, principal, sketch_id, Source};
use super::super::{loss, write, write_bytes};
use super::holes::{depth, links, mint, named_faces, tracked_sides};
use super::{
    blind, entry, kinds, planned, reference_at, sketch_and_extrude_of, u32_at, u64_at, SIDE,
    TOLERANCE,
};

/// A curve of a planned test sketch, in cm.
#[derive(Clone, Copy)]
enum Draw {
    Line([f64; 2], [f64; 2]),
    /// Counterclockwise from `angles[0]` to `angles[1]`.
    Arc {
        center: [f64; 2],
        radius: f64,
        angles: [f64; 2],
    },
    Circle([f64; 2], f64),
}

fn at(center: [f64; 2], radius: f64, angle: f64) -> [f64; 2] {
    let (sin, cos) = angle.portable_sin_cos();
    [center[0] + radius * cos, center[1] + radius * sin]
}

/// A planned sketch on `XY` of `curves` in order.
fn drawn<'a>(
    id: &'a cadmpeg_ir::features::FeatureId,
    sketch: &'a cadmpeg_ir::sketches::SketchId,
    curves: &[Draw],
) -> PlannedSketch<'a> {
    let mut planned = planned(id, sketch, &[]);
    for curve in curves {
        let mut point = |at: [f64; 2]| {
            planned.points.push(at);
            planned.points.len() - 1
        };
        let geometry = match *curve {
            Draw::Line(start, end) => CurveGeometry::Line {
                start: point(start),
                end: point(end),
            },
            Draw::Arc {
                center,
                radius,
                angles,
            } => CurveGeometry::Arc {
                center: point(center),
                start: point(at(center, radius, angles[0])),
                end: point(at(center, radius, angles[1])),
                radius,
                angles,
            },
            Draw::Circle(center, radius) => CurveGeometry::Circle {
                center: point(center),
                radius,
            },
        };
        planned.curves.push(PlannedCurve {
            geometry,
            construction: false,
        });
    }
    planned
}

/// A slot of half length `half` and radius `radius` about `center`, drawn
/// counterclockwise: bottom line, right arc, top line, left arc.
fn slot(center: [f64; 2], half: f64, radius: f64) -> [Draw; 4] {
    let [x, y] = center;
    [
        Draw::Line([x - half, y - radius], [x + half, y - radius]),
        Draw::Arc {
            center: [x + half, y],
            radius,
            angles: [-FRAC_PI_2, FRAC_PI_2],
        },
        Draw::Line([x + half, y + radius], [x - half, y + radius]),
        Draw::Arc {
            center: [x - half, y],
            radius,
            angles: [FRAC_PI_2, 3.0 * FRAC_PI_2],
        },
    ]
}

/// The closed polygon through `corners` as lines.
fn polygon(corners: &[[f64; 2]]) -> Vec<Draw> {
    corners
        .iter()
        .zip(corners.iter().cycle().skip(1))
        .map(|(a, b)| Draw::Line(*a, *b))
        .collect()
}

fn exterior(entries: Vec<LoopEntry>) -> ProfileLoop {
    ProfileLoop {
        exterior: true,
        entries,
    }
}

fn hole(entries: Vec<LoopEntry>) -> ProfileLoop {
    ProfileLoop {
        exterior: false,
        entries,
    }
}

#[test]
fn a_slot_runs_counterclockwise_from_its_last_curve() {
    let id = feature_id("slot");
    let sketch = sketch_id("slot");
    let drawn_slot = drawn(&id, &sketch, &slot([0.0, 0.0], 4.0, 3.0));
    assert_eq!(
        profile(&drawn_slot, TOLERANCE),
        Ok(vec![exterior(vec![
            entry(3, false),
            entry(0, false),
            entry(1, false),
            entry(2, false)
        ])])
    );
    // Both lines drawn against the ring.
    let [bottom, right, top, left] = slot([0.0, 0.0], 4.0, 3.0);
    let flip = |line: Draw| match line {
        Draw::Line(a, b) => Draw::Line(b, a),
        other => other,
    };
    let lines_against = drawn(&id, &sketch, &[flip(bottom), right, flip(top), left]);
    assert_eq!(
        profile(&lines_against, TOLERANCE),
        Ok(vec![exterior(vec![
            entry(3, false),
            entry(0, true),
            entry(1, false),
            entry(2, true)
        ])])
    );
}

/// The ends of a line and a half circle bound no polygon; the arc's
/// segment gives the loop its turn.
#[test]
fn a_half_disc_turns_by_its_arc() {
    let id = feature_id("half");
    let sketch = sketch_id("half");
    let arc = Draw::Arc {
        center: [0.0, 0.0],
        radius: 1.0,
        angles: [FRAC_PI_2, 3.0 * FRAC_PI_2],
    };
    let up = drawn(&id, &sketch, &[Draw::Line([0.0, -1.0], [0.0, 1.0]), arc]);
    assert_eq!(
        profile(&up, TOLERANCE),
        Ok(vec![exterior(vec![entry(1, false), entry(0, false)])])
    );
    let down = drawn(&id, &sketch, &[Draw::Line([0.0, 1.0], [0.0, -1.0]), arc]);
    assert_eq!(
        profile(&down, TOLERANCE),
        Ok(vec![exterior(vec![entry(1, false), entry(0, true)])])
    );
}

/// A concave arc of a counterclockwise exterior runs clockwise: flag `1`.
/// As `C` with flag `1`, the walk starts at the curve after it.
#[test]
fn a_concave_arc_runs_against_its_direction() {
    let id = feature_id("notch");
    let sketch = sketch_id("notch");
    let notch = drawn(
        &id,
        &sketch,
        &[
            Draw::Line([0.0, 0.0], [4.0, 0.0]),
            Draw::Line([4.0, 0.0], [4.0, 4.0]),
            Draw::Line([4.0, 4.0], [0.0, 4.0]),
            Draw::Arc {
                center: [0.0, 2.0],
                radius: 2.0,
                angles: [-FRAC_PI_2, FRAC_PI_2],
            },
        ],
    );
    assert_eq!(
        profile(&notch, TOLERANCE),
        Ok(vec![exterior(vec![
            entry(0, false),
            entry(1, false),
            entry(2, false),
            entry(3, true)
        ])])
    );
}

/// A hole loop of lines runs clockwise from `C` when `C`'s flag is `1`, and
/// from the line before `C` when it is `0` ("Walk of a loop of lines").
#[test]
fn a_square_hole_runs_clockwise_from_the_hole_start() {
    let id = feature_id("frame");
    let sketch = sketch_id("frame");
    let outer = polygon(&[[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]]);
    let walk = exterior(vec![
        entry(3, false),
        entry(0, false),
        entry(1, false),
        entry(2, false),
    ]);
    let counterclockwise = [
        outer.clone(),
        polygon(&[[1.0, 1.0], [3.0, 1.0], [3.0, 3.0], [1.0, 3.0]]),
    ]
    .concat();
    assert_eq!(
        profile(&drawn(&id, &sketch, &counterclockwise), TOLERANCE),
        Ok(vec![
            walk.clone(),
            hole(vec![
                entry(7, true),
                entry(6, true),
                entry(5, true),
                entry(4, true)
            ])
        ])
    );
    let clockwise = [
        outer,
        polygon(&[[1.0, 1.0], [1.0, 3.0], [3.0, 3.0], [3.0, 1.0]]),
    ]
    .concat();
    assert_eq!(
        profile(&drawn(&id, &sketch, &clockwise), TOLERANCE),
        Ok(vec![
            walk,
            hole(vec![
                entry(6, false),
                entry(7, false),
                entry(4, false),
                entry(5, false)
            ])
        ])
    );
}

#[test]
fn a_slot_hole_runs_clockwise_with_its_arcs_reversed() {
    let id = feature_id("washer");
    let sketch = sketch_id("washer");
    let curves = [
        vec![Draw::Circle([0.0, 0.0], 10.0)],
        slot([0.0, 0.0], 4.0, 2.0).to_vec(),
    ]
    .concat();
    assert_eq!(
        profile(&drawn(&id, &sketch, &curves), TOLERANCE),
        Ok(vec![
            exterior(vec![entry(0, false)]),
            hole(vec![
                entry(4, true),
                entry(3, true),
                entry(2, true),
                entry(1, true)
            ])
        ])
    );
}

#[test]
fn arcs_outside_a_closed_loop_are_refused() {
    let id = feature_id("stray");
    let sketch = sketch_id("stray");
    let stray = drawn(
        &id,
        &sketch,
        &[
            Draw::Circle([0.0, 0.0], 3.0),
            Draw::Arc {
                center: [0.0, 0.0],
                radius: 1.0,
                angles: [0.0, FRAC_PI_2],
            },
        ],
    );
    assert_eq!(profile(&stray, TOLERANCE), Err("its profile is not closed"));
    let whole = drawn(
        &id,
        &sketch,
        &[Draw::Arc {
            center: [0.0, 0.0],
            radius: 1.0,
            angles: [0.0, TAU],
        }],
    );
    assert_eq!(
        profile(&whole, TOLERANCE),
        Err("an arc of its profile has coincident ends")
    );
}

/// Loops that hold arcs meet where the arcs, not their whole circles, come
/// within the tolerance of another loop.
#[test]
fn loops_with_arcs_that_cross_or_touch_are_refused() {
    let id = feature_id("arcmeet");
    let sketch = sketch_id("arcmeet");
    let sketch_of = |loops: &[&[Draw]]| drawn(&id, &sketch, &loops.concat());
    let outer = slot([0.0, 0.0], 4.0, 3.0);
    let loops_meet = [
        // A circle that crosses the right arc.
        sketch_of(&[&outer, &[Draw::Circle([6.5, 0.0], 1.0)]]),
        // A circle tangent to the right arc from inside.
        sketch_of(&[&outer, &[Draw::Circle([5.0, 0.0], 2.0)]]),
        // A circle closer to the right arc than the tolerance.
        sketch_of(&[&outer, &[Draw::Circle([5.0 - TOLERANCE / 2.0, 0.0], 2.0)]]),
        // A square whose corner lies on the right arc.
        sketch_of(&[
            &outer,
            &polygon(&[[7.0, 0.0], [6.0, 1.0], [5.0, 0.0], [6.0, -1.0]]),
        ]),
        // A rectangle that crosses the right arc.
        sketch_of(&[
            &outer,
            &polygon(&[[5.0, -0.5], [8.0, -0.5], [8.0, 0.5], [5.0, 0.5]]),
        ]),
    ];
    let disc = [Draw::Circle([0.0, 0.0], 20.0)];
    let holes_meet = [
        // Two slots that cross.
        sketch_of(&[
            &disc,
            &slot([0.0, 0.0], 4.0, 2.0),
            &slot([0.0, 3.0], 4.0, 2.0),
        ]),
        // Two slots whose arcs touch.
        sketch_of(&[
            &disc,
            &slot([-6.0, 0.0], 4.0, 2.0),
            &slot([6.0, 0.0], 4.0, 2.0),
        ]),
        // Two slots whose arcs are closer than the tolerance.
        sketch_of(&[
            &disc,
            &slot([-6.0, 0.0], 4.0, 2.0),
            &slot([6.0 + TOLERANCE / 2.0, 0.0], 4.0, 2.0),
        ]),
    ];
    let refusals = |cases: &[PlannedSketch<'_>]| {
        cases
            .iter()
            .map(|case| profile(case, TOLERANCE))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        (refusals(&loops_meet), refusals(&holes_meet)),
        (
            vec![Err("loops of its profile meet"); loops_meet.len()],
            vec![Err("holes of its profile meet"); holes_meet.len()]
        )
    );
}

/// A hole that crosses the whole circle of an arc, away from the arc, is
/// clear of the loop.
#[test]
fn holes_clear_of_the_arcs_are_written() {
    let id = feature_id("arcclear");
    let sketch = sketch_id("arcclear");
    let outer = slot([0.0, 0.0], 4.0, 3.0);
    let walk = exterior(vec![
        entry(3, false),
        entry(0, false),
        entry(1, false),
        entry(2, false),
    ]);
    let round = drawn(
        &id,
        &sketch,
        &[outer.to_vec(), vec![Draw::Circle([1.0, 0.0], 0.5)]].concat(),
    );
    assert_eq!(
        profile(&round, TOLERANCE),
        Ok(vec![walk.clone(), hole(vec![entry(4, true)])])
    );
    let square = drawn(
        &id,
        &sketch,
        &[
            outer.to_vec(),
            polygon(&[[0.5, -0.5], [1.5, -0.5], [1.5, 0.5], [0.5, 0.5]]),
        ]
        .concat(),
    );
    assert_eq!(
        profile(&square, TOLERANCE),
        Ok(vec![
            walk,
            hole(vec![
                entry(7, true),
                entry(6, true),
                entry(5, true),
                entry(4, true)
            ])
        ])
    );
    let apart = drawn(
        &id,
        &sketch,
        &[
            vec![Draw::Circle([0.0, 0.0], 20.0)],
            slot([-6.0, 0.0], 4.0, 2.0).to_vec(),
            slot([6.0 + 2.0 * TOLERANCE, 0.0], 4.0, 2.0).to_vec(),
        ]
        .concat(),
    );
    assert_eq!(profile(&apart, TOLERANCE).map(|loops| loops.len()), Ok(3));
}

/// A major arc closed by two lines through its centre: the polygon of its
/// ends and the centre runs clockwise, and the arc's segment turns the loop
/// counterclockwise.
#[test]
fn a_major_arc_turns_its_loop_by_its_segment() {
    let id = feature_id("pie");
    let sketch = sketch_id("pie");
    let angles = [FRAC_PI_4, 7.0 * FRAC_PI_4];
    let (start, end) = (
        at([0.0, 0.0], 1.0, angles[0]),
        at([0.0, 0.0], 1.0, angles[1]),
    );
    let pie = drawn(
        &id,
        &sketch,
        &[
            Draw::Arc {
                center: [0.0, 0.0],
                radius: 1.0,
                angles,
            },
            Draw::Line(end, [0.0, 0.0]),
            Draw::Line([0.0, 0.0], start),
        ],
    );
    assert_eq!(
        profile(&pie, TOLERANCE),
        Ok(vec![exterior(vec![
            entry(2, false),
            entry(0, false),
            entry(1, false)
        ])])
    );
}

/// A round hole in the end of a slot, beyond the chord of its arc, is
/// inside the slot.
#[test]
fn a_hole_beyond_the_chord_of_an_arc_is_inside_the_loop() {
    let id = feature_id("slotend");
    let sketch = sketch_id("slotend");
    let curves = [
        slot([0.0, 0.0], 4.0, 3.0).to_vec(),
        vec![Draw::Circle([5.5, 0.0], 0.5)],
    ]
    .concat();
    assert_eq!(
        profile(&drawn(&id, &sketch, &curves), TOLERANCE),
        Ok(vec![
            exterior(vec![
                entry(3, false),
                entry(0, false),
                entry(1, false),
                entry(2, false)
            ]),
            hole(vec![entry(4, true)])
        ])
    );
}

/// Each case at `gap` from an arc of the other loop: within the tolerance
/// the loops meet, at twice the tolerance the profile has a hole.
fn meets_by_gap(name: &str, case: impl Fn(f64) -> Vec<Draw>) {
    let id = feature_id(name);
    let sketch = sketch_id(name);
    for (gap, expected) in [
        (TOLERANCE / 2.0, Err("loops of its profile meet")),
        (2.0 * TOLERANCE, Ok(2)),
    ] {
        assert_eq!(
            profile(&drawn(&id, &sketch, &case(gap)), TOLERANCE).map(|loops| loops.len()),
            expected,
            "{name} at {gap}"
        );
    }
}

/// A lens of two arcs with its lower corner at `(0, y)`.
fn lens(y: f64) -> [Draw; 2] {
    let left = [-1.5, y + 2.0];
    let right = [1.5, y + 2.0];
    [
        Draw::Arc {
            center: left,
            radius: 2.5,
            angles: [(-2.0_f64).portable_atan2(1.5), 2.0_f64.portable_atan2(1.5)],
        },
        Draw::Arc {
            center: right,
            radius: 2.5,
            angles: [
                2.0_f64.portable_atan2(-1.5),
                TAU + (-2.0_f64).portable_atan2(-1.5),
            ],
        },
    ]
}

/// The nearest point of a hole's edge lies within the span of a concave
/// arc of the exterior.
#[test]
fn a_line_beside_an_arc_meets_it() {
    meets_by_gap("linearc", |gap| {
        [
            vec![
                Draw::Line([-10.0, -10.0], at([0.0, -10.0], 3.0, PI)),
                Draw::Arc {
                    center: [0.0, -10.0],
                    radius: 3.0,
                    angles: [0.0, PI],
                },
                Draw::Line([3.0, -10.0], [10.0, -10.0]),
                Draw::Line([10.0, -10.0], [10.0, 10.0]),
                Draw::Line([10.0, 10.0], [-10.0, 10.0]),
                Draw::Line([-10.0, 10.0], [-10.0, -10.0]),
            ],
            polygon(&[
                [-1.0, -7.0 + gap],
                [1.0, -7.0 + gap],
                [1.0, -5.0],
                [-1.0, -5.0],
            ]),
        ]
        .concat()
    });
}

/// A corner of a hole lies inside the circle of an arc of the exterior,
/// its edges running away from the arc.
#[test]
fn a_line_end_beside_an_arc_meets_it() {
    meets_by_gap("cornerarc", |gap| {
        [
            slot([0.0, 0.0], 4.0, 3.0).to_vec(),
            polygon(&[[7.0 - gap, 0.0], [5.0, 1.0], [5.0, -1.0]]),
        ]
        .concat()
    });
}

/// The corner where the two arcs of a lens hole meet lies beside a line
/// of the exterior.
#[test]
fn an_arc_end_beside_a_line_meets_it() {
    meets_by_gap("arcline", |gap| {
        [
            polygon(&[[-10.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]]),
            lens(-10.0 + gap).to_vec(),
        ]
        .concat()
    });
}

/// The corner where the two arcs of a lens hole meet lies beside the
/// exterior circle.
#[test]
fn an_arc_end_beside_an_arc_meets_it() {
    meets_by_gap("arcarc", |gap| {
        [
            vec![Draw::Circle([0.0, 0.0], 10.0)],
            lens(-10.0 + gap).to_vec(),
        ]
        .concat()
    });
}

/// Holes stand in the order of their smallest curve: a slot drawn around a
/// round hole comes first, although its last curve comes after the circle.
#[test]
fn holes_stand_in_the_order_of_their_smallest_curve() {
    let id = feature_id("holeorder");
    let sketch = sketch_id("holeorder");
    let [bottom, right, top, left] = slot([-6.0, 0.0], 2.0, 1.5);
    let curves = [
        Draw::Circle([0.0, 0.0], 20.0),
        bottom,
        right,
        top,
        Draw::Circle([8.0, 0.0], 2.0),
        left,
    ];
    let loops = profile(&drawn(&id, &sketch, &curves), TOLERANCE).expect("one region");
    let curves_of = loops
        .iter()
        .map(|profile_loop| {
            let mut curves = profile_loop
                .entries
                .iter()
                .map(|entry| entry.curve)
                .collect::<Vec<_>>();
            curves.sort_unstable();
            curves
        })
        .collect::<Vec<_>>();
    assert_eq!(curves_of, [vec![0], vec![1, 2, 3, 5], vec![4]]);
}

/// One side of a prism loop, from its corner to the next corner.
#[derive(Clone, Copy)]
pub(super) enum Side {
    Line,
    Arc {
        center: [f64; 2],
        counterclockwise: bool,
    },
}

/// A prism from `z = 0` to [`SIDE`] mm whose section is bounded by `loops`.
/// Each loop lists its corners in run order with the material on the left
/// (the exterior counterclockwise, holes clockwise), each with its side to
/// the next corner; one corner with an arc side is a whole circle.
fn prism(loops: &[Vec<([f64; 2], Side)>]) -> CadIr {
    let height = SIDE;
    let z_axis = Vector3::new(0.0, 0.0, 1.0);
    let x_axis = Vector3::new(1.0, 0.0, 0.0);
    let at = |p: [f64; 2], z: f64| Point3::new(p[0], p[1], z);
    let mut ir = CadIr::empty();
    let mut surfaces = Vec::new();
    for (key, z) in [("bottom", 0.0), ("top", height)] {
        surfaces.push((
            key.to_owned(),
            SolvedSurfaceGeometry::Plane(
                PlaneSurface::try_new(Point3::new(0.0, 0.0, z), z_axis, x_axis).expect("a plane"),
            ),
        ));
    }
    let mut sides = Vec::new();
    let mut edges = Vec::new();
    let mut coedges = Vec::new();
    let mut rings = Vec::new();
    let mut caps = [Vec::new(), Vec::new()];
    for (l, corners) in loops.iter().enumerate() {
        let n = corners.len();
        let name = |part: &str| format!("l{l}-{part}");
        for (i, (corner, _)) in corners.iter().enumerate() {
            for (level, z) in [("b", 0.0), ("t", height)] {
                let key = name(&format!("c{i}{level}"));
                ir.geometry.points.push(Point::new(
                    mint(PointId::mint, "point", &key),
                    FinitePoint3::new(at(*corner, z)).expect("a finite point"),
                    None,
                ));
                ir.geometry.vertices.push(Vertex {
                    id: mint(VertexId::mint, "vertex", &key),
                    point: mint(PointId::mint, "point", &key),
                    tolerance: None,
                    native_ref: None,
                });
            }
            let key = name(&format!("v{i}"));
            edges.push((
                key.clone(),
                SolvedCurveGeometry::Line(
                    LineCurve::try_new(at(*corner, 0.0), z_axis).expect("a line"),
                ),
                Some([0.0, height]),
                [name(&format!("c{i}b")), name(&format!("c{i}t"))],
            ));
        }
        for (i, (from, side)) in corners.iter().enumerate() {
            let j = (i + 1) % n;
            let to = corners[j].0;
            let ends = |level: &str, forward: bool| {
                let (a, b) = if forward { (i, j) } else { (j, i) };
                [name(&format!("c{a}{level}")), name(&format!("c{b}{level}"))]
            };
            let face = name(&format!("f{i}"));
            let forward = match *side {
                Side::Line => {
                    let length = (to[0] - from[0]).portable_hypot(to[1] - from[1]);
                    let along =
                        Vector3::new((to[0] - from[0]) / length, (to[1] - from[1]) / length, 0.0);
                    let outward = Vector3::new(along.y, -along.x, 0.0);
                    sides.push((face.clone(), Sense::Forward));
                    surfaces.push((
                        face.clone(),
                        SolvedSurfaceGeometry::Plane(
                            PlaneSurface::try_new(at(*from, 0.0), outward, along).expect("a plane"),
                        ),
                    ));
                    for (level, z) in [("b", 0.0), ("t", height)] {
                        edges.push((
                            name(&format!("s{i}{level}")),
                            SolvedCurveGeometry::Line(
                                LineCurve::try_new(at(*from, z), along).expect("a line"),
                            ),
                            Some([0.0, length]),
                            ends(level, true),
                        ));
                    }
                    true
                }
                Side::Arc {
                    center,
                    counterclockwise,
                } => {
                    let radius = (from[0] - center[0]).portable_hypot(from[1] - center[1]);
                    let angle = |p: [f64; 2]| (p[1] - center[1]).portable_atan2(p[0] - center[0]);
                    let (first, last) = if counterclockwise {
                        (angle(*from), angle(to))
                    } else {
                        (angle(to), angle(*from))
                    };
                    let range = (n > 1).then(|| [first, first + (last - first).rem_euclid(TAU)]);
                    sides.push((
                        face.clone(),
                        if counterclockwise {
                            Sense::Forward
                        } else {
                            Sense::Reversed
                        },
                    ));
                    surfaces.push((
                        face.clone(),
                        SolvedSurfaceGeometry::Cylinder(
                            CylinderSurface::try_new(at(center, 0.0), z_axis, x_axis, radius)
                                .expect("a cylinder"),
                        ),
                    ));
                    for (level, z) in [("b", 0.0), ("t", height)] {
                        edges.push((
                            name(&format!("s{i}{level}")),
                            SolvedCurveGeometry::Circle(
                                CircleCurve::try_new(at(center, z), z_axis, x_axis, radius)
                                    .expect("a circle"),
                            ),
                            range,
                            ends(level, counterclockwise),
                        ));
                    }
                    counterclockwise
                }
            };
            let (run, against) = if forward {
                (Sense::Forward, Sense::Reversed)
            } else {
                (Sense::Reversed, Sense::Forward)
            };
            let before = (i + n - 1) % n;
            // (coedge, loop, edge, sense, radial partner).
            coedges.extend([
                (
                    name(&format!("f{i}-bottom")),
                    face.clone(),
                    name(&format!("s{i}b")),
                    run,
                    name(&format!("cb{i}")),
                ),
                (
                    name(&format!("f{i}-up")),
                    face.clone(),
                    name(&format!("v{j}")),
                    Sense::Forward,
                    name(&format!("f{j}-down")),
                ),
                (
                    name(&format!("f{i}-top")),
                    face.clone(),
                    name(&format!("s{i}t")),
                    against,
                    name(&format!("ct{i}")),
                ),
                (
                    name(&format!("f{i}-down")),
                    face.clone(),
                    name(&format!("v{i}")),
                    Sense::Reversed,
                    name(&format!("f{before}-up")),
                ),
                (
                    name(&format!("cb{i}")),
                    name("cb"),
                    name(&format!("s{i}b")),
                    against,
                    name(&format!("f{i}-bottom")),
                ),
                (
                    name(&format!("ct{i}")),
                    name("ct"),
                    name(&format!("s{i}t")),
                    run,
                    name(&format!("f{i}-top")),
                ),
            ]);
            rings.push((
                face.clone(),
                face.clone(),
                ["bottom", "up", "top", "down"]
                    .map(|part| name(&format!("f{i}-{part}")))
                    .to_vec(),
            ));
        }
        rings.push((
            name("cb"),
            "bottom".to_owned(),
            (0..n).rev().map(|i| name(&format!("cb{i}"))).collect(),
        ));
        rings.push((
            name("ct"),
            "top".to_owned(),
            (0..n).map(|i| name(&format!("ct{i}"))).collect(),
        ));
        caps[0].push(mint(LoopId::mint, "loop", &name("cb")));
        caps[1].push(mint(LoopId::mint, "loop", &name("ct")));
    }
    let geometry = &mut ir.geometry;
    for (key, solved) in surfaces {
        geometry.surfaces.push(Surface {
            id: mint(SurfaceId::mint, "surface", &key),
            geometry: SurfaceGeometry::Solved(solved),
            source_object: None,
            native_ref: None,
        });
    }
    for (key, curve, range, [start, end]) in edges {
        geometry.curves.push(Curve {
            parameter_range: None,
            id: mint(CurveId::mint, "curve", &key),
            geometry: ModelCurve::Solved(curve),
            source_object: None,
            native_ref: None,
        });
        geometry.edges.push(Edge {
            id: mint(EdgeId::mint, "edge", &key),
            carrier: EdgeCarrier::new(Some(mint(CurveId::mint, "curve", &key)), range)
                .expect("a carrier"),
            ends: Some(EdgeEnds::new(
                mint(VertexId::mint, "vertex", &start),
                mint(VertexId::mint, "vertex", &end),
            )),
            tolerance: None,
            native_ref: None,
        });
    }
    for (coedge, owner, edge, sense, partner) in coedges {
        geometry.coedges.push(Coedge {
            id: mint(CoedgeId::mint, "coedge", &coedge),
            owner_loop: mint(LoopId::mint, "loop", &owner),
            edge: mint(EdgeId::mint, "edge", &edge),
            radial_next: mint(CoedgeId::mint, "coedge", &partner),
            sense,
            pcurves: Vec::new(),
            use_curve: None,
            native_ref: None,
        });
    }
    for (owner, face, ring) in rings {
        geometry.loops.push(Loop {
            id: mint(LoopId::mint, "loop", &owner),
            face: mint(FaceId::mint, "face", &face),
            boundary: LoopBoundary::Ring(
                LoopRing::new(
                    ring.iter()
                        .map(|coedge| mint(CoedgeId::mint, "coedge", coedge))
                        .collect(),
                    Vec::new(),
                )
                .expect("a ring"),
            ),
            native_ref: None,
        });
    }
    let shell = mint(ShellId::mint, "shell", "prism");
    let [bottom, top] = caps;
    let mut faces = sides
        .into_iter()
        .map(|(face, sense)| {
            (
                face.clone(),
                sense,
                FaceLoops::classified(mint(LoopId::mint, "loop", &face), Vec::new()),
            )
        })
        .collect::<Vec<_>>();
    for (key, sense, mut loops) in [
        ("bottom", Sense::Reversed, bottom),
        ("top", Sense::Forward, top),
    ] {
        let outer = loops.remove(0);
        faces.push((key.to_owned(), sense, FaceLoops::classified(outer, loops)));
    }
    for (face, sense, loops) in faces {
        geometry.faces.push(Face {
            id: mint(FaceId::mint, "face", &face),
            shell: shell.clone(),
            surface: mint(SurfaceId::mint, "surface", &face),
            sense,
            loops,
            name: Some(face),
            color: None,
            tolerance: None,
            native_ref: None,
        });
    }
    geometry.shells.push(Shell::new(
        shell.clone(),
        mint(RegionId::mint, "region", "prism"),
    ));
    geometry.regions.push(Region {
        id: mint(RegionId::mint, "region", "prism"),
        body: mint(BodyId::mint, "body", "prism"),
        shells: vec![shell],
        native_ref: None,
    });
    geometry.bodies.push(Body {
        id: mint(BodyId::mint, "body", "prism"),
        kind: BodyKind::Solid,
        transform: None,
        name: Some("prism".into()),
        color: None,
        visible: None,
        native_ref: None,
    });
    ir.finalize();
    ir
}

/// A curve of a source sketch, in mm.
#[derive(Clone, Copy)]
pub(super) enum Stroke {
    Line([f64; 2], [f64; 2]),
    /// Counterclockwise from the first angle to the second, in radians.
    Arc([f64; 2], f64, [f64; 2]),
    Circle([f64; 2], f64),
}

/// `strokes` as the extrude's sketch `c0`, `c1`, ... over the prism of
/// `loops`, extruded [`SIDE`] mm.
pub(super) fn extruded(strokes: &[Stroke], loops: &[Vec<([f64; 2], Side)>]) -> CadIr {
    let mut source = Source::new(
        "profile",
        picked(principal(PrincipalEntity::XyPlane)),
        frame([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    );
    let angle = |radians: f64| Angle::new(radians).expect("an angle");
    let radius = |radius: f64| PositiveLength::new(radius).expect("a radius");
    for (index, stroke) in strokes.iter().enumerate() {
        let curve = match *stroke {
            Stroke::Line(a, b) => PlanarCurve::Line {
                start: point2(a[0], a[1]),
                end: point2(b[0], b[1]),
            },
            Stroke::Arc(center, r, [start, end]) => PlanarCurve::Arc {
                center: point2(center[0], center[1]),
                radius: radius(r),
                start: angle(start),
                end: angle(end),
            },
            Stroke::Circle(center, r) => PlanarCurve::Circle {
                center: point2(center[0], center[1]),
                radius: radius(r),
            },
        };
        source.entity(&format!("c{index}"), curve, &[]);
    }
    let mut ir = sketch_and_extrude_of(source, blind(), false);
    let solid = prism(loops);
    let body = solid.geometry.bodies[0].id.clone();
    ir.geometry = solid.geometry;
    ir.design.scopes[0].current[0].bodies = vec![body];
    ir
}

/// A slot of half length 4 mm and radius 3 mm about the origin: bottom
/// line, right arc, top line, left arc.
fn slot_strokes(center: [f64; 2], radius: f64) -> [Stroke; 4] {
    let [x, y] = center;
    [
        Stroke::Line([x - 4.0, y - radius], [x + 4.0, y - radius]),
        Stroke::Arc([x + 4.0, y], radius, [3.0 * FRAC_PI_2, FRAC_PI_2]),
        Stroke::Line([x + 4.0, y + radius], [x - 4.0, y + radius]),
        Stroke::Arc([x - 4.0, y], radius, [FRAC_PI_2, 3.0 * FRAC_PI_2]),
    ]
}

/// The prism loop of [`slot_strokes`], counterclockwise for an exterior.
fn slot_loop(center: [f64; 2], radius: f64, hole: bool) -> Vec<([f64; 2], Side)> {
    let [x, y] = center;
    let corners = [
        [x - 4.0, y - radius],
        [x + 4.0, y - radius],
        [x + 4.0, y + radius],
        [x - 4.0, y + radius],
    ];
    let arc = |cx: f64| Side::Arc {
        center: [cx, y],
        counterclockwise: !hole,
    };
    if hole {
        vec![
            (corners[0], arc(x - 4.0)),
            (corners[3], Side::Line),
            (corners[2], arc(x + 4.0)),
            (corners[1], Side::Line),
        ]
    } else {
        vec![
            (corners[0], Side::Line),
            (corners[1], arc(x + 4.0)),
            (corners[2], Side::Line),
            (corners[3], arc(x - 4.0)),
        ]
    }
}

/// The region identity record `N + 3` of the written `Extrude`, and the
/// `Extrude` scope.
fn regions(design: &super::super::roster::Segment) -> (Vec<u8>, u64) {
    let timeline = design.record(207);
    let scope = reference_at(timeline, 47);
    let selection = reference_at(design.record(scope), 327 - 4 * 11);
    (design.record(selection + 3).to_vec(), scope)
}

/// The loops of a region identity record: per loop, its entries' primary
/// identities and traversal flags, and its exterior byte.
fn stored_loops(regions: &[u8]) -> Vec<(Vec<(u64, u32)>, u8)> {
    assert_eq!(u32_at(regions, 32), 1, "one region group");
    let mut at = 40;
    let loops = (0..u32_at(regions, 36))
        .map(|_| {
            let count = usize::try_from(u32_at(regions, at)).expect("a count");
            let entries = (0..count)
                .map(|entry| {
                    let start = at + 4 + 40 * entry;
                    assert_eq!(u32_at(regions, start), 3, "a curve entry");
                    (u64_at(regions, start + 4), u32_at(regions, start + 20))
                })
                .collect();
            let exterior = regions[at + 4 + 40 * count];
            at += 5 + 40 * count;
            (entries, exterior)
        })
        .collect();
    assert_eq!(u32_at(regions, at), 0, "no further region");
    loops
}

/// The curve record of a line or arc side: its curve, sense, and the one
/// curve that meets it at its first and at its second end.
fn neighbours(record: &[u8]) -> (u64, u32, u64, u64) {
    assert_eq!((u32_at(record, 73), u32_at(record, 85)), (1, 1));
    (
        u64_at(record, 37),
        u32_at(record, 53),
        u64_at(record, 77),
        u64_at(record, 89),
    )
}

fn surface_of<'a>(
    faces: &'a [(String, SolvedSurfaceGeometry)],
    token: &str,
) -> &'a SolvedSurfaceGeometry {
    match faces.iter().find(|(name, _)| name == token) {
        Some((_, surface)) => surface,
        None => panic!("token {token}: {faces:?}"),
    }
}

/// The axis of the cylinder named `token` in the plane and its radius.
fn cylinder(faces: &[(String, SolvedSurfaceGeometry)], token: &str) -> ([f64; 2], f64) {
    match surface_of(faces, token) {
        SolvedSurfaceGeometry::Cylinder(cylinder) => {
            let origin = cylinder.origin().get();
            ([origin.x, origin.y], cylinder.radius().get())
        }
        other => panic!("token {token} names a cylinder: {other:?}"),
    }
}

/// Whether the plane named `token` holds the model point `(x, y, 0)`.
fn plane_holds(faces: &[(String, SolvedSurfaceGeometry)], token: &str, at: [f64; 2]) -> bool {
    match surface_of(faces, token) {
        SolvedSurfaceGeometry::Plane(plane) => {
            let (origin, normal) = (plane.origin().get(), plane.frame().axis().as_raw());
            let offset =
                (at[0] - origin.x) * normal.x + (at[1] - origin.y) * normal.y - origin.z * normal.z;
            offset.abs() <= TOLERANCE && normal.z.abs() <= TOLERANCE
        }
        other => panic!("token {token} names a plane: {other:?}"),
    }
}

fn near(a: [f64; 2], b: [f64; 2]) -> bool {
    (a[0] - b[0]).abs() <= TOLERANCE && (a[1] - b[1]).abs() <= TOLERANCE
}

const ARC_RESIDUAL: &str = "each loop that holds an arc";

#[test]
fn a_slot_is_written_as_an_extrude() {
    let ir = extruded(
        &slot_strokes([0.0, 0.0], 3.0),
        &[slot_loop([0.0, 0.0], 3.0, false)],
    );
    let (report, decoded, bytes) = write_bytes(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    assert!((depth(&decoded) - SIDE).abs() <= TOLERANCE);
    // The walk from the left arc `c3`: the left arc, the bottom line, the
    // right arc, the top line.
    let faces = named_faces(&decoded);
    assert_eq!(faces.len(), 6, "{faces:?}");
    let (left, right) = (cylinder(&faces, "3"), cylinder(&faces, "5"));
    assert!(near(left.0, [-4.0, 0.0]) && (left.1 - 3.0).abs() <= TOLERANCE);
    assert!(near(right.0, [4.0, 0.0]) && (right.1 - 3.0).abs() <= TOLERANCE);
    assert!(plane_holds(&faces, "4", [0.0, -3.0]));
    assert!(plane_holds(&faces, "6", [0.0, 3.0]));
    let links = links(&decoded);
    assert_eq!(links.len(), 8, "{links:?}");
    assert!(
        links.iter().all(|(_, _, sense)| *sense == Some(0)),
        "{links:?}"
    );

    let design = segment(&bytes, "FusionDesignSegmentType1");
    let (regions, scope) = regions(&design);
    let loops = stored_loops(&regions);
    let [(entries, 1)] = loops.as_slice() else {
        panic!("one exterior loop: {loops:?}");
    };
    let primaries = entries
        .iter()
        .map(|(primary, _)| *primary)
        .collect::<Vec<_>>();
    // `c3` has the largest identity; the others follow in drawing order.
    assert!(
        primaries[1] < primaries[2] && primaries[2] < primaries[3] && primaries[3] < primaries[0],
        "{primaries:?}"
    );
    assert!(entries.iter().all(|(_, flag)| *flag == 0), "{entries:?}");
    // Each curve meets the curve before it in the ring at its start point.
    for (position, (key, record)) in tracked_sides(&design, scope, 4).into_iter().enumerate() {
        assert_eq!(key, 3 + u64::try_from(position).expect("a key"));
        assert_eq!(
            neighbours(record),
            (
                primaries[position],
                1,
                primaries[(position + 3) % 4],
                primaries[(position + 1) % 4]
            ),
            "key {key}"
        );
    }
    let residual = loss(&report, "export/record.residual-unavailable").join("\n");
    assert!(
        residual.contains(ARC_RESIDUAL) && residual.contains("DR-883"),
        "{residual}"
    );
}

#[test]
fn a_square_with_a_square_hole_is_written_as_an_extrude() {
    let square = |corners: [[f64; 2]; 4]| -> Vec<Stroke> {
        (0..4)
            .map(|i| Stroke::Line(corners[i], corners[(i + 1) % 4]))
            .collect()
    };
    let outer = [[0.0, 0.0], [20.0, 0.0], [20.0, 20.0], [0.0, 20.0]];
    let inner = [[5.0, 5.0], [15.0, 5.0], [15.0, 15.0], [5.0, 15.0]];
    let strokes = [square(outer), square(inner)].concat();
    let lines = |corners: &[[f64; 2]]| {
        corners
            .iter()
            .map(|corner| (*corner, Side::Line))
            .collect::<Vec<_>>()
    };
    let ir = extruded(
        &strokes,
        &[
            lines(&outer),
            lines(&[inner[0], inner[3], inner[2], inner[1]]),
        ],
    );
    let (report, decoded, bytes) = write_bytes(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    // The hole runs clockwise from its last line `c7` (x = 5).
    let faces = named_faces(&decoded);
    assert_eq!(faces.len(), 10, "{faces:?}");
    for (token, at) in [
        ("3", [0.0, 10.0]),
        ("4", [10.0, 0.0]),
        ("7", [5.0, 10.0]),
        ("8", [10.0, 15.0]),
        ("9", [15.0, 10.0]),
        ("10", [10.0, 5.0]),
    ] {
        assert!(plane_holds(&faces, token, at), "token {token}");
    }
    let links = links(&decoded);
    assert_eq!(
        (
            links
                .iter()
                .filter(|(_, _, sense)| *sense == Some(0))
                .count(),
            links.iter().filter(|(_, _, sense)| sense.is_none()).count(),
        ),
        (8, 8),
        "{links:?}"
    );

    let design = segment(&bytes, "FusionDesignSegmentType1");
    let (regions, scope) = regions(&design);
    let loops = stored_loops(&regions);
    let [(outer, 1), (inner, 0)] = loops.as_slice() else {
        panic!("an exterior loop and a hole: {loops:?}");
    };
    assert!(outer.iter().all(|(_, flag)| *flag == 0), "{outer:?}");
    assert!(inner.iter().all(|(_, flag)| *flag == 1), "{inner:?}");
    let sides = tracked_sides(&design, scope, 8);
    // An exterior line meets the line before it at its start point; a
    // hole line meets the line before it where the ring starts its run.
    assert_eq!(
        neighbours(sides[0].1),
        (outer[0].0, 1, outer[3].0, outer[1].0)
    );
    assert_eq!(
        neighbours(sides[4].1),
        (inner[0].0, u32::MAX, inner[3].0, inner[1].0)
    );
    assert!(
        loss(&report, "export/record.residual-unavailable")
            .iter()
            .all(|note| !note.contains(ARC_RESIDUAL) && !note.contains("hole loops")),
        "{:#?}",
        report.losses
    );
}

#[test]
fn a_circle_with_a_slot_hole_is_written_as_an_extrude() {
    let strokes = [
        vec![Stroke::Circle([0.0, 0.0], 10.0)],
        slot_strokes([0.0, 0.0], 2.0).into_iter().collect(),
    ]
    .concat();
    let ir = extruded(
        &strokes,
        &[
            vec![(
                [10.0, 0.0],
                Side::Arc {
                    center: [0.0, 0.0],
                    counterclockwise: true,
                },
            )],
            slot_loop([0.0, 0.0], 2.0, true),
        ],
    );
    let (report, decoded) = write(&ir);
    assert_eq!(
        kinds(&decoded),
        ["sketch", "extrude"],
        "{:#?}",
        report.losses
    );
    // The hole runs clockwise from its last curve, the left arc `c4`.
    let faces = named_faces(&decoded);
    assert_eq!(faces.len(), 7, "{faces:?}");
    let outer = cylinder(&faces, "3");
    assert!(near(outer.0, [0.0, 0.0]) && (outer.1 - 10.0).abs() <= TOLERANCE);
    assert!(near(cylinder(&faces, "4").0, [-4.0, 0.0]));
    assert!(plane_holds(&faces, "5", [0.0, 2.0]));
    assert!(near(cylinder(&faces, "6").0, [4.0, 0.0]));
    assert!(plane_holds(&faces, "7", [0.0, -2.0]));
    let links = links(&decoded);
    assert_eq!(
        links.iter().filter(|(_, _, sense)| sense.is_none()).count(),
        8,
        "{links:?}"
    );
    let residual = loss(&report, "export/record.residual-unavailable").join("\n");
    assert!(residual.contains(ARC_RESIDUAL), "{residual}");
}
