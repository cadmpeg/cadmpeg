// SPDX-License-Identifier: Apache-2.0
//! Independent encode helper contexts share one caller account.

use std::io::Cursor;

use crate::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, LimitScope, ResourceDimension,
    ResourceFailure,
};
use crate::encode::EncodeSession;
use crate::CodecError;

fn resource(error: &CodecError) -> crate::decode::ResourceLimit {
    let CodecError::ResourceLimit(limit) = error else {
        panic!("actual caller resource refusal")
    };
    *limit
}

#[test]
fn independent_contexts_and_live_output_share_the_exact_retained_ceiling() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    policy.mode = crate::decode::DecodeMode::Strict;
    let first = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&first).expect("encode session opens from a fresh context");
    let second_arena = DecodeArena::new();
    let second = DecodeContext::for_encode_session(&second_arena, &session)
        .expect("attached context opens on a live session");
    assert_eq!(second.policy(), &policy);
    first
        .retain_bytes(3, "first held record")
        .expect("three retained bytes fit the eight-byte ceiling");
    second
        .retain_bytes(3, "second held record")
        .expect("three more retained bytes fit the ceiling");
    let mut output = session.buffer();
    output
        .append(b"12", "live output")
        .expect("two output bytes fit the remaining ceiling");
    assert_eq!(output.as_bytes(), b"12");
    assert_eq!(first.retained_bytes_available(), 0);
    assert_eq!(second.retained_bytes_available(), 0);
    let refusal = resource(
        &second
            .retain_bytes(1, "aggregate retained overflow")
            .expect_err("the ninth retained byte exceeds the ceiling"),
    );
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 8, 1));
    assert_eq!(first.resource_refusal(), Some(refusal));
    drop(output);
    assert_eq!(session.finish(), Err(refusal));
    assert_eq!(
        resource(
            &first
                .retain_bytes(0, "empty after output drop")
                .expect_err("the latched refusal blocks a later charge")
        ),
        refusal
    );
}

#[test]
fn independent_contexts_and_emission_share_the_exact_entity_ceiling() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 3;
    let first = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&first).expect("encode session opens from a fresh context");
    let second = DecodeContext::for_encode_session(&arena, &session)
        .expect("attached context opens on a live session");
    first
        .charge_entities(1, "first record")
        .expect("first entity fits the ceiling");
    second
        .charge_entities(1, "second record")
        .expect("second entity fits the ceiling");
    session
        .charge_entities(1, "written record")
        .expect("written entity fits the ceiling");
    let refusal = resource(
        &second
            .charge_entities(1, "aggregate entity overflow")
            .expect_err("the fourth entity exceeds the ceiling"),
    );
    assert_eq!(refusal.dimension, ResourceDimension::Entities);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (3, 3, 1));
    assert_eq!(first.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
    assert!(
        matches!(DecodeContext::for_encode_session(&arena, &session), Err(actual) if actual == refusal)
    );
}

#[test]
fn independent_contexts_charge_actual_input_to_one_eight_byte_ceiling() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 8;
    let first = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&first).expect("encode session opens from a fresh context");
    let second = DecodeContext::for_encode_session(&arena, &session)
        .expect("second context opens on a live session");
    let third = DecodeContext::for_encode_session(&arena, &session)
        .expect("third context opens on a live session");
    assert_eq!(
        first
            .read_input_prefix(&mut Cursor::new(b"abc"), 3)
            .expect("three input bytes fit the ceiling"),
        b"abc"
    );
    assert_eq!(
        second
            .read_input_prefix(&mut Cursor::new(b"defgh"), 5)
            .expect("five more input bytes fit the ceiling"),
        b"defgh"
    );
    let refusal = resource(
        &third
            .read_input_prefix(&mut Cursor::new(b"i"), 1)
            .expect_err("the ninth input byte exceeds the ceiling"),
    );
    assert_eq!(refusal.dimension, ResourceDimension::InputBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 8, 1));
    assert_eq!(first.resource_refusal(), Some(refusal));
    assert_eq!(second.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
}

#[test]
fn encode_session_recovers_a_part_scoped_expansion_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = 8;
    policy.limits.max_decompressed_bytes_per_expand = 1;
    let ctx = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&ctx).unwrap();
    let outcome = ctx.run_part(|| {
        let mut expansion = ctx.begin_expand(ExpandSpec::Exact(2))?;
        expansion.write(b"ab")?;
        expansion.finalize_owned()
    });
    assert!(matches!(
        outcome,
        Ok(Err(CodecError::ResourceLimit(limit))) if limit.scope == LimitScope::Part
    ));
    assert_eq!(session.finish(), Ok(()));
}

