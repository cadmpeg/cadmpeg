// SPDX-License-Identifier: Apache-2.0
//! Circles the decoder builds from a dimension record (open item DI-83). One
//! that repeats a stored circle or arc, or that no cylinder of the solid
//! lies on and whose center lies on a stored curve of its radius, is left
//! out and its dimension names that curve; one no relation names is left
//! out; another is construction unless a cylinder of the solid lies on it.

use std::collections::HashSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

use super::assert_no_design_findings;
use super::sketch_and_extrude;
use super::state_zero::{entity, project, set_profiles};
use crate::loss::SldprtLossCode;

const DIAMETER: &str = "sldprt:model:sketch-constraint#test:4";
/// The fixture's native relation without a value.
const NATIVE: &str = "sldprt:model:sketch-constraint#test:3";
/// The fixture's native distance dimension, which has a value.
const NATIVE_DISTANCE: &str = "sldprt:model:sketch-constraint#test:2";

/// The fixture's rectangle and circle as the stored profile, and a circle
/// `built` from a dimension record at `center` with `radius`, which the
/// fixture's diameter dimension names.
fn with_dimension_circle(center: [f64; 2], radius: f64) -> CadIr {
    let (mut ir, _) = sketch_and_extrude();
    push_built(&mut ir, "built", 2, center, radius);
    let diameter = ir
        .model
        .sketch_constraints
        .iter_mut()
        .find(|constraint| constraint.id.as_str() == DIAMETER)
        .unwrap();
    let mut value = serde_json::to_value(&*diameter).unwrap();
    value["definition"]["entity"] = serde_json::json!(entity("built").as_str());
    *diameter = serde_json::from_value(value).unwrap();
    set_profiles(
        &mut ir,
        &[
            &[("l0", false), ("l1", false), ("l2", false), ("l3", false)],
            &[("hole", false)],
        ],
    );
    ir
}

/// Adds the circle `key` the decoder built from the relation record
/// `relation` of the fixture's sketch.
fn push_built(ir: &mut CadIr, key: &str, relation: u32, center: [f64; 2], radius: f64) {
    push(
        ir,
        serde_json::json!({
            "id": entity(key).as_str(), "construction": false,
            "geometry_ref": format!("sldprt:feature-input:relation-instance#test:{relation}"),
            "endpoint_refs": [],
            "geometry": {"kind": "circle", "center": {"u": center[0], "v": center[1]},
                "radius": radius}}),
    );
}

/// Adds the entity `value` to the fixture's sketch.
fn push(ir: &mut CadIr, mut value: serde_json::Value) {
    value["sketch"] = serde_json::json!(ir.model.sketches[0].id.as_str());
    ir.model
        .sketch_entities
        .push(serde_json::from_value(value).unwrap());
}

/// Makes the fixture's native relation `constraint` name `key`.
fn name_by_native(ir: &mut CadIr, constraint: &str, key: &str) {
    let native = ir
        .model
        .sketch_constraints
        .iter_mut()
        .find(|candidate| candidate.id.as_str() == constraint)
        .unwrap();
    let mut value = serde_json::to_value(&*native).unwrap();
    value["definition"]["entities"] = serde_json::json!([entity(key).as_str()]);
    *native = serde_json::from_value(value).unwrap();
}

fn construction(ir: &CadIr, key: &str) -> Option<bool> {
    ir.design
        .sketch_entities
        .iter()
        .find(|candidate| candidate.id == entity(key))
        .unwrap()
        .construction
}

fn in_design(ir: &CadIr, key: &str) -> bool {
    ir.design
        .sketch_entities
        .iter()
        .any(|candidate| candidate.id == entity(key))
}

/// The design's relation `id`, serialized.
fn relation(ir: &CadIr, id: &str) -> serde_json::Value {
    ir.design
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.id.as_str() == id)
        .map(|constraint| serde_json::to_value(&constraint.relation).unwrap())
        .unwrap()
}

