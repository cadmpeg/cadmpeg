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
    let mut session = CommitSession::new(ir, &ctx).unwrap();
    assert!(session.contains(&identity).unwrap());
    assert!(session.contains(&identity).unwrap());
    let ir = session.into_document();
    assert_eq!(ir.model.points[0].id.as_str(), identity);
    let reservation = ctx.reserve_scoped(4096, "owned cache released").unwrap();
    drop(reservation);
    ctx.finish_session().unwrap();
}

#[test]
fn owned_commit_session_preserves_cross_candidate_references_and_rejections() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut session = CommitSession::new(CadIr::empty(), &ctx).unwrap();
    let point = "test:model:point#committed";
    let vertex = "test:model:vertex#committed";
    session.commit_model(super::point_draft(point)).unwrap().unwrap();
    assert!(session.contains(point).unwrap());
    assert!(session.commit_model(super::point_draft(point)).unwrap().is_err());
    session.commit_model(super::vertex_draft(vertex, point)).unwrap().unwrap();
    assert!(session.contains(point).unwrap());
    assert!(session.contains(vertex).unwrap());
    let ir = session.into_document();
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
    let mut session = CommitSession::new(ir, &ctx).unwrap();
    assert!(session.contains(first).unwrap());
    assert!(session.contains(second).unwrap());
    session.document_mut().unwrap().model.points.swap(0, 1);
    assert!(session.contains(first).unwrap());
    assert!(session.contains(second).unwrap());
    session.document_mut().unwrap().model.points.remove(0);
    assert!(session.contains(first).unwrap());
    assert!(!session.contains(second).unwrap());
    let ir = session.into_document();
    assert_eq!(ir.model.points.len(), 1);
    assert_eq!(ir.model.points[0].id.as_str(), first);
    ctx.finish_session().unwrap();
}
