// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;
use std::collections::BTreeSet;

use cadmpeg_core::json;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, DesignConfiguration as Configuration, ParameterValue,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{ConfigurationId, Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::scalar::Length;

use cadmpeg_ir::design::scope::DesignScope;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::ProductDefinitionId;
use cadmpeg_ir::report::loss::LossNote;

use super::{evaluated, regenerate, row_id, Written};
use crate::design::configurations::{project, reconcile_active_configurations, Projection};
use crate::ids::neutral_configuration_id;
use crate::records::configuration::table::ConfiguredValue;
use crate::records::configuration::{
    DesignConfiguration, DesignConfigurationActiveRow, DesignConfigurationKind,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopeTailFlags,
};
use crate::test_support::configuration_table::{
    bool_cell, expression_cell, neutral_feature, object_target, property_target, scope,
    string_cell, table_document, Natives,
};
use crate::writer::primitives::projects;

/// The write of `configurations` over `documents`, which project as
/// `decoded`, in a document whose design scope states `scoped` active: the
/// written documents, and the losses of the table and of its read-back with
/// the row that the root record names active.
fn write_scoped<'a>(
    documents: &'a [DesignConfiguration],
    decoded: &Projection,
    configurations: &'a [Configuration],
    natives: &Natives,
    scoped: Option<&ConfigurationId>,
) -> (Written<'a, 'a>, Vec<LossNote>) {
    let targets = natives.targets();
    let (written, mut losses) = regenerate(documents, decoded, configurations, &targets).unwrap();
    let mut ir = CadIr::empty();
    ir.design.scopes.extend(scoped.map(|scoped| DesignScope {
        definition: ProductDefinitionId::mint("test:design:product#0").unwrap(),
        roots: Vec::new(),
        active_configuration: Some(scoped.clone()),
        active_arrangements: std::collections::BTreeSet::new(),
        occurrences: std::collections::BTreeMap::new(),
        current: Vec::new(),
        unproduced: std::collections::BTreeSet::new(),
    }));
    let plan = super::super::configuration_root::plan(
        &ir,
        documents,
        decoded.table,
        &written,
        configurations,
    );
    losses.extend(
        written
            .compare_in_memory(plan.active(), &targets, &natives.producers)
            .unwrap(),
    );
    (written, losses)
}

/// [`write_scoped`] in a document with no design scope.
fn write<'a>(
    documents: &'a [DesignConfiguration],
    decoded: &Projection,
    configurations: &'a [Configuration],
    natives: &Natives,
) -> (Written<'a, 'a>, Vec<LossNote>) {
    write_scoped(documents, decoded, configurations, natives, None)
}

const ENTRY: &str =
    "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.w.dsgcfg";
const RULE: &str =
    "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationRule.w.dsgcfgrule";

/// [`Natives::complete`] with a length value for each neutral parameter.
fn natives() -> Natives {
    let mut natives = Natives::complete();
    for parameter in &mut natives.neutral_parameters {
        parameter.value = Some(ParameterValue::Length(Length::new(10.0).unwrap()));
    }
    natives
}

/// A table document whose columns name a property, parameter 202, the
/// parameter of owner 330 and the feature of scope 314, with rows `tall` and
/// `c1` in the order `c1`, `tall`; and a rule that activates `Tall`.
fn documents() -> Vec<DesignConfiguration> {
    let row = |id, name, height, computed| {
        (
            id,
            name,
            vec![
                ("P", string_cell(name)),
                ("H", expression_cell(height, 3.5)),
                ("D", expression_cell("10.00 mm", 1.0)),
                ("F", bool_cell(computed)),
            ],
        )
    };
    let table = table_document(
        "1",
        &[
            ("P", property_target("Part Number")),
            ("H", object_target("202", "UserParameterValue")),
            ("D", object_target("330", "ParameterDoubleValue")),
            ("F", object_target("314", "DcFeature")),
        ],
        &[
            row("tall", "Tall", "35 mm", false),
            row("c1", "Configuration 1", "25 mm", true),
        ],
        &["c1", "tall"],
    );
    vec![
        DesignConfiguration::try_new(
            ENTRY.into(),
            DesignConfigurationKind::Table,
            table.as_object().unwrap().clone(),
        )
        .unwrap(),
        DesignConfiguration::try_new(
            RULE.into(),
            DesignConfigurationKind::Rule,
            json!({"when": "Width > 1", "activate": "Tall"})
                .as_object()
                .unwrap()
                .clone(),
        )
        .unwrap(),
    ]
}

fn id(row: &str) -> ConfigurationId {
    neutral_configuration_id(ENTRY, row)
}

fn rule_key() -> String {
    format!("activation_rule:{RULE}")
}

/// The write of `documents` with the configurations they project, edited
/// by `edit`.
struct Outcome {
    /// Whether the write holds `documents` as stated.
    unchanged: bool,
    documents: Vec<DesignConfiguration>,
    /// The code and message of each loss.
    losses: Vec<(String, String)>,
    /// The configurations the written documents read back as.
    read: Vec<Configuration>,
    /// The edited configurations.
    configurations: Vec<Configuration>,
}

fn regenerated(
    natives: &Natives,
    documents: &[DesignConfiguration],
    edit: impl FnOnce(&mut Vec<Configuration>),
) -> Outcome {
    let decoded = project(documents, &natives.targets()).unwrap();
    let mut configurations = decoded.configurations.clone();
    edit(&mut configurations);
    let (written, losses) = write(documents, &decoded, &configurations, natives);
    Outcome {
        unchanged: matches!(written.documents, Cow::Borrowed(_)),
        documents: written.documents.into_owned(),
        losses: losses
            .iter()
            .map(|loss| (loss.code.to_string(), loss.message.clone()))
            .collect(),
        read: written.projection.configurations,
        configurations,
    }
}

fn configuration<'c>(configurations: &'c mut [Configuration], row: &str) -> &'c mut Configuration {
    configurations
        .iter_mut()
        .find(|configuration| configuration.id == id(row))
        .unwrap()
}

/// The ids of the rows of the written table, in `rowOrder` order.
fn rows(outcome: &Outcome) -> Vec<&str> {
    outcome.documents[0].tables()[0]
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect()
}

