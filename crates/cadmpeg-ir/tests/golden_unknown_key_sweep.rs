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

use cadmpeg_ir::CadIr;
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
    use std::collections::BTreeMap;

    let named = |value: &str| BTreeMap::from([("zz_source_name".to_string(), value.to_string())]);

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
                "zz_source_role".to_string(),
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

/// Every hand-written `Deserialize` in the three wire crates, with how it is
/// covered.
///
/// A hand impl never reaches `scripts/check-deny-census.py`, which reads
/// `derive(Deserialize)` items only, so its coverage is stated here. The test
/// below parses the source at run time and fails when this table and the source
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
    ("crates/cadmpeg-asm/src/brep/mod.rs", "AsmBrep", "wire"),
    ("crates/cadmpeg-asm/src/brep/records.rs", "$name", "wire"),
    (
        "crates/cadmpeg-asm/src/brep/records.rs",
        "FaceSidedness",
        "wire",
    ),
    ("crates/cadmpeg-asm/src/brep/stats.rs", "Stats", "wire"),
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
    (
        "crates/cadmpeg-ir/src/features/patterns.rs",
        "PatternKind",
        "wire",
    ),
    ("crates/cadmpeg-ir/src/features.rs", "$name", "wire"),
    ("crates/cadmpeg-ir/src/features.rs", "BodyMember", "wire"),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "BodyMembers",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "ConfigurationEvaluation",
        "wire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "DistinctMembers",
        "keyless",
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
        "NonEmptyMembers",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "PolygonSideCount",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SelectionMembers",
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
        "NonBlankString",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "CodecFormat",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/provenance.rs", "Provenance", "wire"),
    ("crates/cadmpeg-ir/src/scalar.rs", "$name", "keyless"),
    (
        "crates/cadmpeg-ir/src/sketches.rs",
        "SpatialSketchNurbsCurve",
        "wire",
    ),
    ("crates/cadmpeg-ir/src/tessellation.rs", "Strip", "keyless"),
    ("crates/cadmpeg-ir/src/tessellation.rs", "Strips", "keyless"),
    ("crates/cadmpeg-ir/src/units.rs", "$name", "keyless"),
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

/// Every hand-written `Deserialize` impl in the three wire crates, outside
/// test modules and test files, as (crate-relative path, type name).
///
/// The source is parsed, not grepped: `syn` reads each file and the walk
/// visits every `impl` item, including one nested in an inline `mod`, so a
/// header wrapped over several lines, a generic parameter list, or a
/// fully-qualified trait path is found just the same. An impl counts when the
/// trait path ends in `Deserialize` and the impl generics declare the `'de`
/// lifetime.
///
/// A `macro_rules!` body is token text, not items, so its impls are read from
/// the macro's own token stream under the same trait-and-lifetime rule; the
/// type name recorded there is the macro's metavariable.
fn hand_written_impls() -> BTreeSet<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let mut found = BTreeSet::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
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
            let text = std::fs::read_to_string(&file).expect("read source");
            let parsed = syn::parse_file(&text)
                .map_err(|error| format!("{relative} does not parse: {error}"))
                .expect("every source file in the wire crates parses");
            collect_hand_impls(&parsed.items, &relative, &mut found);
        }
    }
    found
}

/// Records every hand-written `Deserialize` impl among `items`, recursing into
/// inline modules and `macro_rules!` bodies.
fn collect_hand_impls(items: &[syn::Item], relative: &str, found: &mut BTreeSet<(String, String)>) {
    for item in items {
        match item {
            syn::Item::Impl(implementation) => {
                if let Some(name) = deserialize_impl_target(implementation) {
                    found.insert((relative.to_owned(), name));
                }
            }
            syn::Item::Mod(module) => {
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    collect_hand_impls(nested, relative, found);
                }
            }
            syn::Item::Macro(macro_item) => {
                for name in macro_body_impl_targets(&macro_item.mac.tokens) {
                    found.insert((relative.to_owned(), name));
                }
            }
            _ => {}
        }
    }
}

/// Whether this inline module is a test module.
fn is_test_module(module: &syn::ItemMod) -> bool {
    module.attrs.iter().any(|attribute| match &attribute.meta {
        syn::Meta::List(list) => {
            list.path.is_ident("cfg") && list.tokens.to_string().contains("test")
        }
        syn::Meta::Path(_) | syn::Meta::NameValue(_) => false,
    })
}

/// The type a hand-written `Deserialize` impl is written for: the last segment
/// of its self type, without generic arguments.
///
/// `None` when the impl is for another trait, is an inherent impl, or declares
/// no `'de` lifetime.
fn deserialize_impl_target(implementation: &syn::ItemImpl) -> Option<String> {
    let (_, path, _) = implementation.trait_.as_ref()?;
    if path.segments.last()?.ident != "Deserialize" {
        return None;
    }
    let has_de = implementation.generics.params.iter().any(|parameter| {
        matches!(parameter, syn::GenericParam::Lifetime(lifetime)
            if lifetime.lifetime.ident == "de")
    });
    if !has_de {
        return None;
    }
    match implementation.self_ty.as_ref() {
        syn::Type::Path(typed) => Some(typed.path.segments.last()?.ident.to_string()),
        _ => None,
    }
}

/// Every `Deserialize<'de> for …` target named in a `macro_rules!` body.
///
/// A macro body is token text, so it is scanned as tokens: the walk flattens
/// every delimited group and looks for the token run
/// `Deserialize < 'de > for`, then reads the target that follows. A target
/// spelled as a metavariable is recorded with its sigil.
fn macro_body_impl_targets(tokens: &proc_macro2::TokenStream) -> Vec<String> {
    let mut flat = Vec::new();
    flatten_tokens(tokens, &mut flat);
    let header = [
        "Deserialize".to_owned(),
        "<".to_owned(),
        "'".to_owned(),
        "de".to_owned(),
        ">".to_owned(),
        "for".to_owned(),
    ];
    let mut targets = Vec::new();
    for (index, window) in flat.windows(header.len()).enumerate() {
        if window != header {
            continue;
        }
        let after = index + header.len();
        let Some(first) = flat.get(after) else {
            continue;
        };
        if first == "$" {
            if let Some(second) = flat.get(after + 1) {
                targets.push(format!("${second}"));
            }
        } else {
            targets.push(first.clone());
        }
    }
    targets
}

/// Appends every token of `tokens` as text, descending into delimited groups.
fn flatten_tokens(tokens: &proc_macro2::TokenStream, flat: &mut Vec<String>) {
    for tree in tokens.clone() {
        match tree {
            proc_macro2::TokenTree::Group(group) => flatten_tokens(&group.stream(), flat),
            other => flat.push(other.to_string()),
        }
    }
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
