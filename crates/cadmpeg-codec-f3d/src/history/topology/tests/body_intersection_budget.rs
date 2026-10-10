// SPDX-License-Identifier: Apache-2.0

fn simple_topology() -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![AsmHistoricalRelation {
            owner_ref: 1,
            member_refs: Vec::new(),
        }],
        ..Default::default()
    }
}

fn intersection_error(max_items: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeSet;
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = max_items;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::history::topology::bodies_intersecting(&ctx, &simple_topology(), &BTreeSet::from([1]))
        .unwrap_err()
}

#[test]
fn historical_relation_index_refuses_collection_limit() {
    let error = intersection_error(0);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical relations")
    );
}

#[test]
fn affected_topology_bodies_refuse_collection_limit() {
    let operation = "collect F3D affected topology bodies";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| {
            crate::history::topology::bodies_intersecting(
                ctx,
                &simple_topology(),
                &std::collections::BTreeSet::from([1]),
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn body_closure_owners_refuse_collection_limit() {
    let operation = "index F3D body closure owners";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        operation,
        0,
        |ctx| crate::history::topology::body_closures(ctx, &wire_topology(true)).map(|_| ()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn body_closure_queries_match_direct_intersection() {
    let topology = wire_topology(true);
    for changed in [
        std::collections::BTreeSet::from([9]),
        std::collections::BTreeSet::from([29]),
        std::collections::BTreeSet::from([99]),
    ] {
        let (direct, indexed) = crate::test_support::with_decode_context(|ctx| {
            let direct =
                crate::history::topology::bodies_intersecting(ctx, &topology, &changed).unwrap();
            let closures = crate::history::topology::body_closures(ctx, &topology).unwrap();
            let indexed =
                crate::history::topology::closures_intersecting(ctx, &closures, &changed).unwrap();
            (direct, indexed)
        });
        assert_eq!(direct, indexed);
    }
}

fn index_error(
    topology: &crate::history_records::AsmHistoricalTopology,
) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    crate::history::topology::bodies_intersecting(
        &ctx,
        topology,
        &std::collections::BTreeSet::new(),
    )
    .unwrap_err()
}

#[test]
fn historical_coedge_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.coedge_topology.push(AsmHistoricalCoedge {
        coedge: 1,
        owner_loop: 2,
        edge: 3,
        next: 1,
        previous: 1,
        radial_next: 1,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical coedges")
    );
}

#[test]
fn historical_edge_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalEdge, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.edge_vertices.push(AsmHistoricalEdge {
        edge: 1,
        start_vertex: 2,
        end_vertex: 3,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical edge vertices")
    );
}

#[test]
fn historical_carrier_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology.face_surfaces.push(AsmHistoricalCarrierBinding {
        entity: 1,
        carrier: 2,
    });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical carriers")
    );
}

#[test]
fn historical_optional_carrier_index_refuses_collection_limit() {
    use crate::history_records::{AsmHistoricalOptionalCarrierBinding, AsmHistoricalTopology};
    let mut topology = AsmHistoricalTopology::default();
    topology
        .edge_curves
        .push(AsmHistoricalOptionalCarrierBinding {
            entity: 1,
            carrier: None,
        });
    let error = index_error(&topology);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == "index F3D historical optional carriers")
    );
}

fn wire_topology(free_vertex: bool) -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalEdge, AsmHistoricalRelation,
        AsmHistoricalTopology,
    };
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, Vec::new())],
        shell_wire_edges: vec![relation(3, vec![7])],
        shell_free_vertices: vec![relation(3, if free_vertex { vec![8] } else { Vec::new() })],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 7,
            start_vertex: 8,
            end_vertex: 9,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 8,
                carrier: 28,
            },
            AsmHistoricalCarrierBinding {
                entity: 9,
                carrier: 29,
            },
        ],
        ..Default::default()
    }
}