#[test]
fn an_unedited_projection_writes_every_document_as_stated() {
    let outcome = regenerated(&natives(), &documents(), |_| {});
    assert!(outcome.unchanged);
    assert_eq!(outcome.losses, Vec::new());
    assert_eq!(outcome.read, outcome.configurations);
    assert_eq!(outcome.read[1].properties[rule_key().as_str()], "Width > 1");
}

#[test]
fn a_definition_the_table_cannot_hold_is_charged_in_each_configuration() {
    let natives = natives();
    let feature = natives.neutral_features[0].id.clone();
    let outcome = regenerated(&natives, &documents(), |configurations| {
        for state in configurations
            .iter_mut()
            .flat_map(|configuration| configuration.feature_states.values_mut())
        {
            state.definition = FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Revolve".into(),
                parameters: std::collections::BTreeMap::new(),
            });
            state.dependencies = DistinctMembers::default();
        }
    });
    assert!(outcome.unchanged);
    assert_eq!(outcome.losses.len(), 1, "{:#?}", outcome.losses);
    let (code, message) = &outcome.losses[0];
    assert_eq!(code, "export/edit.dropped");
    for row in ["tall", "c1"] {
        assert!(
            message.contains(&format!(
                "{}: feature_states.{feature}.definition stated ",
                id(row)
            )),
            "{message}"
        );
    }
    assert!(message.contains("Revolve"), "{message}");
    assert!(
        message.contains(&format!(
            "feature_states.{feature}.dependencies stated [], reads back [\""
        )),
        "{message}"
    );
}

#[test]
fn a_decoded_definition_equal_to_the_feature_s_is_not_charged() {
    let mut natives = natives();
    let other = neutral_feature(&scope(2), "f3d:test:feature#a");
    natives.neutral_features.push(other.clone());
    let feature = natives.neutral_features[0].clone();
    let decoded = project(&documents(), &natives.targets()).unwrap();
    for configuration in &decoded.configurations {
        let state = &configuration.feature_states[&feature.id];
        assert_eq!(state.definition, *feature.evaluation.definition());
        assert_eq!(state.dependencies, feature.dependencies);
    }
    let outcome = regenerated(&natives, &documents(), |configurations| {
        configuration(configurations, "tall")
            .feature_states
            .insert(other.id.clone(), other.configuration_state());
    });
    assert!(outcome.unchanged);
    assert_eq!(outcome.losses, Vec::new());
}

/// Feature 0 of [`natives`] as [`State`] states it, and the losses of a
/// write of [`documents`], with row `c1` active in the source, whose active
/// row suppresses that feature: `tall` made active by the configurations
/// (`Moved`) or by the design scope (`Scoped`), or `c1` edited to suppress
/// it (`Edited`).
fn active_row_suppression(state: State, change: Change) -> (Feature, Vec<(String, String)>) {
    let mut natives = natives();
    match state {
        State::Undecided => natives.neutral_features[0].suppressed = None,
        State::Sketch => {
            natives.neutral_features[0].suppressed = Some(false);
            natives.neutral_features[0]
                .evaluation
                .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                }));
        }
        State::Decided => natives.neutral_features[0].suppressed = Some(false),
    }
    if !matches!(state, State::Undecided) {
        store_active_feature_state(&mut natives.scopes[0]);
    }
    let feature = natives.neutral_features[0].clone();
    let mut documents = documents();
    documents[0]
        .set_active_row(DesignConfigurationActiveRow {
            record_index: 349,
            table_id: "1".into(),
            row_id: "c1".into(),
        })
        .unwrap();
    let decoded = reconciled(&natives, &documents);
    let mut configurations = decoded.configurations.clone();
    let mut scoped = None;
    match change {
        Change::Moved => {
            configuration(&mut configurations, "c1").active = false;
            configuration(&mut configurations, "tall").active = true;
        }
        Change::Scoped => scoped = Some(id("tall")),
        Change::Edited => {
            configuration(&mut configurations, "c1")
                .feature_states
                .get_mut(&feature.id)
                .unwrap()
                .evaluation = ConfigurationEvaluation::Suppressed {};
        }
    }
    let (_, losses) = write_scoped(
        &documents,
        &decoded,
        &configurations,
        &natives,
        scoped.as_ref(),
    );
    (
        feature,
        losses
            .iter()
            .map(|loss| (loss.code.to_string(), loss.message.clone()))
            .collect(),
    )
}

/// How [`active_row_suppression`] makes the active row suppress the feature.
#[derive(Debug, Clone, Copy)]
enum Change {
    Moved,
    Scoped,
    Edited,
}

/// The current state of the feature of [`active_row_suppression`]: a
/// suppression of `None`, a Sketch that is not suppressed, or another feature
/// that is not suppressed. A stored state decides a Sketch as it decides
/// every feature.
#[derive(Debug, Clone, Copy)]
enum State {
    Undecided,
    Sketch,
    Decided,
}

#[test]
fn a_feature_the_written_active_row_suppresses_is_charged() {
    for state in [State::Undecided, State::Sketch, State::Decided] {
        for change in [Change::Moved, Change::Scoped, Change::Edited] {
            let (feature, losses) = active_row_suppression(state, change);
            assert_eq!(losses.len(), 1, "{state:?} {change:?}: {losses:#?}");
            let (code, message) = &losses[0];
            assert_eq!(code, "export/value.unwritable");
            let decided = !matches!(state, State::Undecided);
            let row = if decided {
                "the active row states"
            } else {
                "reads back"
            };
            assert!(
                message.contains(&format!(
                    "{} stated {{\"kind\":\"active\",\"outputs\":[]}}, {row} {{\"kind\":\"suppressed\"}}",
                    feature.id
                )),
                "{state:?} {change:?}: {losses:#?}"
            );
            assert_eq!(
                message.contains("states these features otherwise than the document"),
                decided,
                "{state:?} {change:?}: {message}"
            );
        }
    }
}

#[test]
fn a_feature_the_decoder_suppressed_with_the_active_row_is_not_charged() {
    let mut natives = natives();
    natives.neutral_features[0].suppressed = None;
    let mut documents = documents();
    documents[0]
        .set_active_row(DesignConfigurationActiveRow {
            record_index: 349,
            table_id: "1".into(),
            row_id: "tall".into(),
        })
        .unwrap();
    let mut decoded = project(&documents, &natives.targets()).unwrap();
    let undecided = BTreeSet::from([natives.neutral_features[0].id.clone()]);
    reconcile_active_configurations(
        &mut natives.neutral_features,
        &mut decoded.configurations,
        &undecided,
    )
    .unwrap();
    assert_eq!(natives.neutral_features[0].suppressed, Some(true));
    let (written, losses) = write(&documents, &decoded, &decoded.configurations, &natives);
    assert!(matches!(written.documents, Cow::Borrowed(_)));
    assert!(losses.is_empty(), "{losses:#?}");
}

