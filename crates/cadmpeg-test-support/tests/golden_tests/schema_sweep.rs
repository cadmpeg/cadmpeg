// SPDX-License-Identifier: Apache-2.0
//! Every type the `CadIr` JSON Schema reaches has one spelling for each value
//! (`docs/cadir-v11.md` §2.5), whatever source shape its reader has.
//!
//! The sweep starts from the committed goldens, the example documents, and the
//! hand-written documents of the unknown-key sweep, and builds every target
//! they leave unreached from the schema. Each struct position then refuses a
//! key no member declares, `null` for an `Option` member, the omission of
//! every other member, and a repeated key. A target the sweep cannot reach
//! fails the test.

use cadmpeg_core::json;
use cadmpeg_core::serde_erase::from_json_value;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::CadIr;
use serde_json::Value;

use super::{collect_documents, golden_files, hand_written_documents, states_native_elision};

#[test]
fn sidecar_class_sets_have_unique_members_in_schema() {
    let schema: Value = serde_json::from_str(cadmpeg_ir::DECODE_SIDECAR_JSON_SCHEMA)
        .expect("the sidecar schema is JSON");
    for path in [
        "/$defs/RetainedSourceRecord/properties/untyped/uniqueItems",
        "/$defs/SourceFidelity/properties/unrepresented/uniqueItems",
    ] {
        assert_eq!(schema.pointer(path), Some(&Value::Bool(true)), "{path}");
    }
}

#[test]
fn every_sidecar_schema_position_has_one_spelling() {
    use cadmpeg_core::decode::{ArenaClass, DecodeDepth};
    use cadmpeg_ir::hash::digest::Sha256Digest;
    use cadmpeg_ir::ids::UnknownId;
    use cadmpeg_ir::report::decode::DecodeReport;
    use cadmpeg_ir::{DecodeSidecar, RetainedSourceRecord, SourceFidelity};

    let digest = Sha256Digest::digest(b"schema sweep");
    let report = DecodeReport::of_cadir_read(Vec::new(), DecodeDepth::Full);
    let mut fidelity = SourceFidelity::new(digest.clone());
    fidelity
        .insert_retained_record(
            UnknownId::mint("synthetic:source:record#one").expect("record identity"),
            RetainedSourceRecord::whole("source", vec![1])
                .with_untyped([ArenaClass::Model, ArenaClass::Design]),
        )
        .expect("distinct record identity");
    let sidecar = DecodeSidecar::bind_sha256(digest, report, fidelity);
    let document =
        cadmpeg_ir::hash::finite_json::to_json_value(&sidecar).expect("the sidecar serializes");
    let schema: Value = serde_json::from_str(cadmpeg_ir::DECODE_SIDECAR_JSON_SCHEMA)
        .expect("the sidecar schema is JSON");
    let read = |text: &str| {
        let value = DecodeSidecar::from_json(text)
            .map_err(|error| <serde_json::Error as serde::de::Error>::custom(error.to_string()))?;
        cadmpeg_ir::hash::finite_json::to_json_value(&value)
    };
    let mut classified = document.clone();
    classified["report"]["identity"] = json!({
        "kind": "classified",
        "dialects": {
            "primary": {"dialect": "step:ap214", "declared": {"version": "1"},
                        "instance": "part", "admission": {"kind": "admitted"}},
            "extra": [],
        },
    });
    classified["fidelity"]["annotations"]["exactness"] = json!({
        "synthetic:source:record#one": {"kind": "fields", "fields": {"name": "derived"}},
    });
    read(&classified.to_string()).expect("the classified sidecar reads");
    let report = cadmpeg_test_support::schema_sweep::sweep(
        &schema,
        &[document.clone(), classified],
        &document,
        &read,
    );
    assert!(report.unreached.is_empty(), "{:?}", report.unreached);
    assert!(report.unprobed.is_empty(), "{:?}", report.unprobed);
    assert!(report.accepting.is_empty(), "{:?}", report.accepting);
}

