// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::{
    feature::scope::{DesignFeatureKind, DesignParameterScope},
    recipes::DesignComponentNamingSpace,
};

fn space(record: u64, uuid: &str) -> DesignComponentNamingSpace {
    DesignComponentNamingSpace {
        id: format!("f3d:Design/BulkStream.dat:component-naming-space#{record}"),
        byte_offset: 0,
        component_record_index: record,
        context_uuid: uuid.to_owned().try_into().unwrap(),
        context_uuid_offset: 0,
        name: None,
        name_offset: None,
    }
}

fn native() -> F3dNative {
    F3dNative {
        design_component_naming_spaces: vec![
            space(10, "11111111-1111-1111-1111-111111111111"),
            space(30, "22222222-2222-2222-2222-222222222222"),
        ],
        design_parameter_scopes: [20, 40]
            .into_iter()
            .map(|record| {
                DesignParameterScope::empty(
                    &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{record}"),
                    DesignFeatureKind::Extrude,
                    record,
                )
            })
            .collect(),
        ..F3dNative::default()
    }
}

fn model(native: &F3dNative) -> Model {
    let mut model = Model::default();
    for space in &native.design_component_naming_spaces {
        model.product_definitions.push(
            serde_json::from_value(serde_json::json!({
                "id": crate::ids::neutral_component_id(&space.context_uuid), "kind": "part"
            }))
            .unwrap(),
        );
    }
    for scope in &native.design_parameter_scopes {
        model.features.push(cadmpeg_ir::features::Feature {
            product_definition: None,
            id: crate::ids::neutral_feature_id(scope),
            ordinal: u64::from(scope.record_index),
            name: None,
            suppressed: None,
            dependencies: DistinctMembers::default(),
            source_properties: BTreeMap::default(),
            source_tag: None,
            source_text: None,
            source_content: cadmpeg_ir::features::FeatureContent::default(),
            evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Native {
                        kind: "synthetic".into(),
                        parameters: BTreeMap::default(),
                    },
                ),
            ),
            native_ref: Some(scope.id.clone()),
        });
    }
    model
}

#[test]
fn product_history_owner_uses_distinct_component_namespaces() {
    let native = native();
    let mut model = model(&native);
    for feature in &model.features {
        model
            .parameters
            .push(cadmpeg_ir::features::DesignParameter {
                id: format!("f3d:test:parameter#{}", feature.ordinal)
                    .try_into()
                    .unwrap(),
                product_definition: None,
                owner: Some(feature.id.clone()),
                ordinal: 0,
                name: "distance".into(),
                expression: "2".into(),
                display: None,
                value: None,
                dependencies: DistinctMembers::default(),
                properties: BTreeMap::default(),
                pmi: None,
                native_ref: None,
            });
    }
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    assert!(losses.is_empty());
    for (feature, definition) in model.features.iter().zip(&model.product_definitions) {
        assert_eq!(feature.product_definition.as_ref(), Some(&definition.id));
    }
    for (parameter, definition) in model.parameters.iter().zip(&model.product_definitions) {
        assert_eq!(parameter.product_definition.as_ref(), Some(&definition.id));
    }
}

#[test]
fn product_history_owner_reports_missing_and_multiple_definitions() {
    let mut native = native();
    let mut model = model(&native);
    model.product_definitions.remove(0);
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::ScopeOwnerUnresolved.kind());
    assert!(losses[0].message.contains("missing-definition"));
    assert!(model.features[0].product_definition.is_none());
    native
        .design_component_naming_spaces
        .push(space(30, "33333333-3333-3333-3333-333333333333"));
    model.product_definitions.push(serde_json::from_value(serde_json::json!({
        "id": crate::ids::neutral_component_id(&native.design_component_naming_spaces[2].context_uuid), "kind": "part"
    })).unwrap());
    losses.clear();
    attach(&native, &mut model, &mut losses).unwrap();
    assert!(losses
        .iter()
        .any(|loss| loss.message.contains("multiple-definitions")));
    assert!(model.features[1].product_definition.is_none());
}

#[test]
fn product_history_owner_absent_source_timeline_has_no_loss() {
    let mut losses = Vec::new();
    attach(&F3dNative::default(), &mut Model::default(), &mut losses).unwrap();
    assert!(losses.is_empty());
}