/// The Base Feature of column `F`, whose scope stores no suppression and
/// which feature projection infers active, decoded under active row `tall`,
/// which suppresses it: the decoder gives it the row's state, and an
/// unedited write is not charged.
#[test]
fn an_unedited_feature_whose_state_the_active_row_supplies_is_not_charged() {
    let mut natives = natives();
    natives.scopes[0] =
        DesignParameterScope::empty(&natives.scopes[0].id, DesignFeatureKind::BaseFeature, 314);
    natives.neutral_features[0] = neutral_feature(&natives.scopes[0], "f3d:model:feature#sketch");
    assert_eq!(natives.neutral_features[0].suppressed, Some(false));
    let documents = active_in_source("tall");
    let configurations = natives.attach(&documents);
    assert_eq!(natives.neutral_features[0].suppressed, Some(true));
    let decoded = Projection {
        configurations,
        ..project(&documents, &natives.targets()).unwrap()
    };
    let (unchanged, losses) = charged(&natives, &documents, &decoded, &decoded.configurations);
    assert!(unchanged);
    assert!(losses.is_empty(), "{losses:#?}");
}

/// The Base Feature of column `F`, whose scope stores state 8 after state 7
/// and no suppression, decoded under active row `tall`, which suppresses it.
/// The body history binds the feature an output body that no product
/// definition holds: the decoder keeps the feature active and drops the
/// output link. An unedited write charges the row's contradiction and states
/// when a feature reads back in the row's state.
#[test]
fn a_producer_the_active_row_suppresses_is_charged_with_the_condition_of_the_read_back() {
    let mut natives = natives();
    natives.scopes[0] =
        DesignParameterScope::empty(&natives.scopes[0].id, DesignFeatureKind::BaseFeature, 314);
    natives.scopes[0]
        .try_edit(|draft| {
            draft.history_state_id = Some(8);
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut producer = neutral_feature(&natives.scopes[0], "f3d:model:feature#sketch");
    producer.native_ref = Some(natives.scopes[0].id.clone());
    producer.evaluation.set_outputs(
        DistinctMembers::try_from(vec![cadmpeg_ir::ids::BodyId::mint(
            "test:model:body#unowned",
        )
        .unwrap()])
        .unwrap(),
    );
    natives.neutral_features[0] = producer;
    let documents = active_in_source("tall");
    let configurations = natives.attach(&documents);
    let feature = &natives.neutral_features[0];
    assert!(feature.evaluation.outputs().is_empty());
    assert_eq!(feature.suppressed, Some(false));
    let decoded = Projection {
        configurations,
        ..project(&documents, &natives.targets()).unwrap()
    };
    let (unchanged, losses) = charged(&natives, &documents, &decoded, &decoded.configurations);
    assert!(unchanged);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(
        losses[0].1.contains(
            "the Design stream stores the state of its active row, so a feature whose scope stores no suppression and to which the body history binds no output body reads back in the row's state"
        ),
        "{losses:#?}"
    );
    assert!(
        losses[0].1.contains(&format!(
            "{} stated {{\"kind\":\"active\",\"outputs\":[]}}, the active row states {{\"kind\":\"suppressed\"}}",
            feature.id
        )),
        "{losses:#?}"
    );
}

/// The feature of column `F`, an Extrude and then a Sketch, whose scope
/// tail stores an active state, under row `tall`, which suppresses it. A
/// write that keeps that stored state and the cell of the source's active row
/// is not charged: the source holds the same contradiction. A write that
/// makes `tall` active over source row `c1`, which computes the feature, is.
#[test]
fn a_stored_state_the_source_row_contradicts_is_charged_only_where_the_write_changes_the_row() {
    for sketch in [false, true] {
        let mut natives = natives();
        store_active_feature_state(&mut natives.scopes[0]);
        if sketch {
            natives.neutral_features[0]
                .evaluation
                .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                    sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
                }));
        }
        let feature = natives.neutral_features[0].id.clone();

        let tall = active_in_source("tall");
        let configurations = natives.attach(&tall);
        assert_eq!(natives.neutral_features[0].suppressed, Some(false));
        let decoded = Projection {
            configurations,
            ..project(&tall, &natives.targets()).unwrap()
        };
        let (unchanged, losses) = charged(&natives, &tall, &decoded, &decoded.configurations);
        assert!(unchanged);
        assert!(losses.is_empty(), "sketch {sketch}: {losses:#?}");

        let c1 = active_in_source("c1");
        let decoded = reconciled(&natives, &c1);
        let mut moved = decoded.configurations.clone();
        configuration(&mut moved, "c1").active = false;
        configuration(&mut moved, "tall").active = true;
        let (_, losses) = charged(&natives, &c1, &decoded, &moved);
        assert_eq!(losses.len(), 1, "sketch {sketch}: {losses:#?}");
        assert_eq!(losses[0].0, "export/value.unwritable");
        assert!(
            losses[0].1.contains(&format!(
                "{feature} stated {{\"kind\":\"active\",\"outputs\":[]}}, the active row states {{\"kind\":\"suppressed\"}}"
            )),
            "sketch {sketch}: {losses:#?}"
        );
    }
}

fn store_active_feature_state(scope: &mut DesignParameterScope) {
    let offset = scope.feature_ordinal_offset();
    scope.tail_flags = Some(DesignScopeTailFlags {
        suppression: [false, false],
        unevaluated: false,
        wide_state: None,
        state_offset: offset + 1,
        no_result: false,
        rolled_back: false,
        no_result_offset: offset + 4,
        envelope_offset: None,
    });
}

/// The table documents of [`documents`] with `row` active.
fn active_in_source(row: &str) -> Vec<DesignConfiguration> {
    let mut documents = documents();
    documents[0]
        .set_active_row(DesignConfigurationActiveRow {
            record_index: 349,
            table_id: "1".into(),
            row_id: row.into(),
        })
        .unwrap();
    documents
}

