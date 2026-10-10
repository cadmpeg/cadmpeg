//! Borrowed termination consensus and its original refusal route.

use super::super::{consensus_termination_vote, FaceCondition, FaceReference, TerminationVote};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn face(reference: &str, canonical: Option<&str>, identity: &str) -> TerminationVote {
    TerminationVote::Face {
        condition: FaceCondition::ToFace,
        reference: FaceReference {
            reference: reference.into(),
            canonical: canonical.map(str::to_owned),
        },
        identity: identity.into(),
    }
}

fn no_storage_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy
}

#[test]
fn same_lane_consensus_borrows_reference_without_copying_identity_or_fallback() {
    let votes = [Some(face(
        "lane-0:100",
        Some("components:1,2,3"),
        "identity",
    ))];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &no_storage_policy()).unwrap();
    let consensus = consensus_termination_vote(&ctx, &votes).unwrap().unwrap();
    let reference = consensus.reference.unwrap();
    assert_eq!(reference, "lane-0:100");
    assert!(std::ptr::eq(
        reference,
        votes[0].as_ref().unwrap().reference().unwrap()
    ));
    assert_eq!(consensus.vote.condition(), "ToFace");
    ctx.finish_session().unwrap();
}

#[test]
fn cross_lane_consensus_borrows_first_canonical_reference() {
    let votes = [
        Some(face("lane-0:100", Some("components:1,2,3"), "identity")),
        Some(face("lane-1:200", Some("other-fallback"), "identity")),
    ];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &no_storage_policy()).unwrap();
    let consensus = consensus_termination_vote(&ctx, &votes).unwrap().unwrap();
    let reference = consensus.reference.unwrap();
    assert_eq!(reference, "components:1,2,3");
    let TerminationVote::Face {
        reference: first, ..
    } = votes[0].as_ref().unwrap()
    else {
        panic!("face fixture");
    };
    assert!(std::ptr::eq(reference, first.canonical.as_deref().unwrap()));
    ctx.finish_session().unwrap();
}

#[test]
fn cross_lane_consensus_without_first_fallback_keeps_reference_unresolved() {
    let votes = [
        Some(face("lane-0:100", None, "identity")),
        Some(face("lane-1:200", Some("components:1,2,3"), "identity")),
    ];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &no_storage_policy()).unwrap();
    let consensus = consensus_termination_vote(&ctx, &votes).unwrap().unwrap();
    assert_eq!(consensus.reference, None);
    assert_eq!(consensus.vote.condition(), "ToFace");
    ctx.finish_session().unwrap();
}

#[test]
fn vertex_consensus_borrows_only_the_agreed_reference() {
    let votes = [Some(TerminationVote::ToVertex {
        reference: "vertex".into(),
    })];
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &no_storage_policy()).unwrap();
    let consensus = consensus_termination_vote(&ctx, &votes).unwrap().unwrap();
    let reference = consensus.reference.unwrap();
    assert_eq!(reference, "vertex");
    assert!(std::ptr::eq(
        reference,
        votes[0].as_ref().unwrap().reference().unwrap()
    ));
    ctx.finish_session().unwrap();
}

#[test]
fn disagreeing_identity_and_missing_votes_do_not_allocate_consensus() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &no_storage_policy()).unwrap();
    for votes in [
        [
            Some(face("lane-0:100", None, "first")),
            Some(face("lane-1:200", None, "second")),
        ],
        [Some(face("lane-0:100", None, "first")), None],
    ] {
        assert!(consensus_termination_vote(&ctx, &votes).unwrap().is_none());
    }
    ctx.finish_session().unwrap();
}

#[test]
fn borrowed_consensus_preserves_original_first_visit_refusal() {
    const OPERATION: &str = "agree SLDPRT extrusion terminations";
    let votes = [Some(face(
        "lane-0:100",
        Some("components:1,2,3"),
        "identity",
    ))];
    let arena = DecodeArena::new();
    let mut policy = no_storage_policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(refusal)) = consensus_termination_vote(&ctx, &votes) else {
        panic!("first visit must refuse before comparing identity");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, OPERATION);
    assert_eq!(refusal.used, 0);
    assert_eq!(refusal.additional, 1);
    assert!(matches!(
        consensus_termination_vote(&ctx, &votes),
        Err(CodecError::ResourceLimit(repeated)) if repeated == refusal
    ));
    assert!(matches!(
        ctx.finish_session(),
        Err(CodecError::ResourceLimit(repeated)) if repeated == refusal
    ));
}
