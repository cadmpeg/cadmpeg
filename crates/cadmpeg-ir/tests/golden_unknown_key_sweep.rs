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
        // Several workspace crates have no golden tree.  An existing tree is
        // required to remain readable once it is selected; recursive
        // enumeration errors must not turn into an empty contribution.
        if golden.is_dir() {
            collect_goldens(&golden, &mut found)
                .unwrap_or_else(|error| panic!("cannot collect {golden:?}: {error}"));
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
        "crates/cadmpeg-ir/src/annotations.rs",
        "NonEmptyMap",
        "free-form",
    ),
    (
        "crates/cadmpeg-ir/src/container.rs",
        "ContainerKind",
        "keyless",
    ),
    ("crates/cadmpeg-ir/src/document.rs", "CadIr", "wire"),
    ("crates/cadmpeg-ir/src/document.rs", "CensusKey", "keyless"),
    ("crates/cadmpeg-ir/src/document.rs", "IrVersion", "keyless"),
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
        "crates/cadmpeg-ir/src/hash/digest.rs",
        "Sha256Digest",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/native/mod.rs",
        "NativeRecord",
        "free-form",
    ),
    ("crates/cadmpeg-ir/src/pmi.rs", "PmiMagnitude", "keyless"),
    (
        "crates/cadmpeg-core/src/text.rs",
        "NonBlankString",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "CodecFormat",
        "keyless",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "SourceOwner",
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

/// Where every hand-written `Deserialize` refuses `null` for an optional
/// field, as (file, type, field, guard site).
///
/// A hand impl states its own key reads, so the derive census cannot see
/// whether `null` and absence reach the same value there. The guard site names
/// the item the impl reads through: a wire struct, or a wire enum and the
/// variant, spelled `Wire::Variant`. The site is located by parsing the file,
/// so an edit anywhere above it moves nothing; the named field must exist
/// there and must state a helper from [`NULL_REFUSING_HELPERS`].
const HAND_IMPL_NULL_REFUSALS: &[(&str, &str, &str, &str)] = &[
    (
        "crates/cadmpeg-ir/src/features.rs",
        "SweepCircularRegion",
        "wall_thickness",
        "SweepCircularRegionWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "TreeChildren",
        "active_child",
        "TreeChildrenWire",
    ),
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShape",
        "exit_kind",
        "HoleShapeWire",
    ),
    (
        "crates/cadmpeg-ir/src/features/holes.rs",
        "HoleShape",
        "diameter",
        "HoleShapeWire",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::Exact",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "version",
        "ProceduralCurveDefinitionWire::Law",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::Law",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::Intersection",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "parameterization",
        "ProceduralCurveDefinitionWire::TolerantIntersection",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::TolerantIntersection",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "blend_surface",
        "ProceduralCurveDefinitionWire::BlendSpine",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "native_kind",
        "ProceduralCurveDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "record",
        "ProceduralCurveDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralCurveDefinition",
        "cache",
        "ProceduralCurveDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "cache",
        "ProceduralSurfaceDefinitionWire::Ruled",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "record",
        "ProceduralSurfaceDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/geometry.rs",
        "ProceduralSurfaceDefinition",
        "cache",
        "ProceduralSurfaceDefinitionWire::Unknown",
    ),
    (
        "crates/cadmpeg-ir/src/document.rs",
        "CadIr",
        "source",
        "CadIrReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "name",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "suppressed",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "source_tag",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "source_text",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/features.rs",
        "Feature",
        "native_ref",
        "FeatureReadWire",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "Provenance",
        "tag",
        "AnnotationProvenanceWire",
    ),
    (
        "crates/cadmpeg-ir/src/provenance.rs",
        "Provenance",
        "tag",
        "SourceProvenanceWire",
    ),
    (
        "crates/cadmpeg-asm/src/brep/records.rs",
        "FaceSidedness",
        "containment",
        "FaceSidednessWire",
    ),
];

