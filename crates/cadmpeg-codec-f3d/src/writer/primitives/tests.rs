// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::parameters::ConfigurationEvaluation;
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{FeatureDefinition, FeatureId, FeatureOperation};

use super::{same_configurations, SourceConfigurations, StoredStates};
use crate::design::configurations::project;
use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::test_support::configuration_table::{bool_cell, object_target, table_document, Natives};

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

/// The feature states that the rows of [`document`] store: each row stores
/// the state of the feature of scope 314.
fn stored() -> StoredStates {
    StoredStates::of(&[document()], Some((0, 0)), &Natives::complete().targets())
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
    assert!(same_configurations(
        &edited,
        &decoded,
        &stored(),
        &SourceConfigurations::new(None)
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
    assert!(!same_configurations(
        &edited,
        &decoded,
        &stored(),
        &SourceConfigurations::new(None)
    ));
    let mut reordered = decoded.clone();
    reordered[0].ordinal = 7;
    assert!(!same_configurations(
        &reordered,
        &decoded,
        &stored(),
        &SourceConfigurations::new(None)
    ));
}

#[test]
fn a_part_no_cell_holds_is_an_edit_where_the_source_states_it_otherwise() {
    let mut decoded = configurations();
    decoded[0].active = true;
    let mut edited = decoded.clone();
    let state = edited[0].feature_states.values().next().unwrap().clone();
    let other = FeatureId::mint("f3d:model:feature#other").unwrap();
    edited[0]
        .feature_states
        .insert(other.clone(), state.clone());
    edited[0].properties.insert(
        cadmpeg_core::text::NonBlankString::new(format!("suppression_inferred:{other}")).unwrap(),
        "true".to_owned(),
    );
    let same = |source: &SourceConfigurations<'_>| {
        same_configurations(&edited, &decoded, &stored(), source)
    };
    assert!(same(&SourceConfigurations::decoded(&edited)));
    assert!(!same(&SourceConfigurations::decoded(&decoded)));
    assert!(!same(&SourceConfigurations::new(None)));
}
