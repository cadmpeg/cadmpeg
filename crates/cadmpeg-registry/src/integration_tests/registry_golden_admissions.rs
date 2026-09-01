// SPDX-License-Identifier: Apache-2.0
//! Cross-workspace golden admissions against the embedded support registry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::{dialect_table, ReadDisposition};

#[test]
fn compiled_read_admissions_match_registry_policy() {
    let expected = dialect_table(None)
        .expect("embedded registry formats")
        .into_iter()
        .flat_map(|format| format.rows)
        .map(|row| (row.id.as_str().to_owned(), row.disposition.read))
        .collect::<BTreeMap<_, _>>();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut snapshots = Vec::new();
    collect_json_files(&root.join("crates"), &mut snapshots);
    let mut observed = BTreeMap::<String, BTreeMap<&'static str, usize>>::new();
    for path in snapshots {
        if !path.to_string_lossy().contains("/tests/golden/") {
            continue;
        }
        let bytes = std::fs::read(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let value: serde_json::Value = serde_json::from_slice(&bytes)
            .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()));
        collect_admissions(&value, &mut observed);
    }

    assert!(!observed.is_empty(), "no golden decoder admissions found");
    for (dialect, families) in observed {
        let read = expected
            .get(&dialect)
            .unwrap_or_else(|| panic!("{dialect}: decoder emitted no registry row"));
        for family in families.keys() {
            let compatible = match *family {
                "admitted" => matches!(read, ReadDisposition::Level(_) | ReadDisposition::Detected),
                "unverified" | "residual" | "legacy_admitted_unverified" => {
                    matches!(read, ReadDisposition::UnclassifiedRecovered)
                }
                "refused" => matches!(read, ReadDisposition::Refused),
                other => panic!("{dialect}: unknown admission family {other}"),
            };
            assert!(
                compatible,
                "{dialect}: decoder admission {family} contradicts registry read {read}"
            );
        }
    }
}

fn collect_json_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("read directory entry").path();
        if path.is_dir() {
            collect_json_files(&path, files);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            files.push(path);
        }
    }
}

fn collect_admissions(
    value: &serde_json::Value,
    observed: &mut BTreeMap<String, BTreeMap<&'static str, usize>>,
) {
    match value {
        serde_json::Value::Object(object) => {
            if let (Some(dialect), Some(admission)) = (
                object.get("dialect").and_then(serde_json::Value::as_str),
                object.get("admission"),
            ) {
                let family = if admission.as_str() == Some("admitted") {
                    "admitted"
                } else if admission.get("unverified").is_some() {
                    "unverified"
                } else if admission.as_str() == Some("residual") {
                    "residual"
                } else if admission.get("admitted_unverified").is_some() {
                    "legacy_admitted_unverified"
                } else if admission.as_str() == Some("refused") {
                    "refused"
                } else {
                    panic!("{dialect}: unrecognized admission value {admission}");
                };
                *observed
                    .entry(dialect.to_owned())
                    .or_default()
                    .entry(family)
                    .or_default() += 1;
            }
            for child in object.values() {
                collect_admissions(child, observed);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                collect_admissions(child, observed);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}