/// The `deserialize_with` helper the field named `field` states at the guard
/// site `site` of `items`, or the reason no such field was reached.
///
/// `site` is a wire type name, or a wire enum name and a variant name joined
/// by `::`. The walk recurses into inline modules, so a wire declared beside
/// its impl inside a private module reads the same.
fn helper_at_guard_site(items: &[syn::Item], site: &str, field: &str) -> Result<String, String> {
    let (type_name, variant_name) = match site.split_once("::") {
        Some((type_name, variant_name)) => (type_name, Some(variant_name)),
        None => (site, None),
    };
    for item in items {
        let fields = match item {
            syn::Item::Struct(declaration) if declaration.ident == type_name => {
                if variant_name.is_some() {
                    return Err(format!(
                        "{site} names a variant, but {type_name} is a struct"
                    ));
                }
                &declaration.fields
            }
            syn::Item::Enum(declaration) if declaration.ident == type_name => {
                let Some(wanted) = variant_name else {
                    return Err(format!(
                        "{site} names no variant, but {type_name} is an enum"
                    ));
                };
                let Some(variant) = declaration
                    .variants
                    .iter()
                    .find(|variant| variant.ident == wanted)
                else {
                    return Err(format!("{type_name} declares no variant {wanted}"));
                };
                &variant.fields
            }
            syn::Item::Mod(module) => {
                if let Some((_, nested)) = &module.content {
                    if let Ok(found) = helper_at_guard_site(nested, site, field) {
                        return Ok(found);
                    }
                }
                continue;
            }
            _ => continue,
        };
        let Some(declared) = fields
            .iter()
            .find(|candidate| candidate.ident.as_ref().is_some_and(|name| name == field))
        else {
            return Err(format!("{site} declares no field {field}"));
        };
        return absence_spelling(declared)
            .ok_or_else(|| format!("{site} field {field} states no deserialize_with"));
    }
    Err(format!("no item named {type_name} was found"))
}

