// SPDX-License-Identifier: Apache-2.0
//! Byte-identical golden harness for `inspect` over the committed fixtures.
//!
//! `corpus/freecad_fcstd/fixtures/*.FCStd` are frozen inputs. This harness never
//! writes them: a snapshot test can only tell a decoder change apart from an
//! input change while the inputs hold still, so regenerating an input destroys
//! the evidence the snapshot exists to carry. `UPDATE_GOLDEN=1` rewrites
//! `tests/golden/decode/` and `tests/golden/inspect/`, and nothing else.
//!
//! Regenerate after an intended container-summary change with
//! `UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-freecad golden` and review the
//! diff.
//!
//! `tests/golden/decode/` pins the decoded document: the IR, the decode
//! report's losses, and source fidelity. That is the branch a feature-typing
//! or loss-accounting change moves, and `inspect` cannot see it — an inspect
//! summary describes the container, not what was transferred out of it.
//!
//! Snapshots serialize through [`serde_json::Value`], whose maps order by key,
//! so reordering a struct field does not rewrite every golden.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_codec_core::decode::InspectOptions;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};

use super::FcstdCodec;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "FCStd";

/// Crate-relative regeneration hint used in every failure message.
const REGENERATE: &str = "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-freecad golden";

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

/// The `FreeCAD` goldens have no `tests/golden/fixtures/` tree. Their inputs are
/// the charter fixtures under `corpus/freecad_fcstd/fixtures/`, one `.FCStd`
/// per golden basename.
fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate manifest sits two levels below the repository root")
        .join("corpus/freecad_fcstd/fixtures")
}

fn inspect_dir() -> PathBuf {
    golden_dir().join("inspect")
}

fn decode_dir() -> PathBuf {
    golden_dir().join("decode")
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
        match FcstdCodec.inspect(&mut Cursor::new(bytes.to_vec()), &InspectOptions::default()) {
            Ok(summary) => serde_json::to_value(&summary).expect("serialize inspect summary"),
            Err(error) => serde_json::json!({ "inspect_error": error.to_string() }),
        };
    let mut text = serde_json::to_string_pretty(&value).expect("serialize inspect snapshot");
    text.push('\n');
    text
}

/// Serialize one decoded document as the stable pretty JSON the goldens hold:
/// the IR, the decode report, and source fidelity. A decode error is frozen
/// too: refusing a document is contract-relevant behavior, so this never
/// panics on codec output.
///
/// `ir.native` holds the retained shape payloads, which reach 5.8 MB of one
/// 6.7 MB document and 25 MB at the largest. Nobody can review a diff that
/// size, which is the whole purpose of a golden, so the arena is reduced to
/// its serialized length and digest. A payload change still fails the test;
/// it just reports as one changed digest instead of megabytes of moved
/// coordinates. Every other branch of the document is pinned in full.
fn decode_snapshot(bytes: &[u8]) -> String {
    let value = match FcstdCodec.decode(&mut Cursor::new(bytes.to_vec()), &DecodeOptions::default())
    {
        Ok(result) => {
            let mut ir = serde_json::to_value(&result.ir).expect("serialize ir");
            if let Some(native) = ir.get_mut("native") {
                *native = serde_json::json!({
                    "__elided": "native arenas are pinned by digest, not by value",
                    "__serialized_len": serde_json::to_string(native)
                        .expect("serialize native arenas")
                        .len(),
                    "__sha256": cadmpeg_ir::hash::canonical_json_sha256(native),
                });
            }
            serde_json::json!({
                "ir": ir,
                "report": serde_json::to_value(&result.report).expect("serialize report"),
                "source_fidelity": serde_json::to_value(&result.source_fidelity)
                    .expect("serialize source_fidelity"),
            })
        }
        Err(error) => serde_json::json!({ "decode_error": error.to_string() }),
    };
    let mut text = serde_json::to_string_pretty(&value).expect("serialize decode snapshot");
    text.push('\n');
    text
}

/// Read a golden with `\r\n` folded to `\n`.
///
/// `.gitattributes` pins these goldens to LF, but folding on read keeps the
/// comparison platform-neutral even in a tree checked out without it.
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

/// Fixture stems, guarded against an empty fixture directory.
fn fixtures() -> Vec<String> {
    let fixtures = stems(&fixture_dir(), FIXTURE_EXTENSION);
    assert!(
        !fixtures.is_empty(),
        "no `*.{FIXTURE_EXTENSION}` fixture under {}; the harness would pass vacuously",
        fixture_dir().display()
    );
    fixtures
}

/// Compare one branch's snapshot for every fixture against its golden.
///
/// Returns one failure per drifted or unreadable golden plus one per golden
/// with no fixture behind it, so a branch reports every difference at once
/// rather than stopping at the first.
fn check_branch(
    kind: &str,
    dir: &Path,
    snapshot: fn(&[u8]) -> String,
    fixtures: &[String],
    update: bool,
) -> Vec<String> {
    let mut failures: Vec<String> = Vec::new();
    for name in fixtures {
        let input = fixture_dir().join(format!("{name}.{FIXTURE_EXTENSION}"));
        let bytes = std::fs::read(&input)
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", input.display()));
        let actual = snapshot(&bytes);
        let path = dir.join(format!("{name}.json"));
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
                    "fixture `{name}`: {kind} diverged from {} at line {line}\n    golden: {golden_line}\n    actual: {actual_line}",
                    path.display()
                ));
            }
            Err(error) => failures.push(format!(
                "fixture `{name}`: cannot read {kind} golden {} ({error}); regenerate with `{REGENERATE}`",
                path.display()
            )),
        }
    }
    for orphan in stems(dir, "json")
        .iter()
        .filter(|name| !fixtures.contains(name))
    {
        failures.push(format!(
            "golden `{orphan}.json` under {} has no `{orphan}.{FIXTURE_EXTENSION}` fixture; delete the golden or restore the input",
            dir.display()
        ));
    }
    failures
}

#[test]
fn golden_snapshots_are_byte_identical() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let fixtures = fixtures();
    let mut failures = check_branch(
        "inspect",
        &inspect_dir(),
        inspect_snapshot,
        &fixtures,
        update,
    );
    failures.extend(check_branch(
        "decode",
        &decode_dir(),
        decode_snapshot,
        &fixtures,
        update,
    ));

    assert!(
        failures.is_empty(),
        "{} golden(s) drifted; if the change is intended regenerate with `{REGENERATE}` and review the diff:\n\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}

/// Guards against nondeterministic output (`HashMap` order, timestamps):
/// putting the same bytes through a branch twice must produce identical JSON.
#[test]
fn golden_output_is_deterministic() {
    for name in fixtures() {
        let input = fixture_dir().join(format!("{name}.{FIXTURE_EXTENSION}"));
        let bytes = std::fs::read(&input)
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", input.display()));
        for (kind, snapshot) in [
            ("inspect", inspect_snapshot as fn(&[u8]) -> String),
            ("decode", decode_snapshot as fn(&[u8]) -> String),
        ] {
            let first = snapshot(&bytes);
            let second = snapshot(&bytes);
            if first != second {
                let (line, one, two) = first_line_diff(&first, &second);
                panic!(
                    "fixture `{name}`: nondeterministic {kind} at line {line}\n    run 1: {one}\n    run 2: {two}"
                );
            }
        }
    }
}
