// SPDX-License-Identifier: Apache-2.0

use super::intervals::{owner_at, parameter_at};
use super::super::{
    dimension_parameter_index, parse_dimension_presentation_frame, record_stream,
    Grouped, IndexedRecordOffsets, PresentationPassTables, PresentationStreamTables,
    StreamIndexRuns,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

/// A table built from the original native dimension fixtures in one session.
fn with_native_table(
    check: impl FnOnce(&DecodeContext<'_>, &PresentationStreamTables<'_, '_, '_, '_>, &str),
) {
    let parameters = [parameter_at(12, 50), parameter_at(22, 70)];
    let owners = [owner_at(20, 13, 22, 21, 140), owner_at(10, 13, 12, 11, 120)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 8192;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let stream = record_stream(&ctx, owners[0].id()).unwrap().unwrap();
    let index_storage;
    let (index, held) = dimension_parameter_index(&ctx, &parameters, "test presentation parameters").unwrap();
    index_storage = held;
    let owner_storage;
    let (grouped, held) = ctx.with_scoped_storage("test presentation owner rows", || {
        Grouped::build(&ctx, &owners, |owner| record_stream(&ctx, owner.id()), "test presentation owner rows")
    }).unwrap();
    owner_storage = held;
    let pass = PresentationPassTables {
        owners: grouped,
        geometry: StreamIndexRuns::new(&ctx, Vec::new(), "test empty presentation geometry").unwrap(),
        sketch_entities: StreamIndexRuns::new(&ctx, Vec::new(), "test empty presentation entities").unwrap(),
        _storage: [owner_storage,
            ctx.reserve_scoped(0, "test empty presentation geometry").unwrap(),
            ctx.reserve_scoped(0, "test empty presentation entities").unwrap()],
    };
    let records = IndexedRecordOffsets::build(&ctx, &[]).unwrap();
    let table_storage;
    let (tables, held) = ctx.with_scoped_storage("test presentation table", || {
        PresentationStreamTables::build(stream, &records, &pass, &index)
    }).unwrap();
    table_storage = held;
    check(&ctx, &tables, stream);
    drop(tables);
    drop(table_storage);
    drop(records);
    drop(pass);
    drop(index);
    drop(index_storage);
    if ctx.resource_refusal().is_none() {
        drop(ctx.reserve_scoped(8192, "after actual presentation table destruction").unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn presentation_session_keeps_native_owner_offset_order_and_releases_actual_backing() {
    with_native_table(|ctx, tables, _| {
        assert_eq!(tables.governing_owner(13, 119).unwrap().unwrap().record_index(), 10);
        assert_eq!(tables.governing_owner(13, 120).unwrap().unwrap().record_index(), 20);
        assert!(tables.governing_owner(13, 140).unwrap().is_none());
        assert!(tables.governing_owner(99, 0).unwrap().is_none());
        assert!(parse_dimension_presentation_frame(&[], 0, tables, |_| Ok(false)).unwrap().is_none());
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn presentation_session_original_fuse_dominates_existing_missing_and_short_frame_queries() {
    with_native_table(|ctx, tables, _| {
        let other_arena = DecodeArena::new();
        let policy = DecodePolicy::service();
        let (other, _) = DecodeContext::from_root_bytes(&[], &other_arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(
            policy.limits.max_work_units + 1, "original presentation table session refusal",
        ) else { panic!("original creating session must refuse"); };
        for scope in [13, 99] {
            assert!(matches!(tables.governing_owner(scope, 0),
                Err(CodecError::ResourceLimit(later)) if later == original));
        }
        assert!(matches!(parse_dimension_presentation_frame(&[], usize::MAX, tables, |_| Ok(false)),
            Err(CodecError::ResourceLimit(later)) if later == original));
        assert_eq!(ctx.resource_refusal(), Some(original));
        assert!(other.resource_refusal().is_none());
        other.charge_work(0, "unrelated presentation session remains healthy").unwrap();
        other.finish_session().unwrap();
    });
}

#[test]
fn presentation_session_known_empty_tables_and_frames_are_free_then_keep_original_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let pass = PresentationPassTables {
        owners: Grouped::build(&ctx, &[] as &[crate::records::parameters::DesignParameterOwner],
            |owner| record_stream(&ctx, owner.id()), "test empty presentation owners").unwrap(),
        geometry: StreamIndexRuns::new(&ctx, Vec::new(), "test empty presentation geometry").unwrap(),
        sketch_entities: StreamIndexRuns::new(&ctx, Vec::new(), "test empty presentation entities").unwrap(),
        _storage: [ctx.reserve_scoped(0, "test empty presentation owners").unwrap(),
            ctx.reserve_scoped(0, "test empty presentation geometry").unwrap(),
            ctx.reserve_scoped(0, "test empty presentation entities").unwrap()],
    };
    let records = IndexedRecordOffsets::build(&ctx, &[]).unwrap();
    let index = std::collections::HashMap::new();
    let tables = PresentationStreamTables::build("f3d:missing", &records, &pass, &index).unwrap();
    assert!(tables.governing_owner(99, 0).unwrap().is_none());
    assert!(parse_dimension_presentation_frame(&[], 0, &tables, |_| Ok(false)).unwrap().is_none());
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_work(1, "original empty presentation refusal")
    else { panic!("original zero-work session must refuse"); };
    assert!(matches!(tables.governing_owner(99, 0),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(parse_dimension_presentation_frame(&[], 0, &tables, |_| Ok(false)),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert!(matches!(PresentationStreamTables::build("f3d:missing", &records, &pass, &index),
        Err(CodecError::ResourceLimit(later)) if later == original));
    assert_eq!(ctx.resource_refusal(), Some(original));
    drop(tables);
    drop(pass);
}
