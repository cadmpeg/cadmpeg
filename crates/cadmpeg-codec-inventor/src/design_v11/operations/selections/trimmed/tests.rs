// SPDX-License-Identifier: Apache-2.0
use std::f64::consts::{FRAC_1_SQRT_2, PI, TAU};

use cadmpeg_core::nonblank_literal;
use cadmpeg_ir::design::selection::{PickTarget, Picks};
use cadmpeg_ir::features::profiles::{PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{EllipseRadii, SketchGeometry};

use super::*;
use crate::design_v11::operations::selections::{planar, SketchData};

/// Agreement of two parameters that the same arithmetic reaches.
const EPS_TEST_PARAMETER: f64 = 1.0e-9;

fn assert_cuts(actual: &[f64], expected: &[f64]) {
    assert_eq!(
        actual.len(),
        expected.len(),
        "{actual:?} against {expected:?}"
    );
    for (found, wanted) in actual.iter().zip(expected) {
        assert!(
            (found - wanted).abs() <= EPS_TEST_PARAMETER,
            "{found} against {wanted}"
        );
    }
}

fn sketch() -> SketchId {
    SketchId::mint("inventor:design:sketch#synthetic").expect("sketch")
}

fn entity_id(key: &str) -> SketchEntityId {
    SketchEntityId::mint(format!("inventor:design:entity#{key}")).expect("entity")
}

fn entity(
    key: &str,
    definition: SketchGeometryDefinition<Point2, Length, f64, f64>,
) -> SketchEntity {
    SketchEntity::new(
        entity_id(key),
        sketch(),
        SketchGeometry::try_from(definition).expect("geometry"),
    )
}

fn line(key: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    entity(
        key,
        SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        },
    )
}

fn circle(key: &str, center: (f64, f64), radius: f64) -> SketchEntity {
    entity(
        key,
        SketchGeometryDefinition::Circle {
            center: Point2::new(center.0, center.1),
            radius: Length::new(radius).expect("radius"),
        },
    )
}

fn arc(key: &str, center: (f64, f64), radius: f64, from: f64, to: f64) -> SketchEntity {
    entity(
        key,
        SketchGeometryDefinition::Arc {
            center: Point2::new(center.0, center.1),
            radius: Length::new(radius).expect("radius"),
            start_angle: Angle::new(from).expect("angle"),
            end_angle: Angle::new(to).expect("angle"),
        },
    )
}

fn construction(mut value: SketchEntity) -> SketchEntity {
    value.construction = true;
    value
}

fn stored(key: &str, range: [f64; 2], reversed: bool) -> SketchProfileBoundaryUse {
    SketchProfileBoundaryUse {
        entity: entity_id(key),
        parameter_range: DirectedParameterRange::new(range).expect("range"),
        reversed,
    }
}

/// The entity keys and fragments of a ring, in order.
fn shape(ring: &[BoundaryUse]) -> Vec<(String, Option<(u32, u32)>)> {
    ring.iter()
        .map(|usage| {
            (
                usage
                    .entity
                    .as_str()
                    .rsplit('#')
                    .next()
                    .expect("key")
                    .to_owned(),
                usage.fragment.map(|piece| (piece.index, piece.count)),
            )
        })
        .collect()
}

fn expected(rows: &[(&str, Option<(u32, u32)>)]) -> Vec<(String, Option<(u32, u32)>)> {
    rows.iter()
        .map(|(key, piece)| ((*key).to_owned(), *piece))
        .collect()
}

/// A square of side 10 from the origin, counterclockwise, cut by a vertical
/// line at u = 5 that runs 5 below and above it.
fn square_and_cut() -> Vec<SketchEntity> {
    vec![
        line("bottom", (0.0, 0.0), (10.0, 0.0)),
        line("right", (10.0, 0.0), (10.0, 10.0)),
        line("top", (10.0, 10.0), (0.0, 10.0)),
        line("left", (0.0, 10.0), (0.0, 0.0)),
        line("cut", (5.0, -5.0), (5.0, 15.0)),
    ]
}

