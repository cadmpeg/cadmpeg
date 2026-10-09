// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::{BTreeSet, HashSet};

use crate::parse::Value;

fn exchange(count: usize) -> crate::parse::Exchange {
    let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;");
    for id in 1..=count {
        use std::fmt::Write;
        write!(source, "#{id}=ITEM();").unwrap();
    }
    source.push_str("ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .unwrap()
        .0
}

#[test]
fn reference_collection_refusal_leaves_the_list_suffix_unvisited() {
    let value = Value::List(vec![Value::Reference(1); 8193]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One list visit precedes the empty B-tree insertion. Core admits three
    // node passes for the insertion: eleven u64 keys, sixteen pointer lanes,
    // and two alignment lanes per node. This admits the first insertion's
    // work but cannot admit a traversal of the whole list before that insertion.
    let node_bytes = 11 * std::mem::size_of::<u64>()
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<u64>().max(std::mem::align_of::<usize>());
    policy.limits.max_work_units = 1 + 3 * node_bytes as u64;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut output = BTreeSet::new();
    let CodecError::ResourceLimit(limit) = super::super::collect_references(&value, &mut output, &ctx)
        .expect_err("first insertion refuses") else {
        panic!("original collection refusal");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "step_reference_walk_ids");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert!(output.is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn second_reference_list_visit_preserves_the_original_refusal() {
    let value = Value::List(vec![Value::Omitted; 8193]);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut output = BTreeSet::new();
    let CodecError::ResourceLimit(limit) = super::super::collect_references(&value, &mut output, &ctx)
        .expect_err("second list visit refuses") else {
        panic!("original work refusal");
    };
    assert_eq!(limit.operation, "STEP collect references value traversal");
    assert_eq!((limit.used, limit.additional), (1, 1));
    assert!(output.is_empty());
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn empty_reference_list_has_no_visits() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let mut output = BTreeSet::new();
    super::super::collect_references(&Value::List(Vec::new()), &mut output, &ctx).unwrap();
    assert!(output.is_empty());
    ctx.finish_session().expect("empty source has no visit");
}

#[test]
fn empty_reference_list_preserves_the_original_fuse() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "orchestration original fuse").unwrap_err() else {
        panic!("original work refusal");
    };
    assert!(matches!(super::super::collect_references(&Value::List(Vec::new()), &mut BTreeSet::new(), &ctx), Err(CodecError::ResourceLimit(limit)) if limit == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn opaque_offset_inspection_admits_only_the_first_visit() {
    let exchange = exchange(8193);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = super::super::inspect_opaque_offsets(&exchange, &HashSet::new(), &ctx)
        .expect_err("first record visit refuses") else {
        panic!("original work refusal");
    };
    assert_eq!(limit.operation, "STEP inspect opaque offsets traversal");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn referenced_record_index_admits_only_the_first_visit() {
    let exchange = exchange(8193);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = super::super::referenced_record_ids(&exchange, &ctx)
        .expect_err("first record visit refuses") else {
        panic!("original work refusal");
    };
    assert_eq!(limit.operation, "STEP referenced record ids map traversal");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn record_closure_refuses_before_the_pending_pop() {
    let exchange = exchange(1);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // One root is copied into fresh preallocated pending backing. The next
    // executed worklist pop requires one additional admission.
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = super::super::record_closure(&BTreeSet::from([1]), &exchange, &ctx)
        .expect_err("the pending pop is not admitted") else {
        panic!("original work refusal");
    };
    assert_eq!(limit.operation, "STEP mod worklist step");
    assert_eq!((limit.used, limit.additional), (1, 1));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}
