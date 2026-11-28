// SPDX-License-Identifier: Apache-2.0
//! JSON Schema checks for `cadmpeg-asm` public types.

use std::collections::BTreeSet;

use cadmpeg_asm::brep::records::WireMembers;
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::ids::{EdgeId, VertexId};

/// `WireMembers` writes a map keyed by `edges` or `free_vertex`, and the
/// `JsonSchema` derive reads `#[serde(try_from = "WireMembersWire")]`, so
/// the published schema states those two properties rather than the
/// neutral enum's `Edges` and `Vertex` arms.
#[test]
fn the_wire_member_schema_states_the_keys_the_writer_states() {
    let schema = json::to_value(schemars::schema_for!(WireMembers)).expect("members schema");
    let properties: BTreeSet<String> = schema["properties"]
        .as_object()
        .expect("the members schema states properties")
        .keys()
        .cloned()
        .collect();
    let edge = EdgeId::mint("asm:test:edge#1").expect("edge id");
    let vertex = VertexId::mint("asm:test:vertex#2").expect("vertex id");
    let written: BTreeSet<String> = [WireMembers::Edges(vec![edge]), WireMembers::Vertex(vertex)]
        .iter()
        .flat_map(|members| {
            json::to_value(members)
                .expect("serialize members")
                .as_object()
                .expect("members write a map")
                .keys()
                .cloned()
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(properties, written);
}
