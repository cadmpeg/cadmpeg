// SPDX-License-Identifier: Apache-2.0
//! Shared golden-snapshot harness for the codec crates.
//!
//! Every codec that pins snapshots does the same six things: enumerate frozen
//! fixture inputs, run each input through one or more branches, serialize the
//! result as stable pretty JSON, compare it against a committed golden, report
//! every difference at once, and refuse to pass when a branch has no inputs.
//! Only the codec type, the fixture extension, and the regeneration hint differ,
//! so the harness lives here once and each crate supplies those three values
//! plus its branch functions.
//!
//! Fixture inputs are frozen. A snapshot test can only tell a decoder change
//! apart from an input change while the inputs hold still, so this harness
//! never writes them; `UPDATE_GOLDEN=1` rewrites golden outputs and nothing
//! else.
//!
//! ## Why the comparison is not byte-exact
//!
//! Decoded geometry passes through `f64::cos`, `f64::sin`, and friends, which
//! resolve to the platform's libm and are not bit-reproducible: glibc, the MSVC
//! runtime, and Apple's libm disagree in the last one or two units in the last
//! place. A `FreeCAD` conical face pins one such value, `origin.v` scaled by
//! `cos(half_angle)`, which serializes as `1.802581857082682` on Linux and
//! `1.8025818570826815` on Windows and macOS — two units in the last place
//! apart, and identical to fourteen significant digits.
//!
//! A byte-exact comparison therefore reports a platform as a regression. That
//! is the case the repository rule against comparing decoded doubles for exact
//! equality already covers, so [`snapshots_agree`] compares parsed JSON with
//! structure and strings exact and only fractional numbers tolerant. Byte
//! equality remains the fast path, and the determinism check stays byte-exact
//! because two runs in one process share one libm and must agree bit for bit.
//!
//! The tolerance hides drift below [`FLOAT_TOLERANCE`] relative magnitude. It
//! does not make decode reproducible across platforms; it only stops the
//! goldens from reporting that as codec drift.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// Relative tolerance for a fractional number in a snapshot comparison.
///
/// Applied against the larger magnitude of the two values, with a floor of one
/// so small values compare absolutely. Platform libm disagreement is a few
/// units in the last place, near `1e-16` relative, so this leaves four decimal
/// orders of headroom while still catching any change with physical meaning.
pub const FLOAT_TOLERANCE: f64 = 1e-12;

/// One snapshot branch: a subdirectory of `tests/golden` and the function that
/// produces the text pinned there.
pub struct Branch {
    /// Subdirectory name under `tests/golden`, also used in failure messages.
    pub kind: &'static str,
    /// Serializes one fixture's bytes as the snapshot text.
    pub snapshot: fn(&[u8]) -> String,
}

impl Branch {
    /// Names a branch and the function that produces its snapshot.
    #[must_use]
    pub const fn new(kind: &'static str, snapshot: fn(&[u8]) -> String) -> Self {
        Self { kind, snapshot }
    }
}

/// Locations and messages one codec's golden harness needs.
pub struct Harness {
    golden_dir: PathBuf,
    fixture_dir: PathBuf,
    fixture_extension: &'static str,
    regenerate: &'static str,
}

impl Harness {
    /// Builds a harness rooted at `manifest_dir`, which callers pass as
    /// `env!("CARGO_MANIFEST_DIR")` so the paths resolve to their own crate.
    ///
    /// Fixtures default to `tests/golden/fixtures`; see
    /// [`with_fixture_dir`](Self::with_fixture_dir) for codecs whose inputs
    /// live elsewhere. `regenerate` is the command quoted in every failure
    /// message.
    #[must_use]
    pub fn new(
        manifest_dir: &str,
        fixture_extension: &'static str,
        regenerate: &'static str,
    ) -> Self {
        let golden_dir = Path::new(manifest_dir).join("tests/golden");
        let fixture_dir = golden_dir.join("fixtures");
        Self {
            golden_dir,
            fixture_dir,
            fixture_extension,
            regenerate,
        }
    }

    /// Points the harness at fixture inputs outside `tests/golden/fixtures`.
    ///
    /// The `FreeCAD` goldens have no fixture tree of their own; their inputs are
    /// the corpus archives under `corpus/freecad_fcstd/fixtures`.
    #[must_use]
    pub fn with_fixture_dir(mut self, fixture_dir: PathBuf) -> Self {
        self.fixture_dir = fixture_dir;
        self
    }

