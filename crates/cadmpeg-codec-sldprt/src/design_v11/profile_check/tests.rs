// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};
use std::f64::consts::PI;

use cadmpeg_ir::design::ops::sketch::PlanarCurve;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, PositiveLength};
use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse};
use cadmpeg_ir::units::FinitePoint2;

use super::{check as compare, needed, Difference, Unchecked};

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

fn map(curves: &[(SketchEntityId, PlanarCurve)]) -> HashMap<&SketchEntityId, &PlanarCurve> {
    curves.iter().map(|(id, curve)| (id, curve)).collect()
}

/// The differences of a sketch of `curves` from the stored `loops`.
fn check(
    curves: &[(&str, PlanarCurve)],
    loops: &[&[(&str, bool)]],
) -> Result<Vec<Difference>, Unchecked> {
    let ids = curves
        .iter()
        .map(|(key, curve)| (id(key), curve.clone()))
        .collect::<Vec<_>>();
    let curves = map(&ids);
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
    compare(&loops, &curves)
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
    assert_eq!(check(&curves, &[RECTANGLE, &[("hole", false)]]), Ok(vec![]));
    assert_eq!(check(&c_ring(), &[C_RING]), Ok(vec![]));
}

#[test]
fn a_line_closing_the_inner_arc_of_a_c_ring_is_an_extra_region() {
    let mut curves = c_ring();
    curves.push(("chord", line([-5.0, 0.0], [5.0, 0.0])));
    assert_eq!(check(&curves, &[C_RING]), Ok(vec![Difference::Extra]));
}

#[test]
fn a_diagonal_of_the_rectangle_is_an_internal_edge() {
    let mut curves = rectangle();
    curves.push(("diagonal", line([0.0, 0.0], [50.0, 30.0])));
    assert_eq!(
        check(&curves, &[RECTANGLE]),
        Ok(vec![Difference::InternalEdge])
    );
}

#[test]
fn a_circle_inside_the_profile_that_it_leaves_out_is_a_hole() {
    let mut curves = rectangle();
    curves.push(("hole", circle([25.0, 15.0], 5.0)));
    assert_eq!(check(&curves, &[RECTANGLE]), Ok(vec![Difference::Hole]));
}

#[test]
fn a_rectangle_around_the_profile_is_an_extra_region() {
    let mut curves = vec![("disc", circle([25.0, 15.0], 5.0))];
    curves.extend(rectangle());
    assert_eq!(
        check(&curves, &[&[("disc", false)]]),
        Ok(vec![Difference::Extra])
    );
}

#[test]
fn dangling_curves_and_bridges_close_nothing_and_are_dangling() {
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
    assert_eq!(
        check(&curves, &[RECTANGLE, triangle]),
        Ok(vec![Difference::Dangling])
    );
}

#[test]
fn a_stored_curve_the_sketch_leaves_out_is_a_missing_region() {
    let curves = rectangle()
        .into_iter()
        .filter(|(key, _)| *key != "l2")
        .collect::<Vec<_>>();
    assert_eq!(check(&curves, &[RECTANGLE]), Ok(vec![Difference::Missing]));
}

#[test]
fn a_sketch_without_stored_loops_or_with_an_ellipse_is_not_compared() {
    let mut curves = rectangle();
    curves.push(("chord", line([0.0, 0.0], [50.0, 30.0])));
    assert_eq!(check(&curves, &[]), Err(Unchecked::NoStoredLoop));
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
    assert_eq!(check(&curves, &[RECTANGLE]), Err(Unchecked::CurveKind));
}

#[test]
fn a_sketch_of_points_only_or_a_zero_sweep_arc_is_not_compared() {
    assert_eq!(check(&[], &[RECTANGLE]), Err(Unchecked::NoProfileCurve));
    let mut curves = rectangle();
    curves.push(("flat", arc(5.0, 1.0, 1.0)));
    assert_eq!(check(&curves, &[RECTANGLE]), Err(Unchecked::Unplaced));
}

#[test]
fn a_second_copy_of_a_stored_curve_is_a_duplicate() {
    let mut curves = rectangle();
    curves.push(("hole", circle([25.0, 15.0], 5.0)));
    curves.push(("copy", circle([25.0, 15.0], 5.0)));
    assert_eq!(
        check(&curves, &[RECTANGLE, &[("hole", false)]]),
        Ok(vec![Difference::Duplicate])
    );
    let mut curves = rectangle();
    curves.push(("copy", line([50.0, 30.0], [0.0, 30.0])));
    assert_eq!(
        check(&curves, &[RECTANGLE]),
        Ok(vec![Difference::Duplicate])
    );
}

/// The stored profile holds the circle twice: two stored curves that repeat
/// each other differ in nothing from the stored profile.
#[test]
fn stored_curves_that_repeat_each_other_differ_in_nothing() {
    let mut curves = rectangle();
    curves.push(("hole", circle([25.0, 15.0], 5.0)));
    curves.push(("again", circle([25.0, 15.0], 5.0)));
    assert_eq!(
        check(
            &curves,
            &[RECTANGLE, &[("hole", false)], &[("again", false)]]
        ),
        Ok(vec![])
    );
}

