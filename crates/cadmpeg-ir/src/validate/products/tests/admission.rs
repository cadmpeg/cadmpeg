// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use crate::report::check::Finding;
use crate::transform::Transform;
use cadmpeg_core::serde_erase::json;

fn occurrence(id: &str, prototype: &str) -> Occurrence {
    Occurrence::new(
        id.try_into().unwrap(),
        prototype.try_into().unwrap(),
        Some(Transform::identity()),
    )
}

fn definition(id: &str, occurrences: &[&str]) -> ProductDefinition {
    let mut definition = ProductDefinition::new(id.try_into().unwrap(), Storage::Embedded);
    definition.occurrences = occurrences
        .iter()
        .map(|id| (*id).try_into().unwrap())
        .collect();
    definition
}

fn findings(ir: &CadIr) -> Vec<Finding> {
    let mut ir = ir.clone();
    crate::examples::add_document_root(&mut ir);
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    findings
}

fn messages(ir: &CadIr) -> Vec<(Option<String>, String)> {
    findings(ir)
        .into_iter()
        .map(|finding| (finding.entity, finding.message))
        .collect()
}

fn missing_body(ir: &mut CadIr) {
    let mut row = definition("test:model:product#definition", &[]);
    row.bodies
        .push("test:model:body#missing".try_into().unwrap());
    ir.model.product_definitions.push(row);
}

fn missing_joint(ir: &mut CadIr) {
    use crate::products::{
        AssemblyJoint, JointConnector, JointOperand, OperandContainer, PairedJointKind,
    };
    ir.model.assembly_joints.push(AssemblyJoint::paired(
        "test:model:joint#missing".try_into().unwrap(),
        PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        [
            JointConnector {
                operand: JointOperand {
                    container: OperandContainer::Occurrence {
                        occurrence: "test:model:occurrence#absent".try_into().unwrap(),
                        occurrence_path: Vec::new(),
                    },
                    object: "first".into(),
                    subelements: Vec::new(),
                },
                frame: Transform::identity(),
                detached: false,
            },
            JointConnector {
                operand: JointOperand::root("second", Vec::new()),
                frame: Transform::identity(),
                detached: false,
            },
        ],
        None,
    ));
}

/// A root that owns one occurrence of a part, which is the valid baseline.
fn assembly() -> CadIr {
    let mut ir = CadIr::empty();
    let mut root = definition("test:model:product#root", &["test:model:occurrence#part"]);
    root.storage = Storage::File {
        document: ExternalDocument::default(),
        source: None,
    };
    ir.model.product_definitions = vec![root, definition("test:model:product#part", &[])];
    ir.model.occurrences = vec![occurrence(
        "test:model:occurrence#part",
        "test:model:product#part",
    )];
    ir
}

#[test]
fn a_root_that_owns_an_occurrence_of_a_part_is_valid() {
    assert!(findings(&assembly()).is_empty());
}

#[test]
fn product_body_validation_reports_a_finding() {
    let mut ir = CadIr::empty();
    missing_body(&mut ir);
    assert!(!findings(&ir).is_empty());
}

#[test]
fn an_occurrence_names_an_existing_prototype() {
    let mut ir = assembly();
    ir.model.occurrences[0].prototype = "test:model:product#absent".try_into().unwrap();
    assert_eq!(
        messages(&ir),
        [(
            Some("test:model:occurrence#part".into()),
            "invalid occurrence prototype".into()
        )]
    );
}

#[test]
fn an_occurrence_has_exactly_one_owner() {
    let mut ir = assembly();
    ir.model.product_definitions[0].occurrences.clear();
    assert_eq!(
        messages(&ir),
        [(
            Some("test:model:occurrence#part".into()),
            "occurrence has no owning definition".into()
        )]
    );

    let mut ir = assembly();
    let mut other = definition("test:model:product#other", &["test:model:occurrence#part"]);
    other.storage = Storage::Embedded;
    ir.model.product_definitions.push(other);
    assert_eq!(
        messages(&ir),
        [(
            Some("test:model:product#other".into()),
            "occurrence has more than one owning definition".into()
        )]
    );

    let mut ir = assembly();
    ir.model.product_definitions[0]
        .occurrences
        .push("test:model:occurrence#absent".try_into().unwrap());
    assert_eq!(
        messages(&ir),
        [(
            Some("test:model:product#root".into()),
            "invalid product occurrence reference".into()
        )]
    );
}

