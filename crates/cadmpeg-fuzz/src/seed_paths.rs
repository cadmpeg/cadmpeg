// SPDX-License-Identifier: Apache-2.0
//! Resolves the fuzz seed tree from `CARGO_MANIFEST_DIR`, so the current
//! working directory does not change where a generator writes.
//!
//! `CADMPEG_FUZZ_SEED_ROOT` redirects the seed tree to another directory.
//! `scripts/check-fuzz-seeds.py` sets it to compare a full generator run with
//! the checked-in tree. The donated fixture `crate_root` resolves is not
//! redirected: it is an input, not generator output.

use std::path::PathBuf;

/// The environment variable that redirects the seed tree.
const SEED_ROOT_ENV: &str = "CADMPEG_FUZZ_SEED_ROOT";

pub fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn seed_dir(target_or_relative: &str) -> PathBuf {
    let root = match std::env::var_os(SEED_ROOT_ENV) {
        Some(value) => PathBuf::from(value),
        None => crate_root().join("seeds"),
    };
    if target_or_relative == "seeds" {
        root
    } else if let Some(target) = target_or_relative.strip_prefix("seeds/") {
        root.join(target)
    } else {
        root.join(target_or_relative)
    }
}
