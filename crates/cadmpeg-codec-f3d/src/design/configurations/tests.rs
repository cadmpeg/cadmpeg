// SPDX-License-Identifier: Apache-2.0

use super::{
    attach_roots, inferred_configuration_suppression_count, project,
    reconcile_active_configurations, root_document, unresolved_configuration_member_count,
    unresolved_configuration_parameter_override_count, unresolved_configuration_rule_count,
    unresolved_configuration_suppressed_feature_count,
};
use crate::records::configuration::{
    DesignConfiguration, DesignConfigurationActiveRow, DesignConfigurationKind,
};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopeTailFlags,
};
use crate::test_support::configuration_table::{
    bool_cell, expression_cell, neutral_feature, neutral_parameter, object_target, owner,
    parameter, property_target, scope, string_cell, table_document, Natives, Row, STREAM,
};
use cadmpeg_core::json;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::features::parameters::ConfigurationEvaluation;
use cadmpeg_ir::features::{Feature, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::BodyId;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const ENTRY: &str =
    "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.t.dsgcfg";

fn table(document: &Value) -> DesignConfiguration {
    DesignConfiguration::try_new(
        ENTRY.into(),
        DesignConfigurationKind::Table,
        document.as_object().unwrap().clone(),
    )
    .unwrap()
}

fn rule(name: &str, payload: &Value) -> DesignConfiguration {
    DesignConfiguration::try_new(
        name.into(),
        DesignConfigurationKind::Rule,
        payload.as_object().unwrap().clone(),
    )
    .unwrap()
}

fn active(table_id: &str, row_id: &str) -> DesignConfigurationActiveRow {
    DesignConfigurationActiveRow {
        record_index: 349,
        table_id: table_id.into(),
        row_id: row_id.into(),
    }
}

fn columns() -> Vec<(&'static str, Value)> {
    vec![
        ("P", property_target("Part Number")),
        ("H", object_target("202", "UserParameterValue")),
        ("D", object_target("330", "ParameterDoubleValue")),
        ("F", object_target("314", "DcFeature")),
    ]
}

fn row<'a>(id: &'a str, name: &'a str, height: &'a str, computed: bool) -> Row<'a> {
    (
        id,
        name,
        vec![
            ("P", string_cell(name)),
            ("H", expression_cell(height, 1.0)),
            ("D", expression_cell("10.00 mm", 1.0)),
            ("F", bool_cell(computed)),
        ],
    )
}

fn document() -> Value {
    table_document(
        "1",
        &columns(),
        &[
            row("tall", "Tall", "35 mm", false),
            row("c1", "Configuration 1", "25 mm", true),
        ],
        &["c1", "tall"],
    )
}

#[test]
fn configuration_rows_project_in_row_order_with_the_active_row() {
    let mut native = table(&document());
    native.set_active_row(active("1", "tall")).unwrap();
    let natives = Natives::complete();
    let projected = project(&[native], &natives.targets())
        .unwrap()
        .configurations;
    let mut rows = projected
        .iter()
        .map(|configuration| {
            (
                configuration.ordinal,
                configuration.name.as_deref().unwrap(),
                configuration.active,
                configuration.id.clone(),
            )
        })
        .collect::<Vec<_>>();
    rows.sort_by_key(|(ordinal, ..)| *ordinal);
    assert_eq!(
        rows,
        [
            (
                0,
                "Configuration 1",
                false,
                crate::ids::neutral_configuration_id(ENTRY, "c1")
            ),
            (
                1,
                "Tall",
                true,
                crate::ids::neutral_configuration_id(ENTRY, "tall")
            ),
        ]
    );

    for table_id in ["2", ""] {
        let mut native = table(&document());
        native.set_active_row(active(table_id, "tall")).unwrap();
        let projected = project(&[native], &natives.targets())
            .unwrap()
            .configurations;
        assert!(projected.iter().all(|configuration| !configuration.active));
    }
}

