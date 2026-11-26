// SPDX-License-Identifier: Apache-2.0
//! A `Fold` input's edges in the fixture cube's current state.

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeSet;

use cadmpeg_core::json;
use serde_json::Value;

use super::super::fixture::{design_findings, document};
use crate::document::CadIr;
use crate::report::Severity;

/// A native pick of edge `edge` of the cube, told apart by `key`.
fn edge_pick(ir: &CadIr, key: usize, edge: usize) -> Value {
    json!({
        "target": {"kind": "native", "value": {}},
        "native": {"codec": "test", "value": {"kind": "text", "value": format!("edge{key}")}},
        "resolved": [{"kind": "edge", "value": ir.geometry.edges[edge].id.as_str()}]
    })
}

/// The fixture with its extrude a frame folded along four cube edges.
fn folded(edges: [usize; 4]) -> CadIr {
    let mut ir = document();
    let picks: Vec<Value> = edges
        .iter()
        .enumerate()
        .map(|(key, edge)| edge_pick(&ir, key, *edge))
        .collect();
    ir.design.features[0].definition.operation = json::from_value(json!({"kind": "datum", "value": {
        "element": {"kind": "frame", "value": {
            "inputs": [{
                "role": {"subject": "whole", "relation": {"kind": "fold"}},
                "reference": {"kind": "picks", "value": {"at": {"kind": "current"}, "picks": {"kind": "named", "value": picks}}}
            }],
            "after": [],
            "resolved_in": {}
        }},
        "native_options": {},
        "source_settings": {}
    }}))
    .unwrap();
    ir
}

/// The edges at one corner of the cube: the first two bound one face.
fn corner(ir: &CadIr) -> (usize, usize, usize) {
    let vertex = ir.geometry.edges[0].start().unwrap();
    let at: Vec<usize> = ir
        .geometry
        .edges
        .iter()
        .enumerate()
        .filter(|(_, edge)| edge.vertices().any(|end| end == vertex))
        .map(|(index, _)| index)
        .collect();
    (at[0], at[1], at[2])
}

/// An edge that touches no end of edge 0.
fn apart(ir: &CadIr) -> usize {
    let ends: BTreeSet<_> = ir.geometry.edges[0].vertices().collect();
    ir.geometry
        .edges
        .iter()
        .position(|edge| edge.vertices().all(|end| !ends.contains(end)))
        .unwrap()
}

fn fold_errors(ir: &CadIr) -> Vec<String> {
    design_findings(ir)
        .into_iter()
        .filter(|finding| finding.severity == Severity::Error && finding.message.contains("Fold"))
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn fold_edges_share_one_end_vertex_and_lie_in_one_plane() {
    let ir = document();
    let (first, second, third) = corner(&ir);
    assert!(fold_errors(&folded([first, second, first, second])).is_empty());
    let errors = fold_errors(&folded([first, second, third, first]));
    assert!(
        errors
            .iter()
            .any(|message| message.contains("do not lie in one plane")),
        "{errors:?}"
    );
    let errors = fold_errors(&folded([0, apart(&ir), 0, apart(&ir)]));
    assert!(
        errors
            .iter()
            .any(|message| message.contains("do not share one end vertex")),
        "{errors:?}"
    );
}
