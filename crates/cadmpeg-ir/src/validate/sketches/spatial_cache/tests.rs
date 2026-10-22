// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::scalar::Length;

fn id(name: &str) -> SpatialSketchEntityId {
    format!("test:spatial:entity#{name}").try_into().unwrap()
}

fn check_pair(first: Geometry, second: Geometry, constraint: &Constraint) -> Option<&'static str> {
    let first: SpatialSketchGeometry = first.try_into().unwrap();
    let second: SpatialSketchGeometry = second.try_into().unwrap();
    let owner: SpatialSketchId = "test:spatial:sketch#owner".try_into().unwrap();
    let first_id = id("first");
    let second_id = id("second");
    let geometry = BorrowedIdentities::build(|add| {
        add(first_id.as_str(), (&owner, &first));
        add(second_id.as_str(), (&owner, &second));
    });
    check(&CadIr::empty(), constraint, &geometry)
}

fn point(x: f64, y: f64, z: f64) -> Geometry {
    Geometry::Point {
        position: Point3::new(x, y, z),
    }
}

#[test]
fn coincidence_checks_positions_instead_of_only_point_types() {
    let constraint = || Constraint::Coincident {
        first: id("first"),
        second: id("second"),
    };
    assert!(check_pair(point(1.0, 2.0, 3.0), point(1.0, 2.0, 3.0), &constraint()).is_none());
    assert!(check_pair(point(1.0, 2.0, 3.0), point(1.0, 2.0, 4.0), &constraint()).is_some());
}

#[test]
fn midpoint_checks_all_three_coordinates() {
    let constraint = || Constraint::Midpoint {
        point: id("first"),
        entity: id("second"),
    };
    let line = || Geometry::Line {
        start: Point3::new(1.0, 2.0, 3.0),
        end: Point3::new(3.0, 4.0, 5.0),
    };
    assert!(check_pair(point(2.0, 3.0, 4.0), line(), &constraint()).is_none());
    assert!(check_pair(point(2.0, 3.0, 5.0), line(), &constraint()).is_some());
}

#[test]
fn line_circle_tangency_checks_contact_distance_and_coplanarity() {
    let circle = || Geometry::Circle {
        center: Point3::new(0.0, 0.0, 2.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        reference_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: Length::new(2.0).unwrap(),
    };
    let constraint = || Constraint::Tangent {
        first: id("first"),
        second: id("second"),
    };
    let line = |y, z| Geometry::Line {
        start: Point3::new(-3.0, y, z),
        end: Point3::new(3.0, y, z),
    };
    assert!(check_pair(line(2.0, 2.0), circle(), &constraint()).is_none());
    assert!(check_pair(line(1.0, 2.0), circle(), &constraint()).is_some());
    assert!(check_pair(line(2.0, 3.0), circle(), &constraint()).is_some());
}

#[test]
fn coplanar_circle_tangency_checks_both_internal_and_external_contact() {
    let circle = |x, radius| Geometry::Circle {
        center: Point3::new(x, 0.0, 2.0),
        normal: Vector3::new(0.0, 0.0, 1.0),
        reference_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: Length::new(radius).unwrap(),
    };
    let constraint = || Constraint::Tangent {
        first: id("first"),
        second: id("second"),
    };
    assert!(check_pair(circle(0.0, 2.0), circle(3.0, 1.0), &constraint()).is_none());
    assert!(check_pair(circle(0.0, 2.0), circle(1.0, 1.0), &constraint()).is_none());
    assert!(check_pair(circle(0.0, 2.0), circle(2.0, 1.0), &constraint()).is_some());
}
