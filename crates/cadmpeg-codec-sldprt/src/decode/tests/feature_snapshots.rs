// SPDX-License-Identifier: Apache-2.0

use crate::history::configuration::normalize_configuration_differences;
use cadmpeg_ir::features::patterns::{
    CompositePattern, PatternKind, PatternSeed, PatternStage, PatternTransform,
};
use cadmpeg_ir::features::{
    ConfigurationId, DesignConfiguration, Feature, FeatureContent, FeatureDefinition,
    FeatureEvaluation, FeatureId, FeatureOperation,
};
use cadmpeg_ir::scalar::Length;
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
use std::collections::BTreeMap;

fn run() {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let dependency = FeatureId::mint("synthetic:test:id#snapshot-dependency").unwrap();
    let stages = vec![
        PatternStage {
            pattern: Box::new(
                PatternKind::new(PatternTransform::LinearOffsets {
                    direction: None,
                    offsets: vec![
                        cadmpeg_ir::Dimension::Literal(Length::new(0.0).unwrap()),
                        Length::new(2.0).unwrap().into(),
                    ],
                })
                .unwrap(),
            ),
        },
        PatternStage {
            pattern: Box::new(
                PatternKind::new(PatternTransform::MirrorReference {
                    plane: cadmpeg_ir::features::FaceSelection::Native(
                        "Native mirror plane".into(),
                    ),
                })
                .unwrap(),
            ),
        },
    ];
    let definitions = [
        FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Native snapshot operation".into(),
            parameters: BTreeMap::from([(
                cadmpeg_core::nonblank_literal!("Snapshot parameter"),
                cadmpeg_ir::Dimension::Literal("Snapshot parameter text".into()),
            )]),
        }),
        FeatureDefinition::Operation(FeatureOperation::Pattern {
            seeds: vec![PatternSeed::Bodies(
                cadmpeg_ir::features::BodySelection::Native("Snapshot seed bodies".into()),
            )],
            pattern: PatternKind::new(PatternTransform::Composite {
                stages: CompositePattern::new(stages).unwrap(),
            })
            .unwrap(),
        }),
    ];
    for (ordinal, definition) in (0_u64..).zip(definitions) {
        ir.model.features.push(Feature {
            product_definition: None,
            id: FeatureId::mint(format!("synthetic:test:id#snapshot-{ordinal}")).unwrap(),
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(vec![dependency.clone()])
                .unwrap(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            needs_recompute: false,
            evaluation: FeatureEvaluation::new(
                definition,
                cadmpeg_ir::features::DistinctMembers::try_from(vec![
                    cadmpeg_ir::ids::BodyId::mint(format!(
                        "synthetic:test:id#snapshot-body-{ordinal}"
                    ))
                    .unwrap(),
                ])
                .unwrap(),
            ),
            native_ref: None,
        });
    }
    ir.model.configurations.push(DesignConfiguration {
        id: ConfigurationId::mint("synthetic:test:id#snapshot-configuration").unwrap(),
        ordinal: 0,
        active: true,
        source_index: Some(0),
        name: Some("Snapshot configuration".into()),
        material: None,
        properties: BTreeMap::new(),
        bodies: None,
        parameter_values: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        native_ref: None,
    });
    let original = ir.clone();
    let mut features = ir.model.features.clone();
    let ctx = cadmpeg_test_support::service_decode_context();
    for feature in &mut features {
        assert_eq!(
            feature.dependencies.as_slice(),
            std::slice::from_ref(&dependency)
        );
        let mut references = Vec::new();
        feature
            .definition()
            .visit_identity_references(&ctx, &mut |reference| {
                references.push(reference.to_owned());
                Ok(())
            })
            .unwrap();
        assert!(!references
            .iter()
            .any(|reference| reference == dependency.as_str()));
        // D26: the input's unrelated dangling dependency is not a definition
        // reference. Only this derived list changes; all other state stays exact.
        feature.dependencies.clear();
    }
    normalize_configuration_differences(&ctx, &mut ir).unwrap();
    assert_eq!(ir.model.features, features);
    assert!(ir.model.configurations[0].feature_states.is_empty());
    assert!(ir.model.configurations[0].parameter_values.is_empty());
    assert!(ir.model.configurations[0].parameter_overrides.is_empty());

    // A separate authored control has an actual feature seed reference. The
    // refresh must retain this edge rather than clearing every dependency.
    let mut referenced = original;
    let seed = referenced.model.features[0].id.clone();
    referenced.model.features[1]
        .evaluation
        .edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition
            else {
                panic!("pattern definition");
            };
            *seeds = vec![PatternSeed::Feature(seed.clone())];
        });
    let mut expected = referenced.model.features.clone();
    expected[0].dependencies.clear();
    expected[1].dependencies = cadmpeg_ir::features::DistinctMembers::try_from(vec![seed]).unwrap();
    normalize_configuration_differences(&ctx, &mut referenced).unwrap();
    assert_eq!(referenced.model.features, expected);
    assert!(referenced.model.configurations[0].feature_states.is_empty());
    assert!(referenced.model.configurations[0]
        .parameter_values
        .is_empty());
    assert!(referenced.model.configurations[0]
        .parameter_overrides
        .is_empty());
}

#[test]
fn active_configuration_does_not_copy_feature_states() {
    run();
}
