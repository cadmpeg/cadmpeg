// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::json;

use super::*;
use crate::test_support::with_decode_context;

fn mark(stream: u32, offset: u64, identifier: u32, forward: bool) -> serde_json::Value {
    json!({
        "id": format!("nx:s{stream}:deltas-metadata#{offset}"),
        "stream_ordinal": stream, "inflated_offset": offset, "byte_len": 33,
        "sha256": "0".repeat(64), "production": "pmark",
        "state": {
            "xmt": 2, "preceding": 1, "first_following": 1,
            "next_sibling": 1, "prev_sibling": 1,
            "n_new_nodes": 0, "n_del_nodes": 0, "n_copy_mod_nodes": 0,
            "delta_key": 0, "delta_is_forward": forward, "identifier": identifier,
        },
    })
}

fn charged(rows: &[serde_json::Value]) -> Vec<LossNote> {
    with_decode_context(|ctx| {
        let mut ir = CadIr::empty();
        ir.native
            .namespace_mut(cadmpeg_ir::codec_id!("nx"))
            .set_arena(ctx, "parasolid_deltas_metadata", rows)
            .unwrap();
        let mut losses = Vec::new();
        charge(ctx, &ir, &mut losses).unwrap();
        losses
    })
}

/// Each Deltas mark is one stored state, charged by its metadata record in
/// stream order; a map record is no state.
#[test]
fn each_deltas_mark_is_a_stored_state_charged_by_name() {
    let map = json!({"id":"nx:s0:deltas-metadata#100", "stream_ordinal":0, "inflated_offset":100,
        "byte_len":10, "sha256":"0".repeat(64), "production":"map", "entries":[[7,19]],
        "terminal_bytes":[0, 0, 0, 21]});
    let losses = charged(&[mark(1, 0, 9, true), map, mark(0, 200, 4, false)]);
    let [loss] = losses.as_slice() else {
        panic!("one loss: {losses:?}");
    };
    assert_eq!(loss.code, NxLossCode::StoredStateUnread.kind());
    assert!(
        loss.message.ends_with(
            "nx:s0:deltas-metadata#200 (mark 4, backward), \
             nx:s1:deltas-metadata#0 (mark 9, forward)"
        ),
        "{}",
        loss.message
    );
}

/// A part with no Deltas mark charges no stored state.
#[test]
fn a_part_with_no_mark_charges_nothing() {
    assert!(charged(&[]).is_empty());
}
