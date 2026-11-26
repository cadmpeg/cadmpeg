// SPDX-License-Identifier: Apache-2.0

use std::borrow::Cow;

use cadmpeg_core::json;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_ir::features::parameters::{
    ConfigurationEvaluation, DesignConfiguration as Configuration, ParameterValue,
};
use cadmpeg_ir::features::selections::DistinctMembers;
use cadmpeg_ir::features::{ConfigurationId, Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::scalar::Length;

use super::{evaluated, regenerate, row_id};
use crate::design::configurations::{project, reconcile_active_configurations};
use crate::ids::neutral_configuration_id;
use crate::records::configuration::table::ConfiguredValue;
use crate::records::configuration::{
    DesignConfiguration, DesignConfigurationActiveRow, DesignConfigurationKind,
};
use crate::test_support::configuration_table::{
    bool_cell, expression_cell, neutral_feature, object_target, property_target, scope,
    string_cell, table_document, Natives,
};
use crate::writer::primitives::projects;

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
    let (written, losses) =
        regenerate(documents, &decoded, &configurations, &natives.targets()).unwrap();
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
fn a_feature_edit_without_a_cell_change_writes_the_rows_as_stated() {
    let natives = natives();
    let decoded = project(&documents(), &natives.targets()).unwrap();
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
    assert_ne!(outcome.configurations, decoded.configurations);
    assert!(outcome.unchanged);
    assert_eq!(outcome.losses, Vec::new());
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
    let (written, losses) =
        regenerate(&documents, &decoded, &configurations, &natives.targets()).unwrap();
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
fn configurations_with_no_table_to_hold_their_rows_are_charged_dropped() {
    let natives = natives();
    let documents = documents();
    let decoded = project(&documents, &natives.targets()).unwrap();
    let empty = vec![DesignConfiguration::try_new(
        ENTRY.into(),
        DesignConfigurationKind::Table,
        serde_json::Map::new(),
    )
    .unwrap()];
    let (written, losses) = regenerate(
        &empty,
        &crate::design::configurations::Projection::default(),
        &decoded.configurations,
        &natives.targets(),
    )
    .unwrap();
    assert!(matches!(written.documents, Cow::Borrowed(_)));
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].code.to_string(), "export/edit.dropped");
    assert!(losses[0].message.contains(id("tall").as_str()));
}

#[test]
fn an_inferred_state_of_an_unedited_active_row_is_not_charged() {
    let mut natives = natives();
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
    )
    .unwrap();
    let tall = decoded
        .configurations
        .iter()
        .find(|configuration| configuration.id == id("tall"))
        .unwrap();
    assert!(tall.feature_states.contains_key(&suppressed.id));
    let mut configurations = decoded.configurations.clone();
    let (written, losses) =
        regenerate(&documents, &decoded, &configurations, &natives.targets()).unwrap();
    assert!(matches!(written.documents, Cow::Borrowed(_)));
    assert!(losses.is_empty(), "{losses:#?}");

    configuration(&mut configurations, "tall")
        .properties
        .insert(
            NonBlankString::new("property:Part Number").unwrap(),
            "T-2".to_owned(),
        );
    let (_, losses) =
        regenerate(&documents, &decoded, &configurations, &natives.targets()).unwrap();
    assert!(losses.is_empty(), "{losses:#?}");
}

/// The table documents of [`documents`] with row `tall` active, the
/// natives with a suppressed feature that no column names, and the
/// configurations the decoder states for them.
fn tall_active() -> (
    Natives,
    Vec<DesignConfiguration>,
    Feature,
    Vec<Configuration>,
) {
    let mut natives = natives();
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
    let (written, losses) =
        regenerate(documents, decoded, configurations, &natives.targets()).unwrap();
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
    let (unchanged, losses) = charged(&natives, &documents, &decoded, &configurations);
    assert!(unchanged);
    assert_eq!(losses.len(), 1, "{losses:#?}");
    assert_eq!(losses[0].0, "export/edit.dropped");
    assert!(losses[0].1.contains(&held), "{losses:#?}");
    assert!(!losses[0].1.contains(id("c1").as_str()), "{losses:#?}");
}

#[test]
fn moving_the_active_row_with_no_feature_change_is_not_charged() {
    let (natives, documents, _, mut configurations) = tall_active();
    let decoded = reconciled(&natives, &documents);
    configuration(&mut configurations, "tall").active = false;
    configuration(&mut configurations, "c1").active = true;
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
    let (natives, documents, suppressed, stated) = tall_active();
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
fn table_shapes() -> Vec<Shape> {
    let mut shapes = Vec::new();
    for bits in 0..32_u8 {
        for active in [None, Some("tall"), Some("short")] {
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
        let (written, losses) =
            regenerate(&documents, &decoded, &neutral, &natives.targets()).unwrap();
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
