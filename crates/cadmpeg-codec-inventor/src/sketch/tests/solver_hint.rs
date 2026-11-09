// SPDX-License-Identifier: Apache-2.0
//! Solver hints in the constraint maps of perpendicular relations.

use super::{closed_square_inventory, hinted_relation, solver_hint};
use crate::pmdc::{PmDcReference, PmDcReferenceList};
use crate::record_identity::Located;
use crate::sketch::{
    project, PmDcSketchConstraint, SketchProjection, PERPENDICULAR_TYPE, SKETCH_TYPE,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// The same record read as a perpendicular relation whose second operand is
/// the reference `second`.
fn as_perpendicular(relation: &PmDcSketchConstraint, second: u32) -> PmDcSketchConstraint {
    let mut wire = serde_json::to_value(relation).expect("relation wire");
    wire["kind"] = serde_json::json!({
        "form": "perpendicular",
        "first": wire["kind"]["first"].clone(),
        "second": {"index": second, "qualified": true},
        "orientation": 0,
    });
    serde_json::from_value(wire).expect("perpendicular wire")
}

/// The same record read as a horizontal alignment.
fn as_horizontal_alignment(relation: &PmDcSketchConstraint) -> PmDcSketchConstraint {
    let mut wire = serde_json::to_value(relation).expect("relation wire");
    wire["kind"] = serde_json::json!({
        "form": "horizontal_alignment",
        "first": wire["kind"]["first"].clone(),
        "second": wire["kind"]["second"].clone(),
    });
    serde_json::from_value(wire).expect("alignment wire")
}

#[test]
fn a_perpendicular_hint_can_name_a_curve_end() {
    for code in [0, 1, 2] {
        assert!(solver_hint(&as_perpendicular(&hinted_relation(code), 4)));
    }
    assert!(!solver_hint(&as_perpendicular(&hinted_relation(3), 4)));
    // The relation kinds that state no hint keep their maps native.
    assert!(!solver_hint(&as_horizontal_alignment(&hinted_relation(0))));
}

/// Projects the closed square with the hinted relation, read as a
/// perpendicular relation between lines 8 and 9, listed by the sketch.
fn project_square_with_hinted_perpendicular(code: u32) -> SketchProjection {
    let (mut inventory, mut sketch) = closed_square_inventory();
    let mut relation = as_perpendicular(&hinted_relation(code), 9);
    relation.identity.type_id =
        crate::record_identity::RecordTypeId::from_bytes(PERPENDICULAR_TYPE);
    inventory.constraints.push(relation);
    let (marker, metadata, mut references) = sketch.entities.clone().into_parts();
    references.push(PmDcReference::new(12, false).expect("test reference index fits 31 bits"));
    sketch.entities =
        PmDcReferenceList::new(marker, metadata, references).expect("extended entity list");
    inventory.sketches[0] = Located::new(
        sketch,
        crate::record_identity::RecordTypeId::from_bytes(SKETCH_TYPE),
        cadmpeg_ir::identity_key!("segment"),
        2,
    );
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service())
        .expect("projection context");
    project(&ctx, &inventory, &[]).expect("sketch projection")
}

#[test]
fn a_perpendicular_relation_with_a_solver_hint_projects() {
    let projected = project_square_with_hinted_perpendicular(2);
    assert_eq!(projected.unresolved_sketches, 0);
    assert_eq!(projected.unresolved_constraints, 0);
    assert_eq!(projected.constraints.len(), 1);
}

#[test]
fn a_perpendicular_relation_with_an_unknown_hint_code_stays_native() {
    let projected = project_square_with_hinted_perpendicular(3);
    assert_eq!(projected.unresolved_sketches, 1);
    assert!(projected.constraints.is_empty());
}

/// The same record read as a perpendicular relation with the same operands.
fn with_perpendicular_kind(relation: &PmDcSketchConstraint) -> PmDcSketchConstraint {
    let mut wire = serde_json::to_value(relation).expect("relation wire");
    wire["kind"] = serde_json::json!({
        "form": "perpendicular",
        "first": wire["kind"]["first"].clone(),
        "second": wire["kind"]["second"].clone(),
        "orientation": 0,
    });
    serde_json::from_value(wire).expect("perpendicular wire")
}

#[test]
fn a_perpendicular_hint_code_is_not_validated_as_a_reference() {
    let coincident = hinted_relation(2);
    let perpendicular = with_perpendicular_kind(&coincident);
    let references = crate::validate::sketch_constraint_references;
    assert_eq!(
        references(&coincident).len(),
        references(&perpendicular).len() + 1
    );
    assert_eq!(
        references(&coincident).last(),
        references(&perpendicular).last()
    );
}
