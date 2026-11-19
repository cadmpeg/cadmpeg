// SPDX-License-Identifier: Apache-2.0
//! The design root: every type written, read back strictly, and written
//! again; the wire forms §2.5 states for them; and the `design` home of a
//! document.

use cadmpeg_core::decode::{ArenaClass, ContentScope};
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

use super::fixtures::{feature, id, name_tables};
use super::root_fixtures as root;
use super::{read_desktop, refuses, round_trip, to_value};
use crate::design::configuration::{OccurrenceOverride, Overrides};
use crate::design::container::Container;
use crate::design::evaluation::StateDelta;
use crate::design::expr::{Neutral, Node};
use crate::design::feature::{
    FeatureDefinition, FeatureOperation, FuzzyTolerance, OperationFamily, StoredOrigin,
};
use crate::design::program::{ParameterProgram, ProgramBody, Statement};
use crate::design::scope::{DesignScope, OccurrenceDesign};
use crate::design::spreadsheet::Spreadsheet;
use crate::document::{ArenaName, CadIr, Design, ScopeViolation};

fn check<T: Serialize + DeserializeOwned>(failures: &mut Vec<String>, name: &str, samples: Vec<T>) {
    assert!(!samples.is_empty(), "{name} has no sample");
    for sample in samples {
        if let Err(error) = round_trip(&sample) {
            failures.push(format!("{name}: {error}"));
        }
    }
}

#[test]
fn every_design_root_type_round_trips() {
    let mut failures = Vec::new();
    let f = &mut failures;
    check(f, "Folder", root::folders());
    check(f, "Rollback", root::rollbacks());
    check(f, "Container", root::containers());
    check(f, "EvaluationStatus", root::statuses());
    check(f, "StateBase", root::state_bases());
    check(f, "StateDelta", root::state_deltas());
    check(f, "Evaluation", root::evaluations());
    check(f, "CurrentState", root::current_states());
    check(f, "OccurrenceDesign", root::occurrence_designs());
    check(f, "DesignScope", root::design_scopes());
    check(f, "Driver", root::drivers());
    check(f, "FuzzyTolerance", root::fuzzy_tolerances());
    check(f, "LinkState", root::link_states());
    check(f, "Link", root::links());
    check(f, "StoredOrigin", root::stored_origins());
    check(f, "OperationFamily", root::operation_families());
    check(f, "FeatureOperation", root::feature_operations());
    check(f, "FeatureDefinition", root::feature_definitions());
    check(f, "Feature", root::features());
    check(f, "PartNumber", root::part_numbers());
    check(f, "ParameterOverride", root::parameter_overrides());
    check(f, "FeatureStateOverride", root::feature_state_overrides());
    check(f, "NestedOverride", root::nested_overrides());
    check(f, "OccurrenceOverride", root::occurrence_overrides());
    check(f, "DefinitionOverride", root::definition_overrides());
    check(f, "Overrides", root::overrides());
    check(f, "MaterialRef", root::material_refs());
    check(f, "MaterialOverride", root::material_overrides());
    check(f, "DesignConfiguration", root::design_configurations());
    check(f, "Publication", root::publications());
    check(f, "ArrangementKind", root::arrangement_kinds());
    check(f, "ArrangementAppearance", root::arrangement_appearances());
    check(f, "ArrangementMove", root::arrangement_moves());
    check(f, "ArrangementRef", root::arrangement_refs());
    check(f, "NestedArrangement", root::nested_arrangements());
    check(f, "Arrangement", root::arrangements());
    check(f, "ParameterOwner", root::parameter_owners());
    check(f, "ParameterDefinition", root::parameter_definitions());
    check(f, "ParameterValue", root::parameter_values());
    check(f, "DimensionDisplay", root::dimension_displays());
    check(f, "Deviations", vec![root::deviations()]);
    check(f, "FitDeviations", root::fit_deviations());
    check(f, "LimitSide", root::limit_sides());
    check(f, "Tolerance", root::tolerances());
    check(f, "DesignParameter", root::design_parameters());
    check(f, "ProgramStage", root::program_stages());
    check(f, "Statement", root::statements());
    check(f, "ProgramBody", root::program_bodies());
    check(f, "ParameterProgram", root::programs());
    check(f, "Spreadsheet", root::spreadsheets());
    check(f, "SheetMetalRule", root::sheet_metal_rules());
    check(f, "Design", root::designs());
    assert!(failures.is_empty(), "{failures:#?}");
}