/// The entity the design's diameter dimension names.
fn dimensioned(ir: &CadIr) -> String {
    relation(ir, DIAMETER)["value"]["of"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Whether the design's relation `id` names the entity `key`.
fn relation_names(ir: &CadIr, id: &str, key: &str) -> bool {
    relation(ir, id)
        .to_string()
        .contains(&format!("\"{}\"", entity(key).as_str()))
}

/// The note's entry for `key`: its id and the parenthesized text.
fn entry(key: &str, text: &str) -> String {
    format!("{} ({text})", entity(key).as_str())
}

fn note(typed: &super::super::typed::Typed) -> String {
    let note = typed
        .dimension_circles
        .as_ref()
        .expect("the built circles are reported");
    assert_eq!(
        note.code,
        SldprtLossCode::DesignSketchDimensionCircle.kind()
    );
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Warning);
    note.message.clone()
}

/// A cylinder face of `radius` through `origin` along `axis`.
fn cylinder(key: &str, origin: [f64; 3], axis: [f64; 3], radius: f64) -> Surface {
    Surface {
        id: SurfaceId::mint(format!("sldprt:test:surface#{key}")).unwrap(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(origin[0], origin[1], origin[2]),
                Vector3::new(axis[0], axis[1], axis[2]),
                Vector3::new(1.0, 0.0, 0.0),
                radius,
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    }
}

/// A built circle on the fixture's circle, its radius off in the ninth
/// digit: left out, and the dimension names the stored circle again, so the
/// pick holds the circle once, not a hole.
#[test]
fn a_dimension_circle_on_a_stored_circle_is_left_out_for_it() {
    let mut ir = with_dimension_circle([25.0, 15.0], 5.0 + 4.0e-9);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert!(!in_design(&ir, "built"));
    assert_eq!(construction(&ir, "hole"), None);
    assert_eq!(dimensioned(&ir), entity("hole").as_str());
    let message = note(&typed);
    assert!(
        message.contains(&entry(
            "built",
            &format!(
                "it repeats {}; left out; the dimension {DIAMETER} names that curve",
                entity("hole").as_str()
            )
        )),
        "{message}"
    );
    assert!(
        typed.profile_differs.is_none(),
        "{:?}",
        typed.profile_differs
    );
    assert_no_design_findings(&ir);
}

/// A built circle whose center lies on the fixture's circle, of its radius:
/// the handle is a point of the stored circle, which the dimension names.
#[test]
fn a_dimension_circle_centered_on_a_stored_circle_is_left_out_for_it() {
    let mut ir = with_dimension_circle([30.0, 15.0], 5.0);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert!(!in_design(&ir, "built"));
    assert_eq!(dimensioned(&ir), entity("hole").as_str());
    let message = note(&typed);
    assert!(
        message.contains(&entry(
            "built",
            &format!(
                "its center lies on {}, of its radius; left out; the dimension {DIAMETER} \
                 names that curve",
                entity("hole").as_str()
            )
        )),
        "{message}"
    );
    assert_no_design_findings(&ir);
}

/// A stored quarter arc of radius 3 about (10, 10), from angle 0 to 90°.
fn push_arc(ir: &mut CadIr) {
    push(
        ir,
        serde_json::json!({
            "id": entity("arc").as_str(), "construction": true, "endpoint_refs": [],
            "geometry": {"kind": "arc", "center": {"u": 10.0, "v": 10.0}, "radius": 3.0,
                "start_angle": 0.0, "end_angle": std::f64::consts::FRAC_PI_2}}),
    );
}

/// A built circle centered on the end of a stored arc of its radius: the
/// dimension names the arc. One centered on the arc's carrier off the arc
/// stays a construction circle, and the note says no face of the solid lies
/// on it.
#[test]
fn a_dimension_circle_at_an_arc_end_names_the_arc_and_one_off_the_arc_stays() {
    let mut ir = with_dimension_circle([13.0, 10.0], 3.0);
    push_arc(&mut ir);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert!(!in_design(&ir, "built"));
    assert_eq!(dimensioned(&ir), entity("arc").as_str());
    assert!(note(&typed).contains(&format!("its center lies on {}", entity("arc").as_str())));

    let mut ir = with_dimension_circle([7.0, 10.0], 3.0);
    push_arc(&mut ir);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    let message = note(&typed);
    assert!(
        message.contains(&entry(
            "built",
            "no face of the solid lies on it; construction"
        )),
        "{message}"
    );
    assert_no_design_findings(&ir);
}

/// Two stored circles of its radius pass through the built circle's center
/// on two carriers: no one curve is the dimension's, so the built circle
/// stays, construction, and the dimension names it.
#[test]
fn a_dimension_circle_on_two_carriers_stays_construction() {
    let mut ir = with_dimension_circle([30.0, 15.0], 5.0);
    push(
        &mut ir,
        serde_json::json!({
            "id": entity("ring").as_str(), "construction": true, "endpoint_refs": [],
            "geometry": {"kind": "circle", "center": {"u": 35.0, "v": 15.0}, "radius": 5.0}}),
    );
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    assert!(note(&typed).contains(&entry(
        "built",
        "no face of the solid lies on it; construction"
    )));
}

/// A built circle off every face of the solid (the fixture has none), with
/// no stored curve of its radius: construction, and the note says the
/// profile lacks the circle its dimension measures. A second circle built
/// from the same record, which no relation names, is left out.
#[test]
fn a_dimension_circle_no_face_lies_on_is_construction_and_its_unnamed_twin_is_left_out() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    push_built(&mut ir, "twin", 9, [40.0, 10.0], 3.0);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    assert!(!in_design(&ir, "twin"));
    let message = note(&typed);
    for expected in [
        entry(
            "built",
            "no stored curve of its sketch has its radius and no face of the solid lies on it, \
             so the profile lacks the circle its dimension measures; construction",
        ),
        entry(
            "twin",
            "no stored curve of its sketch has its radius and no face of the solid lies on it; \
             left out; no relation names it",
        ),
    ] {
        assert!(message.contains(&expected), "{message}");
    }
    assert!(
        typed.profile_differs.is_none(),
        "{:?}",
        typed.profile_differs
    );
    assert_no_design_findings(&ir);
}

/// A second built circle on the first one's carrier, which a relation names:
/// construction, and the note names the circle it repeats.
#[test]
fn a_dimension_circle_repeating_an_earlier_one_is_construction() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    push_built(&mut ir, "again", 9, [10.0, 10.0], 3.0);
    name_by_native(&mut ir, NATIVE, "again");
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "again"), Some(true));
    assert!(note(&typed).contains(&entry(
        "again",
        &format!("it repeats {}; construction", entity("built").as_str())
    )));
}

