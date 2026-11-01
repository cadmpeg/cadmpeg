use super::{
    is_reference_relation_parameter, owned_relation_parameters, LaneIds, RelationParameters,
    RELATION_PARAMETER_ID_PROPERTY,
};
use crate::records::relation_scalars::RelationScalars;
use crate::records::{
    FeatureInputLane, FeatureInputName, FeatureInputRelationFamily, FeatureInputRelationInstance,
    FeatureInputScalar, FeatureInputScalarRole,
};
use cadmpeg_ir::features::{
    DesignParameter, Feature, FeatureDefinition, FeatureId, FeatureOperation, ParameterId,
};
use std::collections::{BTreeMap, HashMap};

pub(super) fn relation_lane() -> FeatureInputLane {
    FeatureInputLane {
        id: "lane".into(),
        configuration: None,
        native_payload: Vec::new().into(),
        classes: Vec::new(),
        names: Vec::new(),
        scalars: Vec::new(),
        relation_bindings: Vec::new(),
        relation_instances: vec![FeatureInputRelationInstance {
            sequence: 0,
            id: "relation".into(),
            parent: "lane".into(),
            ordinal: 0,
            offset: 0,
            family: FeatureInputRelationFamily::CircleDiameter,
            class_ref: "class".into(),
            feature_ref: "feature".into(),
            scalars: RelationScalars::from_refs(vec!["scalar".into()], Some("scalar".into()), None)
                .unwrap(),
            operands: Vec::new(),
        }],
        body_selections: Vec::new(),
        edge_selections: Vec::new(),
        surface_selections: Vec::new(),
        generated_surface_identities: Vec::new(),
        references: Vec::new(),
        sketch_entities: Vec::new(),
    }
}

fn project_with_policy() -> HashMap<String, Option<ParameterId>> {
    let lane = relation_lane();
    owned_relation_parameters(&[], &[], &[lane])
}

#[test]
fn relation_ownership_keeps_unmatched_driver_unowned() {
    let ownership = project_with_policy();
    assert_eq!(ownership.get("relation"), Some(&None));
}

