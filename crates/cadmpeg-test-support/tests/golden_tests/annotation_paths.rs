// SPDX-License-Identifier: Apache-2.0
//! Every exactness field path a committed golden annotates names a field of
//! its entity.
//!
//! A decoder states a path by name, so a field that a type renames or removes
//! leaves the path behind with no compile error. The validator reports such a
//! path as a warning; this sweep runs it over every golden document with the
//! annotations stored beside it, so the stale path fails here.

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
    let mut validated = 0_usize;
    let mut stale = Vec::new();
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
            stale.extend(
                report
                    .findings
                    .iter()
                    .filter(|finding| names_a_stale_path(finding))
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
    assert!(
        stale.is_empty(),
        "{} annotated field paths name no field:\n{}",
        stale.len(),
        stale.join("\n")
    );
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

fn names_a_stale_path(finding: &Finding) -> bool {
    finding.check == Check::Annotations && finding.message.starts_with("exactness field path")
}