    /// Sorted fixture stems, guarded against an empty fixture directory so the
    /// harness cannot pass without comparing anything.
    ///
    /// # Panics
    ///
    /// Panics when the fixture directory cannot be read or holds no input with
    /// the configured extension.
    fn fixtures(&self) -> Vec<String> {
        let fixtures = stems(&self.fixture_dir, self.fixture_extension);
        assert!(
            !fixtures.is_empty(),
            "no `*.{}` fixture under {}; the harness would pass vacuously",
            self.fixture_extension,
            self.fixture_dir.display()
        );
        fixtures
    }

    /// Reads one fixture's bytes.
    ///
    /// # Panics
    ///
    /// Panics when the fixture cannot be read, which means a golden lost its
    /// input rather than that a codec misbehaved.
    fn fixture_bytes(&self, name: &str) -> Vec<u8> {
        let input = self
            .fixture_dir
            .join(format!("{name}.{}", self.fixture_extension));
        std::fs::read(&input)
            .unwrap_or_else(|error| panic!("read fixture {}: {error}", input.display()))
    }

    /// Compares every branch for every fixture and asserts that none drifted.
    ///
    /// Set `UPDATE_GOLDEN` to rewrite the golden outputs instead of comparing;
    /// fixture inputs are never written.
    ///
    /// # Panics
    ///
    /// Panics listing every drifted, unreadable, and input-less golden at once
    /// rather than stopping at the first.
    pub fn check(&self, branches: &[Branch]) {
        let update = std::env::var_os("UPDATE_GOLDEN").is_some();
        let fixtures = self.fixtures();
        let mut failures: Vec<String> = Vec::new();
        for branch in branches {
            failures.extend(self.check_branch(branch, &fixtures, update));
        }
        assert!(
            failures.is_empty(),
            "{} golden(s) drifted; if the change is intended regenerate with `{}` and review the diff:\n\n{}",
            failures.len(),
            self.regenerate,
            failures.join("\n\n")
        );
    }

    /// Asserts that each branch produces identical text for two runs over the
    /// same bytes, catching hash-map ordering and embedded timestamps.
    ///
    /// This comparison is byte-exact: both runs share one libm, so there is no
    /// platform disagreement to tolerate and any difference is real
    /// nondeterminism.
    ///
    /// # Panics
    ///
    /// Panics on the first branch whose two runs disagree.
    pub fn check_determinism(&self, branches: &[Branch]) {
        for name in self.fixtures() {
            let bytes = self.fixture_bytes(&name);
            for branch in branches {
                let first = (branch.snapshot)(&bytes);
                let second = (branch.snapshot)(&bytes);
                if first != second {
                    let (line, one, two) = first_line_diff(&first, &second);
                    panic!(
                        "fixture `{name}`: nondeterministic {} at line {line}\n    run 1: {one}\n    run 2: {two}",
                        branch.kind
                    );
                }
            }
        }
    }

    /// Compares one branch, returning one failure per drifted or unreadable
    /// golden plus one per golden with no fixture behind it.
    fn check_branch(&self, branch: &Branch, fixtures: &[String], update: bool) -> Vec<String> {
        let dir = self.golden_dir.join(branch.kind);
        let kind = branch.kind;
        let mut failures: Vec<String> = Vec::new();
        for name in fixtures {
            let bytes = self.fixture_bytes(name);
            let actual = (branch.snapshot)(&bytes);
            let path = dir.join(format!("{name}.json"));
            if update {
                std::fs::write(&path, actual.as_bytes())
                    .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
                continue;
            }
            match read_golden(&path) {
                Ok(expected) => {
                    if let Err(mismatch) = snapshots_agree(&expected, &actual) {
                        failures.push(format!(
                            "fixture `{name}`: {kind} diverged from {}\n    {mismatch}",
                            path.display()
                        ));
                    }
                }
                Err(error) => failures.push(format!(
                    "fixture `{name}`: cannot read {kind} golden {} ({error}); regenerate with `{}`",
                    path.display(),
                    self.regenerate
                )),
            }
        }
        for orphan in stems(&dir, "json")
            .iter()
            .filter(|name| !fixtures.contains(name))
        {
            failures.push(format!(
                "golden `{orphan}.json` under {} has no `{orphan}.{}` fixture; delete the golden or restore the input",
                dir.display(),
                self.fixture_extension
            ));
        }
        failures
    }
}

/// Sorted stems of every file in `dir` whose extension is `extension`.
///
/// # Panics
///
/// Panics when `dir` cannot be read.
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

