// SPDX-License-Identifier: Apache-2.0
use serde_json::{json, Value};

use super::{base_text_at, decode_golden_from_text};

/// A scratch git repository, removed when the scope ends.
struct TempRepo {
    root: std::path::PathBuf,
    _scratch: tempfile_guard::ScratchDir,
}

mod tempfile_guard {
    pub(super) struct ScratchDir {
        pub(super) path: std::path::PathBuf,
    }

    impl ScratchDir {
        pub(super) fn named(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("cadmpeg-base-{}-{name}", std::process::id()));
            if path.exists() {
                std::fs::remove_dir_all(&path).expect("clear the scratch directory");
            }
            std::fs::create_dir_all(&path).expect("create the scratch directory");
            Self { path }
        }
    }

    impl Drop for ScratchDir {
        fn drop(&mut self) {
            if let Err(error) = std::fs::remove_dir_all(&self.path) {
                eprintln!("remove {}: {error}", self.path.display());
            }
        }
    }
}

impl TempRepo {
    fn named(name: &str) -> Self {
        let scratch = tempfile_guard::ScratchDir::named(name);
        let root = scratch.path.clone();
        let repo = Self {
            root,
            _scratch: scratch,
        };
        repo.git(&["init", "-q", "-b", "main"]);
        repo.git(&["config", "user.name", "Test"]);
        repo.git(&["config", "user.email", "test@example.invalid"]);
        repo.git(&["config", "commit.gpgsign", "false"]);
        repo
    }

    fn git(&self, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .args(args)
            .current_dir(&self.root)
            .output()
            .expect("run git");
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    fn commit(&self, files: &[(&str, &str)]) {
        for (path, text) in files {
            let file = self.root.join(path);
            std::fs::create_dir_all(file.parent().expect("golden has a parent"))
                .expect("create the golden parent");
            std::fs::write(file, text).expect("write the golden");
        }
        self.git(&["add", "-A"]);
        self.git(&["commit", "-q", "-m", "goldens"]);
    }
}

fn golden_text(ir: &Value) -> String {
    super::super::golden::snapshot_text(&json!({"decode": {"ir": ir}}))
}

#[test]
fn a_golden_present_at_base_reads_back() {
    let repo = TempRepo::named("present");
    let ir = json!({"model": {}, "native": {}});
    repo.commit(&[("a/decode/x.json", &golden_text(&ir))]);
    let base = repo.git(&["rev-parse", "HEAD"]);
    let text = base_text_at(&repo.root, base.trim(), "a/decode/x.json").expect("present at base");
    let golden = decode_golden_from_text("a/decode/x.json", &text).expect("holds a document");
    assert_eq!(golden.ir, ir);
    assert_eq!(golden.path, "a/decode/x.json");
}

#[test]
fn a_golden_absent_at_base_reads_as_absent() {
    let repo = TempRepo::named("absent");
    repo.commit(&[("a/decode/kept.json", &golden_text(&json!({})))]);
    let base = repo.git(&["rev-parse", "HEAD"]);
    repo.commit(&[("a/decode/added.json", &golden_text(&json!({})))]);
    assert!(base_text_at(&repo.root, base.trim(), "a/decode/added.json").is_none());
    assert!(base_text_at(&repo.root, base.trim(), "a/decode/kept.json").is_some());
}

#[test]
fn a_base_decode_error_holds_no_document() {
    let text = super::super::golden::snapshot_text(&json!({"decode": {"decode_error": "nope"}}));
    assert!(decode_golden_from_text("a/decode/x.json", &text).is_none());
}

#[test]
#[should_panic(expected = "neither `ir` nor `decode_error`")]
fn a_corrupt_base_golden_fails() {
    assert!(decode_golden_from_text("a/decode/x.json", "{}").is_some());
}
