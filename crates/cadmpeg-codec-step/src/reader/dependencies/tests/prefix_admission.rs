// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

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
fn dependency_decode_admits_the_first_record_before_visiting_it() {
    let exchange = exchange(8193);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = super::super::decode(&exchange, &ctx)
        .err().expect("first record visit refuses") else {
        panic!("original work refusal");
    };
    assert_eq!(limit.operation, "STEP decode traversal");
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn dependency_partial_refusal_leaves_the_record_suffix_unvisited() {
    let exchange = exchange(8193);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(limit) = super::super::decode(&exchange, &ctx)
        .err().expect("first partial lookup refuses") else {
        panic!("original work refusal");
    };
    assert_eq!(limit.operation, "STEP partial record search");
    assert_eq!((limit.used, limit.additional), (1, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn empty_dependency_decode_has_no_record_visits() {
    let exchange = exchange(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let outcome = super::super::decode(&exchange, &ctx).expect("empty record maps have no visits");
    assert!(outcome.claims.is_empty());
    assert!(outcome.losses.is_empty());
    assert!(outcome.notes.is_empty());
    drop(outcome);
    ctx.finish_session().expect("no exhausted record visit");
}

#[test]
fn empty_dependency_decode_preserves_the_original_fuse() {
    let exchange = exchange(0);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
    let CodecError::ResourceLimit(original) = ctx.charge_work(1, "dependency original fuse")
        .unwrap_err() else {
        panic!("original work refusal");
    };
    assert!(matches!(super::super::decode(&exchange, &ctx), Err(CodecError::ResourceLimit(limit)) if limit == original));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}