/// Every hand-written `Deserialize` reads its optional keys the one way.
///
/// For each impl the coverage census enumerates, the type either declares no
/// `Option` field -- nothing for `null` to spell twice -- or every such field
/// is listed in [`HAND_IMPL_NULL_REFUSALS`] with the guard site where the impl
/// refuses `null`. A type declared by a macro states `$name` in the census and
/// declares no fields of its own.
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
        declared_optional_fields(&parsed.items, name, &mut skipping);
        let mut listed: Vec<String> = HAND_IMPL_NULL_REFUSALS
            .iter()
            .filter(|(listed_path, listed_name, _, _)| listed_path == path && listed_name == name)
            .map(|(_, _, field, site)| {
                match helper_at_guard_site(&parsed.items, site, field) {
                    Ok(helper) => assert!(
                        NULL_REFUSING_HELPERS.contains(&helper.as_str())
                            || NULL_STATING_HELPERS.contains(&helper.as_str()),
                        "{path} {site} is listed as where {name} states the spelling of null for {field}, but it states the unlisted helper {helper}"
                    ),
                    Err(reason) => panic!(
                        "{path} {site} is listed as where {name} refuses null for {field}, but {reason}"
                    ),
                }
                (*field).to_owned()
            })
            .collect();
        skipping.sort();
        skipping.dedup();
        listed.sort();
        listed.dedup();
        if skipping != listed {
            disagreements.push(format!(
                "{path} {name}: optional fields {skipping:?}, guard sites listed for {listed:?}"
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

/// Records the name of every `Option` field of the type named `name`,
/// recursing into inline modules.
fn declared_optional_fields(items: &[syn::Item], name: &str, found: &mut Vec<String>) {
    fn record(fields: &syn::Fields, found: &mut Vec<String>) {
        for field in fields {
            if !is_option_type(&field.ty) {
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
                    declared_optional_fields(nested, name, found);
                }
            }
            _ => {}
        }
    }
}

/// `None` is spelled one way on every field a document can be read into, and
/// the reading declaration states the spelling its writer produces.
///
/// An optional key has two writers and so two admitted forms. A field carrying
/// `skip_serializing_if = "Option::is_none"` is written by omission: the
/// reading declaration states `cadmpeg_core::absent_key::present` (or a
/// field-local shim that ends in it) and a `default`, so absence is `None` and
/// `null` is refused by name. A field with no skip is always written: the
/// reading declaration states `cadmpeg_core::absent_key::nullable` and no
/// `default`, so `null` is `None` and an absent key is a missing field serde
/// names. Either way one state has one spelling.
///
/// The census reads both halves. It finds the writing declaration of each read
/// type — the type itself when it derives `Serialize`, the type its container
/// `into` or `remote` names, the type whose `try_from` or `from` routes the
/// read, or the pair stated in [`WRITING_DECLARATIONS`] — and refuses a
/// reading declaration whose spelling that writer does not produce.
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
        "{} `Option` field(s) on a read type do not state the one spelling of `None` their writer produces, so a state reaches the reader two ways:\n{}",
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
fn shim_definitions() -> BTreeMap<String, BTreeSet<String>> {
    fn walk(items: &[syn::Item], found: &mut BTreeMap<String, BTreeSet<String>>) {
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
                    found.entry(shim.clone()).or_default().insert(macro_name);
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
    let mut found = BTreeMap::new();
    for source in ["crates/cadmpeg-ir/src", "crates/cadmpeg-core/src"] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
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

/// One key's spelling of `None` on the writing declaration.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WriteSpelling {
    /// `skip_serializing_if` on the writing field: `None` leaves the key out.
    Omitted,
    /// An optional writing field with no skip: `None` writes the key as `null`.
    Null,
    /// A writing field that is not optional: the key always states a value.
    Always,
}

/// The writing declaration of a read type the routing attributes do not name.
///
/// A read-only wire whose owner states `try_from` or `from` is paired by that
/// attribute, and a type that derives `Serialize` writes its own keys. What is
/// left is a wire a hand-written reader names in code, and an owner whose
/// `Serialize` is hand-written. Each pair here was read at both declarations.
const WRITING_DECLARATIONS: &[(&str, &str)] = &[
    // `Feature` writes through `FeatureWriteWire` on both routes: the
    // standalone `Serialize` impl builds one, and the model route builds a
    // `Vec<FeatureWriteWire>` beside the `Vec<FeatureRowWire>` it reads.
    ("FeatureReadWire", "FeatureWriteWire"),
    ("FeatureRowWire", "FeatureWriteWire"),
    // `CadIr` writes through `CadIrWriteWire` in its hand-written `Serialize`.
    ("CadIrReadWire", "CadIrWriteWire"),
    // Each of these is read by a hand-written `Deserialize` impl for the type
    // named beside it, which derives `Serialize` and writes the keys.
    ("HoleShapeWire", "HoleShape"),
    ("SweepCircularRegionWire", "SweepCircularRegion"),
    ("TreeChildrenWire", "TreeChildren"),
    // `TSplineSubtransform` writes its `Inline` form by delegating to
    // `InlineTSplineSubtransform`, which derives `Serialize`.
    ("TSplineSubtransformWire", "InlineTSplineSubtransform"),
];

/// Keys whose writing declaration is hand-written code, with the spelling read
/// at that code.
///
/// `WireMembers` serializes a map by hand: it writes `free_vertex` only for
/// the `Vertex` variant, so the key is left out for every other state.
const HAND_WRITTEN_KEYS: &[(&str, &str, WriteSpelling)] =
    &[("WireMembersWire", "free_vertex", WriteSpelling::Omitted)];

/// Every declaration of the three wire crates, indexed for writer resolution.
#[derive(Default)]
struct WireIndex {
    /// Per type name that derives `Serialize`, the `None` spelling of each
    /// named field. A key one type writes two ways is absent here and present
    /// in `split_keys`.
    written_keys: BTreeMap<String, BTreeMap<String, WriteSpelling>>,
    /// The `(type, key)` pairs one declaration writes two ways.
    split_keys: BTreeSet<(String, String)>,
    /// Type names whose declaration derives `Serialize`.
    writes_own_keys: BTreeSet<String>,
    /// Type name -> the type its container `into` names.
    writes_through: BTreeMap<String, String>,
    /// Wire type name -> every type whose container `try_from` or `from`
    /// routes a read through it.
    read_routers: BTreeMap<String, BTreeSet<String>>,
    /// Type names declared twice in the three crates, which makes a name no
    /// address.
    twice_declared: BTreeSet<String>,
}

impl WireIndex {
    /// The writing declaration for the type `reader`, or why there is none.
    fn writing_declaration(&self, reader: &str) -> Result<String, String> {
        if self.twice_declared.contains(reader) {
            return Err(format!(
                "{reader} is declared twice in the wire crates, so the name addresses no declaration"
            ));
        }
        if let Some((_, writer)) = WRITING_DECLARATIONS
            .iter()
            .find(|(named, _)| *named == reader)
        {
            return Ok((*writer).to_owned());
        }
        if let Some(target) = self.writes_through.get(reader) {
            return Ok(target.clone());
        }
        if self.writes_own_keys.contains(reader) {
            return Ok(reader.to_owned());
        }
        let mut owners = self.read_routers.get(reader).into_iter().flatten();
        let (Some(owner), None) = (owners.next(), owners.next()) else {
            return Err(format!(
                "{reader} writes no keys of its own and no single container attribute routes a read through it"
            ));
        };
        if let Some((_, writer)) = WRITING_DECLARATIONS
            .iter()
            .find(|(named, _)| named == owner)
        {
            return Ok((*writer).to_owned());
        }
        if let Some(target) = self.writes_through.get(owner) {
            return Ok(target.clone());
        }
        if self.writes_own_keys.contains(owner) {
            return Ok(owner.clone());
        }
        Err(format!(
            "{reader} is read for {owner}, which derives no Serialize and names no writing declaration"
        ))
    }

    /// How the writing declaration `writer` spells `None` at `key`.
    fn spelling(&self, writer: &str, key: &str) -> Result<WriteSpelling, String> {
        if self
            .split_keys
            .contains(&(writer.to_owned(), key.to_owned()))
        {
            return Err(format!(
                "the writing declaration {writer} states {key} twice with different skips"
            ));
        }
        self.written_keys
            .get(writer)
            .and_then(|keys| keys.get(key))
            .copied()
            .ok_or_else(|| format!("the writing declaration {writer} states no key {key}"))
    }
}

/// Reads every declaration of the three wire crates into a [`WireIndex`].
fn wire_index() -> WireIndex {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let mut index = WireIndex::default();
    let mut seen = BTreeSet::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
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
            index_declarations(&parsed.items, &mut seen, &mut index);
        }
    }
    index
}

/// Records every struct and enum among `items`, recursing into inline modules.
fn index_declarations(items: &[syn::Item], seen: &mut BTreeSet<String>, index: &mut WireIndex) {
    for item in items {
        let (name, attrs, fields) = match item {
            syn::Item::Struct(declaration) => (
                declaration.ident.to_string(),
                &declaration.attrs,
                vec![&declaration.fields],
            ),
            syn::Item::Enum(declaration) => (
                declaration.ident.to_string(),
                &declaration.attrs,
                declaration
                    .variants
                    .iter()
                    .map(|variant| &variant.fields)
                    .collect(),
            ),
            syn::Item::Mod(module) => {
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    index_declarations(nested, seen, index);
                }
                continue;
            }
            _ => continue,
        };
        let writes = attrs.iter().any(derive_list_names_serialize);
        if !writes && !attrs.iter().any(derive_list_names_deserialize) {
            continue;
        }
        if !seen.insert(name.clone()) {
            index.twice_declared.insert(name.clone());
        }
        for key in ["into", "remote"] {
            if let Some(target) = container_conversion(attrs, key) {
                index.writes_through.insert(name.clone(), target);
            }
        }
        for attribute in attrs {
            for key in ["try_from", "from"] {
                if let Some(wire) = container_conversion(std::slice::from_ref(attribute), key) {
                    index
                        .read_routers
                        .entry(wire)
                        .or_default()
                        .insert(name.clone());
                }
            }
        }
        if !writes {
            continue;
        }
        index.writes_own_keys.insert(name.clone());
        for group in fields {
            for field in group {
                let Some(key) = field.ident.as_ref().map(syn::Ident::to_string) else {
                    continue;
                };
                let spelling = write_spelling(field);
                let keys = index.written_keys.entry(name.clone()).or_default();
                match keys.insert(key.clone(), spelling) {
                    Some(earlier) if earlier != spelling => {
                        index.split_keys.insert((name.clone(), key));
                    }
                    Some(_) | None => {}
                }
            }
        }
    }
}

