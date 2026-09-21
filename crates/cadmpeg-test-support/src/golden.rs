// SPDX-License-Identifier: Apache-2.0
//! Shared golden-snapshot harness for the codec crates.
//!
//! Every codec that pins snapshots does the same six things: enumerate
//! inputs, run each input through one or more branches, serialize the
//! result as stable pretty JSON, compare it against a committed golden, report
//! every difference at once, and refuse to pass when a branch has no inputs.
//! Frozen-file codecs call [`Harness::check`]; codecs that build inputs in
//! code call [`Harness::check_inputs`]. Only the codec type, the fixture
//! extension (file-backed codecs), and the regeneration hint differ.
//!
//! File-backed fixture inputs are frozen. A snapshot test can only tell a
//! decoder change apart from an input change while the inputs hold still, so
//! this harness never writes them; `UPDATE_GOLDEN=1` rewrites golden outputs
//! and nothing else.
//!
//! A regeneration writes a golden only where that golden's own comparison
//! fails. A golden the check accepts keeps its committed bytes, so a run over a
//! clean tree leaves the tree clean. Without that predicate every golden the
//! tolerance accepts is rewritten verbatim, and a regeneration of one codec
//! moves the last place of every other codec's numbers.
//!
//! `GOLDEN_STRICT=1` compares golden text byte-exactly instead of through
//! [`snapshots_agree`]; use it on one machine to confirm a change is exactly
//! behavior-preserving. Never enable it in CI — cross-platform libm drift
//! makes the mode flaky there.
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
//! to [`cadmpeg_ir::compare::values_agree`], which holds structure exact, tolerates
//! fractional numbers, and also tolerates fractional tokens embedded in string
//! fields (IGES encode goldens). Byte equality remains the fast path, and the
//! determinism check stays byte-exact because two runs in one process share one
//! libm and must agree bit for bit.
//!
//! The tolerance hides drift below [`cadmpeg_ir::compare::FLOAT_TOLERANCE`] relative
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

use cadmpeg_ir::compare::{is_local_digest_attribute, values_agree};

/// One snapshot branch: a subdirectory of `tests/golden` and the function that
/// produces the text pinned there.
pub struct Branch {
    /// Named snapshot subdirectory, or the golden root.
    kind: Option<&'static str>,
    /// Serializes one fixture's bytes as the snapshot text.
    pub snapshot: fn(&[u8]) -> String,
}

impl Branch {
    /// Names a branch and the function that produces its snapshot.
    #[must_use]
    pub const fn named(kind: &'static str, snapshot: fn(&[u8]) -> String) -> Self {
        Self {
            kind: Some(kind),
            snapshot,
        }
    }