#[test]
fn configuration_cells_bind_targets_by_record_index() {
    let natives = Natives::complete();
    let projected = project(&[table(&document())], &natives.targets())
        .unwrap()
        .configurations;
    let tall = projected
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Tall"))
        .unwrap();
    let height = &natives.neutral_parameters[0].id;
    let depth = &natives.neutral_parameters[1].id;
    assert_eq!(tall.parameter_overrides[height], "35 mm");
    assert_eq!(tall.parameter_overrides[depth], "10.00 mm");
    assert_eq!(tall.parameter_overrides.len(), 2);
    let feature = &natives.neutral_features[0];
    let state = &tall.feature_states[&feature.id];
    assert!(state.evaluation.is_suppressed());
    assert_eq!(state.dependencies, feature.dependencies);
    assert_eq!(&state.definition, feature.evaluation.definition());
    assert_eq!(
        tall.properties
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str()))
            .collect::<Vec<_>>(),
        [("property:Part Number", "Tall")]
    );
    let first = projected
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Configuration 1"))
        .unwrap();
    assert!(matches!(
        first.feature_states[&feature.id].evaluation,
        ConfigurationEvaluation::Active { ref outputs } if outputs.is_empty()
    ));
    assert_eq!(
        unresolved_configuration_parameter_override_count(&projected),
        0
    );
    assert_eq!(
        unresolved_configuration_suppressed_feature_count(&projected),
        0
    );
}

#[test]
fn configuration_cells_without_a_neutral_target_stay_properties() {
    let mut cases: Vec<(&str, Natives)> = Vec::new();
    let mut natives = Natives::complete();
    natives.neutral_parameters = vec![neutral_parameter(&parameter(500, "other"))];
    natives.neutral_features = vec![neutral_feature(&scope(600), "f3d:model:feature#sketch")];
    cases.push(("other neutral entities", natives));
    let mut natives = Natives::complete();
    natives.parameters.clear();
    natives.scopes.clear();
    natives.owners.clear();
    cases.push(("no native records", natives));
    let mut natives = Natives::complete();
    natives.owners = vec![owner("f3d:Other/BulkStream.dat", 330, 329)];
    natives.scopes.push(DesignParameterScope::empty(
        "f3d:Other/BulkStream.dat:design-parameter-scope#314",
        DesignFeatureKind::Extrude,
        314,
    ));
    cases.push(("other streams", natives));
    for (name, natives) in cases {
        let projected = project(&[table(&document())], &natives.targets())
            .unwrap()
            .configurations;
        let tall = projected
            .iter()
            .find(|configuration| configuration.name.as_deref() == Some("Tall"))
            .unwrap();
        assert!(tall.feature_states.is_empty(), "{name}");
        assert_eq!(tall.properties["suppressed:F"], "true", "{name}");
        assert_eq!(tall.properties["parameter:D"], "10.00 mm", "{name}");
        let unresolved_height = name != "other streams";
        assert_eq!(
            tall.properties.get("parameter:H").map(String::as_str),
            unresolved_height.then_some("35 mm"),
            "{name}"
        );
        let first = projected
            .iter()
            .find(|configuration| configuration.name.as_deref() == Some("Configuration 1"))
            .unwrap();
        assert_eq!(first.properties["suppressed:F"], "false", "{name}");
        assert_eq!(
            unresolved_configuration_suppressed_feature_count(&projected),
            2,
            "{name}"
        );
        assert_eq!(
            unresolved_configuration_parameter_override_count(&projected),
            if unresolved_height { 4 } else { 2 },
            "{name}"
        );
    }
}