#[test]
fn every_schema_position_has_one_spelling() {
    let schema: Value =
        serde_json::from_str(cadmpeg_ir::CADIR_JSON_SCHEMA).expect("the schema is JSON");
    let empty = cadmpeg_ir::hash::finite_json::to_json_value(&CadIr::empty())
        .expect("the empty document serializes");
    let read = |text: &str| {
        cadmpeg_ir::hash::finite_json::to_json_value(&serde_json::from_str::<CadIr>(text)?)
    };
    let report = cadmpeg_test_support::schema_sweep::sweep(&schema, &documents(), &empty, &read);
    println!(
        "reached {} of {} definitions and probed {} struct positions, {} data-free \
         enums, {} sets and pair-maps, and {} ordered member lists; {} targets from \
         documents, {} built from the schema; {} uninhabited; read {} documents",
        report.definitions_reached,
        report.definitions,
        report.positions,
        report.names,
        report.sets,
        report.ordered,
        report.from_documents,
        report.built,
        report.uninhabited.len(),
        report.reads,
    );
    assert!(
        report.unreached.is_empty(),
        "{} schema targets are reached by no document:\n{}",
        report.unreached.len(),
        report.unreached.join("\n")
    );
    assert!(
        report.unprobed.is_empty(),
        "{} value positions have no sample to probe. Give each one in `seed_documents()` \
         in crates/cadmpeg-test-support/tests/golden_tests/schema_sweep.rs: a document \
         the readers accept that holds the position, and for a unique list two of its \
         members:\n{}",
        report.unprobed.len(),
        report.unprobed.join("\n")
    );
    let mut accepting = report.accepting;
    for admitted in INTERIM_SECOND_SPELLINGS {
        assert!(
            accepting.remove(*admitted),
            "the interim second spelling is no longer admitted; delete its entry: {admitted}"
        );
    }
    assert!(
        accepting.is_empty(),
        "{} second spellings are admitted:\n{}",
        accepting.len(),
        accepting.iter().cloned().collect::<Vec<_>>().join("\n")
    );
}

/// The reader preserves the established modifier-list admission, including
/// any order and repeated members; the writer emits a sorted set.
const INTERIM_SECOND_SPELLINGS: &[&str] = &[
    "#/$defs/PmiDefinition/oneOf/4/properties/modifiers: duplicate member is accepted",
    "#/$defs/PmiDefinition/oneOf/4/properties/modifiers: two members out of order are read and written back sorted",
];

/// Every readable golden document, every example, and every hand-written
/// document.
fn documents() -> Vec<Value> {
    let mut documents = Vec::new();
    for path in golden_files() {
        let text = std::fs::read_to_string(&path).expect("read golden");
        let golden: Value = serde_json::from_str(&text).expect("parse golden");
        let mut roots = Vec::new();
        collect_documents(&golden, &mut String::new(), &mut roots, &mut Vec::new());
        documents.extend(
            roots
                .into_iter()
                .filter(|(_, ir)| !states_native_elision(ir))
                .map(|(_, ir)| ir.clone()),
        );
    }
    let examples = [
        CadIr::empty(),
        cadmpeg_ir::examples::unit_cube().expect("the unit cube builds"),
        cadmpeg_ir::examples::unit_cube_with_history_body().expect("the history cube builds"),
        cadmpeg_ir::examples::directed_subd_sum().expect("the subd sum builds"),
    ];
    documents.extend(
        examples
            .iter()
            .map(|ir| json::to_value(ir).expect("a CadIr serializes")),
    );
    documents.extend(hand_written_documents().into_iter().map(|(_, ir)| ir));
    for (name, seed) in seed_documents() {
        if let Err(error) = from_json_value::<CadIr>(&seed) {
            panic!("the {name} seed does not read back: {error}");
        }
        documents.push(seed);
    }
    documents
}

