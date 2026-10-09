// SPDX-License-Identifier: Apache-2.0
//! Reach-node storage survives the independently owned graph result.

use std::mem::{align_of, size_of};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::CadIr;

use super::super::{AnnotationDiscoveryIndex, AnnotationReach};

fn reach_remains_charged_after_graph_discard(replace: bool) {
    let exchange = super::exchange("#1=ITEM();");
    let arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("setup context");
    let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup)
        .expect("geometry");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut index = AnnotationDiscoveryIndex::new(ctx).expect("index");
        let result = super::super::independent_annotation_graph(
            1, 0, &exchange, &geometry.value, &mut index,
        ).expect("independent graph");
        drop(result);
        if replace {
            let replacement = super::super::independent_annotation_graph(
                1, 0, &exchange, &geometry.value, &mut index,
            ).expect("replacement graph");
            drop(replacement);
        }
        assert!(index.graphs.is_empty());
        assert_eq!(index.independent_reach.len(), 1);
        assert_eq!(index.independent_reach[&(1, 0)].nodes, std::collections::BTreeSet::from([1]));

        // Each one-entry tree retains one node. The core node bound admits
        // eleven key/value slots, sixteen pointer words and two alignments.
        let reach_alignment = align_of::<(u64, usize)>()
            .max(align_of::<AnnotationReach<'_>>())
            .max(align_of::<usize>());
        let map_node = 11 * (size_of::<(u64, usize)>() + size_of::<AnnotationReach<'_>>())
            + 16 * size_of::<usize>() + 2 * reach_alignment;
        let set_alignment = align_of::<u64>().max(align_of::<usize>());
        let set_node = 11 * size_of::<u64>() + 16 * size_of::<usize>() + 2 * set_alignment;
        let CodecError::ResourceLimit(refusal) = ctx.reserve_scoped(
            u64::MAX, "test reach lifetime observation",
        ).expect_err("observation exceeds materialized limit") else {
            panic!("materialized refusal");
        };
        assert_eq!(refusal.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(refusal.operation, "test reach lifetime observation");
        assert_eq!(refusal.used, u64::try_from(map_node + set_node).expect("two node bounds"));
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

#[test]
fn independent_annotation_reach_survives_discarded_graph() {
    reach_remains_charged_after_graph_discard(false);
}

#[test]
fn independent_annotation_reach_replacement_releases_only_old_nodes() {
    reach_remains_charged_after_graph_discard(true);
}

#[test]
fn independent_annotation_reach_survives_graph_entry_refusal() {
    let exchange = super::exchange("#1=ITEM();");
    let arena = DecodeArena::new();
    let (setup, _) = DecodeContext::from_root_bytes(b"", &arena, &DecodePolicy::service())
        .expect("setup context");
    let geometry = crate::reader::geometry::decode(&exchange, &mut CadIr::empty(), &setup)
        .expect("geometry");
    let mut policy = DecodePolicy::service();
    // One initial worklist slot, one reach node, one cyclic-query entry and
    // one reach-map entry precede the fifth, graph-map entry.
    policy.limits.max_collection_items = 4;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut index = AnnotationDiscoveryIndex::new(ctx).expect("index");
        let result = super::super::independent_annotation_graph(
            1, 0, &exchange, &geometry.value, &mut index,
        ).expect("independent graph");
        let CodecError::ResourceLimit(refusal) = index.storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut index.graphs, (1, 0), result, "step_annotation_graph_entries",
            )
        }).err().expect("graph entry exceeds collection limit") else {
            panic!("collection refusal");
        };
        assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
        assert_eq!(refusal.operation, "step_annotation_graph_entries");
        assert_eq!(refusal.used, 4);
        assert_eq!(refusal.additional, 1);
        assert!(index.graphs.is_empty());
        assert_eq!(index.independent_reach.len(), 1);
        assert_eq!(index.independent_reach[&(1, 0)].nodes, std::collections::BTreeSet::from([1]));
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}
