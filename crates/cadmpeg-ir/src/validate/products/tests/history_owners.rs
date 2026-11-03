// SPDX-License-Identifier: Apache-2.0

use crate::document::CadIr;
use crate::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation,
    FeatureOperation,
};
use crate::report::check::Finding;

fn model() -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.bodies.push(crate::topology::Body {
        id: "test:model:body#result".try_into().unwrap(),
        kind: crate::topology::BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.product_definitions.push(
        serde_json::from_value(serde_json::json!({
            "id": "test:model:product#owner", "kind": "part", "bodies": ["test:model:body#result"]
        }))
        .unwrap(),
    );
    let mut evaluation = FeatureEvaluation::from_definition(FeatureDefinition::Operation(
        FeatureOperation::Native {
            kind: "synthetic".into(),
            parameters: std::collections::BTreeMap::default(),
        },
    ));
    evaluation.set_outputs(DistinctMembers::try_from(vec![ir.model.bodies[0].id.clone()]).unwrap());
    ir.model.features.push(Feature {
        id: "test:model:feature#operation".try_into().unwrap(),
        product_definition: Some(ir.model.product_definitions[0].id.clone()),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation,
        native_ref: None,
    });
    ir
}

fn findings(ir: &CadIr) -> Vec<Finding> {
    let mut findings = Vec::new();
    super::super::check_products(
        &cadmpeg_test_support::service_decode_context(),
        ir,
        &mut findings,
    )
    .unwrap();
    findings
}

#[test]
fn product_history_owner_accepts_owned_outputs_and_absent_links() {
    let mut ir = model();
    assert!(findings(&ir).is_empty());
    let json = serde_json::to_value(&ir.model.features[0]).unwrap();
    assert_eq!(json["product_definition"], "test:model:product#owner");
    let read: Feature = serde_json::from_value(json).unwrap();
    assert_eq!(
        read.product_definition,
        ir.model.features[0].product_definition
    );
    let wire = serde_json::to_value(&ir.model).unwrap();
    let restored: crate::document::Model = serde_json::from_value(wire).unwrap();
    assert_eq!(
        restored.features[0].product_definition,
        ir.model.features[0].product_definition
    );
    ir.model.features[0].product_definition = None;
    assert!(findings(&ir).is_empty());
}

#[test]
fn product_history_owner_rejects_missing_definition() {
    let mut ir = model();
    ir.model.features[0].product_definition =
        Some("test:model:product#missing".try_into().unwrap());
    assert_eq!(findings(&ir)[0].message, "invalid product definition owner");
}

#[test]
fn product_history_owner_rejects_inconsistent_output_owner() {
    let mut ir = model();
    ir.model.product_definitions[0].bodies.clear();
    assert_eq!(
        findings(&ir)[0].message,
        "feature output is not owned by its product definition"
    );
}

#[test]
fn product_history_owner_validates_parameter_definition() {
    let mut ir = model();
    ir.model.parameters.push(crate::features::DesignParameter {
        id: "test:model:parameter#distance".try_into().unwrap(),
        product_definition: Some(ir.model.product_definitions[0].id.clone()),
        owner: None,
        ordinal: 0,
        name: "distance".into(),
        expression: "2".into(),
        display: None,
        value: None,
        dependencies: crate::features::DistinctMembers::default(),
        properties: std::collections::BTreeMap::default(),
        pmi: None,
        native_ref: None,
    });
    assert!(findings(&ir).is_empty());
    ir.model.parameters[0].product_definition =
        Some("test:model:product#missing".try_into().unwrap());
    assert_eq!(findings(&ir)[0].message, "invalid product definition owner");
}
