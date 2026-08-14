// SPDX-License-Identifier: Apache-2.0
// Resolve the fuzz seed tree from CARGO_MANIFEST_DIR.
// Included by the seed generator binaries. Fully qualified std::path types
// so include sites do not need extra imports.

fn crate_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn seed_dir(target_or_relative: &str) -> std::path::PathBuf {
    let root = crate_root();
    if target_or_relative == "seeds" || target_or_relative.starts_with("seeds/") {
        root.join(target_or_relative)
    } else {
        root.join("seeds").join(target_or_relative)
    }
}
