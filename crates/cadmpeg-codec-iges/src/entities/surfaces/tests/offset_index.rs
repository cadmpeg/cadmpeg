// SPDX-License-Identifier: Apache-2.0

use super::super::OffsetLookups;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    ProceduralSurface, ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{ProceduralSurfaceId, SurfaceId};
use cadmpeg_ir::CadIr;

const SURFACE_ID: &str = "test:model:surface#one";
const PROCEDURAL_ID: &str = "test:model:construction#one";

fn duplicate_surfaces(count: usize) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.surfaces = (0..count).map(|_| Surface {
        id: SurfaceId::mint(SURFACE_ID).unwrap(),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    }).collect();
    ir
}

fn unowned_procedural_surfaces(count: usize) -> CadIr {
    let mut ir = CadIr::empty();
    ir.model.procedural_surfaces = (0..count).map(|_| ProceduralSurface::new(
        ProceduralSurfaceId::mint(PROCEDURAL_ID).unwrap(),
        ProceduralSurfaceDefinition::Unknown { record: None, cache: None },
        None,
    )).collect();
    ir
}

fn surface_node_bytes() -> u64 {
    // The shared B-tree bound has eleven key/value lanes, sixteen pointer
    // lanes and twice the greatest key/value/pointer alignment.
    u64::try_from(11 * (std::mem::size_of::<SurfaceId>()
        + std::mem::size_of::<(usize, usize)>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<SurfaceId>()
            .max(std::mem::align_of::<(usize, usize)>())
            .max(std::mem::align_of::<usize>())).unwrap()
}

fn completed_duplicate_prefix(count: usize) -> u64 {
    if count == 0 { return 0; }
    // One first visit, one key copy and three first-node passes. Each
    // duplicate then needs one visit and one lookup of the same key.
    1 + u64::try_from(SURFACE_ID.len()).unwrap() + 3 * surface_node_bytes()
        + u64::try_from(count - 1).unwrap()
            * (1 + u64::try_from(SURFACE_ID.len()).unwrap())
}

fn assert_refusal(ir: &CadIr, work: u64, operation: &'static str, additional: u64) {
    let original = ir.model.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = work;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test offset index storage").unwrap();
    let first = match storage.with_storage(|| OffsetLookups::from_ir(ir, &ctx)) {
        Err(CodecError::ResourceLimit(first)) => first,
        _ => panic!("expected owning source or copy refusal"),
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!(first.operation, operation);
    assert_eq!((first.limit, first.used, first.additional), (work, work, additional));
    let empty = CadIr::empty();
    for replay in [ir, &empty] {
        assert!(matches!(storage.with_storage(|| OffsetLookups::from_ir(replay, &ctx)),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
    assert_eq!(ir.model, original);
    drop(storage);
    assert!(matches!(ctx.finish_session(),
        Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn offset_surface_index_refuses_first_and_last_actual_source_visits() {
    for count in [1, 64] {
        let ir = duplicate_surfaces(count);
        for visited in [0, count - 1] {
            assert_refusal(&ir, completed_duplicate_prefix(visited),
                "iges offset surface index traversal", 1);
        }
    }
}

#[test]
fn offset_surface_key_copy_refuses_after_one_visit_without_admitting_the_tail() {
    let ir = duplicate_surfaces(64);
    assert_refusal(&ir, 1, "iges offset surface index keys",
        u64::try_from(SURFACE_ID.len()).unwrap());
}

#[test]
fn offset_duplicate_index_keeps_one_key_and_first_last_positions_in_live_storage() {
    for count in [1, 64] {
        let ir = duplicate_surfaces(count);
        let original = ir.model.clone();
        let bytes = u64::try_from(SURFACE_ID.len()).unwrap() + surface_node_bytes();
        for refuse_while_live in [false, true] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_materialized_bytes = bytes;
            policy.limits.max_retained_bytes = 0;
            policy.limits.max_collection_items = 1;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test offset index storage").unwrap();
            let lookups = storage.with_storage(|| OffsetLookups::from_ir(&ir, &ctx)).unwrap();
            assert_eq!(lookups.surfaces.len(), 1);
            assert_eq!(lookups.surfaces.get(&ir.model.surfaces[0].id), Some(&(0, count - 1)));
            assert!(lookups.procedural.is_empty());
            assert_eq!(ir.model, original);
            if refuse_while_live {
                let first = match ctx.reserve_scoped(1, "test live offset index") {
                    Err(CodecError::ResourceLimit(first)) => first,
                    _ => panic!("expected live offset index storage refusal"),
                };
                assert_eq!(first.dimension, ResourceDimension::MaterializedBytes);
                assert_eq!((first.limit, first.used, first.additional), (bytes, bytes, 1));
                assert!(matches!(storage.with_storage(|| OffsetLookups::from_ir(&CadIr::empty(), &ctx)),
                    Err(CodecError::ResourceLimit(last)) if last == first));
                drop(lookups);
                drop(storage);
                assert!(matches!(ctx.finish_session(),
                    Err(CodecError::ResourceLimit(last)) if last == first));
            } else {
                drop(lookups);
                drop(storage);
                let released = ctx.reserve_scoped(bytes, "test released offset index").unwrap();
                drop(released);
                ctx.finish_session().unwrap();
            }
        }
    }
}

#[test]
fn offset_unowned_procedural_index_refuses_first_and_last_actual_source_visits() {
    for count in [1, 64] {
        let ir = unowned_procedural_surfaces(count);
        for visited in [0, count - 1] {
            // The current shared unique_index implementation has one charged
            // exhausted probe for its empty owner input. This control isolates
            // the following source walk; it does not certify that shared probe.
            let before_source = 1 + u64::try_from(visited).unwrap()
                * (1 + u64::try_from(PROCEDURAL_ID.len()).unwrap());
            assert_refusal(&ir, before_source, "iges offset procedural index traversal", 1);
        }
    }
}

#[test]
fn offset_unowned_procedural_index_does_not_create_an_owner_or_output_backing() {
    for count in [1, 64] {
        let ir = unowned_procedural_surfaces(count);
        let original = ir.model.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let lookups = OffsetLookups::from_ir(&ir, &ctx).unwrap();
        assert!(lookups.surfaces.is_empty());
        assert!(lookups.procedural.is_empty());
        assert_eq!(ir.model, original);
        ctx.finish_session().unwrap();
    }
}