/// A line along the bottom edge from its middle to past the corner: the
/// part along the edge repeats it, and the part past the corner dangles.
#[test]
fn a_line_along_part_of_a_stored_curve_repeats_that_part() {
    let mut curves = rectangle();
    curves.push(("along", line([25.0, 0.0], [70.0, 0.0])));
    assert_eq!(
        check(&curves, &[RECTANGLE]),
        Ok(vec![Difference::Duplicate, Difference::Dangling])
    );
}

/// Two circles joined by the two lines that touch both: the stored profile
/// is the circles, and the lines and the circles also bound the web between
/// them.
#[test]
fn lines_that_end_on_circles_close_a_region_through_them() {
    let curves = vec![
        ("hub", circle([0.0, 0.0], 10.0)),
        ("eye", circle([100.0, 0.0], 10.0)),
        ("upper", line([0.0, 10.0], [100.0, 10.0])),
        ("lower", line([0.0, -10.0], [100.0, -10.0])),
    ];
    assert_eq!(
        check(&curves, &[&[("hub", false)], &[("eye", false)]]),
        Ok(vec![Difference::Extra])
    );
}

#[test]
fn a_line_ending_mid_span_on_two_edges_is_an_internal_edge() {
    let mut curves = rectangle();
    curves.push(("split", line([25.0, 0.0], [25.0, 30.0])));
    assert_eq!(
        check(&curves, &[RECTANGLE]),
        Ok(vec![Difference::InternalEdge])
    );
}

/// A line across the rectangle and past both sides: the part inside splits
/// it, and the parts outside dangle.
#[test]
fn a_line_crossing_the_profile_is_cut_where_it_crosses() {
    let mut curves = rectangle();
    curves.push(("across", line([-10.0, 15.0], [60.0, 15.0])));
    assert_eq!(
        check(&curves, &[RECTANGLE]),
        Ok(vec![Difference::InternalEdge, Difference::Dangling])
    );
}

/// The top edge has state `0`, as do a second copy of the bottom edge and
/// a rectangle around the profile: the profile closes only through the top
/// edge, so every curve of state `0` of the sketch is needed. Where the top
/// edge has state `1`, none is.
#[test]
fn state_zero_curves_are_needed_where_live_curves_close_only_through_them() {
    let rectangle = rectangle();
    let outer = [[-10.0, -10.0], [60.0, -10.0], [60.0, 40.0], [-10.0, 40.0]];
    let mut zero = vec![("copy", line([0.0, 0.0], [50.0, 0.0]))];
    for index in 0..4 {
        zero.push((
            ["o0", "o1", "o2", "o3"][index],
            line(outer[index], outer[(index + 1) % 4]),
        ));
    }
    let mut with_top = zero.clone();
    with_top.push(("l2", rectangle[2].1.clone()));
    let keyed = |curves: &[(&str, PlanarCurve)]| {
        curves
            .iter()
            .map(|(key, curve)| (id(key), curve.clone()))
            .collect::<Vec<_>>()
    };
    let open = keyed(
        &rectangle
            .iter()
            .filter(|(key, _)| *key != "l2")
            .cloned()
            .collect::<Vec<_>>(),
    );
    let (closed, zero, with_top) = (keyed(&rectangle), keyed(&zero), keyed(&with_top));
    let all = map(&with_top).keys().copied().collect::<HashSet<_>>();
    assert_eq!(needed(&map(&open), &map(&with_top)), all);
    assert_eq!(needed(&map(&closed), &map(&zero)), HashSet::new());
    assert_eq!(needed(&HashMap::new(), &map(&with_top)), HashSet::new());
}

#[test]
fn stored_regions_keep_holes_with_their_nearest_outer_boundary() {
    let rings = [
        (id("outer"), circle([0.0, 0.0], 10.0)),
        (id("hole"), circle([0.0, 0.0], 7.0)),
        (id("island"), circle([0.0, 0.0], 4.0)),
        (id("island-hole"), circle([0.0, 0.0], 2.0)),
        (id("other"), circle([25.0, 0.0], 5.0)),
    ];
    let loops = rings
        .iter()
        .rev()
        .map(|(id, _)| {
            vec![SketchEntityUse {
                entity: id.clone(),
                reversed: false,
            }]
        })
        .collect::<Vec<_>>();
    let regions = super::stored_regions(&loops, &map(&rings)).expect("disjoint closed boundaries");
    let boundaries = regions
        .iter()
        .map(|region| {
            (
                &region.outer[0].entity,
                region
                    .holes
                    .iter()
                    .map(|hole| &hole[0].entity)
                    .collect::<Vec<_>>(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        boundaries,
        vec![
            (&rings[4].0, vec![]),
            (&rings[2].0, vec![&rings[3].0]),
            (&rings[0].0, vec![&rings[1].0]),
        ]
    );
}
