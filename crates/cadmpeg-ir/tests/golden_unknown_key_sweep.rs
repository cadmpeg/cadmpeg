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

/// Where every hand-written `Deserialize` refuses `null` for an optional field
/// its type writes by omission, as (file, type, field, line).
///
/// A hand impl states its own key reads, so the derive census cannot see
/// whether `null` and absence reach the same value there. The listed line is
/// read at run time and must state the refusal; an entry that names a line
/// which no longer does fails, and a type whose skipping fields and entries
/// disagree fails.
const HAND_IMPL_NULL_REFUSALS: &[(&str, &str, &str, usize)] = &[
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SweepCircularRegion",
        "wall_thickness",
        7283,
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "TreeChildren",
        "active_child",
        2248,
    ),
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShape",
        "exit_kind",
        60,
    ),
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShape",
        "diameter",
        62,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        6663,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "version",
        6672,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        6682,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        6695,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "parameterization",
        6704,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        6711,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "blend_surface",
        6737,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "native_kind",
        6745,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "record",
        6751,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        6758,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "cache",
        802,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "record",
        812,
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "cache",
        819,
    ),
];

/// Every hand-written `Deserialize` reads its optional keys the one way.
///
/// For each impl the coverage census enumerates, the type either declares no
/// optional field written by omission -- nothing for `null` to spell twice --
/// or every such field is listed in [`HAND_IMPL_NULL_REFUSALS`] with the line
/// where the impl refuses `null`. A type declared by a macro states `$name` in
/// the census and declares no fields of its own.
#[test]
fn every_hand_written_deserialize_refuses_a_null_spelling() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let mut disagreements = Vec::new();
    for (path, name, _class) in HAND_IMPLS {
        if name.starts_with('$') {
            continue;
        }
        let text = std::fs::read_to_string(root.join(path)).expect("read source");
        let parsed = syn::parse_file(&text).expect("every source file in the wire crates parses");
        let mut skipping = Vec::new();
        declared_skipping_fields(&parsed.items, name, &mut skipping);
        let mut listed: Vec<String> = HAND_IMPL_NULL_REFUSALS
            .iter()
            .filter(|(listed_path, listed_name, _, _)| listed_path == path && listed_name == name)
            .map(|(_, _, field, line)| {
                let stated = text
                    .lines()
                    .nth(line - 1)
                    .unwrap_or_else(|| panic!("{path} states no line {line}"));
                assert!(
                    stated.contains("absent_key::present"),
                    "{path}:{line} is listed as where {name} refuses null for {field}, but the line states {stated}"
                );
                (*field).to_owned()
            })
            .collect();
        skipping.sort();
        listed.sort();
        if skipping != listed {
            disagreements.push(format!(
                "{path} {name}: fields written by omission {skipping:?}, refusals listed {listed:?}"
            ));
        }
    }
    assert!(
        disagreements.is_empty(),
        "{} hand-written Deserialize impl(s) disagree with the null-refusal list:\n{}",
        disagreements.len(),
        disagreements.join("\n")
    );
}

/// Records the name of every field of the type named `name` that skips
/// serialization on `None`, recursing into inline modules.
fn declared_skipping_fields(items: &[syn::Item], name: &str, found: &mut Vec<String>) {
    fn record(fields: &syn::Fields, found: &mut Vec<String>) {
        for field in fields {
            let (skips, _) = absence_spelling(field);
            if !skips {
                continue;
            }
            found.push(
                field
                    .ident
                    .as_ref()
                    .map_or_else(|| "<tuple field>".to_owned(), syn::Ident::to_string),
            );
        }
    }

    for item in items {
        match item {
            syn::Item::Struct(declaration) if declaration.ident == name => {
                record(&declaration.fields, found);
            }
            syn::Item::Enum(declaration) if declaration.ident == name => {
                for variant in &declaration.variants {
                    record(&variant.fields, found);
                }
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    declared_skipping_fields(nested, name, found);
                }
            }
            _ => {}
        }
    }
}

