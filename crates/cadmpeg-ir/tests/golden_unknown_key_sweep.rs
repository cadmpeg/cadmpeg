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
}

/// The non-native nodes a document may accept an unknown key on.
///
/// Each is a free-form map whose keys are source names, so it declares no key
/// set for a deny to bind. Every owner declares `deny_unknown_fields`, and that
/// deny is live one level up; the map itself is the node the source fills.
/// Probing four value kinds is what makes them visible: a
/// `BTreeMap<String, String>` refuses `true` and accepts `"zz"`, so the earlier
/// single-value probe read eight of these as refusing.
///
/// * `/model/appearance_bindings/#/channels` - `AppearanceBinding::channels`,
///   `BTreeMap<String, String>` of source channel names;
/// * `/model/appearances/#/properties` - `Appearance::properties`,
///   `BTreeMap<String, f64>` of source renderer properties;
/// * `/model/configurations/#/properties` - `DesignConfiguration::properties`,
///   configuration-local named values;
/// * `/model/drawings/#/parameters` - `DrawingView::parameters`, retained view
///   parameters by source name;
/// * `/model/features/#/definition/parameters` -
///   `FeatureOperation::Native::parameters`, the native operation's own fields;
/// * `/model/features/#/source_properties` - `Feature::source_properties`,
///   source operation attributes the neutral definition does not consume;
/// * `/model/parameters/#/properties` - `Parameter::properties`, source
///   parameter properties no other field represents;
/// * `/source/attributes` - `SourceMeta::attributes`, format-specific
///   attributes;
/// * `/model/presentation_documents/#/states/#/attributes` and
///   `/model/presentation_documents/#/states/#/kind/value/properties` -
///   `PresentationState::attributes` and the view state's `properties`;
/// * `/model/product_definitions/#/bom_properties` -
///   `ProductDefinition::bom_properties`, the source's bill-of-materials
///   fields;
/// * `/model/semantic_annotations/#/parameters` -
///   `SemanticAnnotation::parameters`, the native note's own fields;
/// * `/model/view_presentations/#/properties` - `ViewPresentation::properties`;
/// * `/source/identity/dialects/primary/declared` and
///   `/source/identity/dialects/extra/#/declared` - `DialectMatch::declared`,
///   the version fields the source declared verbatim.
///
/// Every other accepting node is a failure.
const FREE_FORM_SHAPES: &[&str] = &[
    "/model/appearance_bindings/#/channels",
    "/model/appearances/#/properties",
    "/model/configurations/#/properties",
    "/model/drawings/#/parameters",
    "/model/features/#/definition/parameters",
    "/model/features/#/source_properties",
    "/model/parameters/#/properties",
    "/model/presentation_documents/#/states/#/attributes",
    "/model/presentation_documents/#/states/#/kind/value/properties",
    "/model/product_definitions/#/bom_properties",
    "/model/semantic_annotations/#/parameters",
    "/model/view_presentations/#/properties",
    "/source/attributes",
    "/source/identity/dialects/extra/#/declared",
    "/source/identity/dialects/primary/declared",
];