#[test]
fn independent_expansions_charge_actual_bytes_to_one_eight_byte_ceiling() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = 8;
    policy.limits.max_decompressed_bytes_per_expand = 8;
    let first = DecodeContext::for_encode(&arena, &policy);
    let session =
        EncodeSession::from_context(&first).expect("encode session opens from a fresh context");
    let second = DecodeContext::for_encode_session(&arena, &session)
        .expect("second context opens on a live session");
    let third = DecodeContext::for_encode_session(&arena, &session)
        .expect("third context opens on a live session");
    let mut a = first
        .begin_expand(ExpandSpec::Exact(3))
        .expect("three-byte expansion fits the ceiling");
    a.write(b"abc")
        .expect("three expanded bytes fit the ceiling");
    assert_eq!(
        a.finalize_owned().expect("three-byte expansion finalizes"),
        b"abc"
    );
    let mut b = second
        .begin_expand(ExpandSpec::Exact(5))
        .expect("five-byte expansion fits the ceiling");
    b.write(b"defgh")
        .expect("five expanded bytes fit the ceiling");
    assert_eq!(
        b.finalize_owned().expect("five-byte expansion finalizes"),
        b"defgh"
    );
    let mut c = third
        .begin_expand(ExpandSpec::Exact(1))
        .expect("one-byte expansion opens before any byte is charged");
    let refusal = resource(
        &c.write(b"i")
            .expect_err("the ninth expanded byte exceeds the ceiling"),
    );
    assert_eq!(refusal.dimension, ResourceDimension::DecompressedBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 8, 1));
    assert_eq!(first.resource_refusal(), Some(refusal));
    assert_eq!(second.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
}

#[test]
fn working_allocation_refusal_remains_shared_after_context_drop_and_attachment() {
    let arena = DecodeArena::new();
    let origin = DecodeContext::for_encode(&arena, &DecodePolicy::service());
    let session =
        EncodeSession::from_context(&origin).expect("encode session opens from a fresh context");
    let attached = DecodeContext::for_encode_session(&arena, &session)
        .expect("attached context opens on a live session");
    let mut values = Vec::<u64>::new();
    let refusal = resource(
        &attached
            .reserve_working(&mut values, usize::MAX, "actual working overflow")
            .expect_err("a usize::MAX working reservation is refused"),
    );
    assert_eq!(refusal.reason, ResourceFailure::AllocationFailed);
    assert_eq!(origin.resource_refusal(), Some(refusal));
    drop(attached);
    assert_eq!(session.finish(), Err(refusal));
    assert!(
        matches!(DecodeContext::for_encode_session(&arena, &session), Err(actual) if actual == refusal)
    );
    assert_eq!(
        resource(
            &origin
                .finish_session()
                .expect_err("the origin session reports the shared refusal")
        ),
        refusal
    );
}

#[test]
fn attached_input_includes_the_origins_already_admitted_root_length() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 8;
    let (origin, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy)
        .expect("three root bytes fit the eight-byte ceiling");
    let session =
        EncodeSession::from_context(&origin).expect("encode session opens from the root context");
    let attached = DecodeContext::for_encode_session(&arena, &session)
        .expect("attached context opens on a live session");
    assert_eq!(
        attached
            .read_input_prefix(&mut Cursor::new(b"defgh"), 5)
            .expect("five more input bytes fit the ceiling"),
        b"defgh"
    );
    let refusal = resource(
        &attached
            .read_input_prefix(&mut Cursor::new(b"i"), 1)
            .expect_err("the ninth input byte exceeds the ceiling"),
    );
    assert_eq!(refusal.dimension, ResourceDimension::InputBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 8, 1));
    assert_eq!(origin.resource_refusal(), Some(refusal));
}

#[test]
fn generated_root_shares_entity_budget_and_has_the_input_digest() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 1;
    let origin = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&origin).expect("encode session");
    session
        .charge_entities(1, "written record")
        .expect("one record fits");
    let (verification, view) =
        DecodeContext::for_encode_session_root(&arena, &session, b"abc").expect("generated root");
    let (independent, _) =
        DecodeContext::from_root_bytes(b"abc", &arena, &policy).expect("independent root");
    assert_eq!(view.window(), b"abc");
    assert_eq!(verification.source_digest(), independent.source_digest());
    let refusal = resource(
        &verification
            .charge_entities(1, "decoded record")
            .expect_err("the written record already consumed the limit"),
    );
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (1, 1, 1));
    assert_eq!(session.finish(), Err(refusal));
}

#[test]
fn generated_root_charges_the_shared_input_limit() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 5;
    let origin = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&origin).expect("encode session");
    origin
        .read_input_prefix(&mut Cursor::new(b"12"), 2)
        .expect("first input");
    DecodeContext::for_encode_session_root(&arena, &session, b"abc")
        .expect("five total input bytes fit");
    let Err(refusal) = DecodeContext::for_encode_session_root(&arena, &session, b"x") else {
        panic!("the sixth input byte exceeds the limit");
    };
    assert_eq!(refusal.dimension, ResourceDimension::InputBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (5, 5, 1));
    assert_eq!(session.finish(), Err(refusal));
}

#[test]
fn ordinary_independent_decodes_keep_separate_caller_accounts() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 3;
    policy.limits.max_entities = 1;
    let (a, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy)
        .expect("first root fits its own ceiling");
    let (b, _) = DecodeContext::from_root_bytes(b"def", &arena, &policy)
        .expect("second root fits its own ceiling");
    a.charge_entities(1, "independent first")
        .expect("first decode admits one entity");
    b.charge_entities(1, "independent second")
        .expect("second decode admits one entity");
    let refusal = resource(
        &a.charge_entities(1, "first decode overflow")
            .expect_err("the second entity exceeds the first decode ceiling"),
    );
    assert_eq!(refusal.dimension, ResourceDimension::Entities);
    assert_eq!(b.resource_refusal(), None);
    b.finish_session()
        .expect("second decode finishes without a refusal");
}