#[test]
fn a_feature_edited_against_an_unchanged_active_row_is_charged_as_a_moved_row() {
    let mut stored = natives();
    stored.neutral_features[0].suppressed = Some(true);
    let edited = natives();
    let feature = edited.neutral_features[0].id.clone();

    let tall = active_in_source("tall");
    let decoded = reconciled(&stored, &tall);
    let configurations = decoded.configurations.clone();
    let (unchanged, losses) = charged(&stored, &tall, &decoded, &configurations);
    assert!(unchanged);
    assert!(losses.is_empty(), "{losses:#?}");

    let (unchanged, edited_losses) = charged(&edited, &tall, &decoded, &configurations);
    assert!(unchanged);
    assert_eq!(edited_losses.len(), 1, "{edited_losses:#?}");
    assert_eq!(edited_losses[0].0, "export/value.unwritable");
    assert!(
        edited_losses[0].1.contains(&format!(
            "{feature} stated {{\"kind\":\"active\",\"outputs\":[]}}, reads back {{\"kind\":\"suppressed\"}}"
        )),
        "{edited_losses:#?}"
    );

    let c1 = active_in_source("c1");
    let decoded = reconciled(&edited, &c1);
    let mut moved = decoded.configurations.clone();
    configuration(&mut moved, "c1").active = false;
    configuration(&mut moved, "tall").active = true;
    assert_eq!(moved, configurations);
    let (_, moved_losses) = charged(&edited, &c1, &decoded, &moved);
    assert_eq!(moved_losses, edited_losses);
}

#[test]
fn a_changed_cell_is_written_from_the_configuration() {
    let natives = natives();
    let hh = natives.neutral_parameters[0].id.clone();
    let feature = natives.neutral_features[0].id.clone();
    let outcome = regenerated(&natives, &documents(), |configurations| {
        let tall = configuration(configurations, "tall");
        tall.properties.insert(
            NonBlankString::new("property:Part Number").unwrap(),
            "T-2".to_owned(),
        );
        tall.parameter_overrides.insert(hh.clone(), "40 mm".into());
        tall.parameter_values.insert(
            hh.clone(),
            ParameterValue::Length(Length::new(40.0).unwrap()),
        );
        tall.feature_states.get_mut(&feature).unwrap().evaluation =
            ConfigurationEvaluation::Active {
                outputs: DistinctMembers::default(),
            };
    });
    assert!(!outcome.unchanged);
    assert_eq!(outcome.losses, Vec::new());
    assert_eq!(outcome.read, outcome.configurations);
    let tall = &outcome.documents[0].tables()[0].rows[1];
    let height = tall
        .cells
        .iter()
        .find(|cell| cell.column_id == "H")
        .unwrap();
    assert!(matches!(
        &height.value,
        ConfiguredValue::Parameter { expression, value, .. }
            if expression == "40 mm" && value.get() == 4.0
    ));
}

#[test]
fn an_added_row_and_a_removed_row_are_written_from_the_configurations() {
    let outcome = regenerated(&natives(), &documents(), |configurations| {
        configurations.retain(|configuration| configuration.id != id("c1"));
        let tall = configuration(configurations, "tall");
        tall.ordinal = 0;
        let mut wide = tall.clone();
        wide.id = id("wide");
        wide.name = Some("Wide".into());
        wide.ordinal = 1;
        wide.properties.remove(rule_key().as_str());
        configurations.push(wide);
    });
    assert_eq!(outcome.losses, Vec::new());
    assert_eq!(rows(&outcome), ["tall", "wide"]);
    let mut expected = outcome.configurations.clone();
    expected.sort_by(|left, right| left.id.cmp(&right.id));
    assert_eq!(outcome.read, expected);
    let stored = outcome.documents[0].payload()["tables"][0]["rows"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(stored, 2);
}

#[test]
fn an_edit_the_table_holds_no_cell_for_is_charged_dropped() {
    let outcome = regenerated(&natives(), &documents(), |configurations| {
        let tall = configuration(configurations, "tall");
        tall.material = Some("Steel".into());
        tall.properties
            .insert(NonBlankString::new("note").unwrap(), "unstored".to_owned());
        tall.properties.insert(
            NonBlankString::new("property:Part Number").unwrap(),
            "T-2".to_owned(),
        );
    });
    assert_eq!(outcome.losses.len(), 1, "{:#?}", outcome.losses);
    let (code, message) = &outcome.losses[0];
    assert_eq!(code, "export/edit.dropped");
    assert!(message.contains(&format!(
        "{}: material stated \"Steel\", reads back absent; properties.note stated \"unstored\", reads back absent",
        id("tall")
    )));
    let read = outcome
        .read
        .iter()
        .find(|configuration| configuration.id == id("tall"))
        .unwrap();
    assert_eq!(read.properties["property:Part Number"], "T-2");
}

#[test]
fn a_renamed_configuration_keeps_its_rule() {
    let outcome = regenerated(&natives(), &documents(), |configurations| {
        configuration(configurations, "tall").name = Some("Towering".into());
    });
    assert_eq!(outcome.losses, Vec::new());
    assert_eq!(outcome.read, outcome.configurations);
    assert_eq!(outcome.documents[1].rule().unwrap()["activate"], "Towering");
    assert_eq!(outcome.documents[1].rule().unwrap()["when"], "Width > 1");
}

#[test]
fn a_rule_whose_row_name_is_not_unique_is_charged_with_the_rule_and_the_configuration() {
    let outcome = regenerated(&natives(), &documents(), |configurations| {
        configuration(configurations, "c1").name = Some("Tall".into());
    });
    assert_eq!(outcome.losses.len(), 1, "{:#?}", outcome.losses);
    let (code, message) = &outcome.losses[0];
    assert_eq!(code, "export/edit.dropped");
    assert!(message.contains(&format!("{}: properties.{}", id("tall"), rule_key())));
}

#[test]
fn a_removed_rule_condition_activates_nothing() {
    let outcome = regenerated(&natives(), &documents(), |configurations| {
        configuration(configurations, "tall")
            .properties
            .remove(rule_key().as_str());
    });
    assert_eq!(outcome.losses, Vec::new());
    assert_eq!(outcome.read, outcome.configurations);
    assert_eq!(
        *outcome.documents[1].rule().unwrap(),
        serde_json::Map::new()
    );
}

#[test]
fn an_added_configuration_whose_id_names_no_row_is_written_under_a_generated_row_id() {
    let natives = natives();
    let documents = documents();
    let decoded = project(&documents, &natives.targets()).unwrap();
    let mut configurations = decoded.configurations.clone();
    let mut added = configurations[0].clone();
    added.id = ConfigurationId::mint("test:configuration:variant#added").unwrap();
    added.name = Some("Added".into());
    added.ordinal = 2;
    added.properties.clear();
    configurations.push(added);
    let (written, losses) = write(&documents, &decoded, &configurations, &natives);
    let (row, configuration) = written.generated.iter().next().unwrap();
    assert_eq!(configuration, "test:configuration:variant#added");
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].code.to_string(), "export/value.unwritable");
    assert!(losses[0]
        .message
        .contains(&format!("test:configuration:variant#added as {row}")));
    let read = written
        .projection
        .configurations
        .iter()
        .find(|configuration| configuration.id.as_str() == row)
        .unwrap();
    assert_eq!(read.name.as_deref(), Some("Added"));
    assert_eq!(read.ordinal, 2);
}