/// Reads a golden with `\r\n` folded to `\n`.
///
/// `.gitattributes` pins these goldens to LF, but folding on read keeps the
/// comparison platform-neutral even in a tree checked out without it.
fn read_golden(path: &Path) -> std::io::Result<String> {
    Ok(std::fs::read_to_string(path)?.replace("\r\n", "\n"))
}

/// Compares a golden against a fresh snapshot, tolerating only last-place
/// disagreement in fractional numbers.
///
/// Byte equality short-circuits. Otherwise both sides parse as JSON and compare
/// structurally: object key sets, array lengths, strings, booleans, nulls, and
/// integers must match exactly, and a fractional number may differ by up to
/// [`FLOAT_TOLERANCE`] relative to the larger magnitude. Text that does not
/// parse as JSON falls back to a line diff.
///
/// # Errors
///
/// Returns a description locating the first disagreement, by JSON path when
/// both sides parsed and by line number otherwise.
pub fn snapshots_agree(expected: &str, actual: &str) -> Result<(), String> {
    if expected == actual {
        return Ok(());
    }
    let (Ok(expected_value), Ok(actual_value)) = (
        serde_json::from_str::<serde_json::Value>(expected),
        serde_json::from_str::<serde_json::Value>(actual),
    ) else {
        let (line, golden_line, actual_line) = first_line_diff(expected, actual);
        return Err(format!(
            "at line {line}\n    golden: {golden_line}\n    actual: {actual_line}"
        ));
    };
    let mut path = String::new();
    values_agree(&expected_value, &actual_value, &mut path)
}

/// Walks two parsed snapshots in step, recording the path to the first
/// disagreement.
fn values_agree(
    expected: &serde_json::Value,
    actual: &serde_json::Value,
    path: &mut String,
) -> Result<(), String> {
    use serde_json::Value;
    match (expected, actual) {
        (Value::Object(expected_map), Value::Object(actual_map)) => {
            if let Some(key) = expected_map
                .keys()
                .find(|key| !actual_map.contains_key(*key))
            {
                return Err(disagreement(
                    path,
                    &format!("golden has key `{key}`, snapshot does not"),
                ));
            }
            if let Some(key) = actual_map
                .keys()
                .find(|key| !expected_map.contains_key(*key))
            {
                return Err(disagreement(
                    path,
                    &format!("snapshot has key `{key}`, golden does not"),
                ));
            }
            for (key, expected_child) in expected_map {
                let restore = path.len();
                path.push('.');
                path.push_str(key);
                values_agree(expected_child, &actual_map[key], path)?;
                path.truncate(restore);
            }
            Ok(())
        }
        (Value::Array(expected_items), Value::Array(actual_items)) => {
            if expected_items.len() != actual_items.len() {
                return Err(disagreement(
                    path,
                    &format!(
                        "golden has {} item(s), snapshot has {}",
                        expected_items.len(),
                        actual_items.len()
                    ),
                ));
            }
            for (index, (expected_child, actual_child)) in
                expected_items.iter().zip(actual_items).enumerate()
            {
                let restore = path.len();
                write!(path, "[{index}]").expect("writing to a String cannot fail");
                values_agree(expected_child, actual_child, path)?;
                path.truncate(restore);
            }
            Ok(())
        }
        (Value::Number(expected_number), Value::Number(actual_number)) => {
            // Only a pair of fractional numbers can carry platform libm
            // disagreement. Counts, indices, and versions serialize as integers
            // and must match exactly, as must a value that changed between an
            // integer and a fractional form.
            let tolerant = match (expected_number.as_f64(), actual_number.as_f64()) {
                (Some(left), Some(right)) if expected_number.is_f64() && actual_number.is_f64() => {
                    floats_agree(left, right)
                }
                _ => expected_number == actual_number,
            };
            if tolerant {
                Ok(())
            } else {
                Err(disagreement(
                    path,
                    &format!("golden {expected_number}, snapshot {actual_number}"),
                ))
            }
        }
        _ if expected == actual => Ok(()),
        _ => Err(disagreement(
            path,
            &format!(
                "golden {}, snapshot {}",
                truncate_value(expected),
                truncate_value(actual)
            ),
        )),
    }
}

/// Renders one disagreement with the JSON path that located it.
fn disagreement(path: &str, detail: &str) -> String {
    let location = if path.is_empty() { "<root>" } else { path };
    format!("at `{location}`: {detail}")
}

