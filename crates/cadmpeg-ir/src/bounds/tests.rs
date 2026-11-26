// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use super::{
    read_json, text_depth, Bound, Measure, Measured, Outside, Place, DIGEST_WALK_DEPTH,
    MAX_DOCUMENT_DEPTH,
};
use crate::design::dimension::ParameterRef;
use crate::design::expr::{Neutral, Node, UnaryOp};

/// `depth` arrays, each holding the next.
fn nest(depth: usize) -> Value {
    (0..depth).fold(Value::Null, |inner, _| Value::Array(vec![inner]))
}

#[test]
fn text_depth_counts_containers_outside_strings() {
    assert_eq!(text_depth("1"), 0);
    assert_eq!(text_depth("[]"), 1);
    assert_eq!(text_depth(r#"{"a":[1,{"b":{}}]}"#), 4);
    assert_eq!(text_depth(r#"["[[{", "\"[["]"#), 1);
}

#[test]
fn a_document_reads_at_the_bound_and_is_refused_past_it() {
    let read = |depth: usize| read_json::<Value>(&nest(depth).to_string());
    assert_eq!(read(MAX_DOCUMENT_DEPTH).unwrap(), nest(MAX_DOCUMENT_DEPTH));
    let refusal = read(MAX_DOCUMENT_DEPTH + 1).unwrap_err();
    assert!(
        refusal.to_string().contains("levels of a CADIR document"),
        "{refusal}"
    );
}

#[test]
fn the_digest_walk_nests_twice_the_document_bound() {
    assert_eq!(u64::try_from(2 * MAX_DOCUMENT_DEPTH), Ok(DIGEST_WALK_DEPTH));
}

fn measure(value: &impl Serialize) -> Measured {
    let mut measure = Measure::new(Vec::new());
    serde_json::to_writer(&mut measure, value).unwrap();
    measure.finish()
}

fn home(home: &str, arena: &str, index: usize) -> Place {
    Place::Home {
        home: home.to_owned(),
        arena: arena.to_owned(),
        index,
    }
}

/// A home record sits at the document's fourth level and a native record at
/// its fifth, so a record within the bound nests 125 or 124 levels.
#[test]
fn the_measure_places_each_record_past_the_document_bound() {
    let document = json!({
        "ir_version": "11",
        "geometry": {
            "edges": [{"id": "e", "x": nest(MAX_DOCUMENT_DEPTH - 3)}],
            "points": [
                {"id": "a", "x": nest(MAX_DOCUMENT_DEPTH - 4)},
                {"id": "b", "x": nest(MAX_DOCUMENT_DEPTH - 3)},
                {"id": "c", "x": nest(MAX_DOCUMENT_DEPTH - 3)}
            ]
        },
        "model": {"parameters": [{"id": "p", "x": nest(MAX_DOCUMENT_DEPTH - 4)}]},
        "native": {"test": {
            "deep": [{"id": "n", "x": nest(MAX_DOCUMENT_DEPTH - 5)}, {"id": "m", "x": nest(MAX_DOCUMENT_DEPTH - 4)}]
        }}
    });
    let found = measure(&document);
    let native = Place::Native {
        codec: "test".to_owned(),
        arena: "deep".to_owned(),
        index: 1,
    };
    assert_eq!(
        found.records.into_iter().collect::<Vec<_>>(),
        [
            (home("geometry", "edges", 0), Bound::Document),
            (home("geometry", "points", 1), Bound::Document),
            (home("geometry", "points", 2), Bound::Document),
            (native, Bound::Document),
        ]
    );
    assert!(found.outside.is_empty());
}

#[test]
fn the_measure_reports_depth_outside_every_record() {
    let found = measure(&json!({"source": nest(MAX_DOCUMENT_DEPTH)}));
    assert!(found.records.is_empty());
    assert_eq!(
        found.outside,
        BTreeMap::from([(
            Outside {
                field: "source".to_owned(),
                entry: None
            },
            Bound::Document
        )])
    );
}

#[derive(Serialize)]
struct Document {
    design: Design,
}

#[derive(Serialize)]
struct Design {
    parameters: Vec<Parameter>,
}

#[derive(Serialize)]
struct Parameter {
    id: &'static str,
    expression: Neutral<ParameterRef>,
}

/// `depth - 1` negations of `true`: a tree `depth` levels deep.
fn negations(depth: usize) -> Neutral<ParameterRef> {
    Neutral(
        (1..depth).fold(Node::Boolean(true), |operand, _| Node::Unary {
            op: UnaryOp::Not,
            operand: Box::new(operand),
        }),
    )
}

/// An expression writes as text, so the measure charges a tree past its
/// bound to the record that writes it.
#[test]
fn the_measure_places_each_record_holding_an_expression_past_its_bound() {
    let bound = super::MAX_EXPRESSION_DEPTH;
    let document = Document {
        design: Design {
            parameters: vec![
                Parameter {
                    id: "a",
                    expression: negations(bound),
                },
                Parameter {
                    id: "b",
                    expression: negations(bound + 1),
                },
                Parameter {
                    id: "c",
                    expression: negations(bound + 1),
                },
            ],
        },
    };
    let found = measure(&document);
    assert_eq!(
        found.records.into_iter().collect::<Vec<_>>(),
        [
            (home("design", "parameters", 1), Bound::Expression),
            (home("design", "parameters", 2), Bound::Expression),
        ]
    );
    assert!(found.outside.is_empty());
}

/// Unstating a field keeps the records of the arenas the CADIR wire omits,
/// so the document's census counts them.
#[test]
fn an_unstated_field_keeps_the_arenas_the_wire_omits() {
    let mut ir = crate::examples::unit_cube_with_history_body().unwrap();
    let held = ir.model.history_bodies.clone();
    assert!(!held.is_empty());
    let before = ir.census();
    let outside = BTreeMap::from([(
        Outside {
            field: "source".to_owned(),
            entry: None,
        },
        Bound::Document,
    )]);
    let unstated = super::unstate_outside(&mut ir, &outside).unwrap();
    assert_eq!(unstated.len(), 1);
    assert_eq!(unstated[0].field, "source");
    assert!(ir.source.is_none());
    assert_eq!(ir.model.history_bodies, held);
    assert_eq!(ir.census(), before);
}

/// Each field of one call takes its value in the empty document.
#[test]
fn every_unstated_field_takes_its_value_in_the_empty_document() {
    let mut ir = crate::examples::unit_cube_with_history_body().unwrap();
    ir.fragment = true;
    ir.tolerances = crate::units::Tolerances::new(0.5, 0.25).unwrap();
    let outside = BTreeMap::from(["fragment", "tolerances"].map(|field| {
        (
            Outside {
                field: field.to_owned(),
                entry: None,
            },
            Bound::Document,
        )
    }));
    let unstated = super::unstate_outside(&mut ir, &outside).unwrap();
    assert_eq!(
        unstated
            .iter()
            .map(|unstated| unstated.field.as_str())
            .collect::<Vec<_>>(),
        ["tolerances", "fragment"]
    );
    assert!(!ir.fragment);
    assert_eq!(ir.tolerances, crate::units::Tolerances::default());
}
