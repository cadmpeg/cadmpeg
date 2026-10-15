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

#[test]
fn carrier_height_overflow_preserves_session_refusal() {
    for (cyclic, operation) in [
        (true, "STEP carrier component span overflow"),
        (false, "STEP carrier graph height overflow"),
    ] {
        with_session(&cadmpeg_core::decode::DecodePolicy::service(), |ctx| {
            let mut graph = CarrierGraph::new(ctx).expect("graph");
            let error = graph
                .ensure(1, |id, _, storage| {
                    let mut children = Vec::new();
                    let edge = match id {
                        1 => Some((2, usize::MAX)),
                        2 => Some((if cyclic { 1 } else { 3 }, 1)),
                        _ => None,
                    };
                    if let Some(edge) = edge {
                        ctx.push_scoped_vec(storage, &mut children, edge, "test height edges")?;
                    }
                    Ok(RecordCarriers {
                        local: [SetId::Empty; 3],
                        children,
                    })
                })
                .expect_err("height does not fit");
            let cadmpeg_core::CodecError::ResourceLimit(refusal) = error else {
                panic!("height resource refusal");
            };
            assert_eq!(refusal.operation, operation);
            assert_eq!(ctx.resource_refusal(), Some(refusal));
        });
    }
}

#[test]
fn bounded_carrier_edge_beyond_remaining_depth_is_empty() {
    with_session(&cadmpeg_core::decode::DecodePolicy::service(), |ctx| {
        let mut graph = CarrierGraph::new(ctx).expect("graph");
        graph
            .ensure(1, |id, _, storage| {
                let mut children = Vec::new();
                if id == 1 {
                    ctx.push_scoped_vec(storage, &mut children, (2, 5), "test weighted edge")?;
                }
                Ok(RecordCarriers {
                    local: if id == 2 {
                        [SetId::Leaf(2); 3]
                    } else {
                        [SetId::Empty; 3]
                    },
                    children,
                })
            })
            .expect("weighted graph");
        assert_eq!(
            graph.summary(1, 4).expect("edge exceeds depth"),
            [SetId::Empty; 3]
        );
        assert_eq!(
            graph.summary(1, 5).expect("edge reaches cutoff"),
            [SetId::Empty; 3]
        );
        assert_eq!(
            graph.summary(1, 6).expect("edge fits depth"),
            [SetId::Leaf(2); 3]
        );
    });
}

fn with_session<T>(
    policy: &cadmpeg_core::decode::DecodePolicy,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    use cadmpeg_core::{
        decode::{DecodeArena, DecodeContext},
        CodecError,
    };
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, policy).expect("root");
    let result = run(&ctx);
    if let Some(refusal) = ctx.resource_refusal() {
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal)
        );
    } else {
        ctx.finish_session().expect("completed session");
    }
    result
}

use cadmpeg_core::decode::DecodePolicy;

fn diamond_graph(
    graph: &mut CarrierGraph<'_, '_>,
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    graph.ensure(1, |id, _, storage| {
        let mut children = Vec::new();
        if id < 25 {
            let edges = if (id - 1) % 3 == 0 {
                [Some(id + 1), Some(id + 2)]
            } else {
                [Some(id + 3 - (id - 1) % 3), None]
            };
            for child in edges.into_iter().flatten() {
                ctx.push_scoped_vec(storage, &mut children, (child, 1), "test diamond edges")?;
            }
        }
        Ok(RecordCarriers {
            local: [SetId::Leaf(id); 3],
            children,
        })
    })
}

#[test]
fn carrier_diamond_summaries_share_nodes_and_preserve_exact_counts() {
    with_session(&DecodePolicy::service(), |ctx| {
        let mut graph = CarrierGraph::new(ctx).expect("graph");
        diamond_graph(&mut graph, ctx).expect("diamond graph");
        assert_eq!(graph.nodes.len(), 25);
        let all = graph.summary(1, 256).expect("complete closure");
        assert_eq!(graph.sets.count(all[0]), 25);
        assert_eq!(graph.sets.first(all[0]), Some(1));
        let bounded = graph.summary(1, 2).expect("bounded closure");
        assert_eq!(graph.sets.count(bounded[0]), 3);
        for id in 1..=25 {
            graph
                .ensure(id, |_, _, _| {
                    panic!("shared node must not be inspected twice")
                })
                .expect("cached discovery");
        }
    });
}

fn prepared_diamond<'ctx, 'arena>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'arena>,
) -> Result<CarrierGraph<'ctx, 'arena>, cadmpeg_core::CodecError> {
    let (nodes, set_nodes, unions) = with_session(&DecodePolicy::service(), |setup| {
        let mut graph = CarrierGraph::new(setup).expect("setup graph");
        diamond_graph(&mut graph, setup).expect("diamond setup");
        (graph.nodes, graph.sets.nodes, graph.sets.unions)
    });
    let mut graph = CarrierGraph::new(ctx)?;
    graph.nodes = nodes;
    graph.sets.nodes = set_nodes;
    graph.sets.unions = unions;
    Ok(graph)
}

fn diamond_refusal(operation: &str) {
    use cadmpeg_core::{
        decode::{DecodePolicy, ResourceDimension},
        CodecError,
    };
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            with_session(&policy, |ctx| {
                let result = (|| {
                    let mut graph = if matches!(
                        operation,
                        "STEP bounded carrier edge traversal"
                            | "STEP bounded carrier frame traversal"
                            | "STEP carrier set traversal"
                    ) {
                        prepared_diamond(ctx)?
                    } else {
                        let mut graph = CarrierGraph::new(ctx)?;
                        diamond_graph(&mut graph, ctx)?;
                        graph
                    };
                    graph.summary(1, 2)?;
                    let all = graph.summary(1, 256)?;
                    graph.sets.visit(all[0], &mut |_| Ok(()))
                })();
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(refusal.operation, operation);
                    assert!(refusal.used < 50_000);
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            })
        },
    );
}

#[test]
fn carrier_diamond_refuses_graph_walk_and_keeps_refusal() {
    diamond_refusal("STEP carrier graph frame traversal");
}

#[test]
fn carrier_diamond_refuses_bounded_walk_and_keeps_refusal() {
    diamond_refusal("STEP bounded carrier edge traversal");
}

#[test]
fn carrier_diamond_refuses_union_walk_and_keeps_refusal() {
    diamond_refusal("STEP carrier union branch");
}

#[test]
fn carrier_diamond_refuses_set_walk_and_keeps_refusal() {
    diamond_refusal("STEP carrier set traversal");
}

#[test]
fn carrier_diamond_refuses_completion_walk_and_keeps_refusal() {
    diamond_refusal("STEP carrier component completion");
}

#[test]
fn carrier_diamond_refuses_summary_walk_and_keeps_refusal() {
    diamond_refusal("STEP carrier component summary traversal");
}

#[test]
fn carrier_diamond_refuses_edge_walk_and_keeps_refusal() {
    diamond_refusal("STEP carrier component edge traversal");
}

#[test]
fn carrier_diamond_refuses_installation_walk_and_keeps_refusal() {
    diamond_refusal("STEP carrier component summary installation");
}

#[test]
fn carrier_diamond_refuses_bounded_frame_walk_and_keeps_refusal() {
    diamond_refusal("STEP bounded carrier frame traversal");
}
