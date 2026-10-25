// SPDX-License-Identifier: Apache-2.0
//! Independent source contexts keep one encode resource allowance.

use crate::decode::{DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceDimension};
use crate::encode::EncodeSession;
use crate::CodecError;

#[test]
fn independent_contexts_share_eight_retained_bytes_in_both_orders() {
    for parent_first in [true, false] {
        let parent_arena = DecodeArena::new();
        let child_arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 8;
        let parent = DecodeContext::new(&parent_arena, &policy, DecodeDepth::Full);
        let (child, _) = DecodeContext::from_root_bytes_shared(&[], &child_arena, &parent)
            .expect("a distinct source context shares the caller account");
        let (first, second) = if parent_first {
            (&parent, &child)
        } else {
            (&child, &parent)
        };
        first.retain_bytes(6, "first actual six bytes").unwrap();
        let CodecError::ResourceLimit(refusal) = second
            .retain_bytes(6, "second actual six bytes")
            .unwrap_err()
        else {
            panic!("aggregate retained ceiling must refuse");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 6, 6));
        assert!(matches!(first.retain_bytes(0, "persistent parent refusal"),
            Err(CodecError::ResourceLimit(actual)) if actual == refusal));
        assert_eq!(EncodeSession::from_context(&child).unwrap_err(), refusal);
        assert_eq!(
            DecodeContext::from_root_bytes_shared(&[], &child_arena, &parent).unwrap_err(),
            refusal
        );
    }
}

#[test]
fn child_keeps_entity_account_and_fatal_refusal_after_parent_drop() {
    let parent_arena = DecodeArena::new();
    let child_arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_entities = 3;
    let parent = DecodeContext::new(&parent_arena, &policy, DecodeDepth::Full);
    let (child, _) = DecodeContext::from_root_bytes_shared(&[], &child_arena, &parent).unwrap();
    parent
        .charge_entities(2, "two actual parent records")
        .unwrap();
    drop(parent);
    child.charge_entities(1, "actual child record").unwrap();
    let CodecError::ResourceLimit(refusal) = child
        .charge_entities(1, "fourth actual record")
        .unwrap_err()
    else {
        panic!("aggregate entity ceiling must refuse");
    };
    assert_eq!(refusal.dimension, ResourceDimension::Entities);
    assert_eq!((refusal.limit, refusal.used, refusal.additional), (3, 3, 1));
    assert!(matches!(child.finish_session(),
        Err(CodecError::ResourceLimit(actual)) if actual == refusal));
}

#[test]
fn shared_account_does_not_share_source_authority_or_stream_names() {
    let parent_arena = DecodeArena::new();
    let child_arena = DecodeArena::new();
    let (parent, parent_root) =
        DecodeContext::from_root_bytes(b"ab", &parent_arena, &DecodePolicy::service()).unwrap();
    let (child, child_root) =
        DecodeContext::from_root_bytes_shared(b"cd", &child_arena, &parent).unwrap();
    let parent_root = parent.name_stream(parent_root, "parent").unwrap();
    let child_root = child.name_stream(child_root, "child").unwrap();
    assert!(parent.require_source(child_root).is_err());
    assert!(child.require_source(parent_root).is_err());
    parent
        .record("parent-row", parent_root, || Ok(parent_root.read_at(0, 1)))
        .unwrap();
    child
        .record("child-row", child_root, || Ok(child_root.read_at(1, 1)))
        .unwrap();
    let parent_rows = parent.take_record_captures().unwrap();
    let child_rows = child.take_record_captures().unwrap();
    assert_eq!(parent_rows.len(), 1);
    assert_eq!(child_rows.len(), 1);
    assert_eq!(parent_rows[0].stream(), "parent");
    assert_eq!(child_rows[0].stream(), "child");
    assert_eq!(parent_rows[0].reads()["parent"], [0..1]);
    assert_eq!(child_rows[0].reads()["child"], [1..2]);
}

#[test]
fn expansions_in_independent_contexts_share_the_cumulative_output_ceiling() {
    use crate::decode::{ExpandSpec, LimitScope};
    for exact_second in [false, true] {
        let parent_arena = DecodeArena::new();
        let child_arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_decompressed_bytes_total = 8;
        policy.limits.max_decompressed_bytes_per_expand = 8;
        let (parent, _) = DecodeContext::from_root_bytes(b"a", &parent_arena, &policy).unwrap();
        let (child, _) =
            DecodeContext::from_root_bytes_shared(b"b", &child_arena, &parent).unwrap();
        let mut first = parent.begin_expand(ExpandSpec::Exact(6)).unwrap();
        first.write(b"native").unwrap();
        assert_eq!(first.finalize().unwrap().window(), b"native");
        let error = if exact_second {
            child.begin_expand(ExpandSpec::Exact(6)).err().unwrap()
        } else {
            let mut second = child.begin_expand(ExpandSpec::Unknown).unwrap();
            second.write(b"output").unwrap_err()
        };
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("shared expansion ceiling")
        };
        assert_eq!(refusal.scope, LimitScope::Session);
        assert_eq!(refusal.dimension, ResourceDimension::DecompressedBytes);
        assert_eq!((refusal.limit, refusal.used, refusal.additional), (8, 6, 6));
        assert_eq!(parent.resource_refusal(), Some(refusal));
        assert_eq!(child.resource_refusal(), Some(refusal));
    }
}