/// Documents that state values whose readers check them against other
/// members in ways a value built from the schema alone does not meet, such
/// as a knot vector as long as its poles and degree need.
fn seed_documents() -> Vec<(&'static str, Value)> {
    let identity = json!(cadmpeg_ir::transform::Transform::identity());
    let polar = |poles: Value| {
        json!({
            "geometry": {
                "kind": "polar_nurbs",
                "degree": 1,
                "knots": [0.0, 0.0, 1.0, 1.0],
                "periodic": false,
                "poles": poles,
            },
            "id": "test:sweep:pcurve#0",
            "metadata": {"form": {"fit_tolerance": 0.01}, "kind": "general"},
        })
    };
    let parameter = |key: u32| json!({"target": {"kind": "parameter", "value": format!("test:sweep:parameter#{key}")}});
    let measure = |kind: &str| json!({"kind": {"kind": kind}, "of": {"kind": "model"}});
    vec![
        (
            "polynomial polar NURBS",
            document(
                "pcurves",
                polar(json!({
                    "kind": "polynomial",
                    "poles": [
                        {"axial": 0.0, "radial": {"u": 1.0, "v": 0.0}},
                        {"axial": 1.0, "radial": {"u": 1.0, "v": 1.0}},
                    ],
                })),
            ),
        ),
        (
            "rational polar NURBS",
            document(
                "pcurves",
                polar(json!({
                    "kind": "rational",
                    "poles": [
                        {"axial": 0.0, "radial": {"u": 1.0, "v": 0.0}, "weight": 1.0},
                        {"axial": 1.0, "radial": {"u": 1.0, "v": 1.0}, "weight": 1.0},
                    ],
                })),
            ),
        ),
        (
            "corner channel",
            document(
                "tessellations",
                json!({
                    "channels": [{
                        "addressing": {"kind": "corner", "indices": [0, 0, 0]},
                        "data": "AAAAAAAAAAA=",
                        "flags": 0,
                        "item_size": 8,
                        "kind": 1,
                    }],
                    "feature_edges": [],
                    "id": "test:sweep:tessellation#0",
                    "of": {"kind": "unresolved"},
                    "mesh": {
                        "kind": "shaded_list",
                        "triangles": [[0, 1, 2]],
                        "vertices": [
                            {"normal": {"x": 0.0, "y": 0.0, "z": 1.0}, "position": {"x": 0.0, "y": 0.0, "z": 0.0}},
                            {"normal": {"x": 0.0, "y": 0.0, "z": 1.0}, "position": {"x": 1.0, "y": 0.0, "z": 0.0}},
                            {"normal": {"x": 0.0, "y": 0.0, "z": 1.0}, "position": {"x": 0.0, "y": 1.0, "z": 0.0}},
                        ],
                    },
                    "texture_assignments": [],
                    "texture_coordinates": [],
                    "triangle_groups": [],
                }),
            ),
        ),
        (
            "neutral parameter expression with a blank native option",
            design_document(
                "parameters",
                json!({
                    "definition": {
                        "kind": "expression",
                        "value": {
                            "kind": "neutral",
                            "value": {
                                "text": "@0 > $0",
                                "refs": [parameter(0)],
                                "measures": [measure("length")],
                            },
                        },
                    },
                    "id": "test:sweep:parameter#1",
                    "kind": {"kind": "boolean"},
                    "native_options": {"test:note": {"kind": "blank", "value": " "}},
                    "scope": "test:sweep:product-definition#0",
                    "source_settings": {},
                }),
            ),
        ),
        (
            "current state with poses",
            design_document(
                "scopes",
                json!({
                    "active_arrangements": [],
                    "current": [{
                        "at": {"kind": "start", "value": "test:sweep:container#0"},
                        "bodies": [],
                        "poses": [
                            [
                                [{"occurrence": {"kind": "occurrence", "value": "test:sweep:occurrence#0"}}],
                                identity,
                            ],
                            [
                                [{"occurrence": {"kind": "native", "value": {"codec": "inventor", "value": {"kind": "text", "value": "occurrence 1"}}}}],
                                identity,
                            ],
                        ],
                        "status": "current",
                    }],
                    "definition": "test:sweep:product-definition#0",
                    "occurrences": {},
                    "roots": ["test:sweep:container#0"],
                    "unproduced": ["test:sweep:body#0", "test:sweep:body#1"],
                }),
            ),
        ),
    ]
}

/// The empty document with `element` as the one element of the `design`
/// arena `arena`.
fn design_document(arena: &str, element: Value) -> Value {
    let mut document = json::to_value(CadIr::empty()).expect("a CadIr serializes");
    assert!(
        document["design"].get(arena).is_some(),
        "no design arena {arena}"
    );
    document["design"][arena] = Value::Array(vec![element]);
    document
}

/// The empty document with `element` as the one element of `arena`.
fn document(arena: &str, element: Value) -> Value {
    let mut document = json::to_value(CadIr::empty()).expect("a CadIr serializes");
    let home = ["geometry", "model"]
        .into_iter()
        .find(|home| document[*home].get(arena).is_some())
        .unwrap_or_else(|| panic!("no document root holds the arena {arena}"));
    document[home][arena] = Value::Array(vec![element]);
    document
}
