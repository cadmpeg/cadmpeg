// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, HashMap};

use super::super::{DisplayJtBaseNodeData, DisplayJtPath, JtKeyed, JtSceneGraph};
use cadmpeg_core::decode::{u64_from_index, ResourceDimension};
use cadmpeg_core::CodecError;

fn base(object_id: u32) -> DisplayJtBaseNodeData {
    DisplayJtBaseNodeData {
        id: format!("base-{object_id}"),
        element: "element".into(),
        object_type_id: [0; 16],
        object_id,
        version: 1,
        flags: 0,
        attribute_object_ids: Vec::new(),
        family_data_byte_len: 0,
        family_data_sha256: "00".repeat(32).try_into().unwrap(),
        source_offset: 0,
    }
}

fn chain_peak() -> u64 {
    // Each single-parent level moves the same four-slot path vector.
    // Both the ancestor stack and the 24-node output path have 32 u32 slots.
    // Node growth from 16 to 32 slots also admits the old 16-slot overlap.
    // Only the root contributes an instance: four String slots and its bytes.
    u64_from_index(4 * std::mem::size_of::<DisplayJtPath<'_>>()
        + 80 * std::mem::size_of::<u32>()
        + 4 * std::mem::size_of::<String>() + "root-instance".len())
}

#[test]
fn recursive_paths_release_transferred_slots_and_keep_instance_bytes_scoped() {
    let bases: Vec<_> = (1..=24).map(base).collect();
    for cap in [chain_peak() - 1, chain_peak()] {
        crate::test_support::with_decode_context_over(&[], |policy| {
            policy.limits.max_materialized_bytes = cap;
            policy.limits.max_retained_bytes = 0;
        }, |ctx| {
            // Prebuilt input indexes isolate the resolver's live storage.
            let graph = JtSceneGraph {
                by_object: bases.iter().map(|value| (value.object_id, value)).collect(),
                parents: (2..=24).map(|id| (id, vec![id - 1])).collect(),
                instance_ids: HashMap::from([(1, "root-instance")]),
                transforms: HashMap::new(),
                materials: HashMap::new(),
                _storage: ctx.reserve_scoped(0, "test graph").unwrap(),
            };
            let result = graph.node_paths(ctx, 24);
            if cap == chain_peak() {
                let paths = result.unwrap().unwrap();
                assert_eq!(paths.values.len(), 1);
                assert_eq!(paths.values[0].node_path, (1..=24).collect::<Vec<_>>());
                assert_eq!(paths.values[0].instance_path, ["root-instance"]);
                drop(paths);
                drop(graph);
                let full = ctx.reserve_scoped(cap, "released recursive paths").unwrap();
                drop(full);
                assert!(ctx.resource_refusal().is_none());
            } else {
                let Err(CodecError::ResourceLimit(limit)) = result else {
                    panic!("one byte below the node buffer growth peak must refuse");
                };
                assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!(limit.operation, "nx JT node path nodes");
                assert_eq!(limit.used + limit.additional, chain_peak());
                assert_eq!(ctx.resource_refusal(), Some(limit));
            }
        });
    }
}

#[test]
fn rejected_paths_release_instance_text_before_the_next_resolution() {
    let mut rejected = base(2);
    rejected.attribute_object_ids.push(9);
    let bases = [base(1), rejected];
    crate::test_support::with_decode_context_over(&[], |policy| {
        policy.limits.max_materialized_bytes = chain_peak();
        policy.limits.max_retained_bytes = 0;
    }, |ctx| {
        let graph = JtSceneGraph {
            by_object: BTreeMap::from([(1, &bases[0]), (2, &bases[1])]),
            parents: HashMap::from([(2, vec![1])]),
            instance_ids: HashMap::from([(1, "root-instance")]),
            transforms: HashMap::from([(9, JtKeyed::Repeated)]),
            materials: HashMap::new(),
            _storage: ctx.reserve_scoped(0, "test graph").unwrap(),
        };
        assert!(graph.node_paths(ctx, 2).unwrap().is_none());
        let full = ctx.reserve_scoped(chain_peak(),
            "released rejected paths").unwrap();
        drop(full);
        let accepted = graph.node_paths(ctx, 1).unwrap().unwrap();
        assert_eq!(accepted.values[0].instance_path, ["root-instance"]);
        drop(accepted);
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn scene_paths_preserve_parent_order_suppression_and_cycle_rejection() {
    let mut suppressed = base(3);
    suppressed.flags = 1;
    let bases = [base(1), base(2), suppressed, base(4)];
    crate::test_support::with_decode_context(|ctx| {
        let mut graph = JtSceneGraph {
            by_object: bases.iter().map(|value| (value.object_id, value)).collect(),
            parents: HashMap::from([(4, vec![2, 3, 1])]),
            instance_ids: HashMap::new(),
            transforms: HashMap::new(),
            materials: HashMap::new(),
            _storage: ctx.reserve_scoped(0, "test graph").unwrap(),
        };
        let paths = graph.node_paths(ctx, 4).unwrap().unwrap();
        assert_eq!(paths.values.len(), 2);
        assert_eq!(paths.values[0].node_path, [2, 4]);
        assert_eq!(paths.values[1].node_path, [1, 4]);
        drop(paths);
        graph.parents.insert(1, vec![4]);
        assert!(graph.node_paths(ctx, 4).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}
