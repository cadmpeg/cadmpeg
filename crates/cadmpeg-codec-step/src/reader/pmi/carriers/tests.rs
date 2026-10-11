// SPDX-License-Identifier: Apache-2.0

use super::{CarrierGraph, CarrierSets, RecordCarriers, SetId};

#[test]
fn carrier_unions_preserve_identity_and_share_overlaps() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut sets = CarrierSets::new(ctx).expect("sets");
        let keys = [0, 1, 2, 63, 64, 255, 256, 1 << 63, u64::MAX];
        let mut a = SetId::default();
        let mut b = SetId::default();
        for (index, key) in keys.into_iter().enumerate() {
            let leaf = CarrierSets::leaf(key);
            if index % 2 == 0 {
                a = sets.union(a, leaf).expect("left set");
            }
            if index % 3 != 0 {
                b = sets.union(b, leaf).expect("right set");
            }
        }
        let all = sets.union(a, b).expect("overlapping union");
        assert_eq!(sets.count(all), 8);
        assert_eq!(sets.first(all), Some(0));
        let mut found = Vec::new();
        sets.visit(all, &mut |id| {
            found.push(id);
            Ok(())
        })
        .expect("enumerate");
        assert_eq!(found, [0, 1, 2, 64, 255, 256, 1 << 63, u64::MAX]);
        assert_eq!(sets.union(b, a).expect("commutative identity"), all);
        assert_eq!(sets.union(all, a).expect("subset union"), all);
        assert_eq!(sets.union(all, b).expect("subset union"), all);
    });
}

#[test]
fn carrier_component_summary_keeps_cycles_and_bounded_paths() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut graph = CarrierGraph::new(ctx).expect("graph");
        let mut inspected = 0;
        graph
            .ensure(1, |id, _sets, storage| {
                inspected += 1;
                let edges: &[(u64, usize)] = match id {
                    1 => &[(2, 1), (3, 1)],
                    2 => &[(4, 1)],
                    3 => &[(4, 1)],
                    4 => &[(2, 1), (5, 1)],
                    _ => &[],
                };
                let local = if id == 5 {
                    [CarrierSets::leaf(id), SetId::default(), SetId::default()]
                } else {
                    [SetId::default(); 3]
                };
                let mut children = Vec::new();
                for &edge in ctx.admit_iter(edges, "test edges")? {
                    ctx.push_scoped_vec(storage, &mut children, edge, "test children")?;
                }
                Ok(RecordCarriers { local, children })
            })
            .expect("component graph");
        assert_eq!(inspected, 5);
        for id in [1, 2, 3, 4, 5] {
            graph
                .ensure(id, |_, _, _| panic!("record must be inspected once"))
                .expect("shared graph");
            let summary = graph.summary(id, 256).expect("complete closure");
            assert_eq!(graph.sets.count(summary[0]), 1);
            assert_eq!(graph.sets.first(summary[0]), Some(5));
        }
        let truncated = graph.summary(1, 3).expect("bounded closure");
        assert_eq!(graph.sets.count(truncated[0]), 0);
        let complete = graph.summary(1, 4).expect("shortest path");
        assert_eq!(graph.sets.first(complete[0]), Some(5));
    });
}

#[test]
fn bounded_carrier_summary_refuses_depth_before_frame_allocation() {
    use cadmpeg_core::{
        decode::{DecodePolicy, ResourceDimension},
        CodecError,
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut graph = CarrierGraph::new(ctx).expect("graph");
        graph
            .ensure(1, |id, _, storage| {
                let mut children = Vec::new();
                if id < 260 {
                    ctx.push_scoped_vec(storage, &mut children, (id + 1, 1), "test chain edges")?;
                }
                Ok(RecordCarriers {
                    local: [SetId::Empty; 3],
                    children,
                })
            })
            .expect("iterative discovery has no nested call");
        assert!(
            matches!(graph.summary(1,256),Err(CodecError::ResourceLimit(refusal)) if refusal.dimension == ResourceDimension::RecursionDepth && refusal.operation == "STEP bounded carrier summary walk" && refusal.used == 0)
        );
    });
}
