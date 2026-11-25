// SPDX-License-Identifier: Apache-2.0
//! The change between two stored states of one blob.

use cadmpeg_ir::design::evaluation::StateDelta;
use cadmpeg_ir::design::refs::RecordRef;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::ids::PointId;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::Point;

use super::step;

fn point(name: &str, x: f64) -> Point {
    Point::new(
        PointId::mint(format!("f3d:brep:entity#{name}")).expect("identity grammar"),
        FinitePoint3::new(Point3::new(x, 0.0, 0.0)).expect("a finite point"),
        None,
    )
}

fn points(points: Vec<Point>) -> Topology {
    Topology {
        points,
        ..Topology::default()
    }
}

#[test]
fn a_step_adds_the_changed_and_new_records_and_removes_the_absent_ones() {
    let before = points(vec![point("1", 0.0), point("2", 1.0), point("4", 4.0)]);
    let after = points(vec![point("1", 0.0), point("2", 2.0), point("3", 3.0)]);
    let head = points(vec![point("3", 3.0)]);
    let current = points(vec![point("3", 5.0)]);
    let mut delta = StateDelta::default();
    step(&before, &after, &head, &current, &mut delta);
    assert_eq!(delta.records.points, [point("2", 2.0), point("3", 5.0)]);
    assert_eq!(delta.removed, [RecordRef::Point(point("4", 0.0).id)]);
}

#[test]
fn a_step_between_equal_states_is_empty() {
    let state = points(vec![point("1", 0.0), point("2", 1.0)]);
    let mut delta = StateDelta::default();
    step(&state, &state, &state, &Topology::default(), &mut delta);
    assert_eq!(delta, StateDelta::default());
}
