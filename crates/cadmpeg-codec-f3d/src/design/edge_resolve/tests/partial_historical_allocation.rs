// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::CodecError;

fn assert_partial_refusal(operation: &'static str, retained: bool) {
    let state = cadmpeg_ir::ids::FeatureInputTopologyId::mint("f3d:history-input:state#feature").unwrap();
    for limit in 0..96 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        if retained { policy.limits.max_retained_bytes = limit; }
        else { policy.limits.max_collection_items = limit; }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match partial_historical_edge_selection(
            [("operand-a", Some(17)), ("operand-b", None)], 41,
            cadmpeg_ir::identity_key!("feature").as_str(), state.clone(), "group", Some(&ctx),
        ) {
            Err(CodecError::ResourceLimit(failure)) if failure.operation == operation => return,
            Err(CodecError::ResourceLimit(_)) => {},
            other => panic!("expected partial selection refusal at {operation}: {other:?}"),
        }
    }
    panic!("no partial selection refusal at {operation}");
}

#[test]
fn partial_edge_slot_refuses_collection_limit() {
    assert_partial_refusal("f3d partial edge slot", false);
}

#[test]
fn partial_unresolved_id_refuses_retained_limit() {
    assert_partial_refusal("f3d partial unresolved id", true);
}

#[test]
fn partial_unresolved_member_refuses_collection_limit() {
    assert_partial_refusal("f3d partial unresolved member", false);
}

#[test]
fn partial_historical_edge_refuses_collection_limit() {
    assert_partial_refusal("f3d partial historical edge", false);
}

#[test]
fn partial_native_id_refuses_retained_limit() {
    assert_partial_refusal("f3d partial native id", true);
}
