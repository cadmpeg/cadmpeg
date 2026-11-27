// SPDX-License-Identifier: Apache-2.0
use super as topology;
use cadmpeg_core::json;
use serde_json::Value;

#[test]
fn tangent_propagation_follows_vertices_and_refuses_incomplete_chains() {
    let edge = |name: &str, vertices: Value, tangent: Value| {
        json!({"id":name,"vertices":vertices,
        "geometry":{"startVector":tangent,"endVector":tangent}})
    };
    let state = topology::State::testing(vec![
        edge("first", json!(["a", "b"]), topology::point([1.0, 0.0, 0.0])),
        edge("next", json!(["b", "c"]), topology::point([-1.0, 0.0, 0.0])),
        edge(
            "corner",
            json!(["c", "d"]),
            topology::point([0.0, 1.0, 0.0]),
        ),
    ]);
    let selected = json!({"kind":"historical","value":{"state":"input","edges":["first"]}});
    let expanded = state.tangent_chain(&selected).unwrap();
    assert_eq!(expanded["value"]["edges"], json!(["first", "next"]));
    let state = topology::State::testing(vec![edge("first", json!(["a", "b"]), Value::Null)]);
    assert!(state
        .tangent_chain(&selected)
        .unwrap_err()
        .contains("endpoint tangents"));
}