#[test]
fn configuration_owner_records_resolve_only_one_parameter() {
    let mut natives = Natives::complete();
    natives
        .owners
        .push(owner("f3d:Other/BulkStream.dat", 330, 329));
    let projected = project(&[table(&document())], &natives.targets())
        .unwrap()
        .configurations;
    assert!(projected
        .iter()
        .all(|configuration| configuration.properties.contains_key("parameter:D")));

    let mut natives = Natives::complete();
    natives.owners.push(owner(STREAM, 330, 329));
    let projected = project(&[table(&document())], &natives.targets())
        .unwrap()
        .configurations;
    assert!(projected
        .iter()
        .all(|configuration| !configuration.properties.contains_key("parameter:D")));
}

#[test]
fn configuration_tables_with_rows_supply_one_list() {
    let natives = Natives::complete();
    let empty = table_document("2", &columns(), &[], &[]);
    let mut second = table(&empty);
    second.set_active_row(active("2", "tall")).unwrap();
    let projected = project(&[table(&document()), second], &natives.targets())
        .unwrap()
        .configurations;
    assert_eq!(projected.len(), 2);

    let other = DesignConfiguration::try_new(
        "Other.dsgcfg".into(),
        DesignConfigurationKind::Table,
        document().as_object().unwrap().clone(),
    )
    .unwrap();
    let error = project(&[table(&document()), other], &natives.targets())
        .expect_err("two populated tables");
    assert!(error
        .to_string()
        .contains("configuration tables have no shared authored order"));
}

#[test]
fn configuration_rules_bind_only_one_named_row() {
    let natives = Natives::complete();
    let bound = rule(
        "rule.dsgcfgrule",
        &json!({"when": "width > 20 mm", "activate": "Tall"}),
    );
    let native = [table(&document()), bound];
    let projected = project(&native, &natives.targets()).unwrap().configurations;
    let tall = projected
        .iter()
        .find(|configuration| configuration.name.as_deref() == Some("Tall"))
        .unwrap();
    assert_eq!(
        tall.properties["activation_rule:rule.dsgcfgrule"],
        "width > 20 mm"
    );
    assert_eq!(unresolved_configuration_rule_count(&native, &projected), 0);

    let duplicate = table_document(
        "1",
        &columns(),
        &[
            row("a", "Tall", "35 mm", false),
            row("b", "Tall", "25 mm", true),
        ],
        &["a", "b"],
    );
    let native = [
        table(&duplicate),
        rule(
            "rule.dsgcfgrule",
            &json!({"when": "width > 20 mm", "activate": "Tall"}),
        ),
        rule("partial.dsgcfgrule", &json!({"when": "w > 1", "x": 7})),
    ];
    let projected = project(&native, &natives.targets()).unwrap().configurations;
    assert_eq!(unresolved_configuration_rule_count(&native, &projected), 2);
    assert_eq!(unresolved_configuration_member_count(&native), 1);
}

#[test]
fn configuration_root_attaches_to_the_table_document_that_holds_its_table() {
    let empty = |name: &str| {
        DesignConfiguration::try_new(
            name.into(),
            DesignConfigurationKind::Table,
            serde_json::Map::new(),
        )
        .unwrap()
    };
    let holding = table(&document());
    let empty_rule = rule("rule.dsgcfgrule", &json!({}));
    assert_eq!(
        root_document(&[empty_rule.clone(), empty("a.dsgcfg")], "1"),
        Some(1)
    );
    assert_eq!(
        root_document(
            &[empty("a.dsgcfg"), holding.clone(), empty_rule.clone()],
            "1"
        ),
        Some(1)
    );
    assert_eq!(
        root_document(&[empty("a.dsgcfg"), empty("b.dsgcfg")], "1"),
        None
    );
    assert_eq!(root_document(&[holding.clone(), holding], "1"), None);
    assert_eq!(root_document(&[empty_rule], "1"), None);
}