#[test]
fn ordinals_the_row_order_cannot_hold_are_charged() {
    let outcome = regenerated(&natives(), &documents(), |configurations| {
        configuration(configurations, "c1").ordinal = 5;
        configuration(configurations, "tall").ordinal = 2;
    });
    assert_eq!(rows(&outcome), ["tall", "c1"]);
    assert_eq!(outcome.losses.len(), 1, "{:#?}", outcome.losses);
    let (code, message) = &outcome.losses[0];
    assert_eq!(code, "export/value.unwritable");
    assert!(message.contains(&format!("{} at 0", id("tall"))));
    assert!(message.contains(&format!("{} at 1", id("c1"))));
}

#[test]
fn configurations_with_an_empty_table_document_are_written_into_a_new_table() {
    let natives = natives();
    let documents = documents();
    let decoded = project(&documents, &natives.targets()).unwrap();
    let mut configurations = decoded.configurations;
    for configuration in &mut configurations {
        configuration
            .properties
            .retain(|key, _| key.as_str().starts_with("property:"));
    }
    assert!(configurations.iter().all(|configuration| configuration
        .properties
        .contains_key("property:Part Number")));
    let empty = vec![DesignConfiguration::try_new(
        ENTRY.into(),
        DesignConfigurationKind::Table,
        serde_json::Map::new(),
    )
    .unwrap()];
    let (written, losses) = write(
        &empty,
        &crate::design::configurations::Projection::default(),
        &configurations,
        &natives,
    );
    assert!(!losses.is_empty());
    for loss in &losses {
        assert_eq!(
            loss.code.to_string(),
            "export/record.residual-unavailable",
            "{losses:#?}"
        );
        assert!(loss.message.contains("DR-1069"), "{losses:#?}");
    }
    let tables = written.documents[0].tables();
    assert_eq!(tables.len(), 1);
    let mut rows = tables[0]
        .rows
        .iter()
        .map(|row| row.id.as_str())
        .collect::<Vec<_>>();
    rows.sort_unstable();
    assert_eq!(rows, ["c1", "tall"]);
    assert!(projects(&configurations, &written.documents, &natives.targets()).unwrap());
    for configuration in &configurations {
        let read = written
            .projection
            .configurations
            .iter()
            .find(|read| read.id == configuration.id)
            .unwrap();
        assert_eq!(read.parameter_overrides, configuration.parameter_overrides);
        assert_eq!(read.properties, configuration.properties);
    }
}

#[test]
fn an_inferred_state_of_an_unedited_active_row_is_not_charged() {
    let mut natives = natives();
    natives.neutral_features[0].suppressed = Some(true);
    let mut suppressed = neutral_feature(&scope(2), "f3d:test:feature#a");
    suppressed.suppressed = Some(true);
    natives.neutral_features.push(suppressed.clone());
    let mut documents = documents();
    documents[0]
        .set_active_row(DesignConfigurationActiveRow {
            record_index: 349,
            table_id: "1".into(),
            row_id: "tall".into(),
        })
        .unwrap();
    let mut decoded = project(&documents, &natives.targets()).unwrap();
    reconcile_active_configurations(
        &mut natives.neutral_features.clone(),
        &mut decoded.configurations,
        &BTreeSet::new(),
    )
    .unwrap();
    let tall = decoded
        .configurations
        .iter()
        .find(|configuration| configuration.id == id("tall"))
        .unwrap();
    assert!(tall.feature_states.contains_key(&suppressed.id));
    let mut configurations = decoded.configurations.clone();
    let (written, losses) = write(&documents, &decoded, &configurations, &natives);
    assert!(matches!(written.documents, Cow::Borrowed(_)));
    assert!(losses.is_empty(), "{losses:#?}");

    configuration(&mut configurations, "tall")
        .properties
        .insert(
            NonBlankString::new("property:Part Number").unwrap(),
            "T-2".to_owned(),
        );
    let (_, losses) = write(&documents, &decoded, &configurations, &natives);
    assert!(losses.is_empty(), "{losses:#?}");
}

/// The table documents of [`documents`] with row `tall` active, the
/// natives with the feature of column `F` suppressed as that row states it
/// and a suppressed feature that no column names, and the configurations
/// the decoder states for them.
fn tall_active() -> (
    Natives,
    Vec<DesignConfiguration>,
    Feature,
    Vec<Configuration>,
) {
    let mut natives = natives();
    natives.neutral_features[0].suppressed = Some(true);
    let mut suppressed = neutral_feature(&scope(2), "f3d:test:feature#a");
    suppressed.suppressed = Some(true);
    natives.neutral_features.push(suppressed.clone());
    let mut documents = documents();
    documents[0]
        .set_active_row(DesignConfigurationActiveRow {
            record_index: 349,
            table_id: "1".into(),
            row_id: "tall".into(),
        })
        .unwrap();
    let configurations = reconciled(&natives, &documents).configurations;
    (natives, documents, suppressed, configurations)
}

fn reconciled(
    natives: &Natives,
    documents: &[DesignConfiguration],
) -> crate::design::configurations::Projection {
    let mut decoded = project(documents, &natives.targets()).unwrap();
    reconcile_active_configurations(
        &mut natives.neutral_features.clone(),
        &mut decoded.configurations,
        &BTreeSet::new(),
    )
    .unwrap();
    decoded
}

