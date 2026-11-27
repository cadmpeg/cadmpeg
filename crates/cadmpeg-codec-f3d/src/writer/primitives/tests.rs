// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::parameters::ConfigurationEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};

use super::{bases, differences, projects, same_row};
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
fn a_definition_or_dependencies_other_than_the_feature_s_make_the_configurations_differ() {
    let natives = Natives::complete();
    let targets = natives.targets();
    let feature = &natives.neutral_features[0];
    let decoded = configurations();
    for state in decoded
        .iter()
        .flat_map(|configuration| configuration.feature_states.values())
    {
        assert_eq!(state.definition, *feature.evaluation.definition());
        assert_eq!(state.dependencies, feature.dependencies);
    }
    assert!(projects(&decoded, &[document()], &targets).unwrap());

    let mut definition = decoded.clone();
    definition[0]
        .feature_states
        .get_mut(&feature.id)
        .unwrap()
        .definition = FeatureDefinition::Operation(FeatureOperation::Native {
        kind: "Revolve".into(),
        parameters: std::collections::BTreeMap::new(),
    });
    assert!(!projects(&definition, &[document()], &targets).unwrap());
    let bases = bases(&natives.neutral_features);
    assert_eq!(
        differences(&definition[0], &decoded[0], &bases)
            .iter()
            .map(|difference| difference.field.clone())
            .collect::<Vec<_>>(),
        [format!("feature_states.{}.definition", feature.id)]
    );

    let mut dependencies = decoded.clone();
    dependencies[1]
        .feature_states
        .get_mut(&feature.id)
        .unwrap()
        .dependencies = DistinctMembers::default();
    assert!(!projects(&dependencies, &[document()], &targets).unwrap());
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
    let targets = natives.targets();
    assert!(!projects(&edited, &[document()], &targets).unwrap());
    let mut reordered = decoded.clone();
    reordered[0].ordinal = 7;
    assert!(!projects(&reordered, &[document()], &targets).unwrap());
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