#[test]
fn product_history_owner_attaches_bound_planar_sketch_to_component() {
    use cadmpeg_ir::features::{
        FeatureDefinition, FeatureEvaluation, FeatureOperation, SketchFeatureBinding,
    };

    let native = native();
    let mut model = model(&native);
    let sketch: cadmpeg_ir::sketches::Sketch = serde_json::from_value(serde_json::json!({
        "id": "test:model:sketch#profile",
        "placement": {"kind": "unresolved"}
    }))
    .unwrap();
    model.features[0].evaluation = FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
        }),
    );
    let sketch_id = sketch.id.clone();
    model.sketches.push(sketch);
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    assert!(losses.is_empty());
    assert_eq!(model.product_definitions[0].sketches, [sketch_id]);
    assert!(model.product_definitions[1].sketches.is_empty());
}

#[test]
fn product_history_owner_without_component_namespace_keeps_document_scope() {
    let mut native = native();
    native.design_parameter_scopes = vec![DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#5",
        DesignFeatureKind::Extrude,
        5,
    )];
    let mut model = model(&native);
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    assert!(model.features[0].product_definition.is_none());
    assert!(losses.is_empty());
}

#[test]
fn product_history_owner_uses_parameter_scope_before_parameter_record() {
    use crate::records::parameters::{
        DesignParameter, DesignParameterDraft, DesignParameterOwner, DesignParameterOwnerWire,
        DesignParameterSource,
    };

    let mut native = native();
    let mut model = model(&native);
    model.features.clear();
    let parameter = DesignParameter::try_from(DesignParameterDraft {
        id: "f3d:Design/BulkStream.dat:design-parameter#50".into(),
        byte_offset: 0,
        class_tag: "301".to_owned().try_into().unwrap(),
        record_index: 50,
        source_ordinal: 0,
        source: DesignParameterSource::new("AlongDistance".into(), Some(51), None).unwrap(),
        expression: "2".into(),
        expression_offset: 40,
        source_kind_offset: 60,
        unit: None,
        name: "distance".into(),
        name_offset: 80,
        evaluated_value: 2.0,
        evaluated_value_offset: 90,
    })
    .unwrap();
    native.design_parameter_owners.push(
        DesignParameterOwner::try_from(DesignParameterOwnerWire {
            id: "f3d:Design/BulkStream.dat:design-parameter-owner#51".into(),
            byte_offset: 0,
            frame_length: 99,
            class_tag: "302".to_owned().try_into().unwrap(),
            record_index: 51,
            scope_record_index: 20,
            local_ordinal: 0,
            evaluated_value: 2.0,
            evaluated_value_offset: 40,
            parameter_record_index: 50,
            owned_ordinal: 0,
            variant: None,
            companion_record_index: 52,
        })
        .unwrap(),
    );
    model
        .parameters
        .push(cadmpeg_ir::features::DesignParameter {
            id: crate::ids::neutral_parameter_id(&parameter),
            product_definition: None,
            owner: None,
            ordinal: 0,
            name: "distance".into(),
            expression: "2".into(),
            display: None,
            value: None,
            dependencies: DistinctMembers::default(),
            properties: BTreeMap::default(),
            pmi: None,
            native_ref: Some(parameter.id.clone()),
        });
    native.design_parameters.push(parameter);
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    assert!(losses.is_empty());
    assert_eq!(
        model.parameters[0].product_definition.as_ref(),
        Some(&model.product_definitions[0].id)
    );
}

#[test]
fn product_history_owner_refuses_outputs_from_another_definition() {
    let native = native();
    let mut model = model(&native);
    let owned: cadmpeg_ir::ids::BodyId = "test:model:body#owned".try_into().unwrap();
    let foreign: cadmpeg_ir::ids::BodyId = "test:model:body#foreign".try_into().unwrap();
    model.product_definitions[0].bodies = vec![owned.clone()];
    model.product_definitions[1].bodies = vec![foreign.clone()];
    model.features[0]
        .evaluation
        .set_outputs(DistinctMembers::try_from(vec![owned.clone(), foreign]).unwrap());
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    assert_eq!(model.features[0].evaluation.outputs(), &[owned]);
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::ScopeOutputOwnerMismatch.kind());
}