#[test]
fn a_crossing_line_cuts_a_rectangle_into_two_regions_of_fragments() {
    let entities = square_and_cut();
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    // The cut line leaves the square at u = 0 and u = 10 of its length 20, so
    // its parameters are 5/20 and 15/20 and it has three pieces.
    let left = arrangement
        .region(
            &[
                stored("bottom", [0.0, 0.5], false),
                stored("cut", [0.25, 0.75], false),
                stored("top", [0.5, 1.0], false),
                stored("left", [0.0, 1.0], false),
            ],
            &[],
        )
        .expect("left half");
    assert_eq!(
        shape(&left.outer),
        expected(&[
            ("bottom", Some((0, 2))),
            ("cut", Some((1, 3))),
            ("top", Some((1, 2))),
            ("left", None),
        ])
    );
    // The right half runs the cut line downwards.
    let right = arrangement
        .region(
            &[
                stored("bottom", [0.5, 1.0], false),
                stored("right", [0.0, 1.0], false),
                stored("top", [0.0, 0.5], false),
                stored("cut", [0.25, 0.75], true),
            ],
            &[],
        )
        .expect("right half");
    assert_eq!(
        shape(&right.outer),
        expected(&[
            ("bottom", Some((1, 2))),
            ("right", None),
            ("top", Some((0, 2))),
            ("cut", Some((1, 3))),
        ])
    );
}

