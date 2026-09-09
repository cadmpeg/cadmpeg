// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

#[cfg(feature = "schema")]
use crate::examples::unit_cube;
use crate::products::{ProductDefinition, ProductDefinitionKind};
use crate::report::Check;
use crate::validate::validate_neutral;
use crate::CadIr;

#[test]
fn typed_reference_walk_ignores_id_shaped_plain_strings() {
    let mut ir = crate::CadIr::empty();
    let owner =
        crate::ids::ProductDefinitionId::mint("test:model:product#owner").expect("valid identity");
    let target = crate::ids::BodyId::mint("test:model:body#missing").expect("valid identity");
    ir.model.product_definitions.push(ProductDefinition {
        id: owner.clone(),
        kind: ProductDefinitionKind::Part,
        source_name: Some("test:model:name#not-a-reference".into()),
        label: None,
        description: None,
        part_number: None,
        bom_properties: std::collections::BTreeMap::new(),
        bodies: vec![target.clone()],
        native_ref: None,
    });

    let mut references = Vec::new();
    ir.model
        .visit_references(&mut |reference| references.push(reference.target));
    assert_eq!(references, vec![target.as_str().to_owned()]);

    let report = validate_neutral(&ir, Vec::new());
    assert!(report.findings.iter().any(|finding| {
        finding.check == Check::ReferentialIntegrity
            && finding.entity.as_deref() == Some(owner.as_str())
            && finding.message.contains(target.as_str())
    }));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.message.contains("not-a-reference")));
}

#[test]
fn typed_reference_walk_treats_historical_members_as_state_local() {
    use crate::features::{
        EdgeSelection, Feature, FeatureDefinition, FeatureId, FeatureInputTopology, FilletGroup,
        RadiusSpec,
    };
    use crate::ids::{FeatureInputTopologyId, HistoricalEdgeId};
    use crate::schema::EntitySchema;

    let feature_id = FeatureId::mint("test:model:feature#owner").expect("identity grammar");
    let state_id =
        FeatureInputTopologyId::mint("test:model:feature-input#owner").expect("valid identity");
    let historical_edge =
        HistoricalEdgeId::mint("test:model:historical-edge#local").expect("valid identity");
    let state = FeatureInputTopology {
        id: state_id.clone(),
        input_of: feature_id.clone(),
        bodies: (Vec::new()).try_into().unwrap(),
        faces: (Vec::new()).try_into().unwrap(),
        edges: (vec![historical_edge.clone()]).try_into().unwrap(),
        vertices: (Vec::new()).try_into().unwrap(),
        native_ref: None,
    };
    let feature = Feature {
        id: feature_id.clone(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: crate::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: crate::features::FeatureContent::default(),

        evaluation: crate::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Fillet {
                groups: crate::features::NonEmptyMembers::one(FilletGroup {
                    edges: EdgeSelection::historical(
                        state_id.clone(),
                        vec![historical_edge],
                        "edge:local".into(),
                    )
                    .unwrap(),
                    radius: RadiusSpec::Constant {
                        radius: crate::scalar::PositiveLength::new(1.0).unwrap(),
                    },
                    tangency_weight: None,
                }),
            },
        ),
        native_ref: None,
    };

    let mut state_references = Vec::new();
    state.visit_references(&mut |reference| state_references.push(reference.target));
    assert_eq!(state_references, vec![feature_id.as_str().to_owned()]);

    let mut feature_references = Vec::new();
    feature.visit_references(&mut |reference| feature_references.push(reference.target));
    assert_eq!(feature_references, vec![state_id.as_str()]);

    let mut ir = CadIr::empty();
    ir.model.feature_input_topologies.push(state);
    ir.model.features.push(feature);
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ReferentialIntegrity));

    let missing = "test:model:historical-edge#missing";
    ir.model.features[0]
        .evaluation
        .try_edit(|definition, _| {
            let FeatureDefinition::Fillet { groups } = definition else {
                unreachable!("test feature is a fillet")
            };
            let EdgeSelection::Historical { edges, .. } = &mut groups[0].edges else {
                unreachable!("test fillet uses a historical selection")
            };
            *edges = vec![HistoricalEdgeId::mint(missing).expect("valid identity")]
                .try_into()
                .unwrap();
        })
        .unwrap();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| {
            finding.check == Check::ReferentialIntegrity
                && finding.entity.as_deref() == Some(feature_id.as_str())
                && finding.message == format!("references missing historical edge `{missing}`")
        }));
}

#[cfg(feature = "schema")]
#[test]
fn schema_constrains_version_and_requires_subd_arena() {
    let schema = serde_json::to_value(crate::cadir_json_schema()).unwrap();
    assert_eq!(
        schema.pointer("/properties/ir_version/const"),
        Some(&serde_json::json!(crate::IR_VERSION))
    );
    assert!(schema
        .pointer("/properties/model/$ref")
        .and_then(serde_json::Value::as_str)
        .is_some());

    let model_schema = schema.pointer("/$defs/Model").unwrap();
    assert!(model_schema
        .pointer("/required")
        .and_then(serde_json::Value::as_array)
        .unwrap()
        .contains(&serde_json::json!("subds")));
    assert!(schema.pointer("/properties/byte_ledger").is_none());

    let mut value = serde_json::to_value(unit_cube()).unwrap();
    value
        .pointer_mut("/model")
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("subds");
    assert!(serde_json::from_value::<CadIr>(value).is_err());
}

#[cfg(feature = "schema")]
#[test]
fn schema_generation_produces_definitions() {
    let schema = crate::cadir_json_schema();
    let json = serde_json::to_string(&schema).unwrap();
    assert!(json.contains("Body"));
    assert!(json.contains("Coedge"));
    assert!(json.contains("SurfaceGeometry"));
    let defs = schema
        .get("$defs")
        .and_then(serde_json::Value::as_object)
        .expect("schema has a $defs object");
    assert!(!defs.is_empty());
}
