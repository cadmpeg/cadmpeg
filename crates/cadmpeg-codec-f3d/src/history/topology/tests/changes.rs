// SPDX-License-Identifier: Apache-2.0

use crate::history::test_support::output_binding_inputs;

#[test]
fn changed_topology_members_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut delta = crate::history_records::AsmHistoricalTopologyDelta::default();
    delta.bodies.inserted.push(1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::history::topology::changed_family_refs(&ctx, &delta, false).unwrap_err();
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D changed topology members")
    );
}

#[test]
fn changed_topology_family_member_scans_refuse_work() {
    use crate::history_records::AsmHistoricalTopologyDelta;

    let mut delta = AsmHistoricalTopologyDelta::default();
    delta.bodies.inserted.push(1);
    delta.bodies.updated.push(2);
    delta.bodies.deleted.push(3);
    for (deleted, skip) in [(false, 0), (false, 1), (true, 0)] {
        let operation = "scan F3D changed topology family members";
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            skip,
            |ctx| crate::history::topology::changed_family_refs(ctx, &delta, deleted).map(|_| ()),
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
}

#[test]
fn affected_history_bodies_refuse_collection_limit() {
    let operation = "collect F3D affected history bodies";
    let (_, _, history, _) = output_binding_inputs();
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| {
            crate::history::topology::affected_body_refs(
                ctx,
                &history.states[0],
                Some(&history.states[1]),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}
