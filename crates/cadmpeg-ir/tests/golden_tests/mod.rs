// SPDX-License-Identifier: Apache-2.0
//! Every document-reachable shape in the committed goldens refuses an unknown
//! key.
//!
//! The sweep walks every `*.json` under `crates/*/tests/golden`, locates each
//! `CadIr` document by structure rather than by a fixed path, inserts one
//! unknown key into every object, and checks each through its document route.
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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use cadmpeg_ir::CadIr;
use cadmpeg_test_support::golden::{NATIVE_ELISION_KEY, NATIVE_ELISION_MARKER};
use cadmpeg_test_support::unknown_keys::accepting_shapes;
use serde_json::Value;

#[test]
fn every_golden_shape_refuses_an_unknown_key() {
    let goldens = golden_files();
    assert!(!goldens.is_empty(), "the sweep found no goldens to walk");
    let mut accepting = BTreeSet::new();
    let mut shapes_swept = 0_usize;
    let mut fallbacks = 0_usize;
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
                Ok(sweep) => {
                    swept_documents += 1;
                    shapes_swept += sweep.swept;
                    fallbacks += sweep.fallbacks;
                    accepting.extend(sweep.accepting);
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
    println!("swept {shapes_swept} shapes, {fallbacks} of them in the full document");
    let unexpected = accepting
        .iter()
        .filter(|shape| !is_free_form(shape))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "{} golden shapes accept an unknown key:\n{}",
        unexpected.len(),
        unexpected.join("\n")
    );

    // Every listed shape is load-bearing: it is reached by a committed golden
    // or by a hand-written document below that exercises it. An entry proven
    // by neither is over-listing, and over-listing hides an entry that has
    // stopped being free-form.
    for (name, document) in hand_written_documents() {
        let sweep = accepting_shapes(&document)
            .unwrap_or_else(|error| panic!("the {name} document reads back as a CadIr: {error}"));
        assert!(
            sweep.swept > 0,
            "the {name} document states a shape to sweep"
        );
        accepting.extend(sweep.accepting);
    }
    let unreached = cadmpeg_test_support::unknown_keys::FREE_FORM_SHAPES
        .iter()
        .filter(|shape| !accepting.contains(**shape))
        .copied()
        .collect::<Vec<_>>();
    assert!(
        unreached.is_empty(),
        "{} free-form shapes are listed and reached by nothing:\n{}",
        unreached.len(),
        unreached.join("\n")
    );
}

/// Minimal documents that reach the free-form shapes no committed golden
/// carries.
///
/// Each is a whole `CadIr` built from the IR's own types, so a field that
/// stops being a free-form map stops reaching its shape here and the sweep
/// says so.
fn hand_written_documents() -> Vec<(&'static str, Value)> {
    use cadmpeg_ir::presentation::{
        CameraState, PresentationDocument, PresentationState, PresentationStateKind,
        ViewPresentation,
    };
    use cadmpeg_ir::products::{ProductDefinition, ProductDefinitionKind};
    use cadmpeg_ir::references::{ReferenceSelection, ReferenceTarget};
    use cadmpeg_ir::semantic_annotations::{SemanticAnnotation, SemanticAnnotationKind};

    let named = |value: &str| {
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("zz_source_name"),
            value.to_string(),
        )])
    };

    let mut presentation = CadIr::empty();
    let mut document = PresentationDocument::new(
        "test:model:presentation#0"
            .try_into()
            .expect("valid identity"),
    );
    document
        .set_states(vec![PresentationState {
            kind: PresentationStateKind::Camera(CameraState {
                position: None,
                orientation: None,
                properties: named("camera"),
            }),
            order: 0,
            attributes: named("state"),
            assets: Vec::new(),
        }])
        .expect("distinct state orders");
    presentation.model.presentation_documents.push(document);
    presentation
        .model
        .view_presentations
        .push(ViewPresentation {
            id: "test:model:presentation#1"
                .try_into()
                .expect("valid identity"),
            object: None,
            order: 0,
            expanded: None,
            visible: None,
            display_mode: None,
            selection_style: None,
            line_width: None,
            point_size: None,
            properties: named("view"),
            native_ref: None,
        });

    let mut products = CadIr::empty();
    products.model.product_definitions.push(ProductDefinition {
        id: "test:model:product#0".try_into().expect("valid identity"),
        kind: ProductDefinitionKind::Part,
        source_name: None,
        label: None,
        description: None,
        part_number: None,
        bom_properties: named("bom"),
        bodies: Vec::new(),
        native_ref: None,
    });

    let mut annotations = CadIr::empty();
    annotations
        .model
        .semantic_annotations
        .push(SemanticAnnotation {
            id: "test:model:semantic-annotation#0"
                .try_into()
                .expect("valid identity"),
            object: "zz:object".to_string(),
            kind: SemanticAnnotationKind::Text,
            runtime_type: "zz:runtime".to_string(),
            order: 0,
            text: Vec::new(),
            references: BTreeMap::from([(
                cadmpeg_core::nonblank_literal!("zz_source_role"),
                vec![ReferenceSelection::new(ReferenceTarget::Null, Vec::new())],
            )]),
            value: None,
            format: None,
            position: None,
            parameters: named("parameter"),
            assets: Vec::new(),
            native_ref: "zz:native-ref".to_string(),
        });

    [
        ("presentation", presentation),
        ("product definition", products),
        ("semantic annotation", annotations),
    ]
    .into_iter()
    .map(|(name, ir)| (name, serde_json::to_value(&ir).expect("a CadIr serializes")))
    .collect()
}

