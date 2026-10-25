// SPDX-License-Identifier: Apache-2.0
//! #217 comment6358242354 item1: independent helpers use one caller account.

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
    assert_eq!(a.completed_bytes().unwrap(), b"abc");
    let mut b = second.begin_expand(ExpandSpec::Exact(5)).unwrap();
    b.write(b"defgh").unwrap();
    assert_eq!(b.completed_bytes().unwrap(), b"defgh");
    let mut c = third.begin_expand(ExpandSpec::Exact(1)).unwrap();
    let refusal = resource(c.write(b"i").unwrap_err());
    assert_eq!(refusal.dimension, ResourceDimension::DecompressedBytes);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 8, 1));
    assert_eq!(first.resource_refusal(), Some(refusal));
    assert_eq!(second.resource_refusal(), Some(refusal));
    assert_eq!(session.finish(), Err(refusal));
}

#[test]
fn tracking_charges_the_account_body_once_even_when_the_origin_is_cold() {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let origin = DecodeContext::for_encode(&arena, &policy);
    let session = EncodeSession::from_context(&origin).unwrap();
    let first = DecodeContext::for_encode_session(&arena, &session).unwrap();
    let second = DecodeContext::for_encode_session(&arena, &session).unwrap();
    let slot = crate::decode::u64_from_index(size_of::<super::super::SessionSlot>());
    let body = crate::decode::u64_from_index(size_of::<crate::operation_budget::OperationBudget>());
    assert_eq!(
        origin.retained_bytes_available(),
        policy.limits.max_retained_bytes
    );
    first
        .budget
        .session(|tracking| tracking.activate())
        .unwrap();
    assert_eq!(
        origin.retained_bytes_available(),
        policy.limits.max_retained_bytes - slot - body
    );
    second
        .budget
        .session(|tracking| tracking.activate())
        .unwrap();
    origin
        .budget
        .session(|tracking| tracking.activate())
        .unwrap();
    assert_eq!(
        origin.retained_bytes_available(),
        policy.limits.max_retained_bytes - 3 * slot - body
    );
    first
        .budget
        .session(|tracking| tracking.activate())
        .unwrap();
    assert_eq!(
        second.retained_bytes_available(),
        policy.limits.max_retained_bytes - 3 * slot - body
    );
}

#[test]
fn attached_domains_keep_sources_and_capture_lifetimes_independent() {
    let session = {
        let arena = DecodeArena::new();
        let origin = DecodeContext::for_encode(&arena, &DecodePolicy::service());
        EncodeSession::from_context(&origin).unwrap()
    };
    let arena = DecodeArena::new();
    let a = DecodeContext::for_encode_session(&arena, &session).unwrap();
    let b = DecodeContext::for_encode_session(&arena, &session).unwrap();
    let ar = a
        .complete_root_input(&mut Cursor::new(b"a"), Vec::new())
        .unwrap();
    let br = b
        .complete_root_input(&mut Cursor::new(b"b"), Vec::new())
        .unwrap();
    assert!(a.require_source(br).is_err());
    assert!(b.require_source(ar).is_err());
    assert!(a.record("foreign", br, || Ok(())).is_err());
    a.record("same-id", ar, || {
        let mut source = ar;
        assert_eq!(source.u8(), Some(b'a'));
        Ok(())
    })
    .unwrap();
    b.record("same-id", br, || {
        let mut source = br;
        assert_eq!(source.u8(), Some(b'b'));
        Ok(())
    })
    .unwrap();
    let captures = a.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "same-id");
    assert!(a.take_record_captures().is_err());
    drop(a);
    b.require_source(br).unwrap();
    let captures = b.take_record_captures().unwrap();
    assert_eq!(captures.len(), 1);
    assert_eq!(captures[0].id(), "same-id");
    b.finish_session().unwrap();
    session.finish().unwrap();
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
    let (origin, root) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
    let session = EncodeSession::from_context(&origin).unwrap();
    let attached = DecodeContext::for_encode_session(&arena, &session).unwrap();
    assert_eq!(
        attached
            .read_input_prefix(&mut Cursor::new(b"defgh"), 5)
            .unwrap(),
        b"defgh"
    );
    origin.require_source(root).unwrap();
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
fn ordinary_independent_decodes_keep_separate_source_and_caller_accounts() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_input_bytes = 3;
    policy.limits.max_entities = 1;
    let (a, ar) = DecodeContext::from_root_bytes(b"abc", &arena, &policy).unwrap();
    let (b, br) = DecodeContext::from_root_bytes(b"def", &arena, &policy).unwrap();
    a.charge_entities(1, "independent first").unwrap();
    b.charge_entities(1, "independent second").unwrap();
    assert!(a.require_source(br).is_err());
    assert!(b.require_source(ar).is_err());
    let refusal = resource(a.charge_entities(1, "first decode overflow").unwrap_err());
    assert_eq!(refusal.dimension, ResourceDimension::Entities);
    assert_eq!(b.resource_refusal(), None);
    b.require_source(br).unwrap();
    b.finish_session().unwrap();
}
