// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use crate::draft::CommitSession;
use crate::CadIr;

#[test]
fn owned_commit_session_reuses_borrowed_identity_storage_until_extraction() {
    let identity = format!("test:model:point#{}", "p".repeat(1024));
    let mut ir = CadIr::empty();
    ir.model.points.push(super::point(&identity));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(ir, &ctx, None).unwrap();
    assert!(session.contains(&identity).unwrap());
    assert!(session.contains(&identity).unwrap());
    let ir = session.into_parts().0;
    assert_eq!(ir.model.points[0].id.as_str(), identity);
    let reservation = ctx.reserve_scoped(4096, "owned cache released").unwrap();
    drop(reservation);
    ctx.finish_session().unwrap();
}

#[test]
fn owned_commit_session_preserves_cross_candidate_references_and_rejections() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
    let point = "test:model:point#committed";
    let vertex = "test:model:vertex#committed";
    session.commit_model(super::point_draft(point)).unwrap().unwrap();
    assert!(session.contains(point).unwrap());
    assert!(session.commit_model(super::point_draft(point)).unwrap().is_err());
    session.commit_model(super::vertex_draft(vertex, point)).unwrap().unwrap();
    assert!(session.contains(point).unwrap());
    assert!(session.contains(vertex).unwrap());
    let ir = session.into_parts().0;
    assert_eq!(ir.model.points.len(), 1);
    assert_eq!(ir.model.vertices.len(), 1);
    assert_eq!(ir.model.vertices[0].point.as_str(), point);
    ctx.finish_session().unwrap();
}

#[test]
fn owned_commit_session_invalidates_positions_before_document_mutation() {
    let first = "test:model:point#first";
    let second = "test:model:point#second";
    let mut ir = CadIr::empty();
    ir.model.points.push(super::point(first));
    ir.model.points.push(super::point(second));
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(ir, &ctx, None).unwrap();
    assert!(session.contains(first).unwrap());
    assert!(session.contains(second).unwrap());
    session.document_mut().unwrap().model.points.swap(0, 1);
    assert!(session.contains(first).unwrap());
    assert!(session.contains(second).unwrap());
    session.document_mut().unwrap().model.points.remove(0);
    assert!(session.contains(first).unwrap());
    assert!(!session.contains(second).unwrap());
    let ir = session.into_parts().0;
    assert_eq!(ir.model.points.len(), 1);
    assert_eq!(ir.model.points[0].id.as_str(), first);
    ctx.finish_session().unwrap();
}

#[test]
fn staged_unknown_cache_moves_source_facts_and_extends_only_new_slots() {
    let first_id = format!("test:source:unknown#{}", "a".repeat(16384));
    let second_id = format!("test:source:unknown#{}", "b".repeat(16384));
    let image = vec![7; 65536];
    let image_pointer = image.as_ptr();
    let first = crate::unknown::UnknownRecord::retained(first_id.as_str().try_into().unwrap(), 17, image, Vec::new());
    let second = crate::unknown::UnknownRecord::unavailable(second_id.as_str().try_into().unwrap(), 29, 31, "source-digest", Vec::new());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 4096;
    policy.limits.max_materialized_bytes = 4096;
    // Each moved record has one slot, one cache key, and one cache position.
    policy.limits.max_collection_items = 6;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
    session.push_unknown(first).unwrap();
    assert!(session.contains(&first_id).unwrap());
    session.push_unknown(second).unwrap();
    assert!(session.contains(&first_id).unwrap());
    assert!(session.contains(&second_id).unwrap());
    session.unknown_links_mut(0).unwrap().1.push("test:model:point#later".into());
    assert!(session.contains(&first_id).unwrap());
    assert!(session.contains(&second_id).unwrap());
    let (ir, records) = session.into_parts();
    assert!(ir.model.points.is_empty());
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].id().as_str(), first_id);
    assert_eq!(records[0].offset(), 17);
    assert_eq!(records[0].data().unwrap().as_ptr(), image_pointer);
    assert_eq!(records[0].links(), ["test:model:point#later"]);
    assert_eq!(records[1].id().as_str(), second_id);
    assert_eq!(records[1].offset(), 29);
    let reservation = ctx.reserve_scoped(4096, "staged unknown cache released").unwrap();
    drop(reservation);
    ctx.finish_session().unwrap();
}