/// Absence is spelled one way on every field a document can be read into.
///
/// A field carrying `skip_serializing_if = "Option::is_none"` is written by
/// omission. When its type is also read, the reader must refuse an explicit
/// `null` at that key, or one `None` has two spellings on the wire. The guard
/// is a `deserialize_with` — `cadmpeg_core::absent_key::present`, or a
/// field-local shim that ends in it.
///
/// The duty follows the reader: a type carries it when it derives
/// `Deserialize` and reads its own keys. A type that derives none, and one
/// that derives it with `try_from` or `from`, both read a document through a
/// separate wire type; this census reaches that wire type on its own, and the
/// serialize-side attributes left on the outer type name no reader.
///
/// The source is parsed, not grepped, by the walker the hand-impl census uses,
/// so an attribute list wrapped over several lines, a field in an inline
/// module, or an optional field of an enum variant is read just the same.
/// Offenders are named by `file:line`.
#[test]
fn every_read_optional_field_refuses_a_null_spelling() {
    let definitions = shim_definitions();
    for helper in NULL_REFUSING_HELPERS {
        if helper.contains("::") {
            continue;
        }
        let defined_by = definitions
            .get(*helper)
            .unwrap_or_else(|| panic!("{helper} is allowlisted but no source defines it"));
        assert_eq!(
            defined_by,
            &BTreeSet::from(["named_optional_field".to_owned()]),
            "{helper} is allowlisted as refusing null, but it is defined by {defined_by:?}"
        );
    }
    let (readable, offenders) = optional_absence_census();
    assert!(
        readable > 0,
        "the optional-field census read no types that derive or implement Deserialize"
    );
    assert!(
        offenders.is_empty(),
        "{} optional field(s) skip on `None` and state no helper that refuses `null`, so `null` and an absent key reach the same value:\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}

/// The macro every field-deserializer shim is defined by, keyed by shim name.
///
/// A shim is a `named_field!` or `named_optional_field!` item invocation; the
/// macro's first token is the shim's own name. The source is parsed, so an
/// invocation wrapped over several lines or nested in an inline module reads
/// the same. A name defined twice carries both macros, which is what lets the
/// allowlist above refuse a shim that only some of its definitions guard.
fn shim_definitions() -> std::collections::BTreeMap<String, BTreeSet<String>> {
    fn walk(
        items: &[syn::Item],
        found: &mut std::collections::BTreeMap<String, BTreeSet<String>>,
    ) {
        for item in items {
            match item {
                syn::Item::Macro(invocation) => {
                    let Some(macro_name) = invocation.mac.path.segments.last() else {
                        continue;
                    };
                    let macro_name = macro_name.ident.to_string();
                    if macro_name != "named_field" && macro_name != "named_optional_field" {
                        continue;
                    }
                    let mut tokens = Vec::new();
                    flatten_tokens(&invocation.mac.tokens, &mut tokens);
                    let Some(shim) = tokens.first() else { continue };
                    found
                        .entry(shim.clone())
                        .or_default()
                        .insert(macro_name);
                }
                syn::Item::Mod(module) => {
                    if let Some((_, nested)) = &module.content {
                        walk(nested, found);
                    }
                }
                _ => {}
            }
        }
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let mut found = std::collections::BTreeMap::new();
    for source in ["crates/cadmpeg-ir/src", "crates/cadmpeg-core/src"] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files);
        files.sort();
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
            walk(&parsed.items, &mut found);
        }
    }
    found
}

/// The count of readable types visited, and every `file:line` where one of
/// their fields skips on `None` without a `deserialize_with`.
fn optional_absence_census() -> (usize, Vec<String>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let mut readable = 0_usize;
    let mut offenders = Vec::new();
    for source in ["crates/cadmpeg-ir/src", "crates/cadmpeg-core/src"] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files);
        files.sort();
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
            collect_optional_fields(&parsed.items, &relative, &mut readable, &mut offenders);
        }
    }
    (readable, offenders)
}

/// Visits every struct and enum among `items`, recursing into inline modules,
/// and records the unguarded optional fields of the readable ones.
fn collect_optional_fields(
    items: &[syn::Item],
    relative: &str,
    readable: &mut usize,
    offenders: &mut Vec<String>,
) {
    for item in items {
        match item {
            syn::Item::Struct(declaration) => {
                if !reads_its_own_keys(&declaration.attrs) {
                    continue;
                }
                *readable += 1;
                record_unguarded_fields(&declaration.fields, relative, offenders);
            }
            syn::Item::Enum(declaration) => {
                if !reads_its_own_keys(&declaration.attrs) {
                    continue;
                }
                *readable += 1;
                for variant in &declaration.variants {
                    record_unguarded_fields(&variant.fields, relative, offenders);
                }
            }
            syn::Item::Mod(module) => {
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    collect_optional_fields(nested, relative, readable, offenders);
                }
            }
            _ => {}
        }
    }
}

/// Consumes whatever follows a nested-meta path: an `= value`, a
/// parenthesized list, or nothing.
fn skip_meta_value(meta: &syn::meta::ParseNestedMeta<'_>) -> syn::Result<()> {
    if meta.input.peek(syn::Token![=]) {
        let _literal: syn::Lit = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        let content;
        syn::parenthesized!(content in meta.input);
        let _rest: proc_macro2::TokenStream = content.parse()?;
    }
    Ok(())
}

