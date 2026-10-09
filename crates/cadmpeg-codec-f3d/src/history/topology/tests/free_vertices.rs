// SPDX-License-Identifier: Apache-2.0
//! Reached free-vertex closure visits and original refusals.

use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn topology(vertices: Vec<i64>) -> AsmHistoricalTopology {
    let relation = |owner_ref, member_refs| AsmHistoricalRelation { owner_ref, member_refs };
    AsmHistoricalTopology {
        bodies: vec![1],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, Vec::new())],
        shell_wire_edges: vec![relation(3, Vec::new())],
        shell_free_vertices: vec![relation(3, vertices)],
        ..Default::default()
    }
}

#[test]
fn missing_first_free_vertex_link_skips_the_unread_tail() {
    let short = topology(vec![99]);
    let long = topology(std::iter::once(99).chain(100..4196).collect());
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let used = |topology: &AsmHistoricalTopology| {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let mut visited = Vec::new();
            assert!(!crate::history::topology::walk_body_closures(ctx, topology, |body, entity| {
                visited.push((body, entity));
                Ok(())
            }).unwrap());
            assert_eq!(visited, [(1, 1), (1, 2), (1, 3), (1, 99)]);
            assert_eq!(ctx.resource_refusal(), None);
            let CodecError::ResourceLimit(limit) = ctx.charge_work(u64::MAX, "measure reached free vertices").unwrap_err() else { panic!("work measurement refusal"); };
            limit.used
        })
    };
    assert_eq!(used(&short), used(&long));
}

#[test]
fn free_vertex_source_admission_preserves_the_original_refusal() {
    let mut topology = topology(vec![99]);
    topology.vertex_points = vec![crate::history_records::AsmHistoricalCarrierBinding { entity: 99, carrier: 199 }];
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let mut visited = Vec::new();
        assert!(crate::history::topology::walk_body_closures(ctx, &topology, |body, entity| {
            visited.push((body, entity));
            Ok(())
        }).unwrap());
        assert_eq!(visited, [(1, 1), (1, 2), (1, 3), (1, 99), (1, 199)]);
        let _probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan F3D historical shell vertices", None);
        let CodecError::ResourceLimit(original) = crate::history::topology::walk_body_closures(ctx, &topology, |_, _| Ok(())).unwrap_err() else { panic!("free vertex refusal"); };
        assert_eq!(original.operation, "scan F3D historical shell vertices");
        let CodecError::ResourceLimit(sticky) = crate::history::topology::walk_body_closures(ctx, &AsmHistoricalTopology::default(), |_, _| Ok(())).unwrap_err() else { panic!("sticky free vertex refusal"); };
        assert_eq!(sticky, original);
    });
}