/// Whether `shape` is the one free-form map the document admits.
fn is_free_form(shape: &str) -> bool {
    FREE_FORM_SHAPES.contains(&shape)
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

/// Every hand-written `Deserialize` in the two wire crates, with how it is
/// covered.
///
/// A hand impl never reaches `scripts/check-deny-census.py`, which reads
/// `derive(Deserialize)` items only, so its coverage is stated here. The test
/// below greps the source at run time and fails when this table and the source
/// disagree in either direction, so a new hand impl cannot land uncovered and a
/// deleted one cannot leave a stale entry.
///
/// The coverage classes are:
///
/// * `wire` - the impl reads one named or inner wire type that declares
///   `deny_unknown_fields`, so the key set is refused by that type;
/// * `keyless` - the impl reads a scalar, a string, a byte string, a fixed
///   array or a list, so it has no object key set at all;
/// * `free-form` - the impl reads an open map by design; `FIXTURES` below
///   states what refuses instead.
const HAND_IMPLS: &[(&str, &str, &str)] = &[
    ("crates/cadmpeg-core/src/dialect.rs", "DialectId", "keyless"),
    (
        "crates/cadmpeg-core/src/dialect.rs",
        "DialectLayers",
        "wire",
    ),
    ("crates/cadmpeg-ir/src/assets.rs", "AssetData", "keyless"),
    (
        "crates/cadmpeg-ir/src/container.rs",
        "ContainerKind",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/document.rs", "CadIr", "wire"),
    ("crates/cadmpeg-ir/src/document.rs", "CensusKey", "keyless"),
    ("crates/cadmpeg-ir/src/document.rs", "Model", "wire"),
    (
        "crates/cadmpeg-ir/src/features/edge_treatments.rs",
        "FullRoundFilletGroup",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShape",
        "wire",
    ),
    ("crates/cadmpeg-ir/src/features.rs", "$name", "wire"),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "ConfigurationEvaluation",
        "wire",
    ),
    ("crates/cadmpeg-ir/src/features.rs", "FaceMaker", "keyless"),
    ("crates/cadmpeg-ir/src/features.rs", "Feature", "wire"),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "FeatureContent",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "GeometryImportPath",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "NativeFeatureKind",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "NativeSelections",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "PolygonSideCount",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SelectionReference",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SewBodySelection",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SheetMetalFlangeEdgeWidths",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SketchProfileRegions",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SplitFacePlanes",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SweepCircularRegion",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "ThreePointSelection",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/features.rs", "TreeChildren", "wire"),
    (
        "crates/cadmpeg-ir/src/geometry/carriers.rs",
        "BsplineSurface",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/carriers.rs",
        "NurbsCurve",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/carriers.rs",
        "NurbsSurface",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/carriers.rs",
        "PcurveNurbs",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/carriers.rs",
        "PolarPcurveNurbs",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry/carriers.rs",
        "PolygonalSurface",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "DirectedParameterRange",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProjectionRole",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "RevisionG2RadiusValue",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/native/mod.rs",
        "NativeRecord",
        "free-form",
    ),
    ("crates/cadmpeg-ir/src/pmi.rs", "PmiMagnitude", "keyless"),
    (
        "crates/cadmpeg-ir/src/products.rs",
        "NonEmptyString",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "CodecFormat",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "Provenance<AnnotationLocation>",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "Provenance<SourceLocation>",
        "wire",
    ),
    ("crates/cadmpeg-ir/src/scalar.rs", "$name", "keyless"),
    (
        "crates/cadmpeg-ir/src/sketches.rs",
        "SpatialSketchNurbsCurve",
        "wire",
    ),
];

/// The coverage classes a `HAND_IMPLS` entry may state.
const COVERAGE_CLASSES: &[&str] = &["wire", "keyless", "free-form"];

#[test]
fn every_hand_written_deserialize_states_its_coverage() {
    let found = hand_written_impls();
    assert!(
        !found.is_empty(),
        "the hand-impl census found no impls to classify"
    );
    let listed: BTreeSet<(String, String)> = HAND_IMPLS
        .iter()
        .map(|(path, name, class)| {
            assert!(
                COVERAGE_CLASSES.contains(class),
                "{path} {name} states the unknown coverage class {class}"
            );
            ((*path).to_owned(), (*name).to_owned())
        })
        .collect();
    let missing: Vec<String> = found
        .difference(&listed)
        .map(|(path, name)| format!("{path} {name}"))
        .collect();
    assert!(
        missing.is_empty(),
        "{} hand-written Deserialize impl(s) state no coverage in HAND_IMPLS:\n{}",
        missing.len(),
        missing.join("\n")
    );
    let stale: Vec<String> = listed
        .difference(&found)
        .map(|(path, name)| format!("{path} {name}"))
        .collect();
    assert!(
        stale.is_empty(),
        "{} HAND_IMPLS entry/entries name no hand-written Deserialize impl:\n{}",
        stale.len(),
        stale.join("\n")
    );

    // The one free-form impl reads codec-owned fields by design and declares no
    // key set. Both levels above it do refuse, which is what keeps an unknown
    // key out of the document.
    for wire in [
        serde_json::json!({"rhino": {"objects": []}, "zz_bogus": true}),
        serde_json::json!({"rhino": {"objects": [], "zz_bogus": true}}),
    ] {
        assert!(
            serde_json::from_value::<cadmpeg_ir::native::Native>(wire).is_err(),
            "the native namespace levels refuse an unknown key"
        );
    }
}

/// Every `impl<'de> Deserialize<'de> for T` in the two wire crates, outside
/// test modules and test files, as (crate-relative path, type text).
fn hand_written_impls() -> BTreeSet<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let mut found = BTreeSet::new();
    for source in ["crates/cadmpeg-ir/src", "crates/cadmpeg-core/src"] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files);
        for file in files {
            let relative = file
                .strip_prefix(&root)
                .expect("a collected source sits under the repository root")
                .to_string_lossy()
                .replace('\\', "/");
            if is_test_path(&relative) {
                continue;
            }
            for line in std::fs::read_to_string(&file).expect("read source").lines() {
                let trimmed = line.trim();
                for prefix in [
                    "impl<'de> Deserialize<'de> for ",
                    "impl<'de> serde::Deserialize<'de> for ",
                ] {
                    if let Some(rest) = trimmed.strip_prefix(prefix) {
                        let name = rest.trim_end_matches('{').trim();
                        found.insert((relative.clone(), name.to_owned()));
                    }
                }
            }
        }
    }
    found
}

/// Whether this crate-relative path is a test file or sits in a test tree.
fn is_test_path(relative: &str) -> bool {
    relative.ends_with("/tests.rs")
        || relative.ends_with("_tests.rs")
        || relative.contains("/tests/")
        || relative.contains("/test_support")
}

/// Appends every `*.rs` under `directory`, recursively.
fn collect_rust_sources(directory: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            collect_rust_sources(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
}
