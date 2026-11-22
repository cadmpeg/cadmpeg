// SPDX-License-Identifier: Apache-2.0
//! Circles the decoder builds from a dimension record, which no curve record
//! stores: construction where they repeat another curve of their sketch or
//! no face of the solid lies on them, so they bound no region of the
//! whole-sketch pick, and their dimension keeps them as its reference.

use std::collections::HashSet;

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

use super::assert_no_design_findings;
use super::sketch_and_extrude;
use super::state_zero::{entity, project, set_profiles};
use crate::loss::SldprtLossCode;

/// The fixture's rectangle and circle as the stored profile, and a circle
/// built from the dimension record `relation` at `center` with `radius`,
/// which the fixture's diameter dimension names.
fn with_dimension_circle(center: [f64; 2], radius: f64) -> CadIr {
    let (mut ir, _) = sketch_and_extrude();
    let sketch = ir.model.sketches[0].id.as_str().to_owned();
    ir.model.sketch_entities.push(
        serde_json::from_value(serde_json::json!({
            "id": entity("built").as_str(), "sketch": sketch, "construction": false,
            "geometry_ref": "sldprt:feature-input:relation-instance#test:2",
            "endpoint_refs": [],
            "geometry": {"kind": "circle", "center": {"u": center[0], "v": center[1]},
                "radius": radius}}))
        .unwrap(),
    );
    let diameter = ir
        .model
        .sketch_constraints
        .iter_mut()
        .find(|constraint| constraint.id.as_str().ends_with("test:4"))
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

fn construction(ir: &CadIr, key: &str) -> Option<bool> {
    ir.design
        .sketch_entities
        .iter()
        .find(|candidate| candidate.id == entity(key))
        .unwrap()
        .construction
}

/// The entity the design's diameter dimension names.
fn dimensioned(ir: &CadIr) -> String {
    let relation = ir
        .design
        .sketch_constraints
        .iter()
        .find(|constraint| constraint.id.as_str().ends_with("test:4"))
        .map(|constraint| serde_json::to_value(&constraint.relation).unwrap())
        .unwrap();
    relation["value"]["of"].as_str().unwrap().to_owned()
}

/// A built circle on the fixture's circle, its radius off in the ninth
/// digit: construction, so the pick holds the circle once, not a hole.
#[test]
fn a_dimension_circle_on_a_stored_circle_is_construction() {
    let mut ir = with_dimension_circle([25.0, 15.0], 5.0 + 4.0e-9);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert_eq!(construction(&ir, "hole"), None);
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    let note = typed
        .dimension_construction
        .expect("the construction is reported");
    assert_eq!(
        note.code,
        SldprtLossCode::DesignSketchDimensionCurveConstruction.kind()
    );
    assert!(
        note.message.contains(&format!(
            "{} (repeats another curve of its sketch)",
            entity("built").as_str()
        )),
        "{}",
        note.message
    );
    assert!(
        typed.profile_differs.is_none(),
        "{:?}",
        typed.profile_differs
    );
    assert_no_design_findings(&ir);
}

/// A built circle off every face of the solid (the fixture has none) is
/// construction: no region of the pick is cut there.
#[test]
fn a_dimension_circle_no_face_lies_on_is_construction() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), Some(true));
    assert_eq!(dimensioned(&ir), entity("built").as_str());
    let note = typed
        .dimension_construction
        .expect("the construction is reported");
    assert!(
        note.message.contains(&format!(
            "{} (no face of the solid lies on it)",
            entity("built").as_str()
        )),
        "{}",
        note.message
    );
    assert!(
        typed.profile_differs.is_none(),
        "{:?}",
        typed.profile_differs
    );
    assert_no_design_findings(&ir);
}

/// A built circle with a cylinder of its radius on an axis along the sketch
/// normal through its center is the solid's hole: it stays profile geometry,
/// and the pick's regions differ from the stored profile by that hole.
#[test]
fn a_dimension_circle_on_a_coaxial_cylinder_stays() {
    let mut ir = with_dimension_circle([10.0, 10.0], 3.0);
    // The fixture's sketch lies in the plane y = 0 with u along x, so
    // (u, v) is (u, 0, -v); the axis runs against the normal.
    ir.geometry.surfaces.push(Surface {
        id: SurfaceId::mint("sldprt:test:surface#hole").unwrap(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(10.0, 5.0, -10.0),
                Vector3::new(0.0, -1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                3.0,
            )
            .unwrap(),
        )),
        source_object: None,
        native_ref: None,
    });
    let (_, typed) = project(&mut ir, &HashSet::new());
    assert_eq!(construction(&ir, "built"), None);
    assert!(
        typed.dimension_construction.is_none(),
        "{:?}",
        typed.dimension_construction
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
