// SPDX-License-Identifier: Apache-2.0

use super::*;
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::ids::AppearanceId;
use cadmpeg_ir::topology::Color;
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::BTreeSet;

fn key(position: usize) -> AppearanceId {
    AppearanceId::mint(format!("iges:model:appearance#D{}", 2 * position + 1)).unwrap()
}

fn model(count: usize) -> CadIr {
    let mut ir = CadIr::empty();
    for position in 0..count {
        ir.model.appearances.push(Appearance {
            id: key(position), name: Some(format!("source {position}")),
            asset_guid: None, library_id: None, visual_guid: None, physical_token: None,
            schema: None, category: None, base_color: Color::new(0.0, 0.0, 1.0, 1.0),
            properties: BTreeMap::new(), textures: Vec::new(),
        });
    }
    ir
}

fn comparisons(length: usize) -> u64 {
    if length == 0 { 0 } else {
        u64_from_index(length).min(11 * u64::from(length.div_ceil(2).ilog(6) + 1))
    }
}

fn prefix_work(count: usize) -> u64 {
    let node = u64_from_index(11 * std::mem::size_of::<String>()
        + 16 * std::mem::size_of::<usize>() + 2 * std::mem::align_of::<usize>());
    (0..count).map(|position| {
        let bytes = u64_from_index(key(position).as_str().len());
        // One actual next, one text copy, two complete-key queries and the
        // existing node shift/split bound. All keys are unique.
        1 + bytes + 2 * bytes * comparisons(position)
            + node * (1 + 2 * u64::from(position.is_multiple_of(5)))
    }).sum()
}

fn lookup_work(count: usize) -> u64 {
    u64_from_index(key(0).as_str().len()) * comparisons(count)
}

fn run(
    ir: &mut CadIr, ctx: &DecodeContext<'_>, identities: &mut Option<BTreeSet<String>>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
) -> Result<(), CodecError> {
    // Existing identities must keep their original name and color. Invalid
    // optional name bytes cannot be decoded on the duplicate route.
    super::super::appearance(ir, Cow::Owned(key(0)), Some(b"\xff"),
        Color::new(1.0, 0.0, 0.0, 1.0).unwrap(), ctx, (identities, storage))
}

#[test]
fn appearance_index_refuses_first_and_last_actual_source_visits() {
    for count in [1, 64] {
        for visited in [0, count - 1] {
            let mut ir = model(count);
            let expected = ir.model.clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            let cap = prefix_work(visited);
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test appearance index storage").unwrap();
            let mut identities = None;
            let Err(CodecError::ResourceLimit(first)) = run(&mut ir, &ctx, &mut identities, &mut storage)
                else { panic!("expected actual appearance source refusal"); };
            assert_eq!(first.dimension, ResourceDimension::WorkUnits);
            assert_eq!(first.operation, "iges appearance index traversal");
            assert_eq!((first.used, first.additional, first.limit), (cap, 1, cap));
            assert!(identities.is_none());
            assert_eq!(ir.model, expected);
            for mut replay in [model(count), CadIr::empty()] {
                let expected = replay.model.clone();
                assert!(matches!(run(&mut replay, &ctx, &mut identities, &mut storage),
                    Err(CodecError::ResourceLimit(last)) if last == first));
                assert!(identities.is_none());
                assert_eq!(replay.model, expected);
            }
            drop(identities);
            drop(storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
}

#[test]
fn appearance_index_completion_enters_the_actual_existing_identity_query() {
    for count in [1, 64] {
        let mut ir = model(count);
        let expected = ir.model.clone();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let cap = prefix_work(count);
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = ctx.reserve_scoped(0, "test appearance index storage").unwrap();
        let mut identities = None;
        let Err(CodecError::ResourceLimit(first)) = run(&mut ir, &ctx, &mut identities, &mut storage)
            else { panic!("expected actual existing identity query refusal"); };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.operation, "iges appearance index lookup");
        assert_eq!((first.used, first.additional, first.limit), (cap, lookup_work(count), cap));
        let expected_index: BTreeSet<String> = ir.model.appearances.iter()
            .map(|appearance| appearance.id.as_str().to_owned()).collect();
        assert_eq!(identities, Some(expected_index));
        assert_eq!(ir.model, expected);
        for mut replay in [model(count), CadIr::empty()] {
            let expected = replay.model.clone();
            let expected_index = identities.clone();
            assert!(matches!(run(&mut replay, &ctx, &mut identities, &mut storage),
                Err(CodecError::ResourceLimit(last)) if last == first));
            assert_eq!(identities, expected_index);
            assert_eq!(replay.model, expected);
        }
        drop(identities);
            drop(storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn appearance_index_accepts_exact_source_and_query_work_and_reuses_its_keys() {
    for count in [1, 64] {
        for calls in [1, 64] {
            let mut ir = model(count);
            let expected = ir.model.clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = prefix_work(count) + lookup_work(count) * calls;
            policy.limits.max_collection_items = u64_from_index(count);
            policy.limits.max_entities = 0;
            policy.limits.max_retained_bytes = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test appearance index storage").unwrap();
            let mut identities = None;
            for _ in 0..calls { run(&mut ir, &ctx, &mut identities, &mut storage).unwrap(); }
            assert_eq!(identities.as_ref().unwrap().len(), count);
            for appearance in &ir.model.appearances {
                assert!(identities.as_ref().unwrap().contains(appearance.id.as_str()));
            }
            assert_eq!(ir.model, expected);
            drop(identities);
            drop(storage);
            ctx.finish_session().unwrap();
        }
    }
}

#[test]
fn appearance_index_preserves_original_refusal_before_empty_or_cached_state_mutation() {
    for count in [0, 1, 64] {
        for cached in [false, true] {
            let mut ir = model(count);
            let expected = ir.model.clone();
            let mut identities = cached.then(|| ir.model.appearances.iter()
                .map(|appearance| appearance.id.as_str().to_owned()).collect::<BTreeSet<_>>());
            let expected_index = identities.clone();
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = 0;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut storage = ctx.reserve_scoped(0, "test appearance index storage").unwrap();
            let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original appearance refusal")
                else { panic!("expected original refusal"); };
            for _ in 0..64 {
                assert!(matches!(run(&mut ir, &ctx, &mut identities, &mut storage),
                    Err(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(identities, expected_index);
                assert_eq!(ir.model, expected);
            }
            drop(identities);
            drop(storage);
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
        }
    }
}
