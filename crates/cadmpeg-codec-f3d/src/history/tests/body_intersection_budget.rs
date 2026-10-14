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
fn repeated_coedge_incidence_visits_each_edge_and_vertex_once() {
    use crate::history_records::{AsmHistoricalCoedge, AsmHistoricalRelation};
    use std::collections::BTreeSet;

    let mut topology = wire_topology(false);
    topology.shell_wire_edges[0].member_refs.clear();
    topology.shell_faces[0].member_refs.push(4);
    topology.face_loops.push(AsmHistoricalRelation {
        owner_ref: 4,
        member_refs: vec![5],
    });
    topology
        .face_surfaces
        .push(crate::history_records::AsmHistoricalCarrierBinding {
            entity: 4,
            carrier: 6,
        });
    topology.loop_coedges.push(AsmHistoricalRelation {
        owner_ref: 5,
        member_refs: (100..200).collect(),
    });
    for coedge in 100..200 {
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge,
            owner_loop: 5,
            edge: 7,
            next: if coedge == 199 { 100 } else { coedge + 1 },
            previous: if coedge == 100 { 199 } else { coedge - 1 },
            radial_next: coedge,
        });
    }
    let bodies = super::super::bodies_intersecting(&topology, &BTreeSet::from([29])).unwrap();
    assert_eq!(bodies, BTreeSet::from([1]));
}

#[test]
fn repeated_body_queries_reuse_incidence_and_validate_each_snapshot() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use std::collections::BTreeSet;
    let topology = simple_topology();
    let incomplete = crate::history_records::AsmHistoricalTopology {
        bodies: vec![1],
        ..Default::default()
    };
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut cache = super::super::HistoricalBodyClosureCache::default();
    for _ in 0..20 {
        assert_eq!(
            cache.intersecting(&topology, &BTreeSet::from([1])),
            Some(BTreeSet::from([1]))
        );
        assert_eq!(
            cache.intersecting(&topology, &BTreeSet::from([2])),
            Some(BTreeSet::new())
        );
        assert_eq!(cache.intersecting(&incomplete, &BTreeSet::from([1])), None);
    }
    ctx.finish_session().unwrap();
}