/// The type named by the container-level `serde` conversion `key`, if any.
fn container_conversion(attrs: &[syn::Attribute], key: &str) -> Option<String> {
    let mut named = None;
    for attribute in attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                let wanted = meta.path.is_ident(key);
                if meta.input.peek(syn::Token![=]) {
                    let literal: syn::Lit = meta.value()?.parse()?;
                    if let syn::Lit::Str(text) = &literal {
                        if wanted {
                            named = Some(text.value());
                        }
                    }
                } else {
                    skip_meta_value(&meta)?;
                }
                Ok(())
            })
            .expect(
                "every container serde attribute in the wire crates parses as a nested meta list",
            );
    }
    named
}

/// Whether this attribute is a `derive` (or a `cfg_attr` carrying one) whose
/// list names `Serialize`.
fn derive_list_names_serialize(attribute: &syn::Attribute) -> bool {
    if !attribute.path().is_ident("derive") && !attribute.path().is_ident("cfg_attr") {
        return false;
    }
    let Ok(list) = attribute.meta.require_list() else {
        return false;
    };
    let mut flat = Vec::new();
    flatten_tokens(&list.tokens, &mut flat);
    flat.iter().any(|token| token == "Serialize")
}

/// The count of readable types visited, and every `file:line` where one of
/// their fields skips on `None` without a `deserialize_with`.
fn optional_absence_census() -> (usize, Vec<String>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the repository root sits two levels above the crate manifest")
        .to_path_buf();
    let index = wire_index();
    let mut readable = 0_usize;
    let mut offenders = Vec::new();
    for source in [
        "crates/cadmpeg-ir/src",
        "crates/cadmpeg-core/src",
        "crates/cadmpeg-asm/src",
    ] {
        let mut files = Vec::new();
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
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
            collect_optional_fields(
                &parsed.items,
                &relative,
                &index,
                &mut readable,
                &mut offenders,
            );
        }
    }
    (readable, offenders)
}

