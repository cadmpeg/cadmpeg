// SPDX-License-Identifier: Apache-2.0
//! Every document-reachable shape in the committed decode goldens refuses an
//! unknown key.
//!
//! The sweep walks each golden's `ir`, dedupes the object shapes it finds by
//! normalised path and key set, inserts one unknown key into each, and asserts
//! that the whole `CadIr` then fails to deserialise. The codec-private
//! `/native` subtree is free-form by design and is skipped.
//!
//! The `FreeCAD` decode goldens store `native` as an elision marker, so their
//! `ir` does not read back as a `CadIr` at all. This walker skips a document
//! whose `ir` does not read back; the companion sweep in
//! `cadmpeg-codec-freecad` covers those documents from a live decode.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cadmpeg_test_support::unknown_keys::accepting_shapes;
use serde_json::Value;

#[test]
fn every_golden_shape_refuses_an_unknown_key() {
    let goldens = golden_documents();
    assert!(
        !goldens.is_empty(),
        "the sweep found no decode goldens to walk"
    );
    let mut accepting = BTreeSet::new();
    let mut swept = 0_usize;
    let mut documents = 0_usize;
    for path in goldens {
        let text = std::fs::read_to_string(&path).expect("read golden");
        let document: Value = serde_json::from_str(&text).expect("parse golden");
        let Some(ir) = document.get("ir") else {
            continue;
        };
        let (shapes, count) = accepting_shapes(ir);
        if count == 0 {
            continue;
        }
        documents += 1;
        swept += count;
        accepting.extend(shapes);
    }
    assert!(
        documents > 0 && swept > 0,
        "the sweep read {documents} documents and {swept} shapes"
    );
    assert!(
        accepting.is_empty(),
        "{} golden shapes accept an unknown key:\n{}",
        accepting.len(),
        accepting.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// Every committed decode golden in the workspace.
fn golden_documents() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf();
    let mut found = Vec::new();
    let entries = std::fs::read_dir(&crates).expect("read crates directory");
    for entry in entries {
        let decode = entry
            .expect("crate entry")
            .path()
            .join("tests/golden/decode");
        let Ok(files) = std::fs::read_dir(&decode) else {
            continue;
        };
        for file in files {
            let file = file.expect("golden entry").path();
            if file
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                found.push(file);
            }
        }
    }
    found.sort();
    found
}