/// A cylinder of their radius lies on two built circles on one carrier: the
/// first stays profile geometry, and the second, which a relation names, is
/// construction before the cylinder is read, so the profile holds the
/// circle once.
#[test]
fn a_dimension_circle_repeating_a_witnessed_one_is_construction() {
    // The fixture's sketch lies in the plane y = 0 with u along x, so
    // (u, v) is (u, 0, -v).
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    push_built(&mut ir, "again", 9, [10.0, 10.0], 3.0);
    name_by_native(&mut ir, NATIVE, "again");
    ir.geometry
        .surfaces
        .push(cylinder("bore", [10.0, 5.0, -10.0], [0.0, -1.0, 0.0], 3.0));
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), None);
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    assert_eq!(construction(&ir, "again"), Some(true));
    let message = note(&typed);
    assert!(
        message.contains(&entry(
            "again",
            &format!("it repeats {}; construction", entity("built").as_str())
        )),
        "{message}"
    );
    assert!(
        !message.contains(&format!("{} (", entity("built").as_str())),
        "{message}"
    );
}

/// A relation other than a dimension names the built circle on the stored
/// circle: the dimension cannot move alone, so the circle stays,
/// construction, and the dimension names it.
#[test]
fn a_dimension_circle_another_relation_names_stays_construction() {
    let mut ir = with_dimension_circle([25.0, 15.0], 5.0);
    name_by_native(&mut ir, NATIVE, "built");
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    let message = note(&typed);
    assert!(
        message.contains(&entry(
            "built",
            &format!(
                "it repeats {}; construction; the relation {NATIVE} names it and does not move \
                 to that curve",
                entity("hole").as_str()
            )
        )),
        "{message}"
    );
}