/// §5.10: a definition has no wrapper level; `refine` and `fuzzy_tolerance`
/// sit beside `operation`, and a stored `false` is written.
#[test]
fn a_feature_definition_has_no_wrapper_level() {
    let definition = FeatureDefinition {
        operation: FeatureOperation::Sketch(id("sketch", 1)),
        refine: None,
        fuzzy_tolerance: None,
    };
    assert_eq!(
        to_value(&definition),
        json!({"operation": {"kind": "sketch", "value": "test:design:sketch#1"}})
    );
    let unrefined = FeatureDefinition {
        refine: Some(false),
        ..definition.clone()
    };
    assert_eq!(to_value(&unrefined)["refine"], json!(false));
    assert!(to_value(&unrefined).get("fuzzy_tolerance").is_none());
    let fuzzy = FeatureDefinition {
        fuzzy_tolerance: Some(FuzzyTolerance::Automatic),
        ..definition
    };
    assert_eq!(
        to_value(&fuzzy)["fuzzy_tolerance"],
        json!({"kind": "automatic"})
    );
    assert!(to_value(&fuzzy).get("refine").is_none());
}

#[test]
fn the_operation_arms_take_the_adjacent_tag() {
    assert_eq!(
        to_value(&FeatureOperation::MaterialRemoval),
        json!({"kind": "material_removal"})
    );
    assert_eq!(
        to_value(&FeatureOperation::Unresolved {
            family: OperationFamily::HelicalSweep
        }),
        json!({"kind": "unresolved", "value": {"family": "helical_sweep"}})
    );
    assert_eq!(
        to_value(&FeatureOperation::Stored {
            bodies: None,
            tessellations: Vec::new(),
            origin: StoredOrigin::Base {},
        }),
        json!({"kind": "stored", "value": {"tessellations": [], "origin": {"kind": "base"}}})
    );
    refuses::<FeatureOperation>(
        r#"{"kind": "stored", "value": {"bodies": null, "tessellations": [], "origin": {"kind": "base"}}}"#,
    );
    refuses::<FeatureOperation>(r#"{"kind": "stored", "value": {"origin": {"kind": "base"}}}"#);
    refuses::<StoredOrigin>(r#"{"kind": "base", "extra": 1}"#);
    refuses::<StoredOrigin>(r#"{"kind": "linked", "colour": 3}"#);
}

/// §2.5: a map with string keys writes an object, any other map sorted
/// `[key, value]` pairs, and each refuses a repeated key.
#[test]
fn maps_are_objects_or_sorted_pairs_and_refuse_a_repeated_key() {
    let mut scope = root::design_scopes().remove(0);
    scope.occurrences.clear();
    let text = serde_json::to_string(&scope).unwrap();
    assert!(text.contains("\"occurrences\":{}"), "{text}");
    let text = serde_json::to_string(&root::design_scopes().remove(0)).unwrap();
    let entry = r#""test:design:occurrence#9":{"arrangements":[],"parameters":{}}"#;
    let repeated = text.replace(
        "\"occurrences\":{",
        &format!("\"occurrences\":{{{entry},{entry},"),
    );
    assert!(
        refuses::<DesignScope>(&repeated).contains("duplicate key"),
        "{repeated}"
    );

    let delta = to_value(&root::state_deltas().remove(1));
    assert!(delta["names"].is_array() && delta["instance_bodies"].is_array());
    assert!(delta["poses"].is_array() && delta["names"][0].as_array().unwrap().len() == 2);
    assert!(delta["owners"].is_object() && delta["members"].is_object());
    let mut repeated = delta.clone();
    let pose = repeated["poses"][0].clone();
    repeated["poses"].as_array_mut().unwrap().push(pose);
    assert!(refuses::<StateDelta>(&repeated.to_string()).contains("twice"));
}

/// §2.5: an omitted optional field refuses `null`, a list is always written,
/// and an unknown key is refused.
#[test]
fn optional_fields_lists_and_unknown_keys() {
    refuses::<OccurrenceDesign>(r#"{"grounded": null, "arrangements": [], "parameters": {}}"#);
    refuses::<OccurrenceDesign>(r#"{"arrangements": [], "parameters": {}, "colour": 3}"#);
    refuses::<OccurrenceDesign>(r#"{"parameters": {}}"#);
    refuses::<crate::design::container::Rollback>(r#"{"kind": "end", "value": null, "extra": 1}"#);
    assert_eq!(
        serde_json::to_string(&OccurrenceOverride::default()).unwrap(),
        r#"{"nested":[]}"#
    );
    assert_eq!(
        to_value(&Overrides::default()),
        json!({"parameters": {}, "features": {}, "occurrences": {}, "definitions": {}})
    );
    let sheet = to_value(&root::spreadsheets().remove(1));
    assert_eq!(
        sheet,
        json!({"id": "test:design:spreadsheet#2", "cells": [], "column_widths": [], "row_heights": [], "merged_ranges": []})
    );
    let mut missing = sheet.clone();
    missing.as_object_mut().unwrap().remove("row_heights");
    refuses::<Spreadsheet>(&missing.to_string());
    let mut feature = sheet;
    feature["feature"] = json!("test:design:feature#1");
    refuses::<Spreadsheet>(&feature.to_string());
    let laid_out = to_value(&root::spreadsheets().remove(0));
    assert_eq!(
        laid_out["column_widths"],
        json!([{"name": "AB", "pixels": 120}])
    );
    assert_eq!(
        laid_out["row_heights"],
        json!([{"name": "2", "pixels": 30}])
    );
}

#[test]
fn a_set_refuses_a_repeated_member_and_an_unsorted_array() {
    let member = json!({"kind": "feature", "value": "test:design:feature#1"});
    let container = json!({"kind": "unordered", "value": {"id": "test:design:container#1", "kind": "group", "members": [member, member]}});
    assert!(refuses::<Container>(&container.to_string()).contains("twice"));
    let later = json!({"kind": "feature", "value": "test:design:feature#2"});
    let container = json!({"kind": "unordered", "value": {"id": "test:design:container#1", "kind": "group", "members": [later, member]}});
    assert!(refuses::<Container>(&container.to_string()).contains("out of order"));
}

/// §5.13, §5.14: a program's statements hold `Neutral`s, which write
/// canonical text.
#[test]
fn a_neutral_program_writes_its_statements_as_canonical_text() {
    let value = to_value(&root::program_bodies().remove(0));
    assert_eq!(value["kind"], json!("neutral"));
    let statement = &value["value"][0];
    assert_eq!(statement["kind"], json!("assign"));
    assert!(statement["value"]["text"].is_string(), "{statement}");
    assert!(statement["value"]["refs"].is_array() && statement["value"]["measures"].is_array());
    let program: ParameterProgram =
        read_desktop(&serde_json::to_string(&root::programs().remove(0)).unwrap()).unwrap();
    assert!(matches!(program.body, ProgramBody::Neutral(_)));
}

/// §5: the design root's keys, in the order the document states them.
#[test]
fn the_design_root_writes_every_arena_in_order() {
    let text = serde_json::to_string(&Design::default()).unwrap();
    assert_eq!(
        text,
        r#"{"scopes":[],"containers":[],"features":[],"sketches":[],"sketch_entities":[],"sketch_constraints":[],"spatial_sketches":[],"spatial_sketch_entities":[],"spatial_sketch_constraints":[],"parameters":[],"spreadsheets":[],"sheet_metal_rules":[],"programs":[],"configurations":[],"arrangements":[],"publications":[],"evaluations":[],"names":[]}"#
    );
    refuses::<Design>(r#"{"scopes": []}"#);
    let mut extra = to_value(&Design::default());
    extra["process"] = json!([]);
    refuses::<Design>(&extra.to_string());
}

#[test]
fn a_document_with_an_empty_design_writes_and_reads() {
    let ir = CadIr::empty();
    let text = serde_json::to_string(&ir).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["design"], to_value(&Design::default()));
    let back = CadIr::from_json(&text).unwrap();
    assert_eq!(back, ir);
    assert_eq!(serde_json::to_string(&back).unwrap(), text);
    let full = {
        let mut ir = CadIr::empty();
        ir.design = root::designs().remove(1);
        ir
    };
    let text = serde_json::to_string(&full).unwrap();
    assert_eq!(CadIr::from_json(&text).unwrap(), full);
}

/// A design record under a scope that excludes the design class is refused,
/// naming the arena by its home.
#[test]
fn a_design_record_outside_the_scope_is_refused() {
    let mut ir = CadIr::empty();
    ir.design.features = root::features();
    let violation = ScopeViolation {
        class: ArenaClass::Design,
        arena: ArenaName::parse("design.features"),
    };
    assert!(violation.arena.is_some());
    assert_eq!(ir.set_scope(ContentScope::geometry()), Err(violation));
    ir.design.features.clear();
    ir.set_scope(ContentScope::geometry()).unwrap();
    let mut value = to_value(&ir);
    value["design"]["features"] = to_value(&root::features());
    let error = CadIr::from_json(&value.to_string()).unwrap_err();
    assert!(error.to_string().contains("design.features"), "{error}");
}

#[test]
fn design_arenas_are_named_by_their_home() {
    let features = ArenaName::parse("design.features").unwrap();
    assert_eq!(features.key(), "features");
    assert_eq!(features.home(), crate::document::ArenaHome::Design);
    assert_eq!(features.class(), ArenaClass::Design);
    let v10 = ArenaName::parse("features").unwrap();
    assert_eq!(v10.home(), crate::document::ArenaHome::Model);
    let census = CadIr::empty().census();
    for arena in ["scopes", "containers", "features", "evaluations"] {
        assert_eq!(census[format!("design.{arena}").as_str()], 0);
    }
}

/// A design record keeps the id of the record it restates: the identity
/// checks of the other homes do not read the design home.
#[test]
fn a_design_record_shares_an_id_with_a_record_of_another_home() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let body = ir.geometry.bodies[0].id.as_str().to_owned();
    let mut restated = root::features().remove(1);
    restated.id = crate::features::FeatureId::try_from(body).unwrap();
    ir.design.features.push(restated);
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(
        !report.findings.iter().any(|finding| {
            finding.check == crate::report::check::Check::Identity
                || finding.message.contains("not unique")
        }),
        "{:#?}",
        report.findings
    );
}

#[test]
fn diff_reports_design_arenas_and_names() {
    let left = CadIr::empty();
    let mut right = left.clone();
    right.design.features.push(root::features().remove(1));
    right.design.names = name_tables().remove(1);
    let result = crate::diff::diff(&left, &right);
    let features = result
        .per_arena
        .iter()
        .find(|arena| arena.kind == "design.features")
        .unwrap();
    assert_eq!(features.added, [feature(2).as_str()]);
    let names = result
        .per_arena
        .iter()
        .find(|arena| arena.kind == "design.names")
        .unwrap();
    assert_eq!(names.added.len(), right.design.names.entries().len());
    assert!(crate::diff::diff(&right, &right).is_empty());
}

/// Runs `work` on a thread whose stack is 2 MiB, the default for a spawned
/// thread.
fn on_a_default_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(work)
        .unwrap()
        .join()
        .unwrap()
}

/// Statement nesting has no ceiling of its own; the JSON depth limit bounds
/// it. One level past the deepest nesting the reader admits is refused by
/// that limit, and the deepest reads, prints the same bytes, clones,
/// compares, and drops on a default thread.
#[test]
fn statements_nest_to_the_json_depth_limit_on_a_default_thread() {
    fn nested(depth: usize) -> String {
        let condition = || Neutral(Node::Boolean(true));
        let leaf = Statement::If {
            condition: condition(),
            then: Vec::new(),
            otherwise: Vec::new(),
        };
        let statement = (1..depth).fold(leaf, |statement, _| Statement::If {
            condition: condition(),
            then: vec![statement],
            otherwise: Vec::new(),
        });
        serde_json::to_string(&statement).unwrap()
    }
    let past = (1..128)
        .find(|&depth| read_desktop::<Statement>(&nested(depth)).is_err())
        .expect("the reader refuses some depth below the JSON limit");
    assert!(past > 1, "a single statement is refused");
    let refusal = on_a_default_thread({
        let text = nested(past);
        move || read_desktop::<Statement>(&text).err()
    });
    assert!(
        matches!(&refusal, Some(CodecError::Malformed(message)) if message.contains("recursion limit exceeded")),
        "depth {past}: {refusal:?}"
    );
    let deepest = nested(past - 1);
    let printed = on_a_default_thread({
        let deepest = deepest.clone();
        move || {
            let read = read_desktop::<Statement>(&deepest).unwrap();
            let printed = serde_json::to_string(&read).unwrap();
            let cloned = read.clone();
            assert_eq!(cloned, read);
            drop(cloned);
            drop(read);
            printed
        }
    });
    assert_eq!(printed, deepest);
}
