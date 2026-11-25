// SPDX-License-Identifier: Apache-2.0
//! The process root: its parameters, toolpaths, and states written, read back
//! strictly, and written again; the wire forms §2.5 states for them; and the
//! `process` home of a document.

use cadmpeg_core::decode::{ArenaClass, ContentScope};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::json;

use super::root_fixtures as root;
use super::{read_desktop, refuses, round_trip, to_value};
use crate::design::evaluation::StateBase;
use crate::document::{ArenaHome, ArenaName, CadIr, Process, ScopeViolation};
use crate::process::groups::ProcessParameter;
use crate::process::state::{ProcessBase, ProcessContent, ProcessState};
use crate::process::toolpath::Toolpath;

fn check<T: Serialize + DeserializeOwned>(failures: &mut Vec<String>, name: &str, samples: Vec<T>) {
    assert!(!samples.is_empty(), "{name} has no sample");
    for sample in samples {
        if let Err(error) = round_trip(&sample) {
            failures.push(format!("{name}: {error}"));
        }
    }
}

#[test]
fn every_process_root_type_round_trips() {
    let mut failures = Vec::new();
    let f = &mut failures;
    check(f, "ProcessParameter", root::process_parameters());
    check(f, "Toolpath", root::toolpaths());
    check(f, "ProcessBase", root::process_bases());
    check(f, "ProcessContent", root::process_contents());
    check(f, "ProcessState", root::process_states());
    check(f, "Process", root::processes());
    assert!(failures.is_empty(), "{failures:#?}");
}

/// Every arena of the populated root holds a record, so each arena's round
/// trip above carries a populated element.
#[test]
fn the_populated_process_root_fills_every_arena() {
    let populated = to_value(&root::processes().remove(1));
    let arenas = populated.as_object().unwrap();
    assert_eq!(arenas.len(), 11);
    for (arena, records) in arenas {
        assert!(
            !records.as_array().unwrap().is_empty(),
            "process.{arena} is empty"
        );
    }
}

/// §7: the process root's keys, in the order the document states them.
#[test]
fn the_process_root_writes_every_arena_in_order() {
    let text = serde_json::to_string(&Process::default()).unwrap();
    assert_eq!(
        text,
        r#"{"setups":[],"operations":[],"features":[],"groups":[],"tools":[],"presets":[],"machines":[],"programs":[],"parameters":[],"toolpaths":[],"states":[]}"#
    );
    let mut missing = to_value(&Process::default());
    missing.as_object_mut().unwrap().remove("toolpaths");
    refuses::<Process>(&missing.to_string());
    let mut extra = to_value(&Process::default());
    extra["evaluations"] = json!([]);
    refuses::<Process>(&extra.to_string());
}