    /// Places snapshots directly in the golden root.
    #[must_use]
    pub const fn root(snapshot: fn(&[u8]) -> String) -> Self {
        Self {
            kind: None,
            snapshot,
        }
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
    /// Set `UPDATE_GOLDEN` to rewrite the goldens this comparison rejects;
    /// an accepted golden and every fixture input stay as they are.
    ///
    /// # Panics
    ///
    /// Panics listing every drifted, unreadable, and input-less golden at once
    /// rather than stopping at the first.
    pub fn check(&self, branches: &[Branch]) {
        self.finish_check(&self.fixture_inputs(), branches, true);
    }

    /// Compares every branch for every caller-built input.
    ///
    /// Use this when fixtures are constructed in code rather than read from
    /// `tests/golden/fixtures`. `UPDATE_GOLDEN` rewrites the rejected goldens
    /// only.
    ///
    /// A [`Branch::root`] branch writes `{name}.json`
    /// directly under `tests/golden`. Otherwise `{name}.json` lives under
    /// `tests/golden/{kind}/`.
    ///
    /// JSON files in those directories whose stems are not in `inputs` fail as
    /// orphans. An empty `inputs` slice panics so the check cannot pass
    /// vacuously.
    ///
    /// # Panics
    ///
    /// Panics listing every drifted, unreadable, and input-less golden at once
    /// rather than stopping at the first. Panics when `inputs` is empty.
    pub fn check_inputs(&self, inputs: &[(String, Vec<u8>)], branches: &[Branch]) {
        assert!(
            !inputs.is_empty(),
            "no in-memory golden inputs; the harness would pass vacuously"
        );
        self.finish_check(inputs, branches, false);
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
        self.finish_determinism(&self.fixture_inputs(), branches);
    }

    /// Byte-exact determinism check over caller-built inputs.
    ///
    /// # Panics
    ///
    /// Panics when `inputs` is empty, or on the first branch whose two runs
    /// disagree.
    pub fn check_determinism_inputs(&self, inputs: &[(String, Vec<u8>)], branches: &[Branch]) {
        assert!(
            !inputs.is_empty(),
            "no in-memory golden inputs; the harness would pass vacuously"
        );
        self.finish_determinism(inputs, branches);
    }

    fn finish_check(&self, inputs: &[(String, Vec<u8>)], branches: &[Branch], from_files: bool) {
        let update = std::env::var_os("UPDATE_GOLDEN").is_some();
        let names: Vec<String> = inputs.iter().map(|(name, _)| name.clone()).collect();
        let mut failures: Vec<String> = Vec::new();
        for branch in branches {
            failures.extend(self.compare_branch(branch, inputs, &names, update, from_files));
        }
        assert!(
            failures.is_empty(),
            "{} golden(s) drifted; if the change is intended regenerate with `{}` and review the diff:\n\n{}",
            failures.len(),
            self.regenerate,
            failures.join("\n\n")
        );
    }

    #[allow(clippy::unused_self)] // pair with `finish_check`; paths unused here
    fn finish_determinism(&self, inputs: &[(String, Vec<u8>)], branches: &[Branch]) {
        for (name, bytes) in inputs {
            for branch in branches {
                let first = (branch.snapshot)(bytes);
                let second = (branch.snapshot)(bytes);
                if first != second {
                    let (line, one, two) = first_line_diff(&first, &second);
                    panic!(
                        "fixture `{name}`: nondeterministic {} at line {line}\n    run 1: {one}\n    run 2: {two}",
                        branch.kind.unwrap_or("snapshot")
                    );
                }
            }
        }
    }

    /// Compares one branch, returning one failure per drifted or unreadable
    /// golden plus one per golden with no input behind it. Under `update` a
    /// drifted or unreadable golden is written instead of reported; a golden
    /// the comparison accepts is left untouched either way.
    fn compare_branch(
        &self,
        branch: &Branch,
        inputs: &[(String, Vec<u8>)],
        names: &[String],
        update: bool,
        from_files: bool,
    ) -> Vec<String> {
        let dir = branch.kind.map_or_else(
            || self.golden_dir.clone(),
            |kind| self.golden_dir.join(kind),
        );
        let kind = branch.kind.unwrap_or("snapshot");
        let mut failures: Vec<String> = Vec::new();
        for (name, bytes) in inputs {
            let actual = (branch.snapshot)(bytes);
            let path = dir.join(format!("{name}.json"));
            let failure = match read_golden(&path) {
                Ok(expected) => compare_branch_snapshot(&expected, &actual).err().map(
                    |mismatch| {
                        format!(
                            "fixture `{name}`: {kind} diverged from {}\n    {mismatch}",
                            path.display()
                        )
                    },
                ),
                Err(error) => Some(format!(
                    "fixture `{name}`: cannot read {kind} golden {} ({error}); regenerate with `{}`",
                    path.display(),
                    self.regenerate
                )),
            };
            let Some(failure) = failure else { continue };
            if update {
                std::fs::write(&path, actual.as_bytes())
                    .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
            } else {
                failures.push(failure);
            }
        }
        for orphan in stems(&dir, "json")
            .iter()
            .filter(|name| !names.contains(name))
        {
            failures.push(if from_files {
                format!(
                    "golden `{orphan}.json` under {} has no `{orphan}.{}` fixture; delete the golden or restore the input",
                    dir.display(),
                    self.fixture_extension
                )
            } else {
                format!(
                    "golden `{orphan}.json` under {} has no input `{orphan}`; delete the golden or restore the input",
                    dir.display()
                )
            });
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
        .filter_map(|path| {
            let (stem, found) = path.file_stem().zip(path.extension())?;
            (found == extension).then(|| stem.to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names
}

/// Reads a golden with `\r\n` folded to `\n`.
fn read_golden(path: &Path) -> std::io::Result<String> {
    Ok(std::fs::read_to_string(path)?.replace("\r\n", "\n"))
}

/// Compares one branch golden to a fresh snapshot.
///
/// When `GOLDEN_STRICT` is set, comparison is byte-exact and reports the first
/// differing line. Otherwise delegates to [`snapshots_agree`].
fn compare_branch_snapshot(expected: &str, actual: &str) -> Result<(), String> {
    if std::env::var_os("GOLDEN_STRICT").is_some() {
        if expected == actual {
            return Ok(());
        }
        let (line, golden_line, actual_line) = first_line_diff(expected, actual);
        return Err(format!(
            "at line {line}\n    golden: {golden_line}\n    actual: {actual_line}"
        ));
    }
    snapshots_agree(expected, actual)
}

/// Compares a golden against a fresh snapshot, tolerating only last-place
/// disagreement in fractional numbers.
///
/// Byte-equal fast path; otherwise JSON via [`cadmpeg_ir::compare::values_agree`],
/// or a line diff for non-JSON text.
///
/// # Errors
///
/// Returns a description locating the first disagreement.
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
                    Some(text) => match text.char_indices().nth(200) {
                        Some((end, _)) => format!("{}…", &text[..end]),
                        None => text.to_owned(),
                    },
                    None => "<end of file>".to_owned(),
                };
                return (line, truncate(left), truncate(right));
            }
        }
    }
}

/// Stands in for a digest a snapshot cannot pin across platforms.
pub const ELIDED_DIGEST: &str = "<elided: digest over tolerantly compared geometry>";

/// Key a snapshot writes in place of a native arena tree.
///
/// The `FreeCAD` decode snapshot writes it; the `cadmpeg-ir` golden sweep reads
/// it to tell an elided native subtree apart from a document that fails to read
/// back. The sweep recognises the key carrying any string.
pub const NATIVE_ELISION_KEY: &str = "__elided";

/// Sentence the `FreeCAD` decode snapshot writes under [`NATIVE_ELISION_KEY`].
pub const NATIVE_ELISION_MARKER: &str =
    "native arena values are omitted; the records are pinned by a digest over their canonical JSON";

/// Replaces every machine-local digest attribute with [`ELIDED_DIGEST`].
///
/// `_local_sha256` digests cover decoded content and are machine-local. Digests
/// over retained source bytes omit the suffix and stay pinned.
pub fn elide_local_digests(
    attributes: &mut std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, String>,
) {
    for (key, value) in attributes.iter_mut() {
        if is_local_digest_attribute(key.as_str()) {
            ELIDED_DIGEST.clone_into(value);
        }
    }
}

/// Serializes a value as the stable pretty JSON with trailing newline that the
/// goldens hold.
///
/// Snapshots serialize through [`serde_json::Value`], whose maps order by key,
/// so reordering a struct field does not rewrite every golden.
#[must_use]
pub fn snapshot_text(value: &serde_json::Value) -> String {
    format!("{value:#}\n")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Mutex, MutexGuard};

