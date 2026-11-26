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

use cadmpeg_core::serde_erase::from_json_value;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::CadIr;
use serde_json::{json, Value};

use super::{collect_documents, golden_files, hand_written_documents, states_native_elision};

#[test]
fn every_schema_position_has_one_spelling() {
    let schema: Value =
        serde_json::from_str(cadmpeg_ir::CADIR_JSON_SCHEMA).expect("the schema is JSON");
    let report = cadmpeg_test_support::schema_sweep::sweep(&schema, &documents())
        .expect("the empty document serializes");
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

/// Second spellings the readers admit until errata item Q rules CADIR
/// reading (RULINGS-P P7-D5, gap 20261119-ERR1-C-I6-3): `main` read the
/// dimension modifiers as a list in any order.
const INTERIM_SECOND_SPELLINGS: &[&str] = &[
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
            "neutral parameter expression",
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
                    "native_options": {},
                    "scope": "test:sweep:product-definition#0",
                    "source_settings": {},
                }),
            ),
        ),
        (
            "neutral program",
            design_document(
                "programs",
                json!({
                    "body": {
                        "kind": "neutral",
                        "value": [
                            {
                                "kind": "assign",
                                "target": parameter(0),
                                "value": {"text": "$0", "refs": [parameter(1)], "measures": []},
                            },
                            {
                                "kind": "if",
                                "condition": {"text": "@0 > 1", "refs": [], "measures": [measure("area")]},
                                "then": [],
                                "otherwise": [],
                            },
                        ],
                    },
                    "id": "test:sweep:parameter-program#0",
                    "scope": "test:sweep:product-definition#0",
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
        (
            "operation limits",
            process_document("operations", operation_limits()),
        ),
        (
            "tool with a library GUID",
            process_document(
                "tools",
                vec![json!({
                    "id": "test:sweep:tool#0",
                    "class": "flat_endmill",
                    "native_kind": {"codec": "f3d", "value": {"kind": "text", "value": "flat end mill"}},
                    "library": {"kind": "guid", "value": "123e4567-e89b-12d3-a456-426614174000"},
                    "nc": {},
                    "cutter": {},
                })],
            ),
        ),
        (
            "neutral process parameter expression",
            process_document(
                "parameters",
                vec![json!({
                    "id": "test:sweep:process-parameter#1",
                    "owner": {"kind": "setup", "value": "test:sweep:setup#0"},
                    "name": "depth",
                    "definition": {
                        "kind": "expression",
                        "value": {
                            "kind": "neutral",
                            "value": {
                                "text": "$0 + $1",
                                "refs": [
                                    {"kind": "process", "value": "test:sweep:process-parameter#0"},
                                    {"kind": "design", "value": parameter(0)},
                                ],
                                "measures": [],
                            },
                        },
                    },
                })],
            ),
        ),
    ]
}

/// Two operations whose height, radius, and axial limits state every arm of
/// `HeightFrom`, `RadialLimit`, `RadiusFrom`, `AxialFrom`, and `LimitPoint`:
/// their values sit under map keys, which the schema alone does not build.
fn operation_limits() -> Vec<Value> {
    let choice = |value: Value| json!({"value": value, "parameters": []});
    let tag = |kind: &str| json!({"kind": kind});
    let tagged = |kind: &str, value: Value| json!({"kind": kind, "value": value});
    let height =
        |from: Value| json!({"from": choice(from), "offset": {"kind": "literal", "value": 5.0}});
    let from = |base: Value| choice(tagged("from", json!({"base": base})));
    let axial = |base: Value, point: Option<&str>| match point {
        Some(point) => json!({"base": choice(base), "point": choice(json!(point))}),
        None => json!({"base": choice(base), "offset": {"kind": "literal", "value": 1.0}}),
    };
    let operation = |key: u32, heights: Value, radii: Value, axial: Value| {
        json!({
            "id": format!("test:sweep:operation#{key}"),
            "kind": {"kind": "machining", "class": {"kind": "probing"}},
            "native_kind": {"codec": "f3d", "value": {"kind": "text", "value": "probe"}},
            "features": [],
            "groups": [],
            "geometry": [],
            "heights": heights,
            "radii": radii,
            "axial": axial,
            "technology": {"feeds": []},
            "configuration_states": {},
        })
    };
    vec![
        operation(
            0,
            json!({
                "clearance": height(tag("wcs")),
                "retract": height(tag("stock_top")),
                "feed": height(tag("stock_bottom")),
                "top": height(tag("model_top")),
                "bottom": height(tag("model_bottom")),
            }),
            json!({
                "outer": choice(tagged("absolute", tagged("diameter", json!({"kind": "literal", "value": 40.0})))),
                "inner": choice(tagged("from", json!({"base": tag("stock_outer"), "offset": {"kind": "literal", "value": 2.0}}))),
                "outer_clearance": from(tag("stock_inner")),
                "inner_clearance": from(tagged("selection_min", json!("turning"))),
                "outer_retract": from(tagged("selection_max", json!("turning"))),
            }),
            json!({
                "front": axial(tag("model_front"), Some("contact_point")),
                "back": axial(tag("model_back"), Some("cutting_edge")),
            }),
        ),
        operation(
            1,
            json!({
                "clearance": height(tagged("height", json!("retract"))),
                "retract": height(tagged("selection", json!("top face"))),
                "feed": height(tag("geometry")),
                "top": height(tagged("native", json!("f3d:x"))),
                "bottom": {},
            }),
            json!({
                "outer": from(tagged("positions", json!({"slot": "limits", "side": choice(json!("front"))}))),
                "inner": from(tagged("limit", json!("outer"))),
                "outer_clearance": from(tagged("native", json!("f3d:tool-radius"))),
            }),
            json!({
                "front": axial(tag("stock_front"), None),
                "back": axial(tagged("native", json!("f3d:selection")), None),
            }),
        ),
    ]
}

/// The empty document with `elements` as the elements of the `process` arena
/// `arena`.
fn process_document(arena: &str, elements: Vec<Value>) -> Value {
    let mut document = json::to_value(CadIr::empty()).expect("a CadIr serializes");
    assert!(
        document["process"].get(arena).is_some(),
        "no process arena {arena}"
    );
    document["process"][arena] = Value::Array(elements);
    document
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
