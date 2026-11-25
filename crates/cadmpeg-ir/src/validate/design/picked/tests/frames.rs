// SPDX-License-Identifier: Apache-2.0
//! Values that a feature in a placed ordered container states, read with the
//! geometry in the definition's frame (§5.2).

use std::collections::BTreeMap;

use super::{
    bore, bottom_edges, current, curve_bottom, hole, place, place_body, rib, ruled, turn_body, with,
};
use crate::design::construction::{
    kinds, Construction, ConstructionInput, FrameValue, InputReference, InputRole, Relation,
    Subject,
};
use crate::design::container::Container;
use crate::design::refs::EntityRef;
use crate::document::CadIr;
use crate::features::geometry::FinitePoint3;
use crate::math::{Point3, Vector3};
use crate::units::UnitVector3;
use crate::validate::design::fixture;

const CENTER: &str = "design.circle-outline-center-off-entry-plane";
const RIB: &str = "design.stiffening-rib-point-off-bend";
const PLACEMENT: &str = "design.body-does-not-container-placement-transform";
const MISSES: &str = "design.construction-misses-input";

fn unit(x: f64, y: f64, z: f64) -> UnitVector3 {
    UnitVector3::new(Vector3::new(x, y, z)).unwrap()
}

/// The frame at `origin` with the x and y axes of the definition and the z
/// axis (0, 0, `z`).
fn frame(origin: [f64; 3], z: f64) -> FrameValue {
    FrameValue {
        origin: FinitePoint3::new(Point3::new(origin[0], origin[1], origin[2])).unwrap(),
        x_axis: unit(1.0, 0.0, 0.0),
        y_axis: unit(0.0, 1.0, 0.0),
        z_axis: unit(0.0, 0.0, z),
    }
}

/// Makes the features build the part, places the timeline by `resolved`,
/// and places the cube's body by the shift `by`.
fn contained(ir: &mut CadIr, resolved: Option<FrameValue>, by: [f64; 3]) {
    let part = ir.design.scopes[0].definition.clone();
    for feature in &mut ir.design.features {
        feature.component = Some(part.clone());
    }
    let Container::Ordered { placement, .. } = &mut ir.design.containers[0] else {
        unreachable!("the fixture's timeline is ordered");
    };
    *placement = Some(Box::new(Construction {
        inputs: Vec::new(),
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved,
        resolved_in: BTreeMap::new(),
    }));
    place_body(
        ir,
        [
            [1.0, 0.0, 0.0, by[0]],
            [0.0, 1.0, 0.0, by[1]],
            [0.0, 0.0, 1.0, by[2]],
        ],
    );
}

#[test]
fn a_circle_outline_center_in_a_placed_container_is_read_in_the_definition_s_frame() {
    let mut ir = fixture::document();
    contained(
        &mut ir,
        Some(frame([0.0, 0.0, 100.0], 1.0)),
        [0.0, 0.0, 100.0],
    );
    with(&mut ir, |ir| hole(ir, 0.0));
    assert!(ruled(&ir, PLACEMENT).is_empty());
    assert!(ruled(&ir, CENTER).is_empty());
    with(&mut ir, |ir| hole(ir, 1.0));
    assert_eq!(ruled(&ir, CENTER).len(), 1);
}

#[test]
fn a_stiffening_rib_point_in_a_placed_container_is_read_in_the_definition_s_frame() {
    let mut ir = fixture::document();
    curve_bottom(&mut ir);
    // The shift carries the cylinder's axis to (12, 5) and (10, 5, 3) to
    // (17, 5, 3), 5 from it.
    contained(&mut ir, Some(frame([7.0, 0.0, 0.0], 1.0)), [7.0, 0.0, 0.0]);
    with(&mut ir, |ir| rib(ir, 10.0));
    assert!(ruled(&ir, PLACEMENT).is_empty());
    assert!(ruled(&ir, RIB).is_empty());
    with(&mut ir, |ir| rib(ir, 5.0));
    assert_eq!(ruled(&ir, RIB).len(), 1);
}

#[test]
fn a_point_under_an_unknown_or_improper_container_placement_is_not_judged() {
    let mut ir = fixture::document();
    contained(&mut ir, None, [0.0, 0.0, 100.0]);
    with(&mut ir, |ir| hole(ir, 1.0));
    assert!(ruled(&ir, CENTER).is_empty());
    contained(
        &mut ir,
        Some(frame([0.0, 0.0, 100.0], -1.0)),
        [0.0, 0.0, 100.0],
    );
    assert!(ruled(&ir, CENTER).is_empty());
}

/// A point stated `Coincident` with `vertex`, resolved at `at`.
fn on_vertex(vertex: EntityRef, at: Point3) -> Construction<kinds::Point> {
    Construction {
        inputs: vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::Coincident,
            }),
            reference: Some(InputReference::Picks(current(vec![vertex]))),
            value: None,
            reversed: None,
        }],
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: FinitePoint3::new(at),
        resolved_in: BTreeMap::new(),
    }
}

/// The findings of the entry-plane rule and of the construction cache rule
/// for a hole centered on a point `Coincident` with a bottom vertex.
/// `position` maps the vertex's stored position into the features' frame,
/// and the point lies `rise` above it.
fn both(ir: &mut CadIr, position: impl Fn(Point3) -> Point3, rise: f64) -> [usize; 2] {
    let vertex = ir.geometry.edges[bottom_edges(ir)[0]]
        .start()
        .unwrap()
        .clone();
    let at = position(place(ir, &vertex));
    let at = Point3::new(at.x, at.y, at.z + rise);
    with(ir, |ir| bore(ir, on_vertex(EntityRef::Vertex(vertex), at)));
    [ruled(ir, CENTER).len(), ruled(ir, MISSES).len()]
}

#[test]
fn one_point_passes_or_fails_the_entry_plane_and_cache_rules_alike() {
    let mut ir = fixture::document();
    contained(
        &mut ir,
        Some(frame([0.0, 0.0, 100.0], 1.0)),
        [0.0, 0.0, 100.0],
    );
    assert_eq!(both(&mut ir, |at| at, 0.0), [0, 0]);
    assert_eq!(both(&mut ir, |at| at, 1.0), [1, 1]);
    let mut ir = fixture::document();
    turn_body(&mut ir);
    let turned = |at: Point3| Point3::new(3.0 - at.y, at.x, at.z + 100.0);
    assert_eq!(both(&mut ir, turned, 0.0), [0, 0]);
    assert_eq!(both(&mut ir, turned, 1.0), [1, 1]);
}
