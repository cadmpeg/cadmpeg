// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::products::{Occurrence, OccurrenceParent, PrototypeReference};
use crate::report::check::Finding;
use crate::transform::Transform;

fn occurrence(id: &str, parent: Option<&str>, ordinal: u32) -> Occurrence {
    Occurrence {
        id: id.try_into().unwrap(),
        prototype: PrototypeReference::Unresolved {},
        parent: parent.map_or(OccurrenceParent::Root {}, |id| {
            OccurrenceParent::Occurrence {
                occurrence: id.try_into().unwrap(),
            }
        }),
        ordinal,
        transform: Transform::identity(),
        linked_prototype: None,
        scale: [crate::scalar::FiniteReal::ONE; 3],
        name: None,
        visible: None,
        link: None,
        native_ref: None,
    }
}

fn reports(ir: &CadIr) {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::check_products(&ctx, ir, &mut findings).unwrap();
    assert!(!findings.is_empty());
}

fn missing_body(ir: &mut CadIr) {
    ir.model.product_definitions.push(serde_json::from_value(serde_json::json!({
        "id": "test:model:product#definition", "kind": "part", "bodies": ["test:model:body#missing"], "bom_properties": {}, "sketches": [], "tessellations": []
    })).unwrap());
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

#[test]
fn product_body_validation_reports_a_finding() {
    let mut ir = CadIr::empty();
    missing_body(&mut ir);
    reports(&ir);
}

#[test]
fn product_sibling_ordinal_validation_reports_a_finding() {
    let mut ir = CadIr::empty();
    ir.model.occurrences = vec![
        occurrence("test:model:occurrence#first", None, 0),
        occurrence("test:model:occurrence#second", None, 0),
    ];
    reports(&ir);
}

#[test]
fn product_auxiliary_definition_validation_reports_a_finding() {
    let mut ir = CadIr::empty();
    let mut row = occurrence("test:model:occurrence#link", None, 0);
    row.link = crate::products::LinkState::new(
        Vec::new(),
        Some("test:model:product#missing".try_into().unwrap()),
        None,
        None,
    );
    ir.model.occurrences.push(row);
    reports(&ir);
}

#[test]
fn product_copy_source_and_group_validation_reports_a_finding() {
    for source in [true, false] {
        let mut ir = CadIr::empty();
        let mut row = occurrence("test:model:occurrence#copy", None, 0);
        let reference = PrototypeReference::Local {
            definition: "test:model:product#missing".try_into().unwrap(),
        };
        row.link = crate::products::LinkState::new(
            Vec::new(),
            None,
            None,
            Some(crate::products::CopyOnChange {
                policy: crate::products::CopyOnChangePolicy::Enabled,
                source: source.then(|| reference.clone()),
                group: (!source).then_some(reference),
                touched: None,
            }),
        );
        ir.model.occurrences.push(row);
        reports(&ir);
    }
}

#[test]
fn product_joint_validation_reports_a_finding() {
    let mut ir = CadIr::empty();
    ir.model
        .occurrences
        .push(occurrence("test:model:occurrence#present", None, 0));
    missing_joint(&mut ir);
    reports(&ir);
}

#[test]
fn product_sibling_orders_distinguish_parent_groups() {
    let mut ir = CadIr::empty();
    ir.model.occurrences = vec![
        occurrence("test:model:occurrence#a", None, 0),
        occurrence("test:model:occurrence#b", None, 1),
        occurrence(
            "test:model:occurrence#first",
            Some("test:model:occurrence#a"),
            0,
        ),
        occurrence(
            "test:model:occurrence#other",
            Some("test:model:occurrence#b"),
            0,
        ),
    ];
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty(), "{findings:?}");
    ir.model.occurrences.push(occurrence(
        "test:model:occurrence#duplicate",
        Some("test:model:occurrence#a"),
        0,
    ));
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].entity.as_deref(),
        Some("test:model:occurrence#duplicate")
    );
}

#[test]
fn product_validation_keeps_definition_graph_occurrence_joint_finding_order() {
    let mut ir = CadIr::empty();
    missing_body(&mut ir);
    let mut row = occurrence(
        "test:model:occurrence#bad",
        Some("test:model:occurrence#absent"),
        0,
    );
    row.prototype = PrototypeReference::Local {
        definition: "test:model:product#absent".try_into().unwrap(),
    };
    ir.model.occurrences.push(row);
    missing_joint(&mut ir);
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings: Vec<Finding> = Vec::new();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.entity.as_deref())
            .collect::<Vec<_>>(),
        [
            Some("test:model:product#definition"),
            Some("model:assembly"),
            Some("test:model:occurrence#bad"),
            Some("test:model:joint#missing")
        ]
    );
    assert_eq!(
        findings
            .iter()
            .map(|finding| finding.message.as_str())
            .collect::<Vec<_>>(),
        [
            "invalid product body reference",
            "invalid occurrence parent graph",
            "invalid occurrence reference, ordinal, or affine transform",
            "invalid assembly joint operands, frames, or limits"
        ]
    );
}

