// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::index::identities::BorrowedIdentities;
use crate::scalar::{Angle, Length};
use crate::sketches::{EllipseRadii, SketchGeometry};

fn checked(helper: Geometry, parent: Geometry, alignment: Alignment) -> Option<bool> {
    let helper: SketchGeometry = helper.try_into().unwrap();
    let parent: SketchGeometry = parent.try_into().unwrap();
    let helper_id: SketchEntityId = "test:sketch:entity#helper".try_into().unwrap();
    let parent_id: SketchEntityId = "test:sketch:entity#parent".try_into().unwrap();
    let geometry = BorrowedIdentities::build(|add| {
        add(helper_id.as_str(), &helper);
        add(parent_id.as_str(), &parent);
    });
    check(
        &CadIr::empty(),
        &helper_id,
        &parent_id,
        alignment,
        &geometry,
    )
}

fn ellipse() -> Geometry {
    Geometry::Ellipse {
        center: Point2::new(1.0, 2.0),
        major_angle: Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
        radii: EllipseRadii {
            major_radius: Length::new(5.0).unwrap(),
            minor_radius: Length::new(3.0).unwrap(),
        },
        bounds: None,
    }
}

#[test]
fn ellipse_diameters_check_length_center_and_axis_without_endpoint_order() {
    for (start, end) in [
        (Point2::new(1.0, -3.0), Point2::new(1.0, 7.0)),
        (Point2::new(1.0, 7.0), Point2::new(1.0, -3.0)),
    ] {
        assert_eq!(
            checked(
                Geometry::Line { start, end },
                ellipse(),
                Alignment::EllipseMajorDiameter
            ),
            Some(true)
        );
    }
    assert_eq!(
        checked(
            Geometry::Line {
                start: Point2::new(1.0, -2.0),
                end: Point2::new(1.0, 6.0),
            },
            ellipse(),
            Alignment::EllipseMajorDiameter
        ),
        Some(false)
    );
    assert_eq!(
        checked(
            Geometry::Line {
                start: Point2::new(-2.0, 2.0),
                end: Point2::new(4.0, 2.0),
            },
            ellipse(),
            Alignment::EllipseMinorDiameter
        ),
        Some(true)
    );
}

#[test]
fn ellipse_focus_checks_geometry_without_inventing_source_label_numbering() {
    for alignment in [Alignment::EllipseFocus1, Alignment::EllipseFocus2] {
        for position in [Point2::new(1.0, -2.0), Point2::new(1.0, 6.0)] {
            assert_eq!(
                checked(Geometry::Point { position }, ellipse(), alignment),
                Some(true)
            );
        }
        assert_eq!(
            checked(
                Geometry::Point {
                    position: Point2::new(1.0, 2.0)
                },
                ellipse(),
                alignment
            ),
            Some(false)
        );
    }
}

#[test]
fn parabola_focus_and_axis_follow_current_vertex_and_axis() {
    let parabola = || Geometry::Parabola {
        vertex: Point2::new(3.0, 4.0),
        axis_angle: Angle::new(0.0).unwrap(),
        focal_length: Length::new(2.0).unwrap(),
        bounds: None,
    };
    assert_eq!(
        checked(
            Geometry::Point {
                position: Point2::new(5.0, 4.0)
            },
            parabola(),
            Alignment::ParabolaFocus
        ),
        Some(true)
    );
    assert_eq!(
        checked(
            Geometry::Point {
                position: Point2::new(1.0, 4.0)
            },
            parabola(),
            Alignment::ParabolaFocus
        ),
        Some(false)
    );
    assert_eq!(
        checked(
            Geometry::Line {
                start: Point2::new(0.0, 4.0),
                end: Point2::new(8.0, 4.0)
            },
            parabola(),
            Alignment::ParabolaFocalAxis
        ),
        Some(true)
    );
    assert_eq!(
        checked(
            Geometry::Line {
                start: Point2::new(0.0, 5.0),
                end: Point2::new(8.0, 5.0)
            },
            parabola(),
            Alignment::ParabolaFocalAxis
        ),
        Some(false)
    );
}

#[test]
fn spline_control_helpers_follow_indexed_poles_without_guessing_knot_indexing() {
    let parent = || Geometry::Nurbs {
        curve: crate::geometry::pcurve::PcurveNurbs::from_lanes(
            1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point2::new(1.0, 2.0), Point2::new(3.0, 4.0)],
            None,
            false,
        )
        .unwrap(),
    };
    let helper = || Geometry::Point {
        position: Point2::new(3.0, 4.0),
    };
    assert_eq!(
        checked(helper(), parent(), Alignment::BsplineControlPoint(1)),
        Some(true)
    );
    assert_eq!(
        checked(helper(), parent(), Alignment::BsplineControlPoint(0)),
        Some(false)
    );
    assert_eq!(
        checked(helper(), parent(), Alignment::BsplineControlPoint(2)),
        Some(false)
    );
    assert_eq!(
        checked(helper(), parent(), Alignment::BsplineKnotPoint(0)),
        None
    );
}