#[test]
fn configuration_roots_attach_only_when_one_root_names_a_table_document() {
    let mut documents = [table(&document())];
    assert_eq!(
        attach_roots(&mut documents, vec![active("1", "tall")]).unwrap(),
        0
    );
    assert_eq!(documents[0].active_row(), Some(&active("1", "tall")));

    let mut documents = [table(&document())];
    assert_eq!(
        attach_roots(&mut documents, vec![active("1", "tall"), active("1", "c1")]).unwrap(),
        2
    );
    assert_eq!(documents[0].active_row(), None);

    let mut documents = [rule("rule.dsgcfgrule", &json!({}))];
    assert_eq!(
        attach_roots(&mut documents, vec![active("1", "tall")]).unwrap(),
        1
    );
    assert_eq!(attach_roots(&mut documents, Vec::new()).unwrap(), 0);
}

#[test]
fn the_active_configuration_and_the_feature_states_fill_each_others_gaps() {
    let row = |evaluation, feature: &Feature| {
        cadmpeg_ir::features::parameters::ConfigurationFeatureState {
            evaluation,
            dependencies: feature.dependencies.clone(),
            definition: feature.evaluation.definition().clone(),
        }
    };
    let mut unlisted = neutral_feature(&scope(1), "f3d:test:feature#a");
    unlisted.suppressed = Some(true);
    let mut unstated = neutral_feature(&scope(2), "f3d:test:feature#b");
    unstated.suppressed = None;
    let mut stored = neutral_feature(&scope(3), "f3d:test:feature#c");
    stored.suppressed = Some(false);
    let configuration = cadmpeg_ir::features::parameters::DesignConfiguration {
        id: crate::ids::neutral_configuration_id(ENTRY, "active"),
        ordinal: 0,
        active: true,
        source_index: None,
        name: None,
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::from([
            (
                unstated.id.clone(),
                row(ConfigurationEvaluation::Suppressed {}, &unstated),
            ),
            (
                stored.id.clone(),
                row(ConfigurationEvaluation::Suppressed {}, &stored),
            ),
        ]),
        bodies: None,
        native_ref: None,
    };
    let mut inactive = configuration.clone();
    inactive.active = false;
    inactive.feature_states.clear();
    let undecided = BTreeSet::from([unstated.id.clone()]);
    let mut features = vec![unlisted, unstated, stored];
    let mut configurations = vec![configuration.clone(), inactive];
    reconcile_active_configurations(&mut features, &mut configurations, &undecided).unwrap();
    assert_eq!(inferred_configuration_suppression_count(&configurations), 0);

    // A suppressed feature no row names gains a row in the active configuration only.
    assert!(configurations[0].feature_states[&features[0].id]
        .evaluation
        .is_suppressed());
    assert!(configurations[1].feature_states.is_empty());
    // A row supplies a state the feature does not have.
    assert_eq!(features[1].suppressed, Some(true));
    // A stored state is not overwritten, so the checker still sees the conflict.
    assert_eq!(features[2].suppressed, Some(false));
}