#[test]
fn product_sketch_references_exist_and_have_one_definition() {
    let mut ir = CadIr::empty();
    let sketch = "test:model:sketch#one";
    ir.model.sketches.push(
        serde_json::from_value(serde_json::json!({
            "id": sketch, "placement": {"kind": "unresolved"}, "profiles": []
        }))
        .unwrap(),
    );
    ir.model.product_definitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "test:model:product#one", "kind": "part", "sketches": [sketch], "tessellations": [], "bom_properties": {}, "bodies": []
        }))
        .unwrap(),
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert!(findings.is_empty());
    ir.model.product_definitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "test:model:product#two", "kind": "part", "sketches": [sketch], "tessellations": [], "bom_properties": {}, "bodies": []
        }))
        .unwrap(),
    );
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.message == "sketch has multiple product definitions"));
    findings.clear();
    ir.model.product_definitions.pop();
    ir.model.sketches.clear();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert!(findings
        .iter()
        .any(|finding| finding.message == "invalid product sketch reference"));
}

#[test]
fn product_sketch_reference_cannot_repeat_in_one_definition() {
    let mut ir = CadIr::empty();
    let sketch = "test:model:sketch#one";
    ir.model.sketches.push(
        serde_json::from_value(serde_json::json!({
            "id": sketch, "placement": {"kind": "unresolved"}, "profiles": []
        }))
        .unwrap(),
    );
    ir.model.product_definitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "test:model:product#one", "kind": "part", "sketches": [sketch, sketch, sketch], "tessellations": [], "bom_properties": {}, "bodies": []
        }))
        .unwrap(),
    );
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut findings = Vec::new();
    super::super::check_products(&ctx, &ir, &mut findings).unwrap();
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].message, "duplicate product sketch reference");
    assert_eq!(
        findings[0].entity.as_deref(),
        Some("test:model:product#one")
    );
    assert_eq!(findings[0].severity, crate::report::Severity::Error);
}

/// A triangle mesh with the identity `id` that represents `body`.
fn mesh(id: &str, body: Option<crate::ids::BodyId>) -> crate::tessellation::Tessellation {
    use crate::math::Point3;
    use crate::tessellation::{Tessellation, TessellationMesh};

    Tessellation::new(
        id.try_into().expect("valid identity"),
        TessellationMesh::List {
            vertices: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
        },
        Vec::new(),
    )
    .expect("a valid mesh")
    .with_body(body)
}

/// A part definition `id` that owns the meshes `tessellations`.
fn mesh_part(id: &str, tessellations: &[&str]) -> crate::products::ProductDefinition {
    serde_json::from_value(serde_json::json!({
        "id": id, "kind": "part", "bom_properties": {}, "bodies": [], "sketches": [],
        "tessellations": tessellations
    }))
    .unwrap()
}

#[test]
fn product_tessellations_exist_represent_no_body_and_have_one_definition() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let messages = |ir: &CadIr| {
        let mut findings = Vec::new();
        super::super::check_products(&ctx, ir, &mut findings).unwrap();
        findings
            .into_iter()
            .map(|finding| finding.message)
            .collect::<Vec<_>>()
    };
    let shared = "test:model:tessellation#shared";
    let mut ir = CadIr::empty();
    ir.model.tessellations.push(mesh(shared, None));
    ir.model
        .product_definitions
        .push(mesh_part("test:model:product#one", &[shared]));
    assert!(messages(&ir).is_empty());

    ir.model
        .product_definitions
        .push(mesh_part("test:model:product#two", &[shared]));
    assert!(messages(&ir).contains(&"tessellation has multiple product definitions".to_owned()));
    ir.model.product_definitions.pop();

    ir.model.product_definitions[0] = mesh_part("test:model:product#one", &[shared, shared]);
    assert!(messages(&ir).contains(&"duplicate product tessellation reference".to_owned()));

    ir.model.product_definitions[0] = mesh_part(
        "test:model:product#one",
        &["test:model:tessellation#missing"],
    );
    assert!(messages(&ir).contains(&"invalid product tessellation reference".to_owned()));

    ir.model.tessellations[0] = mesh(
        shared,
        Some("test:model:body#solid".try_into().expect("valid identity")),
    );
    ir.model.product_definitions[0] = mesh_part("test:model:product#one", &[shared]);
    assert!(messages(&ir).contains(&"product tessellation represents a body".to_owned()));
}
