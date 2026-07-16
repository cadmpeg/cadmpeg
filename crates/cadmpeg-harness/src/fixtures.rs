// SPDX-License-Identifier: Apache-2.0
//! Fixture discovery from the checked-in per-codec corpora.
//!
//! The whole-file inputs the harness decodes are the `<codec>_container/`
//! corpora already maintained for the fuzz fleet. Each such directory's prefix
//! names its codec; every regular file inside is a fixture. Discovery is a
//! read-only walk, so the harness owns no fixture data of its own.

use std::path::{Path, PathBuf};

use crate::execute::CODEC_IDS;

/// Environment override for the corpus root.
pub const ENV_CORPUS: &str = "CADMPEG_HARNESS_CORPUS";

/// The curated per-codec fixtures the fast regression gate covers, as
/// corpus-relative paths. One representative whole-file input per codec keeps
/// the gate quick; the full sweep discovers every fixture.
pub const GATE_FIXTURES: &[&str] = &[
    "f3d_container/full_f3d_with_smbh",
    "sldprt_container/synthetic_sldprt",
    "catia_container/standard_nested",
    "creo_container/minimal_prt",
    "nx_container/single_part",
    "rhino_container/archive_50",
];

/// One discovered fixture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fixture {
    /// Owning codec id.
    pub codec_id: String,
    /// Path relative to the corpus root, used as the baseline `fixture` key.
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

/// Resolve the curated [`GATE_FIXTURES`] under `corpus_root`, skipping any that
/// are absent so a slimmed corpus still runs.
pub fn gate_fixtures(corpus_root: &Path) -> Vec<Fixture> {
    let mut fixtures = Vec::new();
    for rel_path in GATE_FIXTURES {
        let abs_path = corpus_root.join(rel_path);
        if !abs_path.is_file() {
            continue;
        }
        let codec_id = rel_path
            .split('/')
            .next()
            .and_then(codec_of_container_dir)
            .unwrap_or("")
            .to_owned();
        fixtures.push(Fixture {
            codec_id,
            rel_path: (*rel_path).to_owned(),
            abs_path,
        });
    }
    fixtures
}
