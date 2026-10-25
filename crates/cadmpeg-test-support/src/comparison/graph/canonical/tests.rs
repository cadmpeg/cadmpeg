// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::document::Model;
use cadmpeg_ir::features::{
    DistinctMembers, Feature, FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId,
    FeatureOperation,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::topology::{Body, BodyKind};
use serde_json::{json, Value};
use std::collections::BTreeSet;

fn pair() -> (Value, Value, crate::comparison::Allowances) {
    let body = BodyId::mint("test:model:body#lost").unwrap();
    let mut model = Model::default();
    model.bodies.push(Body {
        id: body.clone(),
        kind: BodyKind::General,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    });
    model.features.push(Feature {
        id: FeatureId::mint("test:model:feature#kept").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: DistinctMembers::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::StoredGeometry {}),
            DistinctMembers::try_from(vec![body.clone()]).unwrap(),
        ),
    });
    let before = json!({"model": model, "native": {}});
    model.bodies.clear();
    model.features[0]
        .evaluation
        .set_outputs(DistinctMembers::default());
    let after = json!({"model": model, "native": {}});
    (
        before,
        after,
        crate::comparison::Allowances {
            removed: BTreeSet::from([body.as_str().to_owned()]),
            ..Default::default()
        },
    )
}

#[test]
fn last_output_removal_uses_the_actual_feature_serde_omission() {
    let (before, after, allowances) = pair();
    assert!(before["model"]["features"][0].get("outputs").is_some());
    assert!(after["model"]["features"][0].get("outputs").is_none());
    crate::comparison::agree(&before, &after, &allowances, crate::comparison::Mode::Same).unwrap();
}

#[test]
fn unrelated_optional_empty_keys_and_native_fields_remain_exact() {
    let (mut before, after, allowances) = pair();
    before["model"]["features"][0]["source_properties"] = json!({});
    assert!(
        crate::comparison::agree(&before, &after, &allowances, crate::comparison::Mode::Same)
            .is_err()
    );

    let (mut before, mut after, allowances) = pair();
    before["native"] =
        json!({"test":{"rows":[{"id":"test:native:row#kept","empty":[],"value":7}]}});
    after["native"] = json!({"test":{"rows":[{"id":"test:native:row#kept","value":7}]}});
    assert!(
        crate::comparison::agree(&before, &after, &allowances, crate::comparison::Mode::Same)
            .is_err()
    );
    after["native"]["test"]["rows"][0]["empty"] = json!([]);
    after["native"]["test"]["rows"][0]["bonus"] = json!(true);
    assert!(
        crate::comparison::agree(&before, &after, &allowances, crate::comparison::Mode::Same)
            .is_err()
    );
}