/// Whether two fractional numbers agree within [`FLOAT_TOLERANCE`] relative to
/// the larger magnitude, with a floor of one so small values compare
/// absolutely.
fn floats_agree(left: f64, right: f64) -> bool {
    if left == right {
        return true;
    }
    if !left.is_finite() || !right.is_finite() {
        return false;
    }
    let magnitude = left.abs().max(right.abs()).max(1.0);
    (left - right).abs() <= FLOAT_TOLERANCE * magnitude
}

/// Renders a value for a failure message, bounded so a whole arena cannot land
/// in a panic.
fn truncate_value(value: &serde_json::Value) -> String {
    let text = value.to_string();
    if text.len() > 120 {
        format!("{}…", &text[..120])
    } else {
        text
    }
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

/// Serializes a value as the stable pretty JSON with trailing newline that the
/// goldens hold.
///
/// Snapshots serialize through [`serde_json::Value`], whose maps order by key,
/// so reordering a struct field does not rewrite every golden.
///
/// # Panics
///
/// Panics when the value cannot be serialized, which no codec output can cause.
#[must_use]
pub fn snapshot_text(value: &serde_json::Value) -> String {
    let mut text = serde_json::to_string_pretty(value).expect("serialize snapshot");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::{floats_agree, snapshots_agree, FLOAT_TOLERANCE};

    /// The two values one `FreeCAD` conical face produces on Linux against
    /// Windows and macOS. Their difference is platform libm disagreement, not
    /// codec drift, so the comparison must accept it.
    const LINUX_CONE_V: f64 = 1.802_581_857_082_682;
    const WINDOWS_CONE_V: f64 = 1.802_581_857_082_681_5;

    #[test]
    fn last_place_platform_disagreement_agrees() {
        assert_ne!(
            LINUX_CONE_V.to_bits(),
            WINDOWS_CONE_V.to_bits(),
            "the fixture values must differ, or this test proves nothing"
        );
        assert!(floats_agree(LINUX_CONE_V, WINDOWS_CONE_V));
        let golden = format!("{{\"v\": {LINUX_CONE_V:?}}}");
        let snapshot = format!("{{\"v\": {WINDOWS_CONE_V:?}}}");
        assert_ne!(golden, snapshot);
        assert!(snapshots_agree(&golden, &snapshot).is_ok());
    }

    #[test]
    fn drift_beyond_the_tolerance_disagrees() {
        let moved = LINUX_CONE_V * (1.0 + 1000.0 * FLOAT_TOLERANCE);
        assert!(!floats_agree(LINUX_CONE_V, moved));
        let error = snapshots_agree(
            &format!("{{\"v\": {LINUX_CONE_V:?}}}"),
            &format!("{{\"v\": {moved:?}}}"),
        )
        .expect_err("a change above the tolerance must be reported");
        assert!(error.contains(".v"), "{error}");
    }

    #[test]
    fn structure_and_strings_stay_exact() {
        for (golden, snapshot, expected_path) in [
            (r#"{"a": "x"}"#, r#"{"a": "y"}"#, ".a"),
            (r#"{"a": true}"#, r#"{"a": false}"#, ".a"),
            (r#"{"a": [1, 2]}"#, r#"{"a": [1, 2, 3]}"#, ".a"),
            (r#"{"a": {"b": 1}}"#, r#"{"a": {"c": 1}}"#, ".a"),
            (r#"{"a": 1}"#, r#"{"a": 2}"#, ".a"),
            (r#"{"a": 1.5}"#, r#"{"a": null}"#, ".a"),
        ] {
            let error = snapshots_agree(golden, snapshot)
                .expect_err("an exact-match field must be reported");
            assert!(
                error.contains(expected_path),
                "{golden} vs {snapshot}: {error}"
            );
        }
    }

    #[test]
    fn a_nested_path_is_reported() {
        let error = snapshots_agree(
            r#"{"ir": {"model": {"pcurves": [{"v": 1.0}, {"v": 2.0}]}}}"#,
            r#"{"ir": {"model": {"pcurves": [{"v": 1.0}, {"v": 9.0}]}}}"#,
        )
        .expect_err("a moved value must be reported");
        assert!(error.contains(".ir.model.pcurves[1].v"), "{error}");
    }

    #[test]
    fn small_values_compare_absolutely() {
        assert!(floats_agree(0.0, FLOAT_TOLERANCE / 2.0));
        assert!(!floats_agree(0.0, FLOAT_TOLERANCE * 10.0));
    }

    #[test]
    fn non_json_text_falls_back_to_a_line_diff() {
        let error = snapshots_agree("not json\nsecond\n", "not json\nthird\n")
            .expect_err("unparseable text must still be compared");
        assert!(error.contains("at line 2"), "{error}");
    }
}