/// The losses of a write of `documents`, which project as `decoded`, with
/// `configurations`.
fn charged(
    natives: &Natives,
    documents: &[DesignConfiguration],
    decoded: &crate::design::configurations::Projection,
    configurations: &[Configuration],
) -> (bool, Vec<(String, String)>) {
    let (written, losses) = write(documents, decoded, configurations, natives);
    (
        matches!(written.documents, Cow::Borrowed(_)),
        losses
            .iter()
            .map(|loss| (loss.code.to_string(), loss.message.clone()))
            .collect(),
    )
}

#[test]
fn a_state_the_features_no_longer_hold_is_charged_in_any_configuration() {
    let (mut natives, documents, suppressed, mut configurations) = tall_active();
    assert!(configuration(&mut configurations, "tall")
        .feature_states
        .contains_key(&suppressed.id));
    natives.neutral_features.last_mut().unwrap().suppressed = Some(false);
    let decoded = reconciled(&natives, &documents);
    let held = format!(
        "{}: feature_states.{} stated {{\"kind\":\"suppressed\"}}, reads back {{\"kind\":\"active\",\"outputs\":[]}}",
        id("tall"),
        suppressed.id
    );

    let (unchanged, losses) = charged(&natives, &documents, &decoded, &configurations);
    assert!(unchanged);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/edit.dropped");
    assert!(losses[0].1.contains(&held), "{losses:#?}");

    configuration(&mut configurations, "tall").active = false;
    configuration(&mut configurations, "c1").active = true;
    natives.neutral_features[0].suppressed = Some(false);
    let (unchanged, losses) = charged(&natives, &documents, &decoded, &configurations);
    assert!(unchanged);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/edit.dropped");
    assert!(losses[0].1.contains(&held), "{losses:#?}");
    assert!(!losses[0].1.contains(id("c1").as_str()), "{losses:#?}");
}

#[test]
fn moving_the_active_row_charges_the_feature_state_the_row_changes() {
    let (mut natives, documents, _, mut configurations) = tall_active();
    let decoded = reconciled(&natives, &documents);
    configuration(&mut configurations, "tall").active = false;
    configuration(&mut configurations, "c1").active = true;
    let feature = natives.neutral_features[0].id.clone();

    let (unchanged, losses) = charged(&natives, &documents, &decoded, &configurations);
    assert!(unchanged);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/value.unwritable");
    assert!(
        losses[0].1.contains(&format!(
            "{feature} stated {{\"kind\":\"suppressed\"}}, reads back {{\"kind\":\"active\""
        )),
        "{losses:#?}"
    );

    natives.neutral_features[0].suppressed = Some(false);
    let (unchanged, losses) = charged(&natives, &documents, &decoded, &configurations);
    assert!(unchanged);
    assert!(losses.is_empty(), "{losses:#?}");
}

#[test]
fn a_state_no_cell_holds_is_charged_where_it_resolves_otherwise() {
    let (natives, documents, suppressed, stated) = tall_active();
    let decoded = reconciled(&natives, &documents);

    let mut removed = stated.clone();
    configuration(&mut removed, "tall")
        .feature_states
        .remove(&suppressed.id);
    let (_, losses) = charged(&natives, &documents, &decoded, &removed);
    assert!(losses.is_empty(), "{losses:#?}");

    let mut active = stated.clone();
    configuration(&mut active, "c1").feature_states.insert(
        suppressed.id.clone(),
        cadmpeg_ir::features::parameters::ConfigurationFeatureState {
            evaluation: ConfigurationEvaluation::Active {
                outputs: DistinctMembers::default(),
            },
            ..suppressed.configuration_state()
        },
    );
    let (_, losses) = charged(&natives, &documents, &decoded, &active);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/edit.dropped");
    assert!(losses[0].1.contains(&format!(
        "{}: feature_states.{} stated {{\"kind\":\"active\",\"outputs\":[]}}, reads back {{\"kind\":\"suppressed\"}}",
        id("c1"),
        suppressed.id
    )));

    let other = neutral_feature(&scope(3), "f3d:test:feature#b");
    let mut added = stated.clone();
    configuration(&mut added, "c1")
        .feature_states
        .insert(other.id.clone(), suppressed.configuration_state());
    let (_, losses) = charged(&natives, &documents, &decoded, &added);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/edit.dropped");
    assert!(losses[0].1.contains(&format!(
        "{}: feature_states.{} stated {{\"kind\":\"suppressed\"}}, reads back absent",
        id("c1"),
        other.id
    )));
}

#[test]
fn a_state_added_to_a_configuration_the_write_makes_inactive_is_charged() {
    let (mut natives, documents, suppressed, stated) = tall_active();
    natives.neutral_features[0].suppressed = Some(false);
    let other = neutral_feature(&scope(3), "f3d:test:feature#b");
    let mut configurations = stated.clone();
    configuration(&mut configurations, "tall").active = false;
    configuration(&mut configurations, "c1").active = true;
    configuration(&mut configurations, "tall")
        .feature_states
        .insert(other.id.clone(), suppressed.configuration_state());
    let decoded = reconciled(&natives, &documents);
    let (_, losses) = charged(&natives, &documents, &decoded, &configurations);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/edit.dropped");
    assert!(losses[0].1.contains(&format!(
        "{}: feature_states.{} stated {{\"kind\":\"suppressed\"}}, reads back absent",
        id("tall"),
        other.id
    )));
}

/// One table shape of [`table_shapes`].
#[derive(Debug, Clone, Copy)]
struct Shape {
    /// Whether the feature that column `F` names, and the feature that no
    /// column names, is suppressed.
    suppressed: [bool; 2],
    /// The active row.
    active: Option<&'static str>,
    /// Whether a rule document activates `Tall`, and whether a second table
    /// document holds a table with no rows.
    documents: [bool; 2],
    /// Whether the configurations move the active row to `wide`.
    moved: bool,
}

