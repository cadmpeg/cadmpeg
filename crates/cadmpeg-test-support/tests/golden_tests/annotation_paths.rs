// SPDX-License-Identifier: Apache-2.0
//! Every annotation key a committed golden states names a record of the
//! document stored beside it, and every exactness field path names a field of
//! its entity.
//!
//! A decoder states a path by name, so a field that a type renames or removes
//! leaves the path behind with no compile error. The validator reports such a
//! path as a warning; this sweep runs it over every golden document with the
//! annotations stored beside it, so the stale path fails here. A golden pins
//! the document as the CADIR wire states it, so a key of a record the wire
//! omits names no record of it (§3.2, §3.4).

use std::collections::BTreeSet;
use std::path::Path;

use cadmpeg_core::serde_erase::from_json_value;
use cadmpeg_ir::report::check::{Check, Finding};
use cadmpeg_ir::{Annotations, CadIr};
use serde_json::Value;

use super::{collect_documents, golden_files, states_native_elision};

/// The member, beside a document root, that holds its source fidelity.
const SOURCE_FIDELITY_KEY: &str = "source_fidelity";

#[test]
fn every_golden_exactness_field_path_resolves() {
    let stale = golden_findings(|finding, _| {
        finding.check == Check::Annotations && finding.message.starts_with("exactness field path")
    });
    assert!(
        stale.is_empty(),
        "{} annotated field paths name no field:\n{}",
        stale.len(),
        stale.join("\n")
    );
}

#[test]
fn every_golden_annotation_key_names_a_record_of_its_document() {
    let dangling = golden_findings(|finding, retained| {
        [
            "annotations_native.provenance-key-does-not-resolve-entity",
            "annotations_native.exactness-key-does-not-resolve-entity",
        ]
        .contains(&finding.rule.as_str())
            && !finding
                .entity
                .as_deref()
                .is_some_and(|key| retained.contains(key))
    });
    assert!(
        dangling.is_empty(),
        "{} annotation keys name no record of their document:\n{}",
        dangling.len(),
        dangling.join("\n")
    );
}

/// Each finding `selected` keeps of the validation of every annotated golden
/// document against the annotations stored beside it. `selected` also takes
/// the ids of the records the source fidelity beside the document retains,
/// which an annotation key may name.
fn golden_findings(selected: impl Fn(&Finding, &BTreeSet<&str>) -> bool) -> Vec<String> {
    let mut validated = 0_usize;
    let mut kept = Vec::new();
    for path in golden_files() {
        let text = std::fs::read_to_string(&path).expect("read golden");
        let golden: Value = serde_json::from_str(&text).expect("parse golden");
        let mut roots = Vec::new();
        collect_documents(&golden, &mut String::new(), &mut roots, &mut Vec::new());
        for (pointer, ir) in roots {
            if states_native_elision(ir) {
                continue;
            }
            let Some(annotations) = sibling_annotations(&golden, &pointer, &path) else {
                continue;
            };
            let retained = sibling_retained_ids(&golden, &pointer);
            let ir = from_json_value::<CadIr>(ir).unwrap_or_else(|error| {
                panic!(
                    "{} at {pointer} does not read back: {error}",
                    path.display()
                )
            });
            let report = cadmpeg_ir::validate::validate_neutral_with_annotations(
                &ir,
                &annotations,
                Vec::new(),
            )
            .unwrap_or_else(|error| {
                panic!("{} at {pointer} does not validate: {error}", path.display())
            });
            validated += 1;
            kept.extend(
                report
                    .findings
                    .iter()
                    .filter(|finding| selected(finding, &retained))
                    .map(|finding| {
                        format!(
                            "{} at {pointer}: {}: {}",
                            path.display(),
                            finding.entity.as_deref().unwrap_or("-"),
                            finding.message
                        )
                    }),
            );
        }
    }
    assert!(validated > 0, "the sweep validated no annotated golden");
    kept
}

/// The ids of the records the source fidelity beside the document at
/// `pointer` retains.
fn sibling_retained_ids<'a>(golden: &'a Value, pointer: &str) -> BTreeSet<&'a str> {
    let parent = pointer.rsplit_once('/').map_or("", |(parent, _)| parent);
    golden
        .pointer(&format!("{parent}/{SOURCE_FIDELITY_KEY}/retained_records"))
        .and_then(Value::as_object)
        .map(|records| records.keys().map(String::as_str).collect())
        .unwrap_or_default()
}

/// The annotations stored beside the document at `pointer`, when the golden
/// states a source fidelity there.
fn sibling_annotations(golden: &Value, pointer: &str, path: &Path) -> Option<Annotations> {
    let parent = pointer.rsplit_once('/').map_or("", |(parent, _)| parent);
    let annotations = golden
        .pointer(&format!("{parent}/{SOURCE_FIDELITY_KEY}"))?
        .get("annotations")?;
    Some(
        from_json_value::<Annotations>(annotations).unwrap_or_else(|error| {
            panic!(
                "{} at {parent}/{SOURCE_FIDELITY_KEY}/annotations does not read back: {error}",
                path.display()
            )
        }),
    )
}
