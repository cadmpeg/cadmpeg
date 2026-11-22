// SPDX-License-Identifier: Apache-2.0
//! Fixture-file writer shared by the `cadmpeg query` integration test binaries.

use std::fs;

/// Write `content` to `name` under `dir` and return the path.
pub(crate) fn write(dir: &std::path::Path, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    fs::write(&path, content).unwrap();
    path
}
