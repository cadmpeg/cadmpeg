// SPDX-License-Identifier: Apache-2.0
//! Byte-identical golden harness for `inspect` over the committed fixtures.
//!
//! `tests/golden/fixtures/*.3dm` are frozen inputs. This harness never writes
//! them: a snapshot test can only tell a decoder change apart from an input
//! change while the inputs hold still, so regenerating an input destroys the
//! evidence the snapshot exists to carry. `UPDATE_GOLDEN=1` rewrites
//! `tests/golden/inspect/` and nothing else.
//!
//! Regenerate after an intended container-summary change with
//! `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-rhino golden` and review the
//! diff.
//!
//! `tests/golden/decode/` has no reader yet. Those snapshots carry
//! `ir_version` 4 against the current 5, so every one of them differs from
//! current output for reasons that predate this harness; pinning them would
//! freeze staleness, not behavior.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_codec_core::decode::InspectOptions;
use cadmpeg_ir::codec::CodecEntry;

use super::RhinoCodec;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "3dm";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-rhino golden";

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn fixture_dir() -> PathBuf {
    golden_dir().join("fixtures")
}

fn inspect_dir() -> PathBuf {
    golden_dir().join("inspect")
}

/// Sorted stems of every file in `dir` whose extension is `extension`.
fn stems(dir: &Path, extension: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap_or_else(|error| panic!("read {}: {error}", dir.display()))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("read entry: {error}"))
                .path()
        })
        .filter(|path| path.extension().is_some_and(|found| found == extension))
        .map(|path| {
            path.file_stem()
                .expect("directory entry with an extension has a stem")
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

/// Serialize one container summary as the stable pretty JSON the goldens hold.
/// An inspect error is frozen too: refusing a container is contract-relevant
/// behavior, so this never panics on codec output.
fn inspect_snapshot(bytes: &[u8]) -> String {
    let value =
        match RhinoCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    let mut text = serde_json::to_string_pretty(&value).expect("serialize inspect snapshot");
    text.push('\n');
    text
}

/// Read a golden with `\r\n` folded to `\n`.
///
/// `.gitattributes` pins only the NX goldens to LF and CI runs the fast suite
/// on Windows, so a Windows checkout hands these files CRLF while the harness
/// emits LF. Folding on read keeps the comparison platform-neutral.
fn read_golden(path: &Path) -> std::io::Result<String> {
    Ok(std::fs::read_to_string(path)?.replace("\r\n", "\n"))
}

/// First differing line, 1-based, with both sides truncated for readability.
fn first_line_diff(expected: &str, actual: &str) -> (usize, String, String) {
    let mut expected_lines = expected.lines();
    let mut actual_lines = actual.lines();
    let mut line = 0usize;
    loop {
        line += 1;
        match (expected_lines.next(), actual_lines.next()) {
            (Some(left), Some(right)) if left == right => {}
            (left, right) => {
                let truncate = |value: Option<&str>| match value {
                    Some(text) if text.len() > 200 => format!("{}…", &text[..200]),
                    Some(text) => text.to_owned(),
                    None => "<end of file>".to_owned(),
                };
                return (line, truncate(left), truncate(right));
            }
        }
    }
}

#[test]
fn golden_inspect_snapshots_are_byte_identical() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let fixtures = stems(&fixture_dir(), FIXTURE_EXTENSION);
    assert!(
        !fixtures.is_empty(),
        "no `*.{FIXTURE_EXTENSION}` fixture under {}; the harness would pass vacuously",
        fixture_dir().display()
    );

    let mut failures: Vec<String> = Vec::new();
    for name in &fixtures {
        let input = fixture_dir().join(format!("{name}.{FIXTURE_EXTENSION}"));
        let bytes = std::fs::read(&input)
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", input.display()));
        let actual = inspect_snapshot(&bytes);
        let path = inspect_dir().join(format!("{name}.json"));
        if update {
            std::fs::write(&path, actual.as_bytes())
                .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
            continue;
        }
        match read_golden(&path) {
            Ok(expected) if expected == actual => {}
            Ok(expected) => {
                let (line, golden_line, actual_line) = first_line_diff(&expected, &actual);
                failures.push(format!(
                    "fixture `{name}`: inspect diverged from {} at line {line}\n    golden: {golden_line}\n    actual: {actual_line}",
                    path.display()
                ));
            }
            Err(error) => failures.push(format!(
                "fixture `{name}`: cannot read golden {} ({error}); regenerate with `{REGENERATE}`",
                path.display()
            )),
        }
    }

    let goldens = stems(&inspect_dir(), "json");
    for orphan in goldens.iter().filter(|name| !fixtures.contains(name)) {
        failures.push(format!(
            "golden `{orphan}.json` has no `{orphan}.{FIXTURE_EXTENSION}` fixture; delete the golden or restore the input"
        ));
    }

    assert!(
        failures.is_empty(),
        "{} inspect golden(s) drifted; if the change is intended regenerate with `{REGENERATE}` and review the diff:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Guards against nondeterministic summaries (`HashMap` order, timestamps):
/// inspecting the same bytes twice must produce identical JSON.
#[test]
fn golden_inspect_output_is_deterministic() {
    for name in stems(&fixture_dir(), FIXTURE_EXTENSION) {
        let input = fixture_dir().join(format!("{name}.{FIXTURE_EXTENSION}"));
        let bytes = std::fs::read(&input)
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", input.display()));
        let first = inspect_snapshot(&bytes);
        let second = inspect_snapshot(&bytes);
        if first != second {
            let (line, one, two) = first_line_diff(&first, &second);
            panic!(
                "fixture `{name}`: nondeterministic inspect at line {line}\n    run 1: {one}\n    run 2: {two}"
            );
        }
    }
}