#[test]
fn a_definition_does_not_place_itself() {
    let mut ir = assembly();
    ir.model.product_definitions[1]
        .occurrences
        .push("test:model:occurrence#back".try_into().unwrap());
    ir.model.occurrences.push(occurrence(
        "test:model:occurrence#back",
        "test:model:product#root",
    ));
    assert_eq!(
        messages(&ir),
        [
            (
                Some("test:model:product#root".into()),
                "an occurrence places the root definition".into()
            ),
            (
                Some("test:model:product#root".into()),
                "product definition is the prototype of an occurrence it owns, directly or transitively".into()
            )
        ]
    );

    let mut ir = assembly();
    ir.model.product_definitions[1]
        .occurrences
        .push("test:model:occurrence#self".try_into().unwrap());
    ir.model.occurrences.push(occurrence(
        "test:model:occurrence#self",
        "test:model:product#part",
    ));
    assert_eq!(
        messages(&ir),
        [(
            Some("test:model:product#part".into()),
            "product definition is the prototype of an occurrence it owns, directly or transitively".into()
        )]
    );
    assert!(findings(&ir)
        .iter()
        .all(|finding| finding.severity == crate::report::Severity::Error));
}

#[test]
fn a_shared_definition_placed_twice_is_no_cycle() {
    let mut ir = assembly();
    ir.model.product_definitions[0]
        .occurrences
        .push("test:model:occurrence#again".try_into().unwrap());
    ir.model.occurrences.push(occurrence(
        "test:model:occurrence#again",
        "test:model:product#part",
    ));
    assert!(findings(&ir).is_empty());
}

#[test]
fn product_joint_validation_reports_a_finding() {
    let mut ir = assembly();
    missing_joint(&mut ir);
    assert!(!findings(&ir).is_empty());
}

#[test]
fn product_validation_keeps_definition_occurrence_joint_finding_order() {
    let mut ir = CadIr::empty();
    missing_body(&mut ir);
    ir.model.occurrences.push(occurrence(
        "test:model:occurrence#bad",
        "test:model:product#absent",
    ));
    missing_joint(&mut ir);
    assert_eq!(
        messages(&ir),
        [
            (
                Some("test:model:product#definition".into()),
                "invalid product body reference".into()
            ),
            (
                Some("test:model:occurrence#bad".into()),
                "invalid occurrence prototype".into()
            ),
            (
                Some("test:model:joint#missing".into()),
                "invalid assembly joint operands, frames, or limits".into()
            ),
        ]
    );
}

#[test]
fn product_sketch_references_exist_and_have_one_definition() {
    let mut ir = CadIr::empty();
    let sketch = "test:model:sketch#one";
    ir.model.sketches.push(
        json::from_value(serde_json::json!({
            "id": sketch, "placement": {"kind": "unresolved"}, "profiles": []
        }))
        .unwrap(),
    );
    let with_sketch = |id: &str| {
        let mut row = definition(id, &[]);
        row.sketches.push(sketch.try_into().unwrap());
        row
    };
    ir.model
        .product_definitions
        .push(with_sketch("test:model:product#one"));
    assert!(findings(&ir).is_empty());
    ir.model
        .product_definitions
        .push(with_sketch("test:model:product#two"));
    assert!(findings(&ir)
        .iter()
        .any(|finding| finding.message == "sketch has multiple product definitions"));
    ir.model.product_definitions.pop();
    ir.model.sketches.clear();
    assert!(findings(&ir)
        .iter()
        .any(|finding| finding.message == "invalid product sketch reference"));
}

#[test]
fn product_sketch_reference_cannot_repeat_in_one_definition() {
    let mut ir = CadIr::empty();
    let sketch = "test:model:sketch#one";
    ir.model.sketches.push(
        json::from_value(serde_json::json!({
            "id": sketch, "placement": {"kind": "unresolved"}, "profiles": []
        }))
        .unwrap(),
    );
    let mut row = definition("test:model:product#one", &[]);
    row.sketches = vec![sketch.try_into().unwrap(); 3];
    ir.model.product_definitions.push(row);
    let found = findings(&ir);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].message, "duplicate product sketch reference");
    assert_eq!(found[0].entity.as_deref(), Some("test:model:product#one"));
    assert_eq!(found[0].severity, crate::report::Severity::Error);
}