/// Every combination of the table shapes that the tests synthesize: rows
/// `short` (a cell for each column), `tall` (a property cell only) and
/// `wide` (no cell); a feature column and a feature no column names, each
/// suppressed or not; no active row, an active row with a feature cell and
/// one without; a rule document or none; a second table document with no
/// rows or none; and the active row moved to `wide` by the configurations.
/// The feature of a stored model has the state of the active row
/// (f3d.md §1.2), so the column's feature is suppressed where `short` is
/// active.
fn table_shapes() -> Vec<Shape> {
    let mut shapes = Vec::new();
    for bits in 0..32_u8 {
        for active in [None, Some("tall"), Some("short")] {
            if active == Some("short") && bits & 1 == 0 {
                continue;
            }
            shapes.push(Shape {
                suppressed: [bits & 1 != 0, bits & 2 != 0],
                active,
                documents: [bits & 4 != 0, bits & 8 != 0],
                moved: bits & 16 != 0,
            });
        }
    }
    shapes
}

/// The natives and the table documents of `shape`, with row `active`
/// active.
fn shaped(shape: Shape, active: Option<&str>) -> (Natives, Vec<DesignConfiguration>) {
    let mut natives = natives();
    let [columned, unnamed_suppressed] = shape.suppressed;
    natives.neutral_features[0].suppressed = Some(columned);
    let mut unnamed = neutral_feature(&scope(2), "f3d:test:feature#a");
    unnamed.suppressed = Some(unnamed_suppressed);
    natives.neutral_features.push(unnamed);
    let table = table_document(
        "1",
        &[
            ("P", property_target("Part Number")),
            ("H", object_target("202", "UserParameterValue")),
            ("D", object_target("330", "ParameterDoubleValue")),
            ("F", object_target("314", "DcFeature")),
            ("U", object_target("999", "DcFeature")),
        ],
        &[
            (
                "short",
                "Short",
                vec![
                    ("P", string_cell("S-1")),
                    ("H", expression_cell("25 mm", 2.5)),
                    ("D", expression_cell("10.00 mm", 1.0)),
                    ("F", bool_cell(false)),
                    ("U", bool_cell(false)),
                ],
            ),
            ("tall", "Tall", vec![("P", string_cell("T-1"))]),
            ("wide", "Wide", Vec::new()),
        ],
        &["short", "tall", "wide"],
    );
    let mut documents = vec![DesignConfiguration::try_new(
        ENTRY.into(),
        DesignConfigurationKind::Table,
        table.as_object().unwrap().clone(),
    )
    .unwrap()];
    if let Some(row) = active {
        documents[0]
            .set_active_row(DesignConfigurationActiveRow {
                record_index: 349,
                table_id: "1".into(),
                row_id: row.into(),
            })
            .unwrap();
    }
    let [rule, second_table] = shape.documents;
    if rule {
        documents.push(
            DesignConfiguration::try_new(
                RULE.into(),
                DesignConfigurationKind::Rule,
                json!({"when": "Width > 1", "activate": "Tall"})
                    .as_object()
                    .unwrap()
                    .clone(),
            )
            .unwrap(),
        );
    }
    if second_table {
        documents.push(
            DesignConfiguration::try_new(
                "FusionAssetName[Active]/DesignConfigurationTable.v.dsgcfg".into(),
                DesignConfigurationKind::Table,
                table_document("2", &[("P", property_target("Part Number"))], &[], &[])
                    .as_object()
                    .unwrap()
                    .clone(),
            )
            .unwrap(),
        );
    }
    (natives, documents)
}

#[test]
fn every_table_shape_written_unedited_is_written_as_stated_with_no_charge() {
    for shape in table_shapes() {
        let (natives, documents) = shaped(shape, shape.active);
        let decoded = reconciled(&natives, &documents);
        let mut neutral = decoded.configurations.clone();
        if shape.moved {
            for configuration in &mut neutral {
                configuration.active = configuration.id == id("wide");
            }
        }
        let (written, losses) = write(&documents, &decoded, &neutral, &natives);
        assert!(losses.is_empty(), "{shape:?}: {losses:#?}");
        assert_eq!(written.documents.len(), documents.len());
        for (written, stated) in written.documents.iter().zip(&documents) {
            assert_eq!(
                serde_json::to_vec(written.payload()).unwrap(),
                serde_json::to_vec(stated.payload()).unwrap(),
                "{shape:?}"
            );
        }
        assert_eq!(
            projects(&neutral, &documents, &natives.targets()).unwrap(),
            !shape.moved,
            "{shape:?}"
        );
    }
}

#[test]
fn a_row_id_is_recovered_from_its_configuration_id() {
    for row in ["tall", "a b%c", "row:7", ""] {
        assert_eq!(
            row_id(ENTRY, neutral_configuration_id(ENTRY, row).as_str()).as_deref(),
            Some(row)
        );
    }
    assert_eq!(
        row_id(RULE, neutral_configuration_id(ENTRY, "tall").as_str()),
        None
    );
    assert_eq!(row_id(ENTRY, "test:configuration#tall"), None);
}

#[test]
fn a_length_is_written_in_the_centimetres_nearest_the_length() {
    let natives = natives();
    let parameter = natives.neutral_parameters[0].id.clone();
    let read_back = |millimetres: f64| {
        let value = ParameterValue::Length(Length::new(millimetres).unwrap());
        let written = evaluated(&parameter, &value, &natives.targets()).unwrap();
        match natives.targets().parameter_value(&parameter, written) {
            Some(ParameterValue::Length(length)) => length.get(),
            other => panic!("{millimetres} reads back as {other:?}"),
        }
    };
    for millimetres in [10.0, 1000.0, 0.0] {
        assert_eq!(read_back(millimetres), millimetres);
    }
    for millimetres in [0.3, 1.0 / 3.0, 25.4, 123.456_789, 1e-9] {
        let read = read_back(millimetres);
        assert!(
            (read - millimetres).abs() <= millimetres * f64::EPSILON,
            "{millimetres} reads back as {read}"
        );
    }
    assert_eq!(
        evaluated(&parameter, &ParameterValue::Integer(3), &natives.targets()),
        None
    );
}

