// SPDX-License-Identifier: Apache-2.0
//! Every document-reachable shape a live `FreeCAD` decode produces refuses an
//! unknown key.
//!
//! The committed `FreeCAD` decode goldens store `native` as an elision marker,
//! so their `ir` does not read back as a `CadIr` and the workspace-wide sweep
//! in `cadmpeg-ir` skips them. This sweep decodes the charter fixtures instead
//! and runs the same walk over the live document, which reaches the sketch,
//! annotation, assembly, and product shapes no other codec's goldens carry.

use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::unknown_keys::accepting_shapes;

/// Extension of the committed fixture inputs.
const FIXTURE_EXTENSION: &str = "FCStd";

#[test]
fn every_decoded_shape_refuses_an_unknown_key() {
    let fixtures = fixture_inputs();
    assert!(
        !fixtures.is_empty(),
        "the sweep found no fixtures to decode"
    );
    let mut accepting = BTreeSet::new();
    let mut swept = 0_usize;
    for path in fixtures {
        let bytes = std::fs::read(&path).expect("read fixture");
        let result = FcstdCodec
            .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
            .expect("decode fixture");
        let ir = serde_json::to_value(result.ir()).expect("serialize ir");
        let (shapes, count) = accepting_shapes(&ir)
            .unwrap_or_else(|error| panic!("{} does not read back: {error}", path.display()));
        assert!(count > 0, "a decoded document reads back as a document");
        swept += count;
        accepting.extend(shapes);
    }
    assert!(swept > 0, "the sweep walked {swept} shapes");
    let unexpected = accepting
        .iter()
        .filter(|shape| !is_free_form(shape))
        .cloned()
        .collect::<Vec<_>>();
    assert!(
        unexpected.is_empty(),
        "{} decoded shapes accept an unknown key:\n{}",
        unexpected.len(),
        unexpected.join("\n")
    );
}

/// The non-native nodes a document may accept an unknown key on.
///
/// Each is a free-form map whose keys are source names, so it declares no key
/// set for a deny to bind. Every owner declares `deny_unknown_fields`, and that
/// deny is live one level up; the map itself is the node the source fills.
/// Probing every JSON value kind is what makes them visible: a
/// `BTreeMap<String, String>` refuses `true` and accepts `"zz"`, and a map
/// whose values are arrays accepts `[]` alone, so a narrower probe list reads
/// such a node as refusing.
///
/// * `/model/appearance_bindings/#/channels` - `AppearanceBinding::channels`,
///   `BTreeMap<String, String>` of source channel names;
/// * `/model/appearances/#/properties` - `Appearance::properties`,
///   `BTreeMap<String, f64>` of source renderer properties;
/// * `/model/configurations/#/properties` - `DesignConfiguration::properties`,
///   configuration-local named values;
/// * `/model/drawings/#/parameters` - `Drawing::parameters`, retained drawing
///   parameters by source name;
/// * `/model/drawings/#/relationships` - `Drawing::relationships`,
///   `BTreeMap<String, Vec<ReferenceSelection>>`, the source's relationship
///   roles by name, each holding the references that role states;
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
/// * `/model/semantic_annotations/#/references` -
///   `SemanticAnnotation::references`, `BTreeMap<String,
///   Vec<ReferenceSelection>>`, the native note's reference roles by name;
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
    "/model/drawings/#/relationships",
    "/model/features/#/definition/parameters",
    "/model/features/#/source_properties",
    "/model/parameters/#/properties",
    "/model/presentation_documents/#/states/#/attributes",
    "/model/presentation_documents/#/states/#/kind/value/properties",
    "/model/product_definitions/#/bom_properties",
    "/model/semantic_annotations/#/parameters",
    "/model/semantic_annotations/#/references",
    "/model/view_presentations/#/properties",
    "/source/attributes",
    "/source/identity/dialects/extra/#/declared",
    "/source/identity/dialects/primary/declared",
];

/// Whether `shape` is the one free-form map the document admits.
fn is_free_form(shape: &str) -> bool {
    FREE_FORM_SHAPES.contains(&shape)
}

/// The charter fixtures this codec's goldens decode, in directory order.
fn fixture_inputs() -> Vec<PathBuf> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the crate manifest sits two levels below the repository root")
        .join("corpus/freecad_fcstd/fixtures");
    let mut found = Vec::new();
    let entries = std::fs::read_dir(&directory).expect("read the fixture directory");
    for entry in entries {
        let file = entry.expect("fixture entry").path();
        if file
            .extension()
            .is_some_and(|extension| extension == FIXTURE_EXTENSION)
        {
            found.push(file);
        }
    }
    found.sort();
    found
}