/// Whether a document reaches this type's own field attributes: it derives
/// `Deserialize`, and its container attributes route no `try_from` or `from`
/// conversion through another type.
fn reads_its_own_keys(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(derive_list_names_deserialize) && !attrs.iter().any(routes_through_a_wire)
}

/// Whether this container-level `serde` attribute states `try_from` or `from`,
/// which makes another type the reader.
fn routes_through_a_wire(attribute: &syn::Attribute) -> bool {
    if !attribute.path().is_ident("serde") {
        return false;
    }
    let mut routed = false;
    attribute
        .parse_nested_meta(|meta| {
            if meta.path.is_ident("try_from") || meta.path.is_ident("from") {
                routed = true;
            }
            skip_meta_value(&meta)?;
            Ok(())
        })
        .expect("every container serde attribute in the wire crates parses as a nested meta list");
    routed
}

/// Whether this attribute is a `derive` (or a `cfg_attr` carrying one) whose
/// list names `Deserialize`.
fn derive_list_names_deserialize(attribute: &syn::Attribute) -> bool {
    if !attribute.path().is_ident("derive") && !attribute.path().is_ident("cfg_attr") {
        return false;
    }
    let Ok(list) = attribute.meta.require_list() else {
        return false;
    };
    let mut flat = Vec::new();
    flatten_tokens(&list.tokens, &mut flat);
    flat.iter().any(|token| token == "Deserialize")
}

/// The `deserialize_with` helpers that refuse an explicit `null` at their key.
///
/// Each entry was read at its definition, not inferred from its name.
/// `cadmpeg_core::absent_key::present` implements both `visit_unit` and
/// `visit_none` as the refusal, so neither serde path admits `null`, and
/// `visit_some` delegates to the field's own type; `crate::absent_key::present`
/// is the same function spelled from inside `cadmpeg-core`. The
/// `named_optional_field!` shims — `deserialize_position`, `deserialize_scale`,
/// `deserialize_direction`, `deserialize_rotation_degrees`,
/// `deserialize_orientation`, `deserialize_line_width`, `deserialize_point_size`,
/// `deserialize_value` and `deserialize_tolerance` — call it and add the field
/// name to whatever it refuses. Anything else on a field that skips on `None`
/// is an offender, because a `deserialize_with` that does not state the refusal
/// gives `None` a second spelling.
const NULL_REFUSING_HELPERS: &[&str] = &[
    "cadmpeg_core::absent_key::present",
    "crate::absent_key::present",
    "deserialize_direction",
    "deserialize_line_width",
    "deserialize_orientation",
    "deserialize_point_size",
    "deserialize_position",
    "deserialize_rotation_degrees",
    "deserialize_scale",
    "deserialize_tolerance",
    "deserialize_value",
];

/// Records every field of `fields` that skips on `None` and states no helper
/// from [`NULL_REFUSING_HELPERS`].
fn record_unguarded_fields(fields: &syn::Fields, relative: &str, offenders: &mut Vec<String>) {
    for field in fields {
        let (skips, helper) = absence_spelling(field);
        if !skips {
            continue;
        }
        if helper
            .as_deref()
            .is_some_and(|helper| NULL_REFUSING_HELPERS.contains(&helper))
        {
            continue;
        }
        let span = field
            .ident
            .as_ref()
            .map_or_else(|| syn::spanned::Spanned::span(&field.ty), syn::Ident::span);
        let name = field
            .ident
            .as_ref()
            .map_or_else(|| "<tuple field>".to_owned(), syn::Ident::to_string);
        let stated = helper.map_or_else(
            || "states no deserialize_with".to_owned(),
            |helper| format!("states the unlisted helper {helper}"),
        );
        offenders.push(format!("{relative}:{} {name} {stated}", span.start().line));
    }
}

/// Whether this field skips serialization on `None`, and the
/// `deserialize_with` (or `with`) path it states.
fn absence_spelling(field: &syn::Field) -> (bool, Option<String>) {
    let mut skips = false;
    let mut helper = None;
    for attribute in &field.attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                let named_skip = meta.path.is_ident("skip_serializing_if");
                let named_helper =
                    meta.path.is_ident("deserialize_with") || meta.path.is_ident("with");
                if meta.input.peek(syn::Token![=]) {
                    let literal: syn::Lit = meta.value()?.parse()?;
                    if let syn::Lit::Str(text) = &literal {
                        if named_skip && text.value() == "Option::is_none" {
                            skips = true;
                        }
                        if named_helper {
                            helper = Some(text.value());
                        }
                    }
                } else {
                    skip_meta_value(&meta)?;
                }
                Ok(())
            })
            .expect("every serde attribute in the wire crates parses as a nested meta list");
    }
    (skips, helper)
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