#[test]
fn staged_unknown_cache_resolves_draft_references_and_rejects_identity_collisions() {
    let target = "test:source:unknown#native";
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
    session.push_unknown(crate::unknown::UnknownRecord::retained(target.try_into().unwrap(), 0, Vec::new(), Vec::new())).unwrap();
    assert!(session.contains(target).unwrap());
    assert!(session.commit_model(super::point_draft(target)).unwrap().is_err());
    session.commit_model(super::vertex_draft("test:model:vertex#first", target)).unwrap().unwrap();
    session.commit_model(super::vertex_draft("test:model:vertex#second", target)).unwrap().unwrap();
    let (ir, records) = session.into_parts();
    assert_eq!(ir.model.vertices.len(), 2);
    assert!(ir.model.points.is_empty());
    assert_eq!(records.len(), 1);
    assert_eq!(ir.model.vertices[0].point.as_str(), target);
    assert_eq!(ir.model.vertices[1].point.as_str(), target);
    ctx.finish_session().unwrap();
}

#[test]
fn staged_unknown_append_refusal_preserves_source_and_cached_positions() {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;
    let first = "test:source:unknown#first";
    let second = "test:source:unknown#second";
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
    session.push_unknown(crate::unknown::UnknownRecord::retained(first.try_into().unwrap(), 0, Vec::new(), Vec::new())).unwrap();
    assert!(session.contains(first).unwrap());
    let Err(CodecError::ResourceLimit(limit)) = session.push_unknown(crate::unknown::UnknownRecord::retained(second.try_into().unwrap(), 0, Vec::new(), Vec::new())) else { panic!("next record slot must refuse"); };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert!(matches!(session.contains(first), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    assert_eq!(session.unknowns().len(), 1);
    let (_, records) = session.into_parts();
    assert_eq!(records[0].id().as_str(), first);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
}

#[test]
fn owned_source_session_replaces_native_unknown_identity_floor() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut ir = CadIr::empty();
    let old = "test:source:unknown#old";
    let other = "test:source:unknown#other";
    for (format, id) in [("rhino", old), ("other", other)] {
        ir.native.namespace_mut(format).arenas_mut().insert("unknowns".into(), vec![crate::NativeRecord::new(
            id.try_into().unwrap(), serde_json::Map::new(),
        ).unwrap()]);
    }
    let mut session = CommitSession::new(ir, &ctx, Some("rhino")).unwrap();
    let staged = "test:source:unknown#staged";
    session.push_unknown(crate::UnknownRecord::retained(staged.try_into().unwrap(), 0, vec![1], Vec::new())).unwrap();
    assert!(!session.contains(old).unwrap());
    assert!(session.contains(other).unwrap());
    assert!(session.contains(staged).unwrap());
    assert!(session.commit_model(super::vertex_draft("test:model:vertex#old", old)).unwrap().is_err());
    session.commit_model(super::vertex_draft("test:model:vertex#staged", staged)).unwrap().unwrap();
    assert_eq!(session.document().model.vertices.len(), 1);
    assert_eq!(session.document().native.namespace("rhino").unwrap().arenas()["unknowns"][0].id(), old);
}

#[test]
fn owned_source_session_clears_cached_source_positions() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    let first = "test:source:unknown#first";
    let second = "test:source:unknown#second";
    session.push_unknown(crate::UnknownRecord::retained(first.try_into().unwrap(), 0, vec![1], Vec::new())).unwrap();
    assert!(session.contains(first).unwrap());
    session.clear_unknowns().unwrap();
    session.push_unknown(crate::UnknownRecord::retained(second.try_into().unwrap(), 1, vec![2], Vec::new())).unwrap();
    assert!(!session.contains(first).unwrap());
    assert!(session.contains(second).unwrap());
    assert_eq!(session.unknown_links_mut(0).unwrap().0, second);
    assert_eq!(session.unknowns()[0].data().unwrap(), &[2]);
}