/// A part whose two components, named at records 10 and 30, hold Sketch
/// scopes 20 and 40 that bind one sketch no collection lists, with a table
/// whose column `F` names scope 40 and whose row `a` is active. The owner
/// pass keeps that sketch native in the feature of scope 40, and the
/// decoder's configurations state the definition the pass leaves.
fn owner_conflict() -> (
    crate::native::F3dNative,
    CadIr,
    Vec<LossNote>,
    Vec<DesignConfiguration>,
) {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::recipes::DesignComponentNamingSpace;
    use cadmpeg_ir::features::SketchFeatureBinding;

    let space = |record: u64, uuid: &str| DesignComponentNamingSpace {
        id: format!("f3d:Design/BulkStream.dat:component-naming-space#{record}"),
        byte_offset: 0,
        component_record_index: record,
        context_uuid: uuid.to_owned().try_into().unwrap(),
        context_uuid_offset: 0,
        name: None,
        name_offset: None,
    };
    let mut table = DesignConfiguration::try_new(
        ENTRY.into(),
        DesignConfigurationKind::Table,
        table_document(
            "1",
            &[("F", object_target("40", "DcFeature"))],
            &[
                ("a", "A", vec![("F", bool_cell(true))]),
                ("b", "B", vec![("F", bool_cell(false))]),
            ],
            &["a", "b"],
        )
        .as_object()
        .unwrap()
        .clone(),
    )
    .unwrap();
    table
        .set_active_row(DesignConfigurationActiveRow {
            record_index: 349,
            table_id: "1".into(),
            row_id: "a".into(),
        })
        .unwrap();
    let native = crate::native::F3dNative {
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
        design_configurations: vec![table],
        ..crate::native::F3dNative::default()
    };
    let mut ir = CadIr::empty();
    for space in &native.design_component_naming_spaces {
        ir.model.product_definitions.push(
            cadmpeg_core::serde_erase::json::from_value(json!({
                "id": crate::ids::neutral_component_id(&space.context_uuid), "kind": "part",
                "bom_properties": {}, "storage": {"kind": "embedded"}, "occurrences": [],
                "instance_bodies": [], "representations": [], "bodies": []
            }))
            .unwrap(),
        );
    }
    let sketch: cadmpeg_ir::sketches::Sketch = cadmpeg_core::serde_erase::json::from_value(json!({
        "id": "test:model:sketch#profile",
        "placement": {"kind": "unresolved"},
        "profiles": []
    }))
    .unwrap();
    for scope in &native.design_parameter_scopes {
        let mut feature = neutral_feature(scope, "f3d:model:feature#origin");
        feature.ordinal = u64::from(scope.record_index);
        feature.native_ref = Some(scope.id.clone());
        feature
            .evaluation
            .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
                sketch: SketchFeatureBinding::Planar(Some(sketch.id.clone())),
            }));
        ir.model.features.push(feature);
    }
    ir.model.sketches.push(sketch);
    let mut losses = Vec::new();
    let archive = crate::test_support::zip_fixture::f3d_with_configurations(
        &crate::test_support::smbh_geometry_fixture::synthetic_geometry_smbh(),
        &[],
    );
    crate::test_support::zip_fixture::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            crate::decode::attach_design(ctx, scan, &native, &mut ir, &mut losses)
        })
    })
    .unwrap();
    let documents = native.design_configurations.clone();
    (native, ir, losses, documents)
}

#[test]
fn configurations_decoded_after_an_owner_conflict_are_written_as_stated() {
    let (native, ir, losses, documents) = owner_conflict();
    assert!(
        losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::ScopeSketchOwnerMismatch.kind()),
        "{losses:#?}"
    );
    let conflicted = &ir.model.features[1];
    assert!(matches!(
        conflicted.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
        })
    ));
    for edited in [false, true] {
        let mut features = ir.model.features.clone();
        if edited {
            features[0].name = Some("Renamed".into());
        }
        let natives = Natives {
            parameters: Vec::new(),
            owners: Vec::new(),
            scopes: native.design_parameter_scopes.clone(),
            neutral_parameters: ir.model.parameters.clone(),
            neutral_features: features,
            producers: BTreeSet::new(),
        };
        assert!(
            projects(&ir.model.configurations, &documents, &natives.targets()).unwrap(),
            "edited {edited}: {:#?}",
            ir.model.configurations
        );
        let decoded = reconciled(&natives, &documents);
        let (written, losses) = write(&documents, &decoded, &ir.model.configurations, &natives);
        assert!(
            matches!(written.documents, Cow::Borrowed(_)),
            "edited {edited}"
        );
        assert!(losses.is_empty(), "edited {edited}: {losses:#?}");
    }
}

/// A move of the active row from `c1` to `tall` writes the stored
/// `testRowData` entry with the value of row `tall`, and charges the rewrite.
#[test]
fn a_moved_active_row_writes_the_test_rows_of_its_row() {
    let natives = natives();
    let targets = natives.targets();
    let mut documents = active_in_source("c1");
    let mut payload = documents[0].payload().clone();
    payload.insert(
        "testRowData".into(),
        json!([json!({
            "DataType": "StringOptionValue",
            "ColumnUniqueID": "P",
            "Value": "Configuration 1",
            "Option": 1
        })
        .to_string()]),
    );
    documents[0] = documents[0].with_payload(payload).unwrap();
    let decoded = project(&documents, &targets).unwrap();
    let mut configurations = decoded.configurations.clone();
    configuration(&mut configurations, "c1").active = false;
    configuration(&mut configurations, "tall").active = true;
    let (written, _) = regenerate(&documents, &decoded, &configurations, &targets).unwrap();
    let tall = id("tall");
    let stored = id("c1");
    let mut losses = Vec::new();
    let test_rows = written
        .test_rows(
            Some(tall.as_str()),
            &targets,
            None,
            &BTreeSet::new(),
            &mut losses,
        )
        .expect("rewritten testRowData");
    assert_eq!(test_rows.index, 0);
    let payload = test_rows.payload.unwrap();
    let entries = payload["testRowData"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| serde_json::from_str::<serde_json::Value>(entry.as_str().unwrap()).unwrap())
        .map(|entry| (entry["ColumnUniqueID"].as_str().unwrap().to_owned(), entry))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(
        entries["P"],
        json!({
            "ColumnUniqueID": "P",
            "DataType": "StringOptionValue",
            "Option": 1,
            "Value": "Tall"
        })
    );
    assert_eq!(entries["H"]["Expression"], "35 mm");
    assert_eq!(entries["D"]["Expression"], "10.00 mm");
    assert_eq!(entries["F"]["Value"], false);
    assert!(
        losses
            .iter()
            .any(|loss| loss.message.contains("writes testRowData entries")),
        "{losses:#?}"
    );
    assert!(written
        .test_rows(
            Some(stored.as_str()),
            &targets,
            None,
            &BTreeSet::new(),
            &mut Vec::new()
        )
        .is_none());
}

mod target;
