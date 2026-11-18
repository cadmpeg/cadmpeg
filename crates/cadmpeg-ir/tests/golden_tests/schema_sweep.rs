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
    let identity = json!(cadmpeg_ir::transform::Transform::identity());
    let mut partial_form_inputs = json!(cadmpeg_ir::features::form::FormInputs::default());
    partial_form_inputs["reference_evidence"] = json!([{
        "source": "test:sweep:record#0",
        "role": "body",
        "ordinal": 0,
        "group": 0,
        "target": {
            "kind": "document",
            "target": 22,
            "segment": 7,
            "asset_guid": "literal-asset",
            "target_type_guid": "11111111-1111-4111-8111-111111111111",
            "link_name": "literal-link",
            "version": {
                "key": "22222222-2222-4222-8222-222222222222",
                "urn": "literal-version"
            }
        }
    }]);
    partial_form_inputs["order_evidence"] = json!([{"order": "source", "members": [0, 1]}]);
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
    let parameter = |key: u32| json!({"target": {"kind": "parameter", "value": format!("test:sweep:parameter#{key}")}});
    let measure = |kind: &str| json!({"kind": {"kind": kind}, "of": {"kind": "model"}});
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
                        "inputs": cadmpeg_ir::features::form::FormInputs::default(),
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
                        "inputs": partial_form_inputs,
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
    let mut document = serde_json::to_value(CadIr::empty()).expect("a CadIr serializes");
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
    let mut document = serde_json::to_value(CadIr::empty()).expect("a CadIr serializes");
    assert!(
        document["design"].get(arena).is_some(),
        "no design arena {arena}"
    );
    document["design"][arena] = Value::Array(vec![element]);
    document
}

/// The empty document with `element` as the one element of `arena`.
fn document(arena: &str, element: Value) -> Value {
    let mut document = serde_json::to_value(CadIr::empty()).expect("a CadIr serializes");
    let home = ["geometry", "model"]
        .into_iter()
        .find(|home| document[*home].get(arena).is_some())
        .unwrap_or_else(|| panic!("no document root holds the arena {arena}"));
    document[home][arena] = Value::Array(vec![element]);
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