#[test]
fn speculative_session_append_reuses_source_cache_across_candidates() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // The source record and its cache use three slots. Each point adds a
    // staging key and position, a committed key and position, and an arena slot.
    policy.limits.max_collection_items = 13;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, Some("test")).unwrap();
    let source = "test:source:unknown#owner";
    session.push_unknown(crate::UnknownRecord::retained(source.try_into().unwrap(), 0, vec![7; 65536], Vec::new())).unwrap();
    let source_pointer = session.unknowns()[0].data().unwrap().as_ptr();
    assert!(session.contains(source).unwrap());
    for identity in ["test:model:point#first", "test:model:point#second"] {
        let model = crate::document::Model { points: vec![super::point(identity)], ..Default::default() };
        session.try_append(model, Default::default(), |combined, unknowns| {
            assert_eq!(unknowns[0].data().unwrap().as_ptr(), source_pointer);
            assert_eq!(combined.model.points.last().unwrap().id.as_str(), identity);
            Ok(Ok::<_, ()>(()))
        }).unwrap().unwrap();
        assert!(session.contains(source).unwrap());
        assert!(session.contains(identity).unwrap());
    }
    assert!(session.contains("test:model:point#first").unwrap());
    assert_eq!(session.document().model.points.len(), 2);
    let (_, unknowns) = session.into_parts();
    assert_eq!(unknowns[0].data().unwrap().as_ptr(), source_pointer);
    ctx.finish_session().unwrap();
}

#[test]
fn speculative_session_append_extends_native_cache_and_preserves_rejected_positions() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
    for (format, arena, identity) in [
        ("middle", "middle", "test:native:record#first"),
        ("middle", "middle", "test:native:record#second"),
        ("alpha", "alpha", "test:native:record#third"),
    ] {
        let mut native = crate::native::Native::default();
        native.namespace_mut(format).arenas_mut().insert(arena.into(), vec![crate::NativeRecord::new(identity.try_into().unwrap(), serde_json::Map::new()).unwrap()]);
        session.try_append(Default::default(), native, |_, _| Ok(Ok::<_, ()>(()))).unwrap().unwrap();
        assert!(session.contains(identity).unwrap());
        assert!(session.contains("test:native:record#first").unwrap());
    }
    let rejected = "test:model:point#rejected";
    let before = session.document().clone();
    let model = crate::document::Model { points: vec![super::point(rejected)], ..Default::default() };
    assert_eq!(session.try_append(model, Default::default(), |_, _| Ok(Err::<(), _>("candidate rejection"))).unwrap(), Err("candidate rejection"));
    assert_eq!(session.document(), &before);
    assert!(!session.contains(rejected).unwrap());
    assert!(session.contains("test:native:record#first").unwrap());
    assert!(session.contains("test:native:record#second").unwrap());
}

#[test]
fn speculative_session_append_keeps_original_resource_refusal_and_model() {
    use cadmpeg_core::CodecError;
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx, None).unwrap();
    session.commit_model(super::point_draft("test:model:point#first")).unwrap().unwrap();
    let before = session.document().clone();
    let model = crate::document::Model { points: vec![super::point("test:model:point#refused")], ..Default::default() };
    let Err(CodecError::ResourceLimit(limit)) = session.try_append(model, Default::default(), |_, _| {
        ctx.charge_work(u64::MAX / 2, "test session callback refusal")?;
        Ok(Ok::<_, ()>(()))
    }) else { panic!("session callback refusal stays outer"); };
    assert_eq!(limit.operation, "test session callback refusal");
    assert_eq!(session.document(), &before);
    assert!(matches!(session.contains("test:model:point#first"), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
    assert_eq!(session.into_parts().0, before);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == limit));
}
