// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn rejected_entity_path_releases_index_and_identity_storage() {
    let placement = placement();
    let sketch_id = neutral_sketch_id(&placement);
    let sketches = [Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let curves = [curve(30, 100, 101), curve(31, 200, 201)];
    let operands = [operand(10, 0, 100), operand(11, 1, 200)];
    let resolution = EntitySelectionPathResolution {
        operands: &operands,
        placements: std::slice::from_ref(&placement),
        curve_identities: &curves,
        sketches: &sketches,
        sketch_entities: &[],
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 1_048_576;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(resolve_entity_selection_path(&group(), &resolution, &ctx)
        .unwrap()
        .is_none());
    ctx.reserve_scoped(
        policy.limits.max_materialized_bytes,
        "released path storage",
    )
    .expect("all rejected path storage was released");
}

#[test]
fn missing_historical_face_relations_skip_deeper_indexes() {
    use crate::history_records::{
        AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation, AsmHistoricalTopology,
    };
    let mut topology = AsmHistoricalTopology {
        face_loops: vec![AsmHistoricalRelation { owner_ref: 10, member_refs: vec![11] }],
        ..AsmHistoricalTopology::default()
    };
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    for operation in [
        "f3d historical coedge index",
        "f3d historical face edge index",
        "f3d historical face vertex index",
        "f3d historical face point index",
    ] {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
                cadmpeg_core::decode::ResourceDimension::WorkUnits, operation, None,
            );
            let points = super::super::historical_face_points(10, &topology, ctx)?;
            assert!(points.is_none());
            assert_eq!(ctx.resource_refusal(), None);
            Ok::<_, cadmpeg_core::CodecError>(())
        })
        .unwrap();
        match operation {
            "f3d historical coedge index" => topology.loop_coedges.push(AsmHistoricalRelation {
                owner_ref: 11, member_refs: vec![12],
            }),
            "f3d historical face edge index" => topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge: 12, owner_loop: 11, edge: 20, next: 12, previous: 12, radial_next: 12,
            }),
            "f3d historical face vertex index" => topology.edge_vertices.push(AsmHistoricalEdge {
                edge: 20, start_vertex: 30, end_vertex: 31,
            }),
            _ => {},
        }
    }
}