/// Visits every struct and enum among `items`, recursing into inline modules,
/// and records the unguarded optional fields of the readable ones.
fn collect_optional_fields(
    items: &[syn::Item],
    relative: &str,
    index: &WireIndex,
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
                let reader = declaration.ident.to_string();
                record_unguarded_fields(&declaration.fields, relative, &reader, index, offenders);
            }
            syn::Item::Enum(declaration) => {
                if !reads_its_own_keys(&declaration.attrs) {
                    continue;
                }
                *readable += 1;
                let reader = declaration.ident.to_string();
                for variant in &declaration.variants {
                    record_unguarded_fields(&variant.fields, relative, &reader, index, offenders);
                }
            }
            syn::Item::Mod(module) => {
                if is_test_module(module) {
                    continue;
                }
                if let Some((_, nested)) = &module.content {
                    collect_optional_fields(nested, relative, index, readable, offenders);
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

/// The `deserialize_with` helpers that state `null` is this key's own spelling
/// of `None`.
///
/// A field with no `skip_serializing_if` on its writing declaration is written
/// for every value, and as `null` for `None`, so `null` is the writer's
/// spelling there and the reader admits it. `cadmpeg_core::absent_key::nullable`
/// states that, and nothing else: it reads the field's own `Option`. The
/// reading declaration states no `default` beside it, so an absent key is a
/// missing field rather than a second spelling of `None`. A key that states
/// neither this nor a helper from [`NULL_REFUSING_HELPERS`] declares no
/// spelling at all, which is what this census refuses.
const NULL_STATING_HELPERS: &[&str] = &["cadmpeg_core::absent_key::nullable"];

/// Records every `Option` field of `fields` that states no helper from
/// [`NULL_REFUSING_HELPERS`].
///
/// The key is the field's own type. A field declared `Option<T>` on a type a
/// document reads gives `null` a second spelling of absence unless a helper
/// refuses it, whatever the field's `skip_serializing_if` says: the writing
/// half and the reading half are separate declarations, and only the reading
/// half decides what `null` becomes.
fn record_unguarded_fields(
    fields: &syn::Fields,
    relative: &str,
    reader: &str,
    index: &WireIndex,
    offenders: &mut Vec<String>,
) {
    for field in fields {
        if !is_option_type(&field.ty) {
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
        let at = format!("{relative}:{} {reader}.{name}", span.start().line);
        let helper = absence_spelling(field);
        let read = match helper.as_deref() {
            Some(helper) if NULL_REFUSING_HELPERS.contains(&helper) => ReadSpelling::Present,
            Some(helper) if NULL_STATING_HELPERS.contains(&helper) => ReadSpelling::Nullable,
            Some(helper) => {
                offenders.push(format!("{at} states the unlisted helper {helper}"));
                continue;
            }
            None => {
                offenders.push(format!("{at} states no deserialize_with"));
                continue;
            }
        };
        let defaulted = states_default(field);
        match (read, defaulted) {
            (ReadSpelling::Present, false) => offenders.push(format!(
                "{at} reads the absent key as its one spelling of None but states no default"
            )),
            (ReadSpelling::Nullable, true) => offenders.push(format!(
                "{at} reads null as its one spelling of None and states a default, which admits the absent key as a second"
            )),
            (ReadSpelling::Present, true) | (ReadSpelling::Nullable, false) => {}
        }
        if let Some((_, _, written)) = HAND_WRITTEN_KEYS
            .iter()
            .find(|(named, key, _)| *named == reader && *key == name)
        {
            record_spelling_match(&at, read, *written, "the hand-written writer", offenders);
            continue;
        }
        let writer = match index.writing_declaration(reader) {
            Ok(writer) => writer,
            Err(reason) => {
                offenders.push(format!("{at} {reason}"));
                continue;
            }
        };
        let written = if writer == reader {
            // The reading and writing declarations are one declaration, so the
            // field states both halves and no key lookup is needed. A field
            // with no name states no key at all, and is only reachable here.
            write_spelling(field)
        } else {
            match index.spelling(&writer, &name) {
                Ok(written) => written,
                Err(reason) => {
                    offenders.push(format!("{at} {reason}"));
                    continue;
                }
            }
        };
        record_spelling_match(&at, read, written, &writer, offenders);
    }
}

/// Records `at` when the reading and writing spellings of `None` disagree.
fn record_spelling_match(
    at: &str,
    read: ReadSpelling,
    written: WriteSpelling,
    writer: &str,
    offenders: &mut Vec<String>,
) {
    match (read, written) {
        (ReadSpelling::Present, WriteSpelling::Null) => offenders.push(format!(
            "{at} refuses null, but {writer} states no skip_serializing_if and writes null for None"
        )),
        (ReadSpelling::Nullable, WriteSpelling::Omitted) => offenders.push(format!(
            "{at} admits null, but {writer} skips the key for None and never writes null"
        )),
        (ReadSpelling::Nullable, WriteSpelling::Always) => offenders.push(format!(
            "{at} admits null, but {writer} states no optional value there and never writes null"
        )),
        (ReadSpelling::Present, WriteSpelling::Omitted | WriteSpelling::Always)
        | (ReadSpelling::Nullable, WriteSpelling::Null) => {}
    }
}

/// Which spelling of `None` a reading declaration admits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadSpelling {
    /// The key, when stated, states a value; absence is `None`.
    Present,
    /// The key is required and `null` is `None`.
    Nullable,
}

/// How the writing declaration spells a `None` at this field.
fn write_spelling(field: &syn::Field) -> WriteSpelling {
    if states_skip(field) {
        WriteSpelling::Omitted
    } else if is_option_type(&field.ty) {
        WriteSpelling::Null
    } else {
        WriteSpelling::Always
    }
}

/// Whether this field states a serde `default`.
fn states_default(field: &syn::Field) -> bool {
    field_states(field, "default")
}

/// Whether this field states a serde `skip_serializing_if`.
fn states_skip(field: &syn::Field) -> bool {
    field_states(field, "skip_serializing_if")
}

/// Whether any `serde` attribute of this field states `name`.
fn field_states(field: &syn::Field, name: &str) -> bool {
    let mut stated = false;
    for attribute in &field.attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                if meta.path.is_ident(name) {
                    stated = true;
                }
                skip_meta_value(&meta)?;
                Ok(())
            })
            .expect("every serde attribute in the wire crates parses as a nested meta list");
    }
    stated
}