#[test]
fn product_history_owner_follows_the_one_definition_holding_every_output() {
    let native = native();
    let mut model = model(&native);
    let body: cadmpeg_ir::ids::BodyId = "test:model:body#child".try_into().unwrap();
    model.product_definitions[1].bodies = vec![body.clone()];
    model.features[0]
        .evaluation
        .set_outputs(DistinctMembers::try_from(vec![body.clone()]).unwrap());
    let holder = model.product_definitions[1].id.clone();
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    assert_eq!(model.features[0].product_definition, Some(holder));
    assert_eq!(model.features[0].evaluation.outputs(), &[body]);
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn product_history_owner_keeps_conflicting_sketch_binding_native() {
    let native = native();
    let mut model = model(&native);
    let sketch: cadmpeg_ir::sketches::Sketch = serde_json::from_value(serde_json::json!({
        "id": "test:model:sketch#profile",
        "placement": {"kind": "unresolved"}
    }))
    .unwrap();
    model.product_definitions[1]
        .sketches
        .push(sketch.id.clone());
    model.features[0].evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
        }),
    );
    let sketch_id = sketch.id.clone();
    model.sketches.push(sketch);
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    assert!(model.product_definitions[0].sketches.is_empty());
    assert_eq!(model.product_definitions[1].sketches, [sketch_id]);
    assert!(matches!(
        model.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(None),
        })
    ));
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::ScopeSketchOwnerMismatch.kind());
}

#[test]
fn product_history_owner_without_member_definition_is_released() {
    let native = native();
    let mut model = model(&native);
    let member_root = crate::ids::archive_product_definition_id();
    model.features[0].product_definition = Some(member_root);
    let kept = model.product_definitions[1].id.clone();
    model.features[1].product_definition = Some(kept.clone());
    let definitions = model.product_definitions.clone();
    release_unbound(&mut model);
    assert_eq!(model.features[0].product_definition, None);
    assert_eq!(model.features[1].product_definition, Some(kept));
    assert_eq!(model.product_definitions, definitions);
}

fn sketch_feature(native: &F3dNative, name: &str) -> cadmpeg_ir::features::Feature {
    let sketch: cadmpeg_ir::sketches::SketchId =
        format!("test:model:sketch#{name}").try_into().unwrap();
    let mut row = model(native).features.remove(0);
    row.id = format!("test:model:feature#{name}").try_into().unwrap();
    row.evaluation = cadmpeg_ir::features::FeatureEvaluation::from_definition(
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: SketchFeatureBinding::Planar(Some(sketch)),
        }),
    );
    row
}

#[test]
fn product_history_owner_moves_a_consumed_sketch_with_its_feature() {
    let native = native();
    let mut model = model(&native);
    let body: cadmpeg_ir::ids::BodyId = "test:model:body#child".try_into().unwrap();
    model.product_definitions[1].bodies = vec![body.clone()];
    let sketch = sketch_feature(&native, "profile");
    let sketch_id = sketch.id.clone();
    model.features.push(sketch);
    model.features[0]
        .evaluation
        .set_outputs(DistinctMembers::try_from(vec![body]).unwrap());
    model.features[0].dependencies = DistinctMembers::try_from(vec![sketch_id.clone()]).unwrap();
    let holder = model.product_definitions[1].id.clone();
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    let moved = model
        .features
        .iter()
        .find(|row| row.id == sketch_id)
        .unwrap();
    assert_eq!(moved.product_definition, Some(holder));
    assert_eq!(
        model.product_definitions[1].sketches,
        ["test:model:sketch#profile".try_into().unwrap()]
    );
    assert!(model.product_definitions[0].sketches.is_empty());
}

#[test]
fn product_history_owner_keeps_a_sketch_still_consumed_by_its_old_definition() {
    let native = native();
    let mut model = model(&native);
    let body: cadmpeg_ir::ids::BodyId = "test:model:body#child".try_into().unwrap();
    model.product_definitions[1].bodies = vec![body.clone()];
    let sketch = sketch_feature(&native, "shared");
    let sketch_id = sketch.id.clone();
    model.features.push(sketch);
    let mut stay = model.features[0].clone();
    stay.id = "test:model:feature#stay".try_into().unwrap();
    stay.dependencies = DistinctMembers::try_from(vec![sketch_id.clone()]).unwrap();
    model.features.push(stay);
    model.features[0]
        .evaluation
        .set_outputs(DistinctMembers::try_from(vec![body]).unwrap());
    model.features[0].dependencies = DistinctMembers::try_from(vec![sketch_id.clone()]).unwrap();
    let old = model.product_definitions[0].id.clone();
    let mut losses = Vec::new();
    attach(&native, &mut model, &mut losses).unwrap();
    let kept = model
        .features
        .iter()
        .find(|row| row.id == sketch_id)
        .unwrap();
    assert_eq!(kept.product_definition, Some(old));
}