#[test]
fn indexed_relation_lookups_match_the_linear_scans() {
    let mut state = 0x2545_f491_u64;
    let mut next = |bound: u64| {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (state >> 33) % bound
    };
    let template = relation_lane();
    let mut named_matches = 0;
    let mut relation_id_matches = 0;
    for _ in 0..40 {
        let mut lane = template.clone();
        lane.scalars = (0..12)
            .map(|ordinal| FeatureInputScalar {
                id: format!("scalar-{}", next(5)),
                parent: "lane".into(),
                feature_ref: None,
                ordinal,
                sequence: ordinal,
                offset: u64::from(ordinal),
                object_id: 1,
                name: format!("name-{}", next(5)),
                value: cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite test scalar"),
                role: FeatureInputScalarRole::Display,
                operands: Vec::new(),
                archive_class: None,
            })
            .collect();
        lane.names = (0..12)
            .map(|ordinal| FeatureInputName {
                feature_ref: None,
                id: format!("name-{}", next(5)),
                parent: "lane".into(),
                ordinal,
                sequence: ordinal,
                offset: u64::from(ordinal),
                value: format!("D{}", next(3)),
                object_id: None,
            })
            .collect();
        let ids = LaneIds::new(&lane);
        for id in (0..6).map(|index| format!("scalar-{index}")) {
            let mut last = None;
            for scalar in &lane.scalars {
                if scalar.id == id {
                    last = Some(scalar);
                }
            }
            let first = lane.scalars.iter().find(|scalar| scalar.id == id);
            assert_eq!(
                ids.first_scalar(&id).map(std::ptr::from_ref),
                first.map(std::ptr::from_ref)
            );
            assert_eq!(
                ids.last_scalar(&id).map(std::ptr::from_ref),
                last.map(std::ptr::from_ref)
            );
        }
        for id in (0..6).map(|index| format!("name-{index}")) {
            let mut last = None;
            for name in &lane.names {
                if name.id == id {
                    last = Some(name.value.as_str());
                }
            }
            let first = lane.names.iter().find(|name| name.id == id);
            assert_eq!(ids.first_name(&id), first.map(|name| name.value.as_str()));
            assert_eq!(ids.last_name(&id), last);
        }

        let features = (0..6)
            .map(|ordinal| Feature {
                product_definition: None,
                id: FeatureId::mint(format!("synthetic:test:id#feature-{}", next(4)))
                    .expect("identity grammar"),
                ordinal,
                name: None,
                suppressed: Some(false),
                dependencies: cadmpeg_ir::features::DistinctMembers::default(),
                source_properties: BTreeMap::new(),
                source_tag: None,
                source_text: None,
                source_content: cadmpeg_ir::features::FeatureContent::default(),
                needs_recompute: false,
                evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
                    FeatureDefinition::Operation(FeatureOperation::Sketch {
                        sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                    }),
                ),
                native_ref: (next(4) != 0).then(|| format!("feature-{}", next(3))),
            })
            .collect::<Vec<_>>();
        let parameters = (0..30)
            .map(|ordinal| {
                let mut properties = BTreeMap::new();
                if next(2) == 0 {
                    properties.insert(
                        cadmpeg_core::nonblank_const!(RELATION_PARAMETER_ID_PROPERTY),
                        format!("relation-{}", next(5)),
                    );
                }
                let reference = next(3) != 0;
                DesignParameter {
                    product_definition: None,
                    driving: !reference,
                    id: ParameterId::mint(format!("synthetic:test:id#parameter-{}", next(20)))
                        .expect("identity grammar"),
                    owner: (next(5) != 0).then(|| {
                        FeatureId::mint(format!("synthetic:test:id#feature-{}", next(4)))
                            .expect("identity grammar")
                    }),
                    name: format!("D{}", next(3)),
                    ordinal,
                    expression: "1".into(),
                    value: None,
                    display: None,
                    properties,
                    pmi: None,
                    native_ref: None,
                }
            })
            .collect::<Vec<_>>();
        let indexed = RelationParameters::new(&features, &parameters);
        for index in 0..6 {
            let relation = FeatureInputRelationInstance {
                id: format!("relation-{index}"),
                feature_ref: format!("feature-{}", next(4)),
                ..template.relation_instances[0].clone()
            };
            for name in ["D0", "D1", "D2", "D3"] {
                let linear = features
                    .iter()
                    .find(|feature| {
                        feature.native_ref.as_deref() == Some(relation.feature_ref.as_str())
                    })
                    .map(|owner| {
                        parameters
                            .iter()
                            .filter(|parameter| {
                                parameter.owner.as_ref() == Some(&owner.id)
                                    && parameter.name == name
                            })
                            .map(std::ptr::from_ref)
                            .collect::<Vec<_>>()
                    });
                let found = indexed.named(&relation, name).map(|parameters| {
                    parameters
                        .iter()
                        .map(|parameter| std::ptr::from_ref(*parameter))
                        .collect::<Vec<_>>()
                });
                named_matches += usize::from(linear.as_ref().is_some_and(|found| found.len() > 1));
                assert_eq!(found, linear);
            }
            let mut by_relation_id = parameters.iter().filter(|parameter| {
                parameter.properties.get(RELATION_PARAMETER_ID_PROPERTY) == Some(&relation.id)
                    && is_reference_relation_parameter(parameter)
            });
            let linear = match (by_relation_id.next(), by_relation_id.next()) {
                (Some(parameter), None) => Some(parameter),
                _ => None,
            };
            relation_id_matches += usize::from(linear.is_some());
            assert_eq!(
                indexed.by_relation_id(&relation).map(std::ptr::from_ref),
                linear.map(std::ptr::from_ref)
            );
        }
    }
    assert!(named_matches > 0);
    assert!(relation_id_matches > 0);
}