/// Whether `ty` is spelled `Option<..>`, through any path prefix and through
/// a borrow. A writing declaration states `&'a Option<T>` for a key it holds
/// by reference, which is the same optional key.
fn is_option_type(ty: &syn::Type) -> bool {
    if let syn::Type::Reference(borrowed) = ty {
        return is_option_type(&borrowed.elem);
    }
    let syn::Type::Path(path) = ty else {
        return false;
    };
    path.qself.is_none()
        && path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Option")
}

/// The `deserialize_with` (or `with`) path this field states.
fn absence_spelling(field: &syn::Field) -> Option<String> {
    let mut helper = None;
    for attribute in &field.attrs {
        if !attribute.path().is_ident("serde") {
            continue;
        }
        attribute
            .parse_nested_meta(|meta| {
                let named_helper =
                    meta.path.is_ident("deserialize_with") || meta.path.is_ident("with");
                if meta.input.peek(syn::Token![=]) {
                    let literal: syn::Lit = meta.value()?.parse()?;
                    if let syn::Lit::Str(text) = &literal {
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
    helper
}

/// Every hand-written `Deserialize` impl in the three wire crates, outside
/// test modules and test files, as (crate-relative path, type name).
///
/// The source is parsed, not grepped: `syn` reads each file and the walk
/// visits every `impl` item, including one nested in an inline `mod`, so a
/// header wrapped over several lines, a generic parameter list, or a
/// fully-qualified trait path is found just the same. An impl counts when the
/// trait path ends in `Deserialize` and carries its lifetime argument,
/// regardless of whether that lifetime is declared by the impl.
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
        collect_rust_sources(&root.join(source), &mut files)
            .unwrap_or_else(|error| panic!("cannot collect {source}: {error}"));
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
    module.attrs.iter().any(|attribute| {
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        let mut requires_test = false;
        attribute
            .parse_nested_meta(|meta| {
                requires_test = cfg_requires_test(meta)?;
                Ok(())
            })
            .expect("every cfg attribute in the wire crates parses");
        requires_test
    })
}

/// Whether a `cfg` expression is restricted to test builds.
///
/// A substring search treats `feature = "testing"` as `cfg(test)`, and also
/// drops modules enabled by a production feature in `any(test, ...)`. The
/// recursive shape keeps only expressions that require `test` on every active
/// branch. Unknown expressions are retained for the census.
fn cfg_requires_test(meta: syn::meta::ParseNestedMeta<'_>) -> syn::Result<bool> {
    if meta.path.is_ident("test") {
        return Ok(true);
    }
    if meta.path.is_ident("all") {
        let mut has_argument = false;
        let mut requires_test = false;
        meta.parse_nested_meta(|nested| {
            has_argument = true;
            requires_test |= cfg_requires_test(nested)?;
            Ok(())
        })?;
        return Ok(has_argument && requires_test);
    }
    if meta.path.is_ident("any") {
        let mut has_argument = false;
        let mut every_branch_requires_test = true;
        meta.parse_nested_meta(|nested| {
            has_argument = true;
            every_branch_requires_test &= cfg_requires_test(nested)?;
            Ok(())
        })?;
        return Ok(has_argument && every_branch_requires_test);
    }
    if meta.path.is_ident("not") {
        // `not(test)` is a production configuration. Retain it in the walk;
        // the source may be compiled outside the test configuration.
        meta.parse_nested_meta(|nested| {
            let _ = cfg_requires_test(nested)?;
            Ok(())
        })?;
        return Ok(false);
    }
    if meta.input.peek(syn::Token![=]) {
        let _: syn::Lit = meta.value()?.parse()?;
    } else if meta.input.peek(syn::token::Paren) {
        meta.parse_nested_meta(|nested| {
            let _ = cfg_requires_test(nested)?;
            Ok(())
        })?;
    }
    Ok(false)
}

/// The type a hand-written `Deserialize` impl is written for: the last segment
/// of its self type, without generic arguments.
///
/// `None` when the impl is for another trait, is an inherent impl, or has no
/// lifetime argument in the `Deserialize` trait path.
fn deserialize_impl_target(implementation: &syn::ItemImpl) -> Option<String> {
    let (_, path, _) = implementation.trait_.as_ref()?;
    let deserialize = path.segments.last()?;
    if deserialize.ident != "Deserialize" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(arguments) = &deserialize.arguments else {
        return None;
    };
    if !arguments
        .args
        .iter()
        .any(|argument| matches!(argument, syn::GenericArgument::Lifetime(_)))
    {
        return None;
    }
    match implementation.self_ty.as_ref() {
        syn::Type::Path(typed) => Some(typed.path.segments.last()?.ident.to_string()),
        _ => None,
    }
}

/// Every `Deserialize<'a> for …` target named in a `macro_rules!` body.
///
/// A macro body is token text, so it is scanned as tokens: the walk flattens
/// every delimited group and looks for `Deserialize<'a> for`, accepting
/// either tokenization proc-macro uses for a lifetime. A target spelled as a
/// metavariable is recorded with its sigil.
fn macro_body_impl_targets(tokens: &proc_macro2::TokenStream) -> Vec<String> {
    let mut flat = Vec::new();
    flatten_tokens(tokens, &mut flat);
    let mut targets = Vec::new();
    for index in 0..flat.len() {
        if flat.get(index).map(String::as_str) != Some("Deserialize")
            || flat.get(index + 1).map(String::as_str) != Some("<")
        {
            continue;
        }
        let Some(mut cursor) = index.checked_add(2) else {
            continue;
        };
        if flat
            .get(cursor)
            .is_some_and(|token| token.starts_with('\'') && token.len() > 1)
        {
            cursor += 1;
        } else if flat.get(cursor).map(String::as_str) == Some("'") {
            cursor += 2;
        } else {
            continue;
        }
        if flat.get(cursor).map(String::as_str) != Some(">")
            || flat.get(cursor + 1).map(String::as_str) != Some("for")
        {
            continue;
        }
        let after = cursor + 2;
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
fn collect_rust_sources(directory: &Path, found: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(directory)
        .map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", directory.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_rust_sources(&path, found)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod scanner_tests {
    use super::*;

    #[test]
    fn cfg_classification_requires_a_test_only_expression() {
        let feature: syn::ItemMod =
            syn::parse_str(r#"#[cfg(feature = "testing")] mod production {}"#)
                .expect("parse feature-gated module");
        assert!(!is_test_module(&feature));

        let mixed: syn::ItemMod =
            syn::parse_str(r#"#[cfg(any(test, feature = "production"))] mod production {}"#)
                .expect("parse mixed module");
        assert!(!is_test_module(&mixed));

        let test_only: syn::ItemMod =
            syn::parse_str(r#"#[cfg(all(test, feature = "test_helpers"))] mod tests {}"#)
                .expect("parse test-only module");
        assert!(is_test_module(&test_only));

        let production: syn::ItemMod = syn::parse_str(r#"#[cfg(not(test))] mod production {}"#)
            .expect("parse production module");
        assert!(!is_test_module(&production));
    }

    #[test]
    fn manual_deserialize_census_accepts_a_named_lifetime() {
        let implementation: syn::ItemImpl =
            syn::parse_str("impl<'wire> serde::Deserialize<'wire> for Manual {}")
                .expect("parse manual Deserialize impl");
        assert_eq!(
            deserialize_impl_target(&implementation),
            Some("Manual".into())
        );

        let static_impl: syn::ItemImpl =
            syn::parse_str("impl serde::Deserialize<'static> for Manual {}").expect("parse");
        assert_eq!(deserialize_impl_target(&static_impl), Some("Manual".into()));

        let elided_impl: syn::ItemImpl =
            syn::parse_str("impl serde::Deserialize<'_> for Manual {}").expect("parse");
        assert_eq!(deserialize_impl_target(&elided_impl), Some("Manual".into()));
    }

    #[test]
    fn macro_deserialize_census_accepts_a_named_lifetime() {
        let file: syn::File = syn::parse_str(
            r#"
                macro_rules! make_reader {
                    ($name:ident) => {
                        impl<'wire> serde::Deserialize<'wire> for $name {}
                    };
                }
            "#,
        )
        .expect("parse macro");
        let syn::Item::Macro(item) = &file.items[0] else {
            panic!("the fixture is a macro");
        };
        assert_eq!(macro_body_impl_targets(&item.mac.tokens), vec!["$name"]);
    }

    #[test]
    fn source_collector_reports_a_missing_root() {
        let missing = std::env::temp_dir().join(format!(
            "cadmpeg-unknown-key-sweep-missing-{}",
            std::process::id()
        ));
        assert!(
            !missing.exists(),
            "test path unexpectedly exists: {missing:?}"
        );

        let mut found = Vec::new();
        let error = collect_rust_sources(&missing, &mut found)
            .expect_err("a missing source root must fail the census");
        assert!(error.contains(&missing.display().to_string()));
        assert!(error.contains("No such file") || error.contains("not found"));
    }

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