/// Whether `shape` is the one free-form map the document admits.
fn is_free_form(shape: &str) -> bool {
    cadmpeg_test_support::unknown_keys::FREE_FORM_SHAPES.contains(&shape)
}

/// Member a golden harness stores a document under.
const DOCUMENT_KEY: &str = "ir";

/// Member every `CadIr` serializes, and the marker of a document root.
const DOCUMENT_VERSION_KEY: &str = "ir_version";

/// Whether this document's `native` arena is the elision block rather than a
/// readable arena.
///
/// The key is the whole predicate. A `native` member is a map of codec
/// namespaces, so a readable document reaches [`NATIVE_ELISION_KEY`] only
/// through a namespace of that name, whose value is an object; a string there
/// is written by nothing but the elision block. The marker's sentence is for a
/// reader of the golden, and reading it here would make this answer depend on
/// whether the `FreeCAD` harness ran before or after this binary in a
/// regeneration that changes the sentence.
fn states_native_elision(ir: &Value) -> bool {
    ir.get("native")
        .and_then(|native| native.get(NATIVE_ELISION_KEY))
        .is_some_and(Value::is_string)
}

#[test]
fn the_elision_predicate_reads_the_key_and_not_its_text() {
    let elided = |marker: Value| serde_json::json!({ "native": { NATIVE_ELISION_KEY: marker } });
    assert!(states_native_elision(&elided(serde_json::json!(
        NATIVE_ELISION_MARKER
    ))));
    assert!(states_native_elision(&elided(serde_json::json!(
        "any other sentence a regeneration writes"
    ))));

    assert!(!states_native_elision(&serde_json::json!({})));
    assert!(!states_native_elision(&serde_json::json!({ "native": {} })));
    assert!(!states_native_elision(&serde_json::json!({
        "native": { "fcstd": { "objects": [] } }
    })));
    // A namespace of that name is an object, not the block's sentence.
    assert!(!states_native_elision(&elided(serde_json::json!({
        "objects": []
    }))));
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
        // Several workspace crates have no golden tree.  An existing tree is
        // required to remain readable once it is selected; recursive
        // enumeration errors must not turn into an empty contribution.
        if golden.is_dir() {
            collect_goldens(&golden, &mut found)
                .unwrap_or_else(|error| panic!("cannot collect {}: {error}", golden.display()));
        }
    }
    found.sort();
    found
}

/// Appends every `*.json` under `directory`, recursively.
fn collect_goldens(directory: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_goldens(&path, found)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            found.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod scanner_tests {
    use super::collect_goldens;

    #[test]
    fn golden_collector_reports_a_missing_root() {
        let missing = std::env::temp_dir().join(format!(
            "cadmpeg-unknown-key-sweep-missing-goldens-{}",
            std::process::id()
        ));
        assert!(
            !missing.exists(),
            "test path unexpectedly exists: {missing:?}"
        );

        let mut found = Vec::new();
        let error = collect_goldens(&missing, &mut found)
            .expect_err("a selected golden root must fail the census");
        assert!(error.contains(&missing.display().to_string()));
        assert!(error.contains("No such file") || error.contains("not found"));
    }
}
