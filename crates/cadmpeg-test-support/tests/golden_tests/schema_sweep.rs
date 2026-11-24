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
    let (two_member_complete_form, two_member_partial_form) = two_member_forms();
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
    let axis = |name: &'static str, axis: Value| {
        (
            name,
            document(
                "features",
                feature(&json!({"kind": "datum_axis", "value": {"axis": axis}})),
            ),
        )
    };
    vec![
        axis(
            "input-only datum axis",
            json!({
                "inputs": [{"reference": {"kind": "built", "value": {
                    "kind": "point", "inputs": [], "after": [], "resolved_in": {},
                    "resolved": {"x": 0.0, "y": 0.0, "z": 0.0}
                }}}],
                "native_options": {}, "source_settings": {}
            }),
        ),
        axis(
            "option-only datum axis",
            json!({
                "inputs": [],
                "native_options": {"test:flag": {"kind": "bool", "value": {"kind": "literal", "value": true}}},
                "source_settings": {}
            }),
        ),
        axis(
            "setting-only datum axis",
            json!({
                "inputs": [], "native_options": {},
                "source_settings": {"test:flag": {"kind": "bool", "value": {"kind": "literal", "value": true}}}
            }),
        ),
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
        (TWO_MEMBER_COMPLETE_FORM, two_member_complete_form),
        (TWO_MEMBER_PARTIAL_FORM, two_member_partial_form),
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

const TWO_MEMBER_COMPLETE_FORM: &str = "two-member complete Form";
const TWO_MEMBER_PARTIAL_FORM: &str = "two-member partial Form";

/// Each unique Form list, as the seed that holds two of its members and the
/// list's pointer below that seed's Form construction.
const FORM_LISTS: [(&str, &str); 13] = [
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/lists"),
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/lists/0/members"),
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/reference_runs"),
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/position_evidence"),
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/order_evidence"),
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/closure_evidence"),
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/resource_evidence"),
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/resource_associations"),
    (TWO_MEMBER_COMPLETE_FORM, "/inputs/resource_state_evidence"),
    (
        TWO_MEMBER_COMPLETE_FORM,
        "/cage_inputs/observations/0/paths",
    ),
    (TWO_MEMBER_COMPLETE_FORM, "/cage_inputs/membership/members"),
    (TWO_MEMBER_PARTIAL_FORM, "/inputs/cardinality_evidence"),
    (TWO_MEMBER_PARTIAL_FORM, "/cages/value/membership/members"),
];

#[test]
fn every_unique_form_list_refuses_a_repeated_member() {
    let seeds = seed_documents();
    for (name, pointer) in FORM_LISTS {
        let (_, seed) = seeds
            .iter()
            .find(|(seed, _)| *seed == name)
            .unwrap_or_else(|| panic!("no {name} seed"));
        assert!(CadIr::deserialize(seed).is_ok(), "the {name} seed reads");
        let mut repeated = seed.clone();
        let list = form_construction(&mut repeated)
            .pointer_mut(pointer)
            .and_then(Value::as_array_mut)
            .unwrap_or_else(|| panic!("the {name} seed states no list at {pointer}"));
        assert_eq!(
            list.len(),
            2,
            "the {name} seed holds two members at {pointer}"
        );
        list[1] = list[0].clone();
        assert!(
            CadIr::deserialize(&repeated).is_err(),
            "the {name} seed reads a repeated member at {pointer}"
        );
    }
}

/// The Form construction of a seed made by [`feature`].
fn form_construction(document: &mut Value) -> &mut Value {
    let home = ["geometry", "model"]
        .into_iter()
        .find(|home| document[*home].get("features").is_some())
        .expect("a document root holds the features arena");
    &mut document[home]["features"][0]["definition"]["value"]["value"]
}

/// A complete and a partial Form whose unique lists ([`FORM_LISTS`]) each
/// hold two members that the Form reader's cross-checks accept.
fn two_member_forms() -> (Value, Value) {
    let record = |key: u32| json!(format!("test:sweep:record#{key}"));
    let path = |key: u32| json!({"references": [record(key)]});
    let resource = |entry: &str| json!({"source": record(0), "entry": entry});
    let members = json!([
        {"ordinal": 0, "reference": record(0)},
        {"ordinal": 1, "reference": record(1)},
    ]);
    let mut complete_inputs = json!(cadmpeg_ir::features::form::FormInputs::default());
    for (list, value) in [
        (
            "lists",
            json!([
                {"source": path(0), "role": "cage_objects", "declared_count": 2, "members": members},
                {"source": path(1), "role": "cage_catalog", "declared_count": 2, "members": members},
            ]),
        ),
        ("reference_runs", json!([[record(0)], [record(1)]])),
        (
            "position_evidence",
            json!([{"observation": 0, "ordinal": 0}, {"observation": 1, "ordinal": 1}]),
        ),
        (
            "order_evidence",
            json!([
                {"order": "source", "members": [0, 1]},
                {"order": "assumed", "members": [0, 1]},
            ]),
        ),
        (
            "closure_evidence",
            json!([
                {"members": [0, 1], "order": "source", "certainty": "source"},
                {"members": [0, 1], "order": "assumed", "certainty": "assumed"},
            ]),
        ),
        ("resource_evidence", json!([resource("a"), resource("b")])),
        (
            "resource_associations",
            json!([
                {"cage": "test:sweep:subd#0", "resource": resource("a")},
                {"cage": "test:sweep:subd#1", "resource": resource("b")},
            ]),
        ),
        (
            "resource_state_evidence",
            json!([
                {"resource": resource("c"), "availability": "active_undecoded"},
                {"resource": resource("d"), "availability": "unknown"},
            ]),
        ),
    ] {
        complete_inputs[list] = value;
    }
    let complete = document(
        "features",
        feature(&json!({
            "kind": "form",
            "value": {
                "cages": {"kind": "complete", "value": ["test:sweep:subd#0", "test:sweep:subd#1"]},
                "cage_inputs": {
                    "observations": [
                        {
                            "binding": {"kind": "decoded", "cage": "test:sweep:subd#0"},
                            "paths": [path(0), path(1)],
                        },
                        {"binding": {"kind": "decoded", "cage": "test:sweep:subd#1"}, "paths": []},
                    ],
                    "membership": {
                        "kind": "complete",
                        "members": [0, 1],
                        "order": "source",
                        "certainty": "source",
                    },
                },
                "inputs": complete_inputs,
            },
        })),
    );
    let mut partial_inputs = json!(cadmpeg_ir::features::form::FormInputs::default());
    partial_inputs["cardinality_evidence"] = json!([2, 3]);
    let partial = document(
        "features",
        feature(&json!({
            "kind": "form",
            "value": {
                "cages": {
                    "kind": "retained",
                    "value": {
                        "observations": [
                            {"binding": {"kind": "decoded", "cage": "test:sweep:subd#0"}, "paths": []},
                            {"binding": {"kind": "unresolved"}, "paths": []},
                        ],
                        "membership": {
                            "kind": "partial",
                            "members": [
                                {"observation": 0, "ordinal": 0},
                                {"observation": 1, "ordinal": 1},
                            ],
                            "order": "unknown",
                        },
                    },
                },
                "inputs": partial_inputs,
            },
        })),
    );
    (complete, partial)
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