/// A native distance names the built circle that repeats the stored
/// circle, whose center is the built circle's: the distance moves with the
/// diameter, and the note names both.
#[test]
fn a_native_distance_moves_to_the_curve_its_circle_repeats() {
    let mut ir = with_dimension_circle([25.0, 15.0], 5.0);
    name_by_native(&mut ir, NATIVE_DISTANCE, "built");
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert!(!in_design(&ir, "built"));
    assert_eq!(dimensioned(&ir), entity("hole").as_str());
    assert!(relation_names(&ir, NATIVE_DISTANCE, "hole"));
    assert!(!relation_names(&ir, NATIVE_DISTANCE, "built"));
    let message = note(&typed);
    assert!(
        message.contains(&entry(
            "built",
            &format!(
                "it repeats {}; left out; the dimensions {NATIVE_DISTANCE}, {DIAMETER} name \
                 that curve",
                entity("hole").as_str()
            )
        )),
        "{message}"
    );
    assert_no_design_findings(&ir);
}

/// A native distance names the built circle whose center lies on the stored
/// circle: it measures from that center, one radius from the stored
/// circle's, so neither it nor the diameter moves, and the circle stays,
/// construction.
#[test]
fn a_native_distance_keeps_a_circle_centered_on_a_stored_curve() {
    let mut ir = with_dimension_circle([30.0, 15.0], 5.0);
    name_by_native(&mut ir, NATIVE_DISTANCE, "built");
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    assert!(relation_names(&ir, NATIVE_DISTANCE, "built"));
    let message = note(&typed);
    assert!(
        message.contains(&entry(
            "built",
            &format!(
                "its center lies on {}, of its radius; construction; the relation \
                 {NATIVE_DISTANCE} names it and does not move to that curve",
                entity("hole").as_str()
            )
        )),
        "{message}"
    );
    assert_no_design_findings(&ir);
}

/// A cylinder of its radius on the built circle's own center confirms it
/// although its center lies on a stored circle: it stays profile geometry
/// and keeps its dimension. A built circle that repeats the stored circle
/// moves its dimension there although a cylinder lies on it.
#[test]
fn a_cylinder_keeps_a_circle_centered_on_a_stored_curve_but_not_a_repeat() {
    // The fixture's sketch lies in the plane y = 0 with u along x, so
    // (u, v) is (u, 0, -v).
    let mut ir = with_dimension_circle([30.0, 15.0], 5.0);
    ir.geometry
        .surfaces
        .push(cylinder("bore", [30.0, 5.0, -15.0], [0.0, -1.0, 0.0], 5.0));
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), None);
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    assert!(
        typed.dimension_circles.is_none(),
        "{:?}",
        typed.dimension_circles
    );

    let mut ir = with_dimension_circle([25.0, 15.0], 5.0);
    ir.geometry
        .surfaces
        .push(cylinder("bore", [25.0, 5.0, -15.0], [0.0, -1.0, 0.0], 5.0));
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert!(!in_design(&ir, "built"));
    assert_eq!(dimensioned(&ir), entity("hole").as_str());
    assert!(note(&typed).contains(&format!("it repeats {}", entity("hole").as_str())));
}

/// Two stored arcs on the built circle's carrier: the file does not say
/// which the dimension measured, so it names the first, and the note says
/// so.
#[test]
fn a_dimension_circle_repeating_two_arcs_names_the_first() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    push_arc(&mut ir);
    push(
        &mut ir,
        serde_json::json!({
            "id": entity("arc2").as_str(), "construction": true, "endpoint_refs": [],
            "geometry": {"kind": "arc", "center": {"u": 10.0, "v": 10.0}, "radius": 3.0,
                "start_angle": std::f64::consts::PI,
                "end_angle": 1.5 * std::f64::consts::PI}}),
    );
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert!(!in_design(&ir, "built"));
    assert_eq!(dimensioned(&ir), entity("arc").as_str());
    let message = note(&typed);
    assert!(
        message.contains(&entry(
            "built",
            &format!(
                "it repeats {}, the first of 2 such curves on one carrier; left out; the \
                 dimension {DIAMETER} names that curve",
                entity("arc").as_str()
            )
        )),
        "{message}"
    );
}

/// A built circle the decoder already marks construction stays so, and the
/// note does not name it.
#[test]
fn a_construction_dimension_circle_is_not_reported() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    let built = ir
        .model
        .sketch_entities
        .iter_mut()
        .find(|candidate| candidate.id() == &entity("built"))
        .unwrap();
    built.construction = true;
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert!(
        typed.dimension_circles.is_none(),
        "{:?}",
        typed.dimension_circles
    );
}

