// SPDX-License-Identifier: Apache-2.0
//! Resolves the fuzz seed tree from `CARGO_MANIFEST_DIR`, so the current
//! working directory does not change where a generator writes.

use std::path::PathBuf;

pub fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

pub fn seed_dir(target_or_relative: &str) -> PathBuf {
    let root = crate_root();
    if target_or_relative == "seeds" || target_or_relative.starts_with("seeds/") {
        root.join(target_or_relative)
    } else {
        root.join("seeds").join(target_or_relative)
    }
}
