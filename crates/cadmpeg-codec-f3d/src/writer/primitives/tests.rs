// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::parameters::ConfigurationEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};

use super::{bases, differences, same_configurations, same_row};
use crate::design::configurations::project;
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::test_support::configuration_table::{
    bool_cell, neutral_feature, object_target, scope, table_document, Natives,
};

/// A table whose one column names the feature of scope 314, with a
/// suppressed row and a computed row.
fn document() -> DesignConfiguration {
    let document = table_document(
        "1",
        &[("F", object_target("314", "DcFeature"))],
        &[
            ("off", "Off", vec![("F", bool_cell(false))]),
            ("on", "On", vec![("F", bool_cell(true))]),
        ],
        &["off", "on"],
    );
    DesignConfiguration::try_new(
        "FusionAssetName[Active]/DesignConfigurationTable.f.dsgcfg".into(),
        DesignConfigurationKind::Table,
        document.as_object().unwrap().clone(),
    )
    .unwrap()
}

/// The configurations of [`document`].
fn configurations() -> Vec<cadmpeg_ir::features::parameters::DesignConfiguration> {
    project(&[document()], &Natives::complete().targets())
        .unwrap()
        .configurations
}

#[test]
fn a_feature_edit_leaves_the_configurations_the_same() {
    let decoded = configurations();
    let mut edited = decoded.clone();
    for state in edited
        .iter_mut()
        .flat_map(|configuration| configuration.feature_states.values_mut())
    {
        state.definition = FeatureDefinition::Operation(FeatureOperation::Native {
            kind: "Revolve".into(),
            parameters: std::collections::BTreeMap::new(),
        });
        state.dependencies = DistinctMembers::default();
    }
    assert_ne!(edited, decoded);
    let natives = Natives::complete();
    assert!(same_configurations(
        &edited,
        &decoded,
        &bases(&natives.neutral_features)
    ));
}

#[test]
fn a_configuration_edit_makes_the_configurations_differ() {
    let decoded = configurations();
    let mut edited = decoded.clone();
    let state = edited[0].feature_states.values_mut().next().unwrap();
    state.evaluation = match state.evaluation {
        ConfigurationEvaluation::Suppressed {} => ConfigurationEvaluation::Active {
            outputs: DistinctMembers::default(),
        },
        ConfigurationEvaluation::Active { .. } => ConfigurationEvaluation::Suppressed {},
    };
    let natives = Natives::complete();
    let bases = bases(&natives.neutral_features);
    assert!(!same_configurations(&edited, &decoded, &bases));
    let mut reordered = decoded.clone();
    reordered[0].ordinal = 7;
    assert!(!same_configurations(&reordered, &decoded, &bases));
}

#[test]
fn a_feature_state_a_configuration_does_not_state_is_the_feature_s_own_state() {
    let mut features = Natives::complete().neutral_features;
    let mut suppressed = neutral_feature(&scope(2), "f3d:test:feature#a");
    suppressed.suppressed = Some(true);
    features.push(suppressed.clone());
    let bases = bases(&features);
    let stated = configurations()[0].clone();

    let mut explicit = stated.clone();
    explicit
        .feature_states
        .insert(suppressed.id.clone(), suppressed.configuration_state());
    assert!(same_row(&explicit, &stated, &bases));

    let mut active = explicit.clone();
    active
        .feature_states
        .get_mut(&suppressed.id)
        .unwrap()
        .evaluation = ConfigurationEvaluation::Active {
        outputs: DistinctMembers::default(),
    };
    assert_eq!(
        differences(&active, &stated, &bases)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [format!(
            "feature_states.{} stated {{\"kind\":\"active\",\"outputs\":[]}}, reads back {{\"kind\":\"suppressed\"}}",
            suppressed.id
        )]
    );

    let unknown = FeatureId::mint("f3d:model:feature#unknown").unwrap();
    let mut stray = stated.clone();
    stray
        .feature_states
        .insert(unknown.clone(), suppressed.configuration_state());
    assert_eq!(
        differences(&stray, &stated, &bases)
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>(),
        [format!(
            "feature_states.{unknown} stated {{\"kind\":\"suppressed\"}}, reads back absent"
        )]
    );

    let mut inferred = explicit;
    inferred.properties.insert(
        NonBlankString::new(format!("suppression_inferred:{}", suppressed.id)).unwrap(),
        "true".to_owned(),
    );
    assert!(same_row(&inferred, &stated, &bases));
}
