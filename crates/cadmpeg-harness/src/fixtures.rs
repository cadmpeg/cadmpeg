// SPDX-License-Identifier: Apache-2.0
//! Fixture discovery from per-codec fuzz corpora.

use std::path::{Path, PathBuf};

use crate::execute::CODEC_IDS;

/// Environment override for the corpus root.
pub const ENV_CORPUS: &str = "CADMPEG_HARNESS_CORPUS";

/// One discovered fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// Owning codec id.
    pub codec_id: String,
    /// Path relative to the corpus root, used in result labels.
    pub rel_path: String,
    /// Absolute path on disk.
    pub abs_path: PathBuf,
}

/// The default corpus root: the fuzz seed corpora beside this crate. Overridden
/// by [`ENV_CORPUS`].
pub fn default_corpus_root() -> PathBuf {
    if let Ok(path) = std::env::var(ENV_CORPUS) {
        return PathBuf::from(path);
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../cadmpeg-fuzz/seeds")
}

/// Map a `<codec>_container` directory name to its codec id.
fn codec_of_container_dir(dir_name: &str) -> Option<&'static str> {
    let base = dir_name.strip_suffix("_container")?;
    CODEC_IDS.iter().copied().find(|id| *id == base)
}

/// Discover every whole-file fixture under `corpus_root`, sorted by relative
/// path for a deterministic order.
pub fn discover(corpus_root: &Path) -> std::io::Result<Vec<Fixture>> {
    let mut fixtures = Vec::new();
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(corpus_root)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            dirs.push(entry.file_name());
        }
    }
    dirs.sort();

    for dir_name in dirs {
        let Some(name) = dir_name.to_str() else {
            continue;
        };
        let Some(codec_id) = codec_of_container_dir(name) else {
            continue;
        };
        let dir_path = corpus_root.join(name);
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&dir_path)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                files.push(entry.file_name());
            }
        }
        files.sort();
        for file_name in files {
            let Some(file) = file_name.to_str() else {
                continue;
            };
            fixtures.push(Fixture {
                codec_id: codec_id.to_owned(),
                rel_path: format!("{name}/{file}"),
                abs_path: dir_path.join(file),
            });
        }
    }
    Ok(fixtures)
}
