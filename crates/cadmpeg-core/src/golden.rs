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
//! equality already covers, so [`snapshots_agree`] parses both sides and defers
//! to [`crate::compare::values_agree`], which holds structure and strings exact
//! and tolerates only fractional numbers. Byte equality remains the fast path,
//! and the determinism check stays byte-exact because two runs in one process
//! share one libm and must agree bit for bit.
//!
//! The tolerance hides drift below [`crate::compare::FLOAT_TOLERANCE`] relative
//! magnitude. It does not make decode reproducible across platforms; it only
//! stops the goldens from reporting that as codec drift.
//!
//! ## What this does not cover
//!
//! Perturbing every libm transcendental by one unit in the last place, which
//! stands in for the disagreement between platforms, leaves seven of the eight
//! codecs' snapshot suites passing. Two kinds of golden remain exposed, and
//! neither has a fix at this layer:
//!
//! - A **byte-exact binary golden over written geometry** admits no tolerance,
//!   because a container's bytes have no numeric structure to compare. Fusion's
//!   `tests/golden/generate/*.bin` move under the shim while every JSON
//!   comparison holds. Comparing such a golden semantically — per archive entry,
//!   with geometry compared numerically — is the only real remedy.
//! - A **digest over decoded geometry** is a bitwise fingerprint of tolerantly
//!   compared values, so it cannot agree across platforms either. See
//!   [`elide_local_digests`].

use std::path::{Path, PathBuf};

use crate::compare::{is_local_digest_attribute, values_agree};

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

    /// The frozen fixture inputs as `(stem, bytes)`, in sorted stem order.
    ///
    /// Exposed for checks that consume the same inputs as the snapshot branches
    /// without pinning an artifact, such as a round-trip over the encoder. It
    /// carries the same guard against an empty fixture directory.
    ///
    /// # Panics
    ///
    /// Panics when the fixture directory cannot be read, holds no input with the
    /// configured extension, or a listed fixture cannot be read.
    #[must_use]
    pub fn fixture_inputs(&self) -> Vec<(String, Vec<u8>)> {
        self.fixtures()
            .into_iter()
            .map(|name| {
                let bytes = self.fixture_bytes(&name);
                (name, bytes)
            })
            .collect()
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
/// structurally through [`crate::compare::values_agree`]. Text that does not
/// parse as JSON falls back to a line diff.
///
/// # Errors
///
/// Returns a description locating the first disagreement, by JSON path when
/// both sides parsed and by line number otherwise. A path-located message reads
/// `left` for the golden and `right` for the fresh snapshot.
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
    values_agree(&expected_value, &actual_value)
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

/// Stands in for a digest a snapshot cannot pin across platforms.
pub const ELIDED_DIGEST: &str = "<elided: digest over tolerantly compared geometry>";

/// Replaces every machine-local digest attribute with [`ELIDED_DIGEST`].
///
/// A digest of decoded geometry is a bitwise fingerprint of the very values this
/// harness compares tolerantly, so pinning one contradicts the comparison: the
/// same document decoded on another platform agrees to fourteen significant
/// digits and still hashes differently, and no tolerance can rescue a hash.
/// Perturbing every libm transcendental by one unit in the last place moves
/// `document_local_sha256` on ten Fusion goldens and `brep_local_sha256` on two
/// `SolidWorks` goldens while leaving every geometry comparison satisfied.
///
/// Which attributes those are is decided by
/// [`is_local_digest_attribute`], not by a per-codec list: a codec that adds a
/// digest over decoded content names it by the convention and is elided without
/// touching this harness. A digest over retained source bytes does not carry the
/// suffix, holds still under the shim, and stays pinned — pinning it is what
/// catches a native write regression.
///
/// Eliding costs almost nothing, because the model each digest covers is pinned
/// in the same snapshot; what remains uncovered is a change to the digest
/// algorithm itself, which `cadmpeg-ir`'s own pinned-digest test covers.
pub fn elide_local_digests(attributes: &mut std::collections::BTreeMap<String, String>) {
    for (key, value) in attributes.iter_mut() {
        if is_local_digest_attribute(key) {
            ELIDED_DIGEST.clone_into(value);
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
    use super::snapshots_agree;
    use crate::compare::FLOAT_TOLERANCE;

    /// The two values one `FreeCAD` conical face produces on Linux against
    /// Windows and macOS. Their difference is platform libm disagreement, not
    /// codec drift, so the comparison must accept it.
    const LINUX_CONE_V: f64 = 1.802_581_857_082_682;
    const WINDOWS_CONE_V: f64 = 1.802_581_857_082_681_5;

    #[test]
    fn byte_identical_text_agrees_without_parsing() {
        assert!(snapshots_agree("not json at all\n", "not json at all\n").is_ok());
    }

    #[test]
    fn last_place_platform_disagreement_agrees() {
        let golden = format!("{{\"v\": {LINUX_CONE_V:?}}}");
        let snapshot = format!("{{\"v\": {WINDOWS_CONE_V:?}}}");
        assert_ne!(
            golden, snapshot,
            "the texts must differ, or this test proves nothing"
        );
        assert!(snapshots_agree(&golden, &snapshot).is_ok());
    }

    #[test]
    fn drift_beyond_the_tolerance_disagrees() {
        let moved = LINUX_CONE_V * (1.0 + 1000.0 * FLOAT_TOLERANCE);
        let error = snapshots_agree(
            &format!("{{\"v\": {LINUX_CONE_V:?}}}"),
            &format!("{{\"v\": {moved:?}}}"),
        )
        .expect_err("a change above the tolerance must be reported");
        assert!(error.contains(".v"), "{error}");
    }

    #[test]
    fn non_json_text_falls_back_to_a_line_diff() {
        let error = snapshots_agree("not json\nsecond\n", "not json\nthird\n")
            .expect_err("unparseable text must still be compared");
        assert!(error.contains("at line 2"), "{error}");
    }
}
