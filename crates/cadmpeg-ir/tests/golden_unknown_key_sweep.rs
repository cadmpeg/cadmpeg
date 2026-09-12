// SPDX-License-Identifier: Apache-2.0
//! Every document-reachable shape in the committed decode goldens refuses an
//! unknown key.
//!
//! The sweep walks each golden's `ir`, dedupes the object shapes it finds by
//! normalised path and key set, inserts one unknown key into each, and asserts
//! that the whole `CadIr` then fails to deserialise. The codec-private
//! `/native` subtree is free-form by design and is skipped.
//!
//! A golden whose `ir` does not read back is not skipped in silence. The only
//! admitted read failure is the `native` elision marker the `FreeCAD` golden
//! test writes in place of the native arena; every other failure fails this
//! test with the golden's path and the serde error. The twelve elided
//! documents are covered by `every_decoded_shape_refuses_an_unknown_key` in
//! `cadmpeg-codec-freecad`, which decodes the charter fixtures and runs this
//! same walk over the live document.

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
    let mut shapes_swept = 0_usize;
    let mut documents_with_ir = 0_usize;
    let mut swept_documents = 0_usize;
    let mut elided_documents = 0_usize;
    for path in goldens {
        let text = std::fs::read_to_string(&path).expect("read golden");
        let document: Value = serde_json::from_str(&text).expect("parse golden");
        let Some(ir) = document.get("ir") else {
            continue;
        };
        documents_with_ir += 1;
        match accepting_shapes(ir) {
            Ok((shapes, count)) => {
                swept_documents += 1;
                shapes_swept += count;
                accepting.extend(shapes);
            }
            Err(error) => {
                assert!(
                    states_native_elision(ir),
                    "{} does not read back as a CadIr and states no native elision: {error}",
                    path.display()
                );
                elided_documents += 1;
            }
        }
    }
    assert_eq!(
        swept_documents + elided_documents,
        documents_with_ir,
        "every golden with an ir is either swept or elided"
    );
    assert!(
        swept_documents > 0 && shapes_swept > 0,
        "the sweep walked {swept_documents} documents and {shapes_swept} shapes"
    );
    assert!(
        accepting.is_empty(),
        "{} golden shapes accept an unknown key:\n{}",
        accepting.len(),
        accepting.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// Key the `FreeCAD` golden test writes in place of the native arena.
const NATIVE_ELISION_KEY: &str = "__elided";

/// Value that key carries.
const NATIVE_ELISION_MARKER: &str =
    "native arena values are omitted; structure is pinned by identity";

/// Whether this document's `native` arena is the elision marker rather than a
/// readable arena.
fn states_native_elision(ir: &Value) -> bool {
    ir.get("native")
        .and_then(|native| native.get(NATIVE_ELISION_KEY))
        .and_then(Value::as_str)
        == Some(NATIVE_ELISION_MARKER)
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
