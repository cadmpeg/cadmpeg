// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::f64::consts::PI;

use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, PositiveLength};
use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse};
use cadmpeg_ir::units::FinitePoint2;

use super::{differences, Difference};

fn id(key: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("synthetic:test:id#{key}")).unwrap()
}

fn at(u: f64, v: f64) -> FinitePoint2 {
    FinitePoint2::new(Point2::new(u, v)).unwrap()
}

fn line(from: [f64; 2], to: [f64; 2]) -> PlanarCurve {
    PlanarCurve::Line {
        start: at(from[0], from[1]),
        end: at(to[0], to[1]),
    }
}

fn circle(center: [f64; 2], radius: f64) -> PlanarCurve {
    PlanarCurve::Circle {
        center: at(center[0], center[1]),
        radius: PositiveLength::new(radius).unwrap(),
    }
}

fn arc(radius: f64, start: f64, end: f64) -> PlanarCurve {
    PlanarCurve::Arc {
        center: at(0.0, 0.0),
        radius: PositiveLength::new(radius).unwrap(),
        start: Angle::new(start).unwrap(),
        end: Angle::new(end).unwrap(),
    }
}

/// The differences of a sketch of `curves` from the stored `loops`.
fn check(curves: &[(&str, PlanarCurve)], loops: &[&[(&str, bool)]]) -> Vec<Difference> {
    let ids = curves
        .iter()
        .map(|(key, curve)| (id(key), curve.clone()))
        .collect::<Vec<_>>();
    let curves = ids
        .iter()
        .map(|(id, curve)| (id, curve))
        .collect::<HashMap<_, _>>();
    let loops = loops
        .iter()
        .map(|uses| {
            uses.iter()
                .map(|(key, reversed)| SketchEntityUse {
                    entity: id(key),
                    reversed: *reversed,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    differences(&loops, &curves)
}

/// A 50 by 30 rectangle, `l0` to `l3` counterclockwise from the origin.
fn rectangle() -> Vec<(&'static str, PlanarCurve)> {
    let corners = [[0.0, 0.0], [50.0, 0.0], [50.0, 30.0], [0.0, 30.0]];
    ["l0", "l1", "l2", "l3"]
        .into_iter()
        .enumerate()
        .map(|(index, key)| (key, line(corners[index], corners[(index + 1) % 4])))
        .collect()
}

const RECTANGLE: &[(&str, bool)] = &[("l0", false), ("l1", false), ("l2", false), ("l3", false)];

/// The upper half of a ring about the origin, radii 5 and 10, and its loop.
fn c_ring() -> Vec<(&'static str, PlanarCurve)> {
    vec![
        ("outer", arc(10.0, 0.0, PI)),
        ("left", line([-10.0, 0.0], [-5.0, 0.0])),
        ("inner", arc(5.0, 0.0, PI)),
        ("right", line([5.0, 0.0], [10.0, 0.0])),
    ]
}

const C_RING: &[(&str, bool)] = &[
    ("outer", false),
    ("left", false),
    ("inner", true),
    ("right", false),
];

#[test]
fn a_sketch_whose_regions_are_the_stored_loops_differs_in_nothing() {
    let mut curves = rectangle();
    curves.push(("hole", circle([25.0, 15.0], 5.0)));
    assert_eq!(check(&curves, &[RECTANGLE, &[("hole", false)]]), []);
    assert_eq!(check(&c_ring(), &[C_RING]), []);
}

#[test]
fn a_line_closing_the_inner_arc_of_a_c_ring_is_an_extra_region() {
    let mut curves = c_ring();
    curves.push(("chord", line([-5.0, 0.0], [5.0, 0.0])));
    assert_eq!(check(&curves, &[C_RING]), [Difference::Extra]);
}

#[test]
fn a_diagonal_of_the_rectangle_is_an_internal_edge() {
    let mut curves = rectangle();
    curves.push(("diagonal", line([0.0, 0.0], [50.0, 30.0])));
    assert_eq!(check(&curves, &[RECTANGLE]), [Difference::InternalEdge]);
}

#[test]
fn a_circle_inside_the_profile_that_it_leaves_out_is_a_hole() {
    let mut curves = rectangle();
    curves.push(("hole", circle([25.0, 15.0], 5.0)));
    assert_eq!(check(&curves, &[RECTANGLE]), [Difference::Hole]);
}

#[test]
fn a_rectangle_around_the_profile_is_an_extra_region() {
    let mut curves = vec![("disc", circle([25.0, 15.0], 5.0))];
    curves.extend(rectangle());
    assert_eq!(check(&curves, &[&[("disc", false)]]), [Difference::Extra]);
}

#[test]
fn dangling_curves_and_bridges_close_nothing() {
    let mut curves = rectangle();
    curves.push(("stray", line([0.0, 0.0], [20.0, 10.0])));
    curves.push(("bridge", line([50.0, 0.0], [60.0, 0.0])));
    for (index, (from, to)) in [
        ([60.0, 0.0], [70.0, 0.0]),
        ([70.0, 0.0], [70.0, 10.0]),
        ([70.0, 10.0], [60.0, 0.0]),
    ]
    .into_iter()
    .enumerate()
    {
        curves.push((["t0", "t1", "t2"][index], line(from, to)));
    }
    let triangle: &[(&str, bool)] = &[("t0", false), ("t1", false), ("t2", false)];
    assert_eq!(check(&curves, &[RECTANGLE, triangle]), []);
}

#[test]
fn a_stored_curve_the_sketch_leaves_out_is_a_missing_region() {
    let curves = rectangle()
        .into_iter()
        .filter(|(key, _)| *key != "l2")
        .collect::<Vec<_>>();
    assert_eq!(check(&curves, &[RECTANGLE]), [Difference::Missing]);
}

#[test]
fn a_sketch_without_stored_loops_or_with_an_ellipse_is_not_compared() {
    let mut curves = rectangle();
    curves.push(("chord", line([0.0, 0.0], [50.0, 30.0])));
    assert_eq!(check(&curves, &[]), []);
    curves.push((
        "ellipse",
        PlanarCurve::Ellipse {
            center: at(25.0, 15.0),
            major: cadmpeg_ir::units::UnitVector2::new([1.0, 0.0]).unwrap(),
            radii: [
                PositiveLength::new(4.0).unwrap(),
                PositiveLength::new(2.0).unwrap(),
            ],
            range: None,
        },
    ));
    assert_eq!(check(&curves, &[RECTANGLE]), []);
}
