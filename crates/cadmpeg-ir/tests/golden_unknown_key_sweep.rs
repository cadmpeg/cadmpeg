// SPDX-License-Identifier: Apache-2.0
//! Every document-reachable shape in the committed goldens refuses an unknown
//! key.
//!
//! The sweep walks every `*.json` under `crates/*/tests/golden`, locates each
//! `CadIr` document by structure rather than by a fixed path, dedupes the
//! object shapes it finds by normalised path and key set, inserts one unknown
//! key into each, and asserts that the whole `CadIr` then fails to deserialise.
//! The codec-private `/native` subtree is free-form by design and is skipped.
//!
//! A document root is any object node carrying `ir_version`. `CadIr` serializes
//! that member as a constant with no skip, so a golden cannot hold a document
//! the walk does not see, wherever the codec's harness stores it: `cadmpeg-ir`
//! keeps its documents at `/ir`, `cadmpeg-codec-nx` at `/decode/ir`. A golden
//! that yields no document root states none: it carries no `ir` member, which
//! the walk checks separately. Those are the `inspect` reports, the `encode`
//! goldens, and the `decode_error` rejects.
//!
//! A golden whose document does not read back is not skipped in silence. The
//! only admitted read failure is the `native` elision marker the `FreeCAD`
//! golden test writes in place of the native arena; every other failure fails
//! this test with the golden's path and the serde error. The elided documents
//! are covered by `every_decoded_shape_refuses_an_unknown_key` in
//! `cadmpeg-codec-freecad`, which decodes the charter fixtures and runs this
//! same walk over the live document.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use cadmpeg_test_support::unknown_keys::accepting_shapes;
use serde_json::Value;

#[test]
fn every_golden_shape_refuses_an_unknown_key() {
    let goldens = golden_files();
    assert!(!goldens.is_empty(), "the sweep found no goldens to walk");
    let mut accepting = BTreeSet::new();
    let mut shapes_swept = 0_usize;
    let mut document_roots = 0_usize;
    let mut swept_documents = 0_usize;
    let mut elided_documents = 0_usize;
    for path in goldens {
        let text = std::fs::read_to_string(&path).expect("read golden");
        let golden: Value = serde_json::from_str(&text).expect("parse golden");
        let mut roots = Vec::new();
        let mut stray = Vec::new();
        collect_documents(&golden, &mut String::new(), &mut roots, &mut stray);
        assert!(
            stray.is_empty(),
            "{} states a document that carries no {DOCUMENT_VERSION_KEY}:\n{}",
            path.display(),
            stray.join("\n")
        );
        for (pointer, ir) in roots {
            document_roots += 1;
            match accepting_shapes(ir) {
                Ok((shapes, count)) => {
                    swept_documents += 1;
                    shapes_swept += count;
                    accepting.extend(shapes);
                }
                Err(error) => {
                    assert!(
                        states_native_elision(ir),
                        "{} at {pointer} does not read back as a CadIr and states no native elision: {error}",
                        path.display()
                    );
                    elided_documents += 1;
                }
            }
        }
    }
    assert_eq!(
        swept_documents + elided_documents,
        document_roots,
        "every golden document root is either swept or elided"
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

/// Member a golden harness stores a document under.
const DOCUMENT_KEY: &str = "ir";

/// Member every `CadIr` serializes, and the marker of a document root.
const DOCUMENT_VERSION_KEY: &str = "ir_version";

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

/// Every document root under `value`, with the pointer that reaches it, and
/// every `ir` member that states a document yet carries no `ir_version`.
///
/// A document root holds no second document root, so its subtree is not
/// descended.
fn collect_documents<'a>(
    value: &'a Value,
    pointer: &mut String,
    roots: &mut Vec<(String, &'a Value)>,
    stray: &mut Vec<String>,
) {
    match value {
        Value::Object(fields) => {
            if fields.contains_key(DOCUMENT_VERSION_KEY) {
                roots.push((pointer.clone(), value));
                return;
            }
            for (key, field) in fields {
                let parent = pointer.len();
                pointer.push('/');
                pointer.push_str(key);
                if key == DOCUMENT_KEY && field.get(DOCUMENT_VERSION_KEY).is_none() {
                    stray.push(pointer.clone());
                }
                collect_documents(field, pointer, roots, stray);
                pointer.truncate(parent);
            }
        }
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                let parent = pointer.len();
                pointer.push('/');
                pointer.push_str(&index.to_string());
                collect_documents(item, pointer, roots, stray);
                pointer.truncate(parent);
            }
        }
        _ => {}
    }
}

/// Every committed golden in the workspace.
fn golden_files() -> Vec<PathBuf> {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates directory")
        .to_path_buf();
    let mut found = Vec::new();
    let entries = std::fs::read_dir(&crates).expect("read crates directory");
    for entry in entries {
        let golden = entry.expect("crate entry").path().join("tests/golden");
        collect_goldens(&golden, &mut found);
    }
    found.sort();
    found
}

/// Appends every `*.json` under `directory`, recursively.
fn collect_goldens(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("golden entry").path();
        if path.is_dir() {
            collect_goldens(&path, found);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            found.push(path);
        }
    }
}