/// Features whose cell in active row `off` suppresses scope 314, computes
/// scope 315 and suppresses scopes 316, 318 and 319: the Base Feature of 314
/// and the Extrude of 315 store no suppression and carry the states feature
/// projection infers for them, 316 stores its state in its tail, the Thread
/// of 318 in its companion suppression flag, and the Sketch of 319 stores an
/// active state in its tail. The suppressed feature of scope 317 has no
/// column.
#[test]
fn the_active_row_decides_a_feature_whose_scope_stores_no_suppression() {
    let mut natives = Natives::complete();
    natives.scopes[0] =
        DesignParameterScope::empty(&natives.scopes[0].id, DesignFeatureKind::BaseFeature, 314);
    natives.neutral_features[0] = neutral_feature(&natives.scopes[0], "f3d:test:feature#s");
    let inferred_active = natives.neutral_features[0].id.clone();
    let mut inferred_suppressed = neutral_feature(&scope(315), "f3d:test:feature#a");
    inferred_suppressed.suppressed = Some(true);
    let mut stored_scope = scope(316);
    stored_scope.tail_flags = Some(DesignScopeTailFlags {
        suppression: [false, false],
        unevaluated: false,
        wide_state: None,
        state_offset: stored_scope.feature_ordinal_offset() + 1,
        no_result: false,
        rolled_back: false,
        no_result_offset: stored_scope.feature_ordinal_offset() + 4,
        envelope_offset: None,
    });
    let stored = neutral_feature(&stored_scope, "f3d:test:feature#b");
    let mut unlisted = neutral_feature(&scope(317), "f3d:test:feature#c");
    unlisted.suppressed = Some(true);
    let mut thread_scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#318"),
        DesignFeatureKind::Thread,
        318,
    );
    thread_scope.companion_suppressed = Some(crate::records::identity::Located {
        value: false,
        offset: thread_scope.feature_ordinal_offset() + 62,
    });
    let thread = neutral_feature(&thread_scope, "f3d:test:feature#d");
    let mut sketch_scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#319"),
        DesignFeatureKind::Sketch,
        319,
    );
    sketch_scope.tail_flags = stored_scope.tail_flags;
    let mut sketch = neutral_feature(&sketch_scope, "f3d:test:feature#e");
    sketch
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::Sketch {
            sketch: cadmpeg_ir::features::SketchFeatureBinding::Planar(None),
        }));
    natives.scopes.extend([
        scope(315),
        stored_scope,
        scope(317),
        thread_scope,
        sketch_scope,
    ]);
    natives.neutral_features.extend([
        inferred_suppressed.clone(),
        stored.clone(),
        unlisted.clone(),
        thread.clone(),
        sketch.clone(),
    ]);
    let mut document = table(&table_document(
        "1",
        &[
            ("F", object_target("314", "DcFeature")),
            ("G", object_target("315", "DcFeature")),
            ("H", object_target("316", "DcFeature")),
            ("I", object_target("318", "DcFeature")),
            ("J", object_target("319", "DcFeature")),
        ],
        &[(
            "off",
            "Off",
            vec![
                ("F", bool_cell(false)),
                ("G", bool_cell(true)),
                ("H", bool_cell(false)),
                ("I", bool_cell(false)),
                ("J", bool_cell(false)),
            ],
        )],
        &["off"],
    ));
    document.set_active_row(active("1", "off")).unwrap();

    let configurations = natives.attach(&[document]);
    let state = |id: &cadmpeg_ir::features::FeatureId| {
        natives
            .neutral_features
            .iter()
            .find(|feature| &feature.id == id)
            .unwrap()
            .suppressed
    };
    assert_eq!(state(&inferred_active), Some(true));
    assert_eq!(state(&inferred_suppressed.id), Some(false));
    assert_eq!(state(&stored.id), Some(false));
    assert_eq!(state(&thread.id), Some(false));
    assert_eq!(state(&sketch.id), Some(false));
    assert_eq!(state(&unlisted.id), Some(true));
    assert_eq!(configurations.len(), 1);
    assert!(configurations[0].feature_states[&unlisted.id]
        .evaluation
        .is_suppressed());
}

