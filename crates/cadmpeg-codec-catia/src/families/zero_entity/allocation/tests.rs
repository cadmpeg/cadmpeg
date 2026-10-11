// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::test_zero_entity::zero_entity_allocation_stream;

fn topology(bytes: &[u8]) -> Option<AllocationTopology> {
    crate::test_support::with_service_context(|ctx| {
        let runs = records::zero_entity_support_runs_in_range(
            ctx,
            bytes,
            0..bytes.len(),
            &mut crate::nurbs::LaneRefusals::new(),
        )?;
        parse(ctx, bytes, 0..bytes.len(), &runs)
    })
    .expect("bounded allocation fixture")
}

#[test]
fn allocation_chain_selects_source_edges_uses_supports_and_vertex_owners() {
    let solved = topology(&zero_entity_allocation_stream()).expect("complete source graph");
    assert_eq!(
        solved
            .vertices
            .iter()
            .map(|v| v.record_ordinal)
            .collect::<Vec<_>>(),
        [19, 21, 23]
    );
    assert_eq!(
        solved
            .edges
            .iter()
            .map(|e| e.record_ordinal)
            .collect::<Vec<_>>(),
        [24, 25, 26]
    );
    let edge = &solved.edges[0];
    assert_eq!(edge.header_record_ordinal, 9);
    assert_eq!(edge.uses.map(|u| u.use_record_ordinal), [10, 11]);
    assert_eq!(edge.uses.map(|u| u.support_record_ordinal), [2, 8]);
    assert_eq!(edge.uses.map(|u| u.incidence_record_ordinal), [18, 20]);
    assert_eq!(edge.uses.map(|u| u.vertex_record_ordinal), [19, 21]);
    assert_eq!(edge.uses.map(|u| u.face_record_ordinal), [29, 30]);
}

#[test]
fn incomplete_or_duplicated_allocation_chains_leave_source_graph_unassigned() {
    let source = zero_entity_allocation_stream();
    let inventory = crate::test_support::with_service_context(|ctx| {
        records::zero_entity_record_inventory(ctx, &source)
    })
    .expect("fixture record inventory");
    for (ordinal, offset, value) in [(24, 18, 4_u32), (18, 19, 8), (27, 19, 1)] {
        let mut bytes = source.clone();
        let at = inventory[ordinal - 1].pos + offset;
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        assert!(
            topology(&bytes).is_none(),
            "contradiction in record {ordinal}"
        );
    }
}

#[test]
fn allocation_graph_obeys_collection_and_work_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodePolicy, ResourceDimension};
    let bytes = zero_entity_allocation_stream();
    let runs = crate::test_support::with_service_context(|ctx| {
        records::zero_entity_support_runs_in_range(
            ctx,
            &bytes,
            0..bytes.len(),
            &mut crate::nurbs::LaneRefusals::new(),
        )
    })
    .expect("fixture support runs");
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::WorkUnits,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!(),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("input fits");
        let error =
            parse(&ctx, &bytes, 0..bytes.len(), &runs).expect_err("explicit budget refused");
        assert!(matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == dimension));
    }
}