/// §7.9: a base wraps a design state base or names a process state, under
/// `value`.
#[test]
fn a_process_base_takes_the_adjacent_tag() {
    assert_eq!(
        to_value(&ProcessBase::Design(StateBase::Empty)),
        json!({"kind": "design", "value": {"kind": "empty"}})
    );
    assert_eq!(
        to_value(&ProcessBase::State(root::state_id(2))),
        json!({"kind": "state", "value": "test:process:process-state#2"})
    );
    refuses::<ProcessBase>(r#"{"kind": "design"}"#);
    refuses::<ProcessBase>(r#"{"kind": "state", "value": null}"#);
    refuses::<ProcessBase>(r#"{"kind": "design", "value": {"kind": "empty", "value": null}}"#);
    refuses::<ProcessBase>(r#"{"kind": "state", "value": "test:process:process-state#2", "x": 1}"#);
}

/// §2.5: an omitted optional field refuses `null`, every required field is
/// stated, and an unknown key is refused.
#[test]
fn optional_fields_required_fields_and_unknown_keys() {
    let toolpath = to_value(&root::toolpaths().remove(1));
    assert_eq!(
        toolpath,
        json!({
            "id": "test:process:toolpath#2",
            "operation": "test:process:operation#2",
            "status": "stale",
            "content": {"kind": "native", "value": "toolpath-2"},
        })
    );
    reads_strictly::<Toolpath>(
        &toolpath,
        &["feature", "configuration"],
        &["content", "status"],
    );

    let state = to_value(&root::process_states().remove(2));
    assert!(state.get("content").is_none() && state.get("configuration").is_none());
    reads_strictly::<ProcessState>(&state, &["content", "configuration"], &["of", "status"]);
    let mut stored = state.clone();
    stored["feature"] = json!(null);
    refuses::<ProcessState>(&stored.to_string());

    let parameter = to_value(&root::process_parameters().remove(2));
    assert!(parameter.get("value").is_none() && parameter.get("native_ref").is_none());
    reads_strictly::<ProcessParameter>(
        &parameter,
        &["value", "native_ref"],
        &["definition", "owner", "name"],
    );
    let mut unknown_kind = parameter.clone();
    unknown_kind.as_object_mut().unwrap().remove("kind");
    let read: ProcessParameter = read_desktop(&unknown_kind.to_string()).unwrap();
    assert_eq!(read.kind, None);
    unknown_kind["kind"] = json!(null);
    refuses::<ProcessParameter>(&unknown_kind.to_string());
    let mut tagged = parameter.clone();
    tagged["kind"] = json!({"kind": "text", "value": null});
    refuses::<ProcessParameter>(&tagged.to_string());
    let mut blank = parameter;
    blank["name"] = json!(" ");
    refuses::<ProcessParameter>(&blank.to_string());

    let content = to_value(&root::process_contents().remove(1));
    let mut missing = content.clone();
    missing.as_object_mut().unwrap().remove("delta");
    refuses::<ProcessContent>(&missing.to_string());
    let mut extra = content;
    extra["status"] = json!("current");
    refuses::<ProcessContent>(&extra.to_string());
}

/// Requires `value` to refuse `null` for each field of `optional`, the
/// omission of each field of `required`, and an unknown key.
fn reads_strictly<T: DeserializeOwned + std::fmt::Debug>(
    value: &serde_json::Value,
    optional: &[&str],
    required: &[&str],
) {
    for field in optional {
        let mut stated = value.clone();
        stated[*field] = json!(null);
        let message = refuses::<T>(&stated.to_string());
        assert!(message.contains("null"), "{field}: {message}");
    }
    for field in required {
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(*field);
        refuses::<T>(&missing.to_string());
    }
    let mut extra = value.clone();
    extra["colour"] = json!(3);
    refuses::<T>(&extra.to_string());
}

#[test]
fn a_document_with_a_process_writes_and_reads() {
    let ir = CadIr::empty();
    let text = serde_json::to_string(&ir).unwrap();
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["process"], to_value(&Process::default()));
    let mut full = CadIr::empty();
    full.process = root::processes().remove(1);
    let text = serde_json::to_string(&full).unwrap();
    assert_eq!(CadIr::from_json(&text).unwrap(), full);
    let read: CadIr = read_desktop(&text).unwrap();
    assert_eq!(serde_json::to_string(&read).unwrap(), text);
    assert_eq!(
        full.entity_count(),
        root::processes().remove(1).entity_count()
    );
}

#[test]
fn finalize_sorts_the_process_arenas() {
    let mut ir = CadIr::empty();
    ir.process.toolpaths = root::toolpaths();
    ir.process.toolpaths.reverse();
    ir.process.states = root::process_states();
    ir.process.states.reverse();
    ir.finalize();
    assert_eq!(ir.process.toolpaths, root::toolpaths());
    assert_eq!(ir.process.states, root::process_states());
}

/// A process record under a scope that excludes the process class is
/// kept, written, and read back, and the breach names the arena by its home.
#[test]
fn a_process_record_outside_the_scope_is_kept_and_named() {
    let mut ir = CadIr::empty();
    ir.process.toolpaths = root::toolpaths();
    ir.set_scope(ContentScope::design());
    assert_eq!(ir.scope(), ContentScope::design());
    let breach = [ScopeViolation::Populated {
        arena: ArenaName::parse("process.toolpaths").unwrap(),
    }];
    assert_eq!(ir.scope_violations(), breach);
    let back = CadIr::from_json(&serde_json::to_string(&ir).unwrap()).unwrap();
    assert_eq!(back.scope(), ContentScope::design());
    assert_eq!(back.scope_violations(), breach);
}

#[test]
fn process_arenas_are_named_by_their_home() {
    for (arena, home) in [
        ("features", ArenaHome::Model),
        ("design.features", ArenaHome::Design),
        ("process.features", ArenaHome::Process),
        ("process.parameters", ArenaHome::Process),
        ("process.programs", ArenaHome::Process),
    ] {
        let name = ArenaName::parse(arena).unwrap();
        assert_eq!(name.home(), home, "{arena}");
        assert!(arena.ends_with(name.key()), "{arena}");
    }
    let process = ArenaName::ALL
        .iter()
        .filter(|arena| arena.class() == ArenaClass::Process)
        .map(|arena| arena.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        process,
        [
            "process.setups",
            "process.operations",
            "process.features",
            "process.groups",
            "process.tools",
            "process.presets",
            "process.machines",
            "process.programs",
            "process.parameters",
            "process.toolpaths",
            "process.states",
        ]
    );
    let census = CadIr::empty().census();
    for arena in process {
        assert_eq!(census[arena], 0, "{arena}");
    }
}

#[test]
fn diff_reports_process_arenas() {
    let left = CadIr::empty();
    let mut right = left.clone();
    right.process.toolpaths.push(root::toolpaths().remove(0));
    let result = crate::diff::diff(&left, &right);
    let toolpaths = result
        .per_arena
        .iter()
        .find(|arena| arena.kind == "process.toolpaths")
        .unwrap();
    assert_eq!(toolpaths.added, [root::toolpath_id(1).as_str()]);
    assert!(crate::diff::diff(&right, &right).is_empty());
}
