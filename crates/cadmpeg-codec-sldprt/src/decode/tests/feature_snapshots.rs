// SPDX-License-Identifier: Apache-2.0

use crate::decode::snapshot_active_configuration;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::patterns::{
    CompositePattern, PatternKind, PatternSeed, PatternStage, PatternTransform,
};
use cadmpeg_ir::features::{
    ConfigurationEvaluation, ConfigurationId, DesignConfiguration, DistinctMembers, Feature,
    FeatureContent, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
};
use cadmpeg_ir::scalar::Length;
use std::collections::BTreeMap;

fn run(policy: &DecodePolicy) -> Result<(), CodecError> {
    let mut ir = cadmpeg_ir::CadIr::empty();
    let dependency = FeatureId::mint("synthetic:test:id#snapshot-dependency").unwrap();
    let stages = vec![
        PatternStage {
            pattern: Box::new(
                PatternKind::new(PatternTransform::LinearOffsets {
                    direction: None,
                    offsets: vec![Length::new(0.0).unwrap(), Length::new(2.0).unwrap()],
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
                "Snapshot parameter text".into(),
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
            id: FeatureId::mint(format!("synthetic:test:id#snapshot-{ordinal}")).unwrap(),
            ordinal,
            name: None,
            suppressed: Some(false),
            dependencies: cadmpeg_ir::features::DistinctMembers::try_from(
                vec![dependency.clone()],
                &cadmpeg_test_support::service_decode_context(),
            )
            .unwrap(),
            source_properties: BTreeMap::new(),
            source_tag: None,
            source_text: None,
            source_content: FeatureContent::default(),
            evaluation: FeatureEvaluation::new(
                definition,
                cadmpeg_ir::features::DistinctMembers::try_from(
                    vec![cadmpeg_ir::ids::BodyId::mint(format!(
                        "synthetic:test:id#snapshot-body-{ordinal}"
                    ))
                    .unwrap()],
                    &cadmpeg_test_support::service_decode_context(),
                )
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
    let features = ir.model.features.clone();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    snapshot_active_configuration(&ctx, &mut ir)?;
    assert_eq!(ir.model.features, features);
    assert_eq!(
        ir.model.configurations[0].feature_states.len(),
        features.len()
    );
    for feature in features {
        let state = &ir.model.configurations[0].feature_states[&feature.id];
        assert_eq!(&state.definition, feature.evaluation.definition());
        assert_eq!(state.dependencies, feature.dependencies);
        let outputs: DistinctMembers<cadmpeg_ir::ids::BodyId> =
            feature.evaluation.outputs().iter().cloned().collect();
        assert_eq!(
            state.evaluation,
            ConfigurationEvaluation::Active { outputs }
        );
    }
    Ok(())
}

fn assert_refusal(dimension: ResourceDimension) {
    let set_limit = |policy: &mut DecodePolicy, limit| match dimension {
        ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
        ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
        ResourceDimension::WorkUnits => policy.limits.max_work_units = limit,
        _ => panic!("unexpected snapshot limit"),
    };
    run(&DecodePolicy::service()).unwrap();
    let mut lower = 0;
    let mut upper = 1_u64;
    loop {
        let mut policy = DecodePolicy::service();
        set_limit(&mut policy, upper);
        match run(&policy) {
            Ok(()) => break,
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, dimension);
                upper = upper.checked_mul(2).unwrap();
            }
            Err(error) => panic!("unexpected snapshot error: {error}"),
        }
    }
    while lower < upper {
        let midpoint = lower + (upper - lower) / 2;
        let mut policy = DecodePolicy::service();
        set_limit(&mut policy, midpoint);
        match run(&policy) {
            Ok(()) => upper = midpoint,
            Err(CodecError::ResourceLimit(limit)) => {
                assert_eq!(limit.dimension, dimension);
                lower = midpoint + 1;
            }
            Err(error) => panic!("unexpected snapshot error: {error}"),
        }
    }
    assert!(upper > 0);
    let mut policy = DecodePolicy::service();
    set_limit(&mut policy, upper);
    run(&policy).unwrap();
    set_limit(&mut policy, upper - 1);
    assert!(
        matches!(run(&policy), Err(CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
    );
}

#[test]
fn active_feature_snapshot_refuses_collection_limit() {
    assert_refusal(ResourceDimension::CollectionItems);
}
#[test]
fn active_feature_snapshot_refuses_retained_limit() {
    assert_refusal(ResourceDimension::RetainedBytes);
}
#[test]
fn active_feature_snapshot_refuses_work_limit() {
    assert_refusal(ResourceDimension::WorkUnits);
}