#[test]
fn historical_body_closure_disjointness_refuses_work() {
    let topology = wire_topology(false);
    let operation = "check F3D changed body closure";
    let changed = std::collections::BTreeSet::from([99]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| crate::history::topology::bodies_intersecting(ctx, &topology, &changed).map(|_| ()),
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

fn complete_topology() -> crate::history_records::AsmHistoricalTopology {
    use crate::history_records::{
        AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation,
        AsmHistoricalTopology,
    };
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, vec![4])],
        shell_wire_edges: vec![relation(3, vec![7])],
        shell_free_vertices: vec![relation(3, Vec::new())],
        face_loops: vec![relation(4, vec![5])],
        loop_coedges: vec![relation(5, vec![6])],
        coedge_topology: vec![AsmHistoricalCoedge {
            coedge: 6,
            owner_loop: 5,
            edge: 7,
            next: 6,
            previous: 6,
            radial_next: 6,
        }],
        edge_vertices: vec![AsmHistoricalEdge {
            edge: 7,
            start_vertex: 8,
            end_vertex: 9,
        }],
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 10,
        }],
        vertex_points: vec![
            AsmHistoricalCarrierBinding {
                entity: 8,
                carrier: 11,
            },
            AsmHistoricalCarrierBinding {
                entity: 9,
                carrier: 12,
            },
        ],
        ..Default::default()
    }
}

fn body_hierarchy_scan_error(operation: &'static str) -> cadmpeg_core::CodecError {
    let topology = complete_topology();
    let changed = std::collections::BTreeSet::from([99]);
    crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| crate::history::topology::bodies_intersecting(ctx, &topology, &changed).map(|_| ()),
    )
}

#[test]
fn historical_body_region_scan_refuses_work() {
    let operation = "scan F3D body regions";
    let error = body_hierarchy_scan_error(operation);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn historical_region_shell_scan_refuses_work() {
    let operation = "scan F3D region shells";
    let error = body_hierarchy_scan_error(operation);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn historical_shell_face_scan_refuses_work() {
    let operation = "scan F3D shell faces";
    let error = body_hierarchy_scan_error(operation);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn historical_face_loop_scan_refuses_work() {
    let operation = "scan F3D face loops";
    let error = body_hierarchy_scan_error(operation);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn historical_loop_coedge_scan_refuses_work() {
    let operation = "scan F3D loop coedges";
    let error = body_hierarchy_scan_error(operation);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn historical_shell_edge_scan_refuses_work() {
    let operation = "scan F3D shell edges";
    let error = body_hierarchy_scan_error(operation);
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn historical_shell_vertex_scan_refuses_work() {
    let operation = "scan F3D historical shell vertices";
    let topology = wire_topology(true);
    let changed = std::collections::BTreeSet::from([99]);
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| crate::history::topology::bodies_intersecting(ctx, &topology, &changed).map(|_| ()),
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.operation == operation)
    );
}

#[test]
fn historical_treatment_carrier_face_scan_refuses_work() {
    use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalTopology};
    let result = AsmHistoricalTopology::default();
    let preceding = AsmHistoricalTopology {
        face_surfaces: vec![AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 10,
        }],
        ..Default::default()
    };
    let operation = "scan F3D carrier face candidates";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        0,
        |ctx| {
            crate::history::topology::treatment_edge_candidates::<true>(
                ctx,
                None,
                &[],
                &result,
                &preceding,
                &[],
            )
            .map(|_| ())
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}

#[test]
fn incomplete_body_closure_releases_owner_prefix_storage() {
    use cadmpeg_core::decode::ResourceDimension;
    let topology = crate::history_records::AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    let operation = "measure F3D incomplete closure storage";
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            let closures = crate::history::topology::body_closures(ctx, &topology)?;
            assert!(crate::history::topology::closures_intersecting(
                ctx,
                &closures,
                &std::collections::BTreeSet::from([1]),
            )?
            .is_none());
            let result = ctx.reserve_scoped(u64::MAX, operation).map(|_| ());
            drop(closures);
            result
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::MaterializedBytes && limit.used == 0
    ));
}

#[test]
fn missing_free_vertex_point_skips_unvisited_suffix() {
    fn work(free_vertices: Vec<i64>) -> u64 {
        let mut topology = wire_topology(false);
        topology.shell_wire_edges[0].member_refs.clear();
        topology.shell_free_vertices[0].member_refs = free_vertices;
        topology.vertex_points.clear();
        crate::test_support::with_decode_context(|ctx| {
            assert!(
                !crate::history::topology::walk_body_closures(ctx, &topology, |_, _| Ok(()),)
                    .unwrap()
            );
            let cadmpeg_core::CodecError::ResourceLimit(limit) = ctx
                .charge_work(u64::MAX, "measure incomplete body walk")
                .unwrap_err()
            else {
                panic!("work refusal");
            };
            limit.used
        })
    }
    assert_eq!(work(vec![8]), work(vec![8; 4096]));
}
