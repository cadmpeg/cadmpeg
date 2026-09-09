// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_ir::features::PatternTransform;

#[test]
fn body_pattern_adds_one_copy_per_non_original_occurrence() {
    let mut ir = complete_block_ir();
    let seed = ir.model.bodies[0].id.clone();
    let first_copy =
        BodyId::mint("test:model:entity#copy-1".to_string()).expect("identity grammar");
    let second_copy =
        BodyId::mint("test:model:entity#copy-2".to_string()).expect("identity grammar");
    ir.model.bodies.push(model_body(first_copy.as_str()));
    ir.model.bodies.push(model_body(second_copy.as_str()));
    let mut pattern = body_neutral_feature(
        "pattern",
        1,
        FeatureDefinition::Pattern {
            seeds: vec![PatternSeed::Bodies(BodySelection::Bodies(vec![
                seed.clone()
            ]))],
            pattern: PatternKind::new(PatternTransform::Linear {
                direction: Some(Vector3::new(1.0, 0.0, 0.0)),
                spacing: Length::new(2.0).unwrap(),
                count: 3,
                second: None,
            })
            .unwrap(),
        },
    );
    pattern
        .evaluation
        .set_outputs(vec![first_copy.clone(), second_copy.clone()])
        .unwrap();
    ir.model.features.push(pattern);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified {
            bodies: vec![seed, first_copy, second_copy]
        }
    );
}

#[test]
fn output_free_unresolved_pattern_is_body_census_neutral() {
    let mut ir = complete_block_ir();
    ir.model.features.push(Feature {
        id: FeatureId::mint("synthetic:test:id#pattern".to_string()).expect("identity grammar"),
        ordinal: 1,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),

        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Pattern {
                seeds: Vec::new(),
                pattern: PatternKind::UNRESOLVED,
            },
        ),
        native_ref: None,
    });

    assert!(matches!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Verified { bodies }
            if bodies == [BodyId::mint("test:model:entity#body".to_string()).expect("identity grammar")]
    ));
}

#[test]
fn body_pattern_requires_exact_copy_cardinality_and_new_identities() {
    let mut ir = complete_block_ir();
    let seed = ir.model.bodies[0].id.clone();
    ir.model.features.push(body_preserving_feature(
        "pattern",
        1,
        seed.clone(),
        FeatureDefinition::Pattern {
            seeds: vec![PatternSeed::Bodies(BodySelection::Bodies(vec![seed]))],
            pattern: PatternKind::new(PatternTransform::Mirror {
                plane_origin: Point3::new(0.0, 0.0, 0.0),
                plane_normal: Vector3::new(1.0, 0.0, 0.0),
            })
            .unwrap(),
        },
    ));

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#pattern".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("pattern".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::InvalidOutputLineage,
        }
    );
}

#[test]
fn feature_seed_pattern_remains_an_explicit_body_effect_boundary() {
    let mut ir = complete_block_ir();
    let seed = ir.model.features[0].id.clone();
    let body = ir.model.bodies[0].id.clone();
    let mut pattern = body_preserving_feature(
        "pattern",
        1,
        body,
        FeatureDefinition::Pattern {
            seeds: vec![PatternSeed::Feature(seed.clone())],
            pattern: PatternKind::new(PatternTransform::Mirror {
                plane_origin: Point3::new(0.0, 0.0, 0.0),
                plane_normal: Vector3::new(1.0, 0.0, 0.0),
            })
            .unwrap(),
        },
    );
    pattern.dependencies.insert(seed);
    ir.model.features.push(pattern);

    assert_eq!(
        evaluate_saved_body_census(&ir),
        BodyCensusEvaluation::Unsupported {
            feature: FeatureBoundary {
                id: FeatureId::mint("synthetic:test:id#pattern".to_string())
                    .expect("identity grammar"),
                name: None,
                family: Some("pattern".to_string()),
                ordinal: 1
            },
            reason: UnsupportedBodyCensusReason::UnsupportedFeatureDefinition,
        }
    );
}