#[test]
fn a_chord_cuts_a_circle_from_the_first_cut_at_or_after_angle_zero() {
    let entities = vec![
        circle("round", (0.0, 0.0), 10.0),
        line("chord", (-12.0, 6.0), (12.0, 6.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    // The chord meets the circle at u = 8 and u = -8, v = 6, at the angles
    // atan(6/8) and pi - atan(6/8). Its length is 24, so its cut parameters
    // are 4/24 and 20/24.
    let low = (6.0_f64).atan2(8.0);
    let high = PI - low;
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[low, high]);
    let above = arrangement
        .region(
            &[
                stored("round", [low, high], false),
                stored("chord", [4.0 / 24.0, 20.0 / 24.0], false),
            ],
            &[],
        )
        .expect("upper part");
    assert_eq!(
        shape(&above.outer),
        expected(&[("round", Some((0, 2))), ("chord", Some((1, 3)))])
    );
    let below = arrangement
        .region(
            &[
                stored("round", [high, low + TAU], false),
                stored("chord", [4.0 / 24.0, 20.0 / 24.0], true),
            ],
            &[],
        )
        .expect("lower part");
    assert_eq!(
        shape(&below.outer),
        expected(&[("round", Some((1, 2))), ("chord", Some((1, 3)))])
    );
}

#[test]
fn an_arc_is_cut_in_its_stored_direction() {
    // A quarter arc from angle 0 to pi/2 of radius 10, crossed by the line
    // u = v at angle pi/4.
    let entities = vec![
        arc("bend", (0.0, 0.0), 10.0, 0.0, PI / 2.0),
        line("slant", (0.0, 0.0), (10.0, 10.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[PI / 4.0]);
    let pieces = arrangement.cover(0, [0.0, PI / 4.0]).expect("first piece");
    assert_eq!(
        pieces
            .iter()
            .map(|piece| piece.fragment)
            .collect::<Vec<_>>(),
        vec![Some(Fragment { index: 0, count: 2 })]
    );
}

#[test]
fn the_end_of_another_curve_on_an_edge_is_a_cut() {
    // A square of side 10 with a spur that rises from the middle of the
    // bottom edge and ends inside the square.
    let mut entities = square_and_cut();
    entities.truncate(4);
    entities.push(line("spur", (5.0, 0.0), (5.0, 4.0)));
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[0.5]);
    // The spur's far end is free, so it cuts nothing else, and the spur has
    // no cut: the edge's own end is excluded.
    assert!(arrangement.cuts(1).expect("cuts").is_empty());
    assert!(arrangement.cuts(4).expect("cuts").is_empty());
}

#[test]
fn a_use_that_ends_between_cut_points_is_refused() {
    let entities = square_and_cut();
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert!(arrangement.cover(0, [0.0, 0.25]).is_err());
    assert!(arrangement.cover(0, [0.25, 0.5]).is_err());
    // A circle use between two cut points is refused as well.
    let entities = vec![
        circle("round", (0.0, 0.0), 10.0),
        line("chord", (-12.0, 6.0), (12.0, 6.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert!(arrangement.cover(0, [0.0, 1.0]).is_err());
}

#[test]
fn a_construction_curve_cuts_nothing_and_cannot_be_used() {
    let mut entities = square_and_cut();
    entities[4] = construction(entities[4].clone());
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert!(arrangement.cuts(0).expect("cuts").is_empty());
    let whole = arrangement
        .region(
            &[
                stored("bottom", [0.0, 1.0], false),
                stored("right", [0.0, 1.0], false),
                stored("top", [0.0, 1.0], false),
                stored("left", [0.0, 1.0], false),
            ],
            &[],
        )
        .expect("square");
    assert_eq!(
        shape(&whole.outer),
        expected(&[
            ("bottom", None),
            ("right", None),
            ("top", None),
            ("left", None)
        ])
    );
    assert!(arrangement
        .ring(&[stored("cut", [0.0, 1.0], false)], false)
        .is_err());
}

/// A strip along the u axis from 5 to 15 and 5 high, with posts at both
/// ends and a spur that rises from the long edge between them.
fn strip() -> Vec<SketchEntity> {
    vec![
        line("base", (0.0, 0.0), (20.0, 0.0)),
        line("post_a", (5.0, 0.0), (5.0, 5.0)),
        line("post_b", (15.0, 0.0), (15.0, 5.0)),
        line("lid", (5.0, 5.0), (15.0, 5.0)),
        line("spur", (10.0, 0.0), (10.0, 3.0)),
    ]
}

/// The strip as a clockwise ring: up one post, across the lid, down the
/// other post, and back along the base.
fn clockwise_strip() -> Vec<SketchProfileBoundaryUse> {
    vec![
        stored("post_a", [0.0, 1.0], false),
        stored("lid", [0.0, 1.0], false),
        stored("post_b", [0.0, 1.0], true),
        stored("base", [0.25, 0.75], true),
    ]
}

#[test]
fn a_reversed_use_emits_its_pieces_in_descending_order() {
    let entities = strip();
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    // The base is cut at u = 5, 10 and 15 of its length 20.
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[0.25, 0.5, 0.75]);
    let ring = arrangement
        .ring(&clockwise_strip(), true)
        .expect("hole ring keeps its direction");
    assert_eq!(
        shape(&ring),
        expected(&[
            ("post_a", None),
            ("lid", None),
            ("post_b", None),
            ("base", Some((2, 4))),
            ("base", Some((1, 4))),
        ])
    );
}

#[test]
fn an_outer_ring_stored_clockwise_is_reversed_to_counterclockwise() {
    let entities = strip();
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    let ring = arrangement
        .ring(&clockwise_strip(), false)
        .expect("outer ring");
    assert_eq!(
        shape(&ring),
        expected(&[
            ("base", Some((1, 4))),
            ("base", Some((2, 4))),
            ("post_b", None),
            ("lid", None),
            ("post_a", None),
        ])
    );
    // A counterclockwise hole is reversed to clockwise in the same way.
    let counterclockwise = [
        stored("base", [0.25, 0.75], false),
        stored("post_b", [0.0, 1.0], false),
        stored("lid", [0.0, 1.0], true),
        stored("post_a", [0.0, 1.0], true),
    ];
    let hole = arrangement
        .ring(&counterclockwise, true)
        .expect("hole ring");
    assert_eq!(
        shape(&hole),
        shape(&ring).into_iter().rev().collect::<Vec<_>>()
    );
}

#[test]
fn a_whole_closed_curve_carries_no_fragment() {
    // A circle that nothing touches, and a circle that one line ends on.
    let entities = vec![
        circle("free", (0.0, 0.0), 10.0),
        circle("touched", (50.0, 0.0), 10.0),
        line("spur", (40.0, 0.0), (45.0, 0.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert!(arrangement.cuts(0).expect("cuts").is_empty());
    // One cut point leaves one piece that starts and ends at the cut.
    assert_cuts(arrangement.cuts(1).expect("cuts"), &[PI]);
    for key in ["free", "touched"] {
        let ring = arrangement
            .ring(&[stored(key, [0.0, TAU], false)], false)
            .expect("circle");
        assert_eq!(shape(&ring), expected(&[(key, None)]));
    }
    // A reversed whole circle is a clockwise ring, which the outer ring
    // turns round.
    let ring = arrangement
        .ring(&[stored("free", [0.0, TAU], true)], false)
        .expect("circle");
    assert_eq!(shape(&ring), expected(&[("free", None)]));
    assert!(arrangement
        .ring(&[stored("touched", [0.0, 1.0], false)], false)
        .is_err());
}

#[test]
fn a_curve_of_unknown_extent_refuses_every_used_curve() {
    let mut entities = square_and_cut();
    entities.push(SketchEntity::new(
        entity_id("spline"),
        sketch(),
        SketchGeometry::native(nonblank_literal!("inventor:spline")),
    ));
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert!(arrangement.cuts(0).is_err());
    // The unread curve itself cannot be a boundary use.
    assert!(arrangement
        .ring(&[stored("spline", [0.0, 1.0], false)], false)
        .is_err());
    // The same curve as construction geometry cuts nothing.
    let last = entities.len() - 1;
    entities[last] = construction(entities[last].clone());
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_eq!(arrangement.cuts(0).expect("cuts").len(), 1);
}

fn ellipse(key: &str, center: (f64, f64), radii: (f64, f64), angle: f64) -> SketchEntity {
    entity(
        key,
        SketchGeometryDefinition::Ellipse {
            center: Point2::new(center.0, center.1),
            major_angle: Angle::new(angle).expect("angle"),
            radii: EllipseRadii {
                major_radius: Length::new(radii.0).expect("radius"),
                minor_radius: Length::new(radii.1).expect("radius"),
            },
            bounds: None,
        },
    )
}

#[test]
fn an_ellipse_box_follows_its_major_axis() {
    // Semi-axes 4 and 2 with the major axis along v reach 2 along u and 4
    // along v.
    let Kind::Unread(Some(found)) = Curve::of(&ellipse("e", (1.0, 2.0), (4.0, 2.0), PI / 2.0))
    else {
        panic!("ellipse box");
    };
    for (actual, wanted) in [
        (found.low.u, -1.0),
        (found.low.v, -2.0),
        (found.high.u, 3.0),
        (found.high.v, 6.0),
    ] {
        assert!(
            (actual - wanted).abs() <= EPS_TEST_PARAMETER,
            "{actual} against {wanted}"
        );
    }
}

#[test]
fn an_unread_curve_cuts_only_the_curves_its_box_can_reach() {
    let region = |extra: SketchEntity| {
        let mut entities = square_and_cut();
        entities.push(extra);
        let profile = PlanarProfileRef::sketch_regions(
            sketch(),
            vec![SketchProfileRegion::trimmed(
                vec![
                    stored("bottom", [0.0, 0.5], false),
                    stored("cut", [0.25, 0.75], false),
                    stored("top", [0.5, 1.0], false),
                    stored("left", [0.0, 1.0], false),
                ],
                Vec::new(),
            )
            .expect("region")],
        )
        .expect("regions");
        let data = SketchData {
            sketches: &[],
            entities: &entities,
            arrangements: None,
        };
        planar(&profile, data).is_ok()
    };
    // An ellipse far from the square is ignored; one inside it, and one that
    // only reaches the square's edge within the tolerance, cut it.
    assert!(region(ellipse("far", (100.0, 100.0), (5.0, 3.0), 0.3)));
    assert!(!region(ellipse("inside", (5.0, 5.0), (2.0, 1.0), 0.3)));
    assert!(!region(ellipse(
        "near",
        (-3.0, 5.0),
        (3.0 - 1.0e-6, 1.0),
        0.0
    )));
}

#[test]
fn a_line_ending_within_the_end_tolerance_does_not_cut_the_line_it_ends_on() {
    // The second line ends 1.5e-5 short of the first line's end: inside the
    // tolerance that matches stored ends, so the two ends are one point.
    let near = 1.5e-5;
    let entities = vec![
        line("a", (0.0, 0.0), (10.0, 0.0)),
        line("b", (10.0 - near, 0.0), (10.0 - near, 5.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert!(arrangement.cuts(0).expect("cuts").is_empty());
    let whole = arrangement.cover(0, [0.0, 1.0]).expect("whole line");
    assert_eq!(whole.len(), 1);
    assert_eq!(whole[0].fragment, None);
    // A use that is only the sliver at the end has no piece.
    assert!(arrangement.cover(0, [1.0 - near / 10.0, 1.0]).is_err());
}

#[test]
fn a_stored_end_matches_the_nearest_bound() {
    // The second line ends 3e-5 short of the first line's end, which is a
    // cut. A stored end 1.2e-5 short of the line end is within the matching
    // tolerance of both bounds and is nearer the end.
    let entities = vec![
        line("a", (0.0, 0.0), (10.0, 0.0)),
        line("b", (10.0 - 3.0e-5, 0.0), (10.0 - 3.0e-5, 5.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[1.0 - 3.0e-6]);
    // The stored end is the line's end, so the use covers the whole line.
    let whole = arrangement
        .cover(0, [0.0, 1.0 - 1.2e-6])
        .expect("whole line");
    assert_eq!(
        whole.iter().map(|piece| piece.fragment).collect::<Vec<_>>(),
        vec![None]
    );
    // Nearer the cut than the end, the same stored end is the cut.
    let pieces = arrangement
        .cover(0, [0.0, 1.0 - 2.5e-6])
        .expect("first piece");
    assert_eq!(
        pieces
            .iter()
            .map(|piece| piece.fragment)
            .collect::<Vec<_>>(),
        vec![Some(Fragment { index: 0, count: 2 })]
    );
}

#[test]
fn a_tangent_contact_is_a_cut_of_both_curves() {
    // The line y = 10 touches the circle of radius 10 at its top, angle
    // pi / 2, and the line's middle.
    let entities = vec![
        circle("round", (0.0, 0.0), 10.0),
        line("tangent", (-5.0, 10.0), (5.0, 10.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[PI / 2.0]);
    assert_cuts(arrangement.cuts(1).expect("cuts"), &[0.5]);
}

#[test]
fn collinear_overlapping_lines_cut_each_other_at_the_ends_of_the_overlap() {
    let entities = vec![
        line("a", (0.0, 0.0), (10.0, 0.0)),
        line("b", (5.0, 0.0), (15.0, 0.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[0.5]);
    assert_cuts(arrangement.cuts(1).expect("cuts"), &[0.5]);
}

#[test]
fn a_line_and_an_arc_cut_a_sector_into_fragments() {
    // The sector between the u axis and the line u = v, bounded by the
    // quarter arc of radius 10. The line leaves the arc at angle pi / 4.
    let entities = vec![
        line("axis", (0.0, 0.0), (10.0, 0.0)),
        arc("bend", (0.0, 0.0), 10.0, 0.0, PI / 2.0),
        line("slant", (0.0, 0.0), (10.0, 10.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    // The slant has length 10 sqrt 2 and meets the arc 10 along it.
    assert_cuts(arrangement.cuts(2).expect("cuts"), &[FRAC_1_SQRT_2]);
    let sector = arrangement
        .region(
            &[
                stored("axis", [0.0, 1.0], false),
                stored("bend", [0.0, PI / 4.0], false),
                stored("slant", [0.0, FRAC_1_SQRT_2], true),
            ],
            &[],
        )
        .expect("sector");
    assert_eq!(
        shape(&sector.outer),
        expected(&[
            ("axis", None),
            ("bend", Some((0, 2))),
            ("slant", Some((0, 2))),
        ])
    );
}

#[test]
fn two_arcs_cut_a_lens_from_each_other() {
    // Circles of radius 10 about the origin and about (10, 0) meet at
    // (5, +-5 sqrt 3). The lens is the right arc of the first, from -pi/3 to
    // pi/3, and the left arc of the second, from 2 pi/3 to 4 pi/3.
    let entities = vec![
        arc("right", (0.0, 0.0), 10.0, -PI / 2.0, PI / 2.0),
        arc("left", (10.0, 0.0), 10.0, PI / 2.0, 3.0 * PI / 2.0),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[-PI / 3.0, PI / 3.0]);
    assert_cuts(
        arrangement.cuts(1).expect("cuts"),
        &[2.0 * PI / 3.0, 4.0 * PI / 3.0],
    );
    let lens = arrangement
        .region(
            &[
                stored("left", [2.0 * PI / 3.0, 4.0 * PI / 3.0], false),
                stored("right", [-PI / 3.0, PI / 3.0], false),
            ],
            &[],
        )
        .expect("lens");
    assert_eq!(
        shape(&lens.outer),
        expected(&[("left", Some((1, 3))), ("right", Some((1, 3)))])
    );
}

#[test]
fn two_circles_cut_a_lens_from_each_other() {
    // The same circles whole. Angles run from the u axis, so the first
    // circle's cuts are pi/3 and 5 pi/3 and the second's 2 pi/3 and 4 pi/3.
    let entities = vec![
        circle("first", (0.0, 0.0), 10.0),
        circle("second", (10.0, 0.0), 10.0),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(
        arrangement.cuts(0).expect("cuts"),
        &[PI / 3.0, 5.0 * PI / 3.0],
    );
    assert_cuts(
        arrangement.cuts(1).expect("cuts"),
        &[2.0 * PI / 3.0, 4.0 * PI / 3.0],
    );
    // The lens: the second circle from 2 pi/3 to 4 pi/3, then the first from
    // 5 pi/3 through angle 0 to pi/3.
    let lens = arrangement
        .region(
            &[
                stored("second", [2.0 * PI / 3.0, 4.0 * PI / 3.0], false),
                stored("first", [5.0 * PI / 3.0, PI / 3.0 + TAU], false),
            ],
            &[],
        )
        .expect("lens");
    assert_eq!(
        shape(&lens.outer),
        expected(&[("second", Some((0, 2))), ("first", Some((1, 2)))])
    );
}

#[test]
fn an_arc_across_angle_zero_is_cut_in_its_stored_direction() {
    // An arc from 5 pi/3 to 7 pi/3 crosses angle 0 as parameter TAU. The u
    // axis line meets it there, and the lower wedge is the first piece.
    let entities = vec![
        line("spoke", (0.0, 0.0), polar_pair(10.0, 5.0 * PI / 3.0)),
        arc("bend", (0.0, 0.0), 10.0, 5.0 * PI / 3.0, 7.0 * PI / 3.0),
        line("axis", (0.0, 0.0), (20.0, 0.0)),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(arrangement.cuts(1).expect("cuts"), &[TAU]);
    assert_cuts(arrangement.cuts(2).expect("cuts"), &[0.5]);
    let upper = arrangement
        .cover(1, [TAU, 7.0 * PI / 3.0])
        .expect("second piece");
    assert_eq!(
        upper.iter().map(|piece| piece.fragment).collect::<Vec<_>>(),
        vec![Some(Fragment { index: 1, count: 2 })]
    );
    let wedge = arrangement
        .region(
            &[
                stored("spoke", [0.0, 1.0], false),
                stored("bend", [5.0 * PI / 3.0, TAU], false),
                stored("axis", [0.0, 0.5], true),
            ],
            &[],
        )
        .expect("wedge");
    assert_eq!(
        shape(&wedge.outer),
        expected(&[
            ("spoke", None),
            ("bend", Some((0, 2))),
            ("axis", Some((0, 2))),
        ])
    );
}

fn polar_pair(radius: f64, angle: f64) -> (f64, f64) {
    (radius * angle.cos(), radius * angle.sin())
}

#[test]
fn a_cut_at_angle_zero_is_one_cut_whether_it_is_just_above_or_just_below() {
    let below = |key: &str, back: f64| {
        line(
            key,
            polar_pair(10.0, -back),
            (polar_pair(10.0, -back).0 + 5.0, -5.0),
        )
    };
    let entities = vec![
        circle("round", (0.0, 0.0), 10.0),
        line("axis", (-20.0, 0.0), (20.0, 0.0)),
        // Two lines that start on the circle 1e-8 and 1.5e-6 radians below
        // angle 0, which is 1e-7 and 1.5e-5 millimetres along it.
        below("near", 1.0e-8),
        below("nearer", 1.5e-6),
    ];
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    assert_cuts(arrangement.cuts(0).expect("cuts"), &[0.0, PI]);
    let upper = arrangement.cover(0, [0.0, PI]).expect("upper half");
    assert_eq!(
        upper.iter().map(|piece| piece.fragment).collect::<Vec<_>>(),
        vec![Some(Fragment { index: 0, count: 2 })]
    );
    let lower = arrangement.cover(0, [PI, TAU]).expect("lower half");
    assert_eq!(
        lower.iter().map(|piece| piece.fragment).collect::<Vec<_>>(),
        vec![Some(Fragment { index: 1, count: 2 })]
    );
}

#[test]
fn a_hole_ring_carries_fragments() {
    // The strip of `strip` inside a square, as a hole.
    let mut entities = strip();
    entities.extend([
        line("o_bottom", (-5.0, -5.0), (25.0, -5.0)),
        line("o_right", (25.0, -5.0), (25.0, 15.0)),
        line("o_top", (25.0, 15.0), (-5.0, 15.0)),
        line("o_left", (-5.0, 15.0), (-5.0, -5.0)),
    ]);
    let region = SketchProfileRegion::trimmed(
        vec![
            stored("o_bottom", [0.0, 1.0], false),
            stored("o_right", [0.0, 1.0], false),
            stored("o_top", [0.0, 1.0], false),
            stored("o_left", [0.0, 1.0], false),
        ],
        vec![clockwise_strip()],
    )
    .expect("region");
    let profile = PlanarProfileRef::sketch_regions(sketch(), vec![region]).expect("regions");
    let data = SketchData {
        sketches: &[],
        entities: &entities,
        arrangements: None,
    };
    let value = planar(&profile, data).expect("selection");
    let Picks::Named(picks) = value.picks else {
        panic!("named picks");
    };
    let [picked] = picks.as_slice() else {
        panic!("one region");
    };
    let PickTarget::SketchRegion { region, .. } = &picked.target else {
        panic!("region pick");
    };
    assert_eq!(region.outer.len(), 4);
    let [hole] = region.holes.as_slice() else {
        panic!("one hole");
    };
    assert_eq!(
        shape(hole),
        expected(&[
            ("post_a", None),
            ("lid", None),
            ("post_b", None),
            ("base", Some((2, 4))),
            ("base", Some((1, 4))),
        ])
    );
}

#[test]
fn a_ring_that_does_not_close_is_refused() {
    let entities = square_and_cut();
    let arrangement = Arrangement::new(&entities, &sketch()).expect("arrangement");
    // The left side is missing: the top ends at (0, 10) and the bottom starts
    // at the origin.
    let result = arrangement.ring(
        &[
            stored("bottom", [0.0, 0.5], false),
            stored("cut", [0.25, 0.75], false),
            stored("top", [0.5, 1.0], false),
        ],
        false,
    );
    assert_eq!(result.err(), Some("a trimmed boundary ring does not close"));
}

#[test]
fn a_sketch_past_the_curve_cap_is_refused() {
    let lines = |count: usize| -> Vec<SketchEntity> {
        (0..count)
            .map(|at| {
                let from = f64::from(u32::try_from(at).expect("count")) * 3.0;
                line(&format!("l{at}"), (from, 0.0), (from + 1.0, 0.0))
            })
            .collect()
    };
    assert!(Arrangement::new(&lines(MAX_SKETCH_CURVES), &sketch()).is_ok());
    assert!(Arrangement::new(&lines(MAX_SKETCH_CURVES + 1), &sketch()).is_err());
}

#[test]
fn a_cache_builds_one_arrangement_per_sketch() {
    let entities = square_and_cut();
    let cache = Arrangements::default();
    let first = cache.get(&entities, &sketch()).expect("first");
    let second = cache.get(&entities, &sketch()).expect("second");
    assert!(std::rc::Rc::ptr_eq(&first, &second));
}

#[test]
fn a_trimmed_region_becomes_a_typed_sketch_region_pick() {
    let entities = square_and_cut();
    let outer = vec![
        stored("bottom", [0.0, 0.5], false),
        stored("cut", [0.25, 0.75], false),
        stored("top", [0.5, 1.0], false),
        stored("left", [0.0, 1.0], false),
    ];
    let profile = PlanarProfileRef::sketch_regions(
        sketch(),
        vec![SketchProfileRegion::trimmed(outer, Vec::new()).expect("region")],
    )
    .expect("regions");
    let data = SketchData {
        sketches: &[],
        entities: &entities,
        arrangements: None,
    };
    let value = planar(&profile, data).expect("selection");
    let Picks::Named(picks) = value.picks else {
        panic!("named picks");
    };
    let [picked] = picks.as_slice() else {
        panic!("one region");
    };
    let PickTarget::SketchRegion { region, .. } = &picked.target else {
        panic!("region pick");
    };
    assert_eq!(region.outer.len(), 4);
    assert!(region.holes.is_empty());
    // A use that ends away from a cut point keeps the whole feature native.
    let bad = vec![stored("bottom", [0.0, 0.25], false)];
    let profile = PlanarProfileRef::sketch_regions(
        sketch(),
        vec![SketchProfileRegion::trimmed(bad, Vec::new()).expect("region")],
    )
    .expect("regions");
    assert!(planar(&profile, data).is_err());
}
