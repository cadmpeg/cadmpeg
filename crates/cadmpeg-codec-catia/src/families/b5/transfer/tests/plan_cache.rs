// SPDX-License-Identifier: Apache-2.0

use super::closure::repeated_source_pcurve_graph;
use crate::families::b5::graph::{B5Graph, B5IncidenceLane};
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::UnknownId;

#[test]
fn repeated_pcurve_plan_avoids_duplicate_lane_storage() {
    let repeated = repeated_source_pcurve_graph();
    let mut distinct = repeated.clone();
    let pcurve = distinct.pcurves[&20].clone();
    for id in [21, 22] {
        let mut candidate = pcurve.clone();
        candidate.object_id = id;
        distinct.pcurves.insert(id, candidate);
    }
    for (member, id) in distinct.loops.get_mut(&2).expect("loop").members.iter_mut().zip([20, 21, 22]) {
        member.pcurve = id;
    }
    for incidence in distinct.parameter_incidences.values_mut() {
        let lane = &incidence.lanes[0];
        incidence.lanes = [20, 21, 22].map(|curve| B5IncidenceLane {
            curve,
            parameter: lane.parameter,
            control: lane.control,
        }).into();
    }
    let payload = UnknownId::mint("catia:test:unknown#plan-cache".to_owned())
        .expect("identity grammar");
    let minimum = |graph: &B5Graph| {
        let succeeds = |cap| {
            crate::test_support::with_collection_limit(cap, |ctx| {
                let mut storage = ctx.reserve_scoped(0, "test_plan_cache")?;
                let plan = super::super::build_plan(
                    ctx, graph, &payload, &mut crate::nurbs::LaneRefusals::new(), &mut storage,
                );
                if let Err(CodecError::ResourceLimit(refusal)) = &plan {
                    assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
                }
                plan.map(|plan| {
                    let plan = plan.expect("closed repeated-occurrence graph");
                    assert_eq!(plan.edge_support_plan.len(), 3);
                })
            })
        };
        let mut low = 0;
        let mut high = cadmpeg_core::decode::DecodePolicy::service().limits.max_collection_items;
        succeeds(high).expect("service collection ceiling admits fixture");
        while low < high {
            let middle = low + (high - low) / 2;
            match succeeds(middle) {
                Ok(()) => high = middle,
                Err(CodecError::ResourceLimit(refusal)) => {
                    assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
                    low = middle + 1;
                }
                Err(error) => panic!("unexpected planner error: {error}"),
            }
        }
        low
    };
    // Each extra identity expands four knots and copies two control points.
    // IR lane construction and map entries can require more storage, so the
    // lower bound uses only these two source-owned collections.
    assert!(minimum(&distinct) >= minimum(&repeated) + 2 * (4 + 2));
}

#[test]
fn cached_pcurve_plan_still_rejects_another_loop_surface() {
    let mut graph = repeated_source_pcurve_graph();
    graph.surfaces.insert(11, graph.surfaces[&10].clone());
    let mut loop_ = graph.loops[&2].clone();
    loop_.object_id = 3;
    loop_.surface = 11;
    graph.loops.insert(3, loop_);
    let mut face = graph.faces[0].clone();
    face.object_id = 4;
    face.surface = 11;
    face.loops = vec![3];
    graph.faces.push(face);
    let payload = UnknownId::mint("catia:test:unknown#cache-other-surface".to_owned())
        .expect("identity grammar");
    crate::test_support::with_service_context(|ctx| {
        let mut storage = ctx.reserve_scoped(0, "test_plan_cache")?;
        assert!(super::super::build_plan(
            ctx, &graph, &payload, &mut crate::nurbs::LaneRefusals::new(), &mut storage,
        )?.is_none());
        assert!(ctx.resource_refusal().is_none());
        Ok::<_, CodecError>(())
    }).expect("incompatible occurrence is a semantic refusal");
}
