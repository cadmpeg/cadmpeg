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

use cadmpeg_ir::CadIr;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{collect_documents, golden_files, hand_written_documents, states_native_elision};

#[test]
fn every_schema_position_has_one_spelling() {
    let schema =
        serde_json::to_value(cadmpeg_ir::cadir_json_schema()).expect("the schema serializes");
    let report = cadmpeg_test_support::schema_sweep::sweep(&schema, &documents())
        .expect("the empty document serializes");
    println!(
        "reached {} of {} definitions and probed {} struct positions; {} targets from \
         documents, {} built from the schema; {} uninhabited; read {} documents",
        report.definitions_reached,
        report.definitions,
        report.positions,
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
        report.accepting.is_empty(),
        "{} second spellings are admitted:\n{}",
        report.accepting.len(),
        report
            .accepting
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

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
            .map(|ir| serde_json::to_value(ir).expect("a CadIr serializes")),
    );
    documents.extend(hand_written_documents().into_iter().map(|(_, ir)| ir));
    for (name, seed) in seed_documents() {
        if let Err(error) = CadIr::deserialize(&seed) {
            panic!("the {name} seed does not read back: {error}");
        }
        documents.push(seed);
    }
    documents
}

/// Documents that state values whose readers check them against other
/// members in ways a value built from the schema alone does not meet: a
/// treatment diameter above the bore it widens, a knot vector as long as its
/// poles and degree need, a membership its cages commit.
fn seed_documents() -> Vec<(&'static str, Value)> {
    let hole = |name: &'static str, shape: Value| {
        let operation = json!({"kind": "hole", "value": {"shape": shape}});
        (name, document("features", feature(&operation)))
    };
    let form = |kind: Value| json!({"kind": "form", "value": {"kind": kind}});
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
    let direction = |direction: [f64; 2], distance: &str| {
        json!({
            "direction": direction,
            "spacing": 1.0,
            "distance": {"kind": distance, "parameter": "test:sweep:parameter#0"},
        })
    };
    vec![
        hole(
            "chamfer and counterdrill exit",
            json!({
                "construction": form(json!({"kind": "chamfer", "angle": 1.0, "diameter": 10.0})),
                "exit_kind": {"kind": "counterdrill", "angle": 1.0, "depth": 1.0, "diameter": 10.0},
                "diameter": 1.0,
            }),
        ),
        hole(
            "counterbore and drilled counterbore exit",
            json!({
                "construction": form(json!({"kind": "counterbore", "depth": 1.0, "diameter": 10.0})),
                "exit_kind": {
                    "kind": "counterbore_drilled",
                    "depth": 1.0,
                    "diameter": 10.0,
                    "drill_point_angle": 1.0,
                },
                "diameter": 1.0,
            }),
        ),
        hole(
            "drilled counterbore",
            json!({
                "construction": form(json!({
                    "kind": "counterbore_drilled",
                    "depth": 1.0,
                    "diameter": 10.0,
                    "drill_point_angle": 1.0,
                })),
                "diameter": 1.0,
            }),
        ),
        hole(
            "countersink",
            json!({
                "construction": form(json!({"kind": "countersink", "angle": 1.0, "diameter": 10.0})),
                "diameter": 1.0,
            }),
        ),
        hole(
            "counterdrill",
            json!({
                "construction": form(json!({
                    "kind": "counterdrill",
                    "angle": 1.0,
                    "depth": 1.0,
                    "diameter": 10.0,
                })),
                "diameter": 1.0,
            }),
        ),
        hole(
            "native thread",
            json!({
                "construction": {
                    "kind": "native_thread",
                    "value": {"drill_point_angle": 1.0, "major_diameter": 10.0, "thread_depth": 1.0},
                },
                "diameter": 1.0,
            }),
        ),
        (
            "complete Form membership",
            document(
                "features",
                feature(&json!({
                    "kind": "form",
                    "value": {
                        "cages": {"kind": "complete", "value": ["test:sweep:subd#0"]},
                        "cage_inputs": {
                            "observations": [{
                                "binding": {"kind": "decoded", "cage": "test:sweep:subd#0"},
                                "paths": [],
                            }],
                            "membership": {
                                "kind": "complete",
                                "members": [0],
                                "order": "source",
                                "certainty": "assumed",
                            },
                        },
                        "inputs": {"lists": [], "parameters": {}, "references": []},
                    },
                })),
            ),
        ),
        (
            "partial Form membership",
            document(
                "features",
                feature(&json!({
                    "kind": "form",
                    "value": {
                        "cages": {
                            "kind": "retained",
                            "value": {
                                "observations": [
                                    {
                                        "binding": {"kind": "decoded", "cage": "test:sweep:subd#0"},
                                        "paths": [],
                                    },
                                    {
                                        "binding": {
                                            "kind": "undecoded",
                                            "availability": "active_undecoded",
                                            "resource": {"entry": "#0", "source": "test:sweep:record#0"},
                                        },
                                        "paths": [],
                                    },
                                    {"binding": {"kind": "unresolved"}, "paths": []},
                                ],
                                "membership": {
                                    "kind": "partial",
                                    "cardinality": 3,
                                    "members": [{"observation": 1, "ordinal": 0}],
                                    "order": "unknown",
                                },
                            },
                        },
                        "inputs": {"lists": [], "parameters": {}, "references": []},
                    },
                })),
            ),
        ),
        (
            "wedge",
            document(
                "features",
                feature(&json!({
                    "kind": "primitive",
                    "value": {
                        "op": "new_body",
                        "solid": {
                            "kind": "wedge",
                            "xmin": 0.0, "xmax": 1.0, "ymin": 0.0, "ymax": 1.0, "zmin": 0.0,
                            "zmax": 1.0, "x2min": 0.0, "x2max": 1.0, "z2min": 0.0, "z2max": 1.0,
                        },
                    },
                })),
            ),
        ),
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
            "rectangular sketch pattern",
            document(
                "sketch_constraints",
                json!({
                    "definition": {
                        "kind": "rectangular_pattern",
                        "pattern": {
                            "directions": [
                                direction([1.0, 0.0], "spacing"),
                                direction([0.0, 1.0], "span"),
                            ],
                            "rows": [[{"entities": ["test:sweep:sketch-entity#0"]}]],
                        },
                    },
                    "id": "test:sweep:sketch-constraint#0",
                    "sketch": "test:sweep:sketch#0",
                }),
            ),
        ),
        (
            "repeated parallel line distance",
            document(
                "spatial_sketch_constraints",
                json!({
                    "definition": {
                        "kind": "repeated_parallel_line_distance",
                        "pairs": [
                            {"first": "test:sweep:entity#0", "second": "test:sweep:entity#1"},
                            {"first": "test:sweep:entity#2", "second": "test:sweep:entity#3"},
                        ],
                        "parameter": "test:sweep:parameter#0",
                    },
                    "id": "test:sweep:spatial-constraint#0",
                    "sketch": "test:sweep:spatial-sketch#0",
                }),
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
                    "faces": [],
                    "feature_edges": [],
                    "id": "test:sweep:tessellation#0",
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
    ]
}

/// The empty document with `element` as the one element of `arena`.
fn document(arena: &str, element: Value) -> Value {
    let mut document = serde_json::to_value(CadIr::empty()).expect("a CadIr serializes");
    document["model"][arena] = Value::Array(vec![element]);
    document
}

/// A feature row stating `operation`.
fn feature(operation: &Value) -> Value {
    json!({
        "definition": {"kind": "operation", "value": operation},
        "dependencies": [],
        "id": "test:sweep:feature#0",
        "ordinal": 0,
        "outputs": [],
        "source_content": [],
        "source_properties": {},
    })
}