/// A sketch with no resolved frame has no faces to compare: the built
/// circle is construction, and the note says why.
#[test]
fn a_dimension_circle_in_an_unplaced_sketch_is_construction() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    ir.model.sketches[0].placement =
        serde_json::from_value(serde_json::json!({"kind": "unresolved"})).unwrap();
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert!(note(&typed).contains(&entry(
        "built",
        "its sketch has no resolved frame to compare with the solid; construction"
    )));
}

/// A circle built from an arc marker is an arc: the rule reads circles
/// only, and the arc stays as decoded.
#[test]
fn a_dimension_arc_stays_as_decoded() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    let built = ir
        .model
        .sketch_entities
        .iter_mut()
        .find(|candidate| candidate.id() == &entity("built"))
        .unwrap();
    let mut value = serde_json::to_value(&*built).unwrap();
    value["geometry"] = serde_json::json!({"kind": "arc", "center": {"u": 10.0, "v": 10.0},
        "radius": 3.0, "start_angle": 0.0, "end_angle": std::f64::consts::PI});
    *built = serde_json::from_value(value).unwrap();
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), None);
    assert!(
        typed.dimension_circles.is_none(),
        "{:?}",
        typed.dimension_circles
    );
}

/// A built circle with a cylinder of its radius on an axis along the sketch
/// normal through its center is the solid's hole: it stays profile geometry,
/// and the pick's regions differ from the stored profile by that hole.
#[test]
fn a_dimension_circle_on_a_coaxial_cylinder_stays() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    // The fixture's sketch lies in the plane y = 0 with u along x, so
    // (u, v) is (u, 0, -v); the axis runs against the normal.
    ir.geometry
        .surfaces
        .push(cylinder("hole", [10.0, 5.0, -10.0], [0.0, -1.0, 0.0], 3.0));
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), None);
    assert!(
        typed.dimension_circles.is_none(),
        "{:?}",
        typed.dimension_circles
    );
    let note = typed
        .profile_differs
        .expect("the hole the stored profile leaves out is reported");
    assert!(
        note.message
            .contains("missing region, a loop inside the stored profile is a hole"),
        "{}",
        note.message
    );
}

/// A cylinder of another radius, one whose axis misses the center, and one
/// whose axis is tilted from the sketch normal witness nothing.
#[test]
fn a_cylinder_off_the_dimension_circle_witnesses_nothing() {
    for surface in [
        cylinder("wide", [10.0, 5.0, -10.0], [0.0, -1.0, 0.0], 3.5),
        cylinder("beside", [11.0, 5.0, -10.0], [0.0, -1.0, 0.0], 3.0),
        cylinder("tilted", [10.0, 0.0, -10.0], [0.0, -0.8, 0.6], 3.0),
    ] {
        let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
        let key = surface.id.as_str().to_owned();
        ir.geometry.surfaces.push(surface);
        project(&mut ir, &HashSet::new());
        assert_eq!(construction(&ir, "built"), Some(true), "{key}");
    }
}

/// A line across the rectangle closes only with the rectangle's state-`0`
/// lines, which therefore stay profile geometry. A construction circle
/// built from a dimension record across that line closes nothing, so the
/// lines stay.
#[test]
fn a_construction_dimension_circle_closes_no_state_zero_loop() {
    let mut ir = with_dimension_circle([10.0, 25.0], 3.0);
    push(
        &mut ir,
        serde_json::json!({
            "id": entity("chord").as_str(), "construction": false, "endpoint_refs": [],
            "geometry": {"kind": "line", "start": {"u": 0.0, "v": 25.0},
                "end": {"u": 50.0, "v": 25.0}}}),
    );
    let outside = ["l0", "l1", "l2", "l3"]
        .into_iter()
        .map(entity)
        .collect::<HashSet<_>>();
    project(&mut ir, &outside);
    assert_eq!(construction(&ir, "built"), Some(true));
    for key in ["l0", "l1", "l2", "l3"] {
        assert_eq!(construction(&ir, key), None, "{key}");
    }
}
