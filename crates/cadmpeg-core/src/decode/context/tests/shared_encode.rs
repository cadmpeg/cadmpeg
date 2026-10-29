// SPDX-License-Identifier: Apache-2.0
//! Independent encode helper contexts share one caller account.

use std::io::Cursor;

use crate::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ExpandSpec, ResourceDimension, ResourceFailure,
};
use crate::encode::EncodeSession;
use crate::CodecError;

fn resource(error: CodecError) -> crate::decode::ResourceLimit {
    let CodecError::ResourceLimit(limit) = error else {
        panic!("actual caller resource refusal")
    };
    limit
}

#[test]
fn independent_contexts_and_live_output_share_the_exact_retained_ceiling() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 8;
    policy.mode = crate::decode::DecodeMode::Strict;
    let first = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&first).unwrap();
    let second_arena = DecodeArena::new();
    let second = DecodeContext::for_encode_session(&second_arena, &session).unwrap();
    assert_eq!(second.policy(), &policy);
    first.retain_bytes(3, "first held record").unwrap();
    second.retain_bytes(3, "second held record").unwrap();
    let mut output = session.buffer();
    output.append(b"12", "live output").unwrap();
    assert_eq!(output.as_bytes(), b"12");
    assert_eq!(first.retained_bytes_available(), 0);
    assert_eq!(second.retained_bytes_available(), 0);
    let refusal = resource(
        second
            .retain_bytes(1, "aggregate retained overflow")
            .unwrap_err(),
    );
    assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 8, 1));
    assert_eq!(first.resource_refusal(), Some(refusal));
    drop(output);
    assert_eq!(session.finish(), Err(refusal));
    assert_eq!(
        resource(
            first
                .retain_bytes(0, "empty after output drop")
                .unwrap_err()
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
    let session = EncodeSession::from_context(&first).unwrap();
    let second = DecodeContext::for_encode_session(&arena, &session).unwrap();
    first.charge_entities(1, "first record").unwrap();
    second.charge_entities(1, "second record").unwrap();
    session.charge_entities(1, "written record").unwrap();
    let refusal = resource(
        second
            .charge_entities(1, "aggregate entity overflow")
            .unwrap_err(),
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
    let session = EncodeSession::from_context(&first).unwrap();
    let second = DecodeContext::for_encode_session(&arena, &session).unwrap();
    let third = DecodeContext::for_encode_session(&arena, &session).unwrap();
    assert_eq!(
        first
            .read_input_prefix(&mut Cursor::new(b"abc"), 3)
            .unwrap(),
        b"abc"
    );
    assert_eq!(
        second
            .read_input_prefix(&mut Cursor::new(b"defgh"), 5)
            .unwrap(),
        b"defgh"
    );
    let refusal = resource(
        third
            .read_input_prefix(&mut Cursor::new(b"i"), 1)
            .unwrap_err(),
    );
    assert_eq!(refusal.dimension, ResourceDimension::InputBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 8, 1));
    assert_eq!(first.resource_refusal(), Some(refusal));
    assert_eq!(second.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
}

#[test]
fn independent_expansions_charge_actual_bytes_to_one_eight_byte_ceiling() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_decompressed_bytes_total = 8;
    policy.limits.max_decompressed_bytes_per_expand = 8;
    let first = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&first).unwrap();
    let second = DecodeContext::for_encode_session(&arena, &session).unwrap();
    let third = DecodeContext::for_encode_session(&arena, &session).unwrap();
    let mut a = first.begin_expand(ExpandSpec::Exact(3)).unwrap();
    a.write(b"abc").unwrap();
    assert_eq!(a.finalize_owned().unwrap(), b"abc");
    let mut b = second.begin_expand(ExpandSpec::Exact(5)).unwrap();
    b.write(b"defgh").unwrap();
    assert_eq!(b.finalize_owned().unwrap(), b"defgh");
    let mut c = third.begin_expand(ExpandSpec::Exact(1)).unwrap();
    let refusal = resource(c.write(b"i").unwrap_err());
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
    let session = EncodeSession::from_context(&origin).unwrap();
    let attached = DecodeContext::for_encode_session(&arena, &session).unwrap();
    let mut values = Vec::<u64>::new();
    let refusal = resource(
        attached
            .reserve_working(&mut values, usize::MAX, "actual working overflow")
            .unwrap_err(),
    );
    assert_eq!(refusal.reason, ResourceFailure::AllocationFailed);
    assert_eq!(origin.resource_refusal(), Some(refusal));
    drop(attached);
    assert_eq!(session.finish(), Err(refusal));
    assert!(
        matches!(DecodeContext::for_encode_session(&arena, &session), Err(actual) if actual == refusal)
    );
    assert_eq!(resource(origin.finish_session().unwrap_err()), refusal);
}

#[test]
fn attached_input_includes_the_origins_already_admitted_root_length() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 8;
    let (origin, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
    let session = EncodeSession::from_context(&origin).unwrap();
    let attached = DecodeContext::for_encode_session(&arena, &session).unwrap();
    assert_eq!(
        attached
            .read_input_prefix(&mut Cursor::new(b"defgh"), 5)
            .unwrap(),
        b"defgh"
    );
    let refusal = resource(
        attached
            .read_input_prefix(&mut Cursor::new(b"i"), 1)
            .unwrap_err(),
    );
    assert_eq!(refusal.dimension, ResourceDimension::InputBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 8, 1));
    assert_eq!(origin.resource_refusal(), Some(refusal));
}

#[test]
fn ordinary_independent_decodes_keep_separate_caller_accounts() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 3;
    policy.limits.max_entities = 1;
    let (a, _) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
    let (b, _) = DecodeContext::from_root_bytes(b"def", &arena, &policy).unwrap();
    a.charge_entities(1, "independent first").unwrap();
    b.charge_entities(1, "independent second").unwrap();
    let refusal = resource(a.charge_entities(1, "first decode overflow").unwrap_err());
    assert_eq!(refusal.dimension, ResourceDimension::Entities);
    assert_eq!(b.resource_refusal(), None);
    b.finish_session().unwrap();
}