/// Active row `off` suppresses scopes 314 and 315, whose tails store no
/// suppression. The Base Feature of 314 carries the active state feature
/// projection infers, and the feature of 315 states none. Each produces a
/// body, so each keeps its state and the row contradicts the features.
#[test]
fn a_feature_that_produces_a_body_keeps_its_state_under_a_suppressing_row() {
    let mut natives = Natives::complete();
    natives.scopes[0] =
        DesignParameterScope::empty(&natives.scopes[0].id, DesignFeatureKind::BaseFeature, 314);
    let mut inferred = neutral_feature(&natives.scopes[0], "f3d:test:feature#s");
    let mut unstated = neutral_feature(&scope(315), "f3d:test:feature#a");
    unstated.suppressed = None;
    for (feature, body) in [
        (&mut inferred, "f3d:test:body#1"),
        (&mut unstated, "f3d:test:body#2"),
    ] {
        feature
            .evaluation
            .edit(|_, outputs| outputs.extend([BodyId::mint(body).unwrap()]));
    }
    natives.neutral_features[0] = inferred.clone();
    natives.scopes.push(scope(315));
    natives.neutral_features.push(unstated.clone());
    let mut document = table(&table_document(
        "1",
        &[
            ("F", object_target("314", "DcFeature")),
            ("G", object_target("315", "DcFeature")),
        ],
        &[(
            "off",
            "Off",
            vec![("F", bool_cell(false)), ("G", bool_cell(false))],
        )],
        &["off"],
    ));
    document.set_active_row(active("1", "off")).unwrap();

    let configurations = natives.attach(&[document]);
    let state = |id: &cadmpeg_ir::features::FeatureId| {
        natives
            .neutral_features
            .iter()
            .find(|feature| &feature.id == id)
            .unwrap()
            .suppressed
    };
    assert_eq!(state(&inferred.id), Some(false));
    assert_eq!(state(&unstated.id), None);
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.features = natives.neutral_features.clone();
    ir.model.configurations = configurations;
    let report = cadmpeg_ir::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report.findings.iter().any(|finding| finding.rule.as_str()
        == "topology.active-configuration-suppression-disagrees-current-feature"));
}

#[test]
fn an_inferred_suppressed_state_names_its_assumption_when_a_column_is_unresolved() {
    let mut feature = neutral_feature(&scope(1), "f3d:test:feature#a");
    feature.suppressed = Some(true);
    let mut configuration = cadmpeg_ir::features::parameters::DesignConfiguration {
        id: crate::ids::neutral_configuration_id(ENTRY, "active"),
        ordinal: 0,
        active: true,
        source_index: None,
        name: None,
        material: None,
        properties: BTreeMap::new(),
        parameter_overrides: BTreeMap::new(),
        parameter_values: BTreeMap::new(),
        feature_states: BTreeMap::new(),
        bodies: None,
        native_ref: None,
    };
    configuration.properties.insert(
        cadmpeg_core::text::NonBlankString::new("suppressed:column".to_owned()).unwrap(),
        "false".to_owned(),
    );
    let mut features = vec![feature];
    let mut configurations = vec![configuration];
    reconcile_active_configurations(&mut features, &mut configurations, &BTreeSet::new()).unwrap();
    assert!(configurations[0].feature_states[&features[0].id]
        .evaluation
        .is_suppressed());
    assert_eq!(inferred_configuration_suppression_count(&configurations), 1);
}

#[test]
fn unreadable_configuration_document_keeps_its_sibling_document() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    let bad = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.bad.dsgcfg";
    zip.start_file(bad, stored).unwrap();
    zip.write_all(b"[").unwrap();
    zip.start_file(ENTRY, stored).unwrap();
    zip.write_all(&json::to_vec(&document()).unwrap()).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let scan = crate::container::scan(&ctx, root).unwrap();
    let mut given_up = Vec::new();
    let decoded = super::decode_configurations(&ctx, &scan, &mut given_up).unwrap();
    assert_eq!(decoded.documents.len(), 1);
    assert_eq!(decoded.documents[0].entry_name().as_str(), ENTRY);
    assert_eq!(given_up.len(), 1);
    assert_eq!(given_up[0].kind(), crate::container::ENTRY_PART);
    assert_eq!(given_up[0].name(), Some(bad));
    let unreadable = given_up[0].loss();
    assert_eq!(
        unreadable.code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(
        unreadable
            .message
            .starts_with(&format!("part archive-entry {bad:?} stream=\"\" offset=")),
        "{}",
        unreadable.message
    );
    assert!(
        unreadable
            .message
            .contains(&format!("record={}: ", given_up[0].id())),
        "{}",
        unreadable.message
    );
}
