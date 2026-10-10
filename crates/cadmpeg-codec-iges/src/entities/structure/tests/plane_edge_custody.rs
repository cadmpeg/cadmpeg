// SPDX-License-Identifier: Apache-2.0

use super::super::{plane_face_draft, LegacyPlaneError};
use crate::entities::geometry::SourceSequences;
use crate::ids::{Stem, Word};
use cadmpeg_core::decode::{
    u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::ids::{EdgeId, VertexId};
use cadmpeg_ir::topology::{Coedge, Edge, EdgeCarrier, Loop};

fn edges(count: usize) -> Vec<Edge> {
    (0..count)
        .map(|index| Edge {
            id: EdgeId::mint(format!("iges:model:edge#D{}", 2 * index + 1)).unwrap(),
            carrier: EdgeCarrier::unbounded(None),
            start: VertexId::mint("iges:model:vertex#D1").unwrap(),
            end: VertexId::mint("iges:model:vertex#D1").unwrap(),
            tolerance: None,
        })
        .collect()
}

fn source_node_bytes() -> usize {
    // Body and face keys are String-backed identity keys, with u32 values.
    11 * (std::mem::size_of::<EdgeId>() + std::mem::size_of::<u32>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<EdgeId>()
}

#[test]
fn consumed_plane_edges_release_source_slots_while_the_draft_remains_live() {
    for count in [1, 64] {
        let fixture = edges(count);
        let expected = fixture.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Raw slots, all three retained arena growth overlaps, three source
        // index nodes and bounded identity text dominate every live phase.
        let cap = u64_from_index(
            count
                * (std::mem::size_of::<Edge>()
                    + std::mem::size_of::<Coedge>()
                    + std::mem::size_of::<Loop>())
                + 3 * source_node_bytes()
                + 1024,
        );
        policy.limits.max_materialized_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let (mut source, storage) = ctx
            .temporary_vec(count, "test plane boundary raw slots")
            .unwrap();
        source.extend(fixture);
        let draft = plane_face_draft(
            1,
            1,
            &Stem::word_directory(Word::BoundedPlane, 1_u32),
            (source, storage),
            0.0,
            &mut sequences,
            &ctx,
        )
        .unwrap_or_else(|error| {
            panic!("expected plane draft: {:?}", error.non_resource());
        });
        assert_eq!(draft.model().edges.len(), count);
        assert_eq!(draft.model().coedges.len(), count);
        assert_eq!(draft.model().loops.len(), count);
        assert_eq!(draft.model().faces.len(), 1);
        for edge in &expected {
            assert!(draft
                .model()
                .edges
                .iter()
                .any(|candidate| candidate == edge));
        }
        drop(sequences);
        let released = ctx
            .reserve_scoped(cap, "test consumed plane raw storage")
            .unwrap();
        assert_eq!(draft.model().bodies.len(), 1);
        assert_eq!(draft.model().regions.len(), 1);
        assert_eq!(draft.model().shells.len(), 1);
        drop(released);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn plane_raw_slots_refuse_before_allocating_one_or_many_edges() {
    for count in [1, 64] {
        let bytes = u64_from_index(count * std::mem::size_of::<Edge>());
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = bytes - 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(first)) =
            ctx.temporary_vec::<Edge>(count, "test plane boundary raw slots")
        else {
            panic!("expected raw allocation refusal");
        };
        assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(first.operation, "test plane boundary raw slots");
        assert_eq!(
            (first.used, first.additional, first.limit),
            (0, bytes, bytes - 1)
        );
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
        );
    }
}

#[test]
fn plane_draft_first_or_last_edge_entity_refusal_remains_sticky() {
    // Each accepted boundary adds an edge, coedge and loop before the next edge.
    for count in [1, 64] {
        let fixture = edges(count);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let entities = u64_from_index(3 * (count - 1));
        policy.limits.max_entities = entities;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut sequences = SourceSequences::new(&ctx).unwrap();
        let (mut source, storage) = ctx
            .temporary_vec(count, "test plane boundary raw slots")
            .unwrap();
        source.extend(fixture);
        let replay_storage = ctx.reserve_scoped(0, "test empty plane replay").unwrap();
        let stem = Stem::word_directory(Word::BoundedPlane, 1_u32);
        let result = plane_face_draft(1, 1, &stem, (source, storage), 0.0, &mut sequences, &ctx);
        let Err(LegacyPlaneError::Resource(CodecError::ResourceLimit(first))) = result else {
            panic!("expected plane edge entity refusal");
        };
        assert_eq!(first.dimension, ResourceDimension::Entities);
        assert_eq!(first.operation, "iges_geometry_structure");
        assert_eq!(
            (first.used, first.additional, first.limit),
            (entities, 1, entities)
        );
        assert!(
            matches!(plane_face_draft(1, 1, &stem, (Vec::new(), replay_storage),
            f64::INFINITY, &mut sequences, &ctx),
            Err(LegacyPlaneError::Resource(CodecError::ResourceLimit(last))) if last == first)
        );
        drop(sequences);
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)
        );
    }
}

#[test]
fn invalid_plane_tolerance_releases_all_unvisited_edge_slots_without_work() {
    let fixture = edges(64);
    let bytes = u64_from_index(fixture.len() * std::mem::size_of::<Edge>());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequences = SourceSequences::new(&ctx).unwrap();
    let (mut source, storage) = ctx
        .temporary_vec(fixture.len(), "test plane boundary raw slots")
        .unwrap();
    source.extend(fixture);
    assert!(matches!(
        plane_face_draft(
            1,
            1,
            &Stem::directory(1_u32),
            (source, storage),
            f64::INFINITY,
            &mut sequences,
            &ctx
        ),
        Err(LegacyPlaneError::Invalid("face tolerance must be finite"))
    ));
    let released = ctx
        .reserve_scoped(bytes, "test invalid plane raw slots destroyed")
        .unwrap();
    drop(released);
    drop(sequences);
    assert!(ctx.resource_refusal().is_none());
    ctx.finish_session().unwrap();
}