    use super::{
        compare_branch_snapshot, first_line_diff, snapshot_text, snapshots_agree, Branch, Harness,
    };
    use cadmpeg_ir::compare::FLOAT_TOLERANCE;

    /// Serializes tests that mutate an environment key this harness reads, so
    /// parallel workers cannot observe a half-applied environment.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Sets or clears one environment key for the duration of a test scope.
    struct EnvGuard {
        _lock: MutexGuard<'static, ()>,
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: Option<&str>) -> Self {
            let lock = ENV_LOCK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let previous = std::env::var_os(key);
            // SAFETY: exclusive access to this process env key is held via
            // `ENV_LOCK` for the guard lifetime.
            unsafe {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
            Self {
                _lock: lock,
                key,
                previous,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            // SAFETY: same exclusive lock as `set`; restores prior value.
            unsafe {
                match &self.previous {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    /// A crate-shaped golden tree in the temporary directory, removed when the
    /// scope ends. [`Harness::new`] takes the manifest directory as text, so a
    /// test can point one at a tree it owns and read back what a regeneration
    /// wrote.
    struct TempTree(PathBuf);

    impl TempTree {
        /// Builds `tests/golden/decode/` under a directory named for this
        /// process and `name`.
        fn named(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("cadmpeg-golden-{}-{name}", std::process::id()));
            std::fs::create_dir_all(root.join("tests/golden/decode"))
                .expect("create the temporary golden tree");
            Self(root)
        }

        /// The directory a harness resolves its golden paths against.
        fn manifest_dir(&self) -> &str {
            self.0.to_str().expect("the temporary path is UTF-8")
        }

        /// The `decode` branch golden of one input.
        fn golden(&self, name: &str) -> PathBuf {
            self.0
                .join("tests/golden/decode")
                .join(format!("{name}.json"))
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.0) {
                eprintln!("remove {}: {error}", self.0.display());
            }
        }
    }

    #[test]
    fn line_diff_truncates_at_a_character_boundary() {
        let prefix = "a".repeat(199);
        let (_, actual, _) = first_line_diff(&format!("{prefix}é-rest"), "different");
        assert_eq!(actual, format!("{prefix}é…"));
        let short = "é".repeat(110);
        let (_, actual, _) = first_line_diff(&short, "different");
        assert_eq!(actual, short);
    }

    #[test]
    fn value_display_preserves_pretty_snapshot_bytes() {
        for value in [
            serde_json::Value::Null,
            serde_json::json!({"z": [1.25, true, null], "a": "é\n\""}),
        ] {
            let expected =
                serde_json::to_string_pretty(&value).expect("pretty JSON for the fixture") + "\n";
            assert_eq!(snapshot_text(&value), expected);
        }
    }

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
    fn sub_tolerance_float_fails_only_under_golden_strict() {
        let golden = format!("{{\"v\": {LINUX_CONE_V:?}}}");
        let snapshot = format!("{{\"v\": {WINDOWS_CONE_V:?}}}");
        assert_ne!(
            golden, snapshot,
            "the texts must differ, or this test proves nothing"
        );

        {
            let _unset = EnvGuard::set("GOLDEN_STRICT", None);
            assert!(
                compare_branch_snapshot(&golden, &snapshot).is_ok(),
                "default path must tolerate sub-tolerance float drift"
            );
        }

        let _strict = EnvGuard::set("GOLDEN_STRICT", Some("1"));
        let error = compare_branch_snapshot(&golden, &snapshot)
            .expect_err("GOLDEN_STRICT must reject byte-unequal text");
        assert!(error.contains("at line 1"), "{error}");
        assert!(error.contains("golden:"), "{error}");
        assert!(error.contains("actual:"), "{error}");
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

    /// The text a committed golden holds in the regeneration tests.
    fn committed_text() -> String {
        format!("{{\n  \"v\": {LINUX_CONE_V:?}\n}}\n")
    }

    /// A snapshot that disagrees with [`committed_text`] in the last place of
    /// one fractional number, which the comparison accepts.
    fn last_place_snapshot(_bytes: &[u8]) -> String {
        format!("{{\n  \"v\": {WINDOWS_CONE_V:?}\n}}\n")
    }

    /// A snapshot that disagrees with [`committed_text`] far above the
    /// tolerance, which the comparison rejects.
    fn drifted_snapshot(_bytes: &[u8]) -> String {
        format!("{{\n  \"v\": {:?}\n}}\n", LINUX_CONE_V * 2.0)
    }

    /// One nameless input, since these branches ignore the input bytes.
    fn one_input() -> Vec<(String, Vec<u8>)> {
        vec![("fixture".to_owned(), Vec::new())]
    }

    #[test]
    fn a_regeneration_keeps_a_golden_the_comparison_accepts() {
        let tree = TempTree::named("accepted");
        let committed = committed_text();
        std::fs::write(tree.golden("fixture"), &committed).expect("write the committed golden");
        let snapshot = last_place_snapshot(&[]);
        assert_ne!(
            snapshot, committed,
            "the texts must differ, or this test proves nothing"
        );

        let _update = EnvGuard::set("UPDATE_GOLDEN", Some("1"));
        Harness::new(tree.manifest_dir(), "bin", "regenerate").check_inputs(
            &one_input(),
            &[Branch::named("decode", last_place_snapshot)],
        );

        assert_eq!(
            std::fs::read_to_string(tree.golden("fixture")).expect("read the golden back"),
            committed,
            "a regeneration must leave a golden its own comparison accepts"
        );
    }

    #[test]
    fn a_regeneration_writes_a_golden_the_comparison_rejects() {
        let tree = TempTree::named("rejected");
        std::fs::write(tree.golden("fixture"), committed_text())
            .expect("write the committed golden");

        let _update = EnvGuard::set("UPDATE_GOLDEN", Some("1"));
        Harness::new(tree.manifest_dir(), "bin", "regenerate")
            .check_inputs(&one_input(), &[Branch::named("decode", drifted_snapshot)]);

        assert_eq!(
            std::fs::read_to_string(tree.golden("fixture")).expect("read the golden back"),
            drifted_snapshot(&[]),
            "a regeneration must write the golden its own comparison rejects"
        );
    }

    #[test]
    #[should_panic(expected = "golden(s) drifted")]
    fn a_check_without_a_regeneration_reports_the_rejected_golden() {
        let tree = TempTree::named("reported");
        std::fs::write(tree.golden("fixture"), committed_text())
            .expect("write the committed golden");

        let _update = EnvGuard::set("UPDATE_GOLDEN", None);
        Harness::new(tree.manifest_dir(), "bin", "regenerate")
            .check_inputs(&one_input(), &[Branch::named("decode", drifted_snapshot)]);
    }
}
