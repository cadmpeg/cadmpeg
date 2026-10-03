// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn replacement_lookup_preserves_each_comparison_refusal_and_empty_fuse() {
    let fixture = cadmpeg_test_support::service_decode_context();
    let replacements = std::collections::BTreeMap::from([
        ("alpha".to_owned(), "A".to_owned()),
        ("beta".to_owned(), "B".to_owned()),
        ("gamma".to_owned(), "C".to_owned()),
    ]);
    let mut storage = fixture.reserve_scoped_limit(0, "fixture replacement index").unwrap();
    let index = super::ReplacementIndex::build(&fixture, &replacements, &mut storage, "fixture replacement index").unwrap();
    // beta differs at its first byte; gamma compares five equal bytes.
    for allowance in 0..=8 {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = allowance;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = index.get(&ctx, "gamma", "actual replacement lookup");
        if allowance < 8 {
            let original = result.unwrap_err();
            assert_eq!(original.dimension, ResourceDimension::WorkUnits);
            assert_eq!(original.operation, "actual replacement lookup");
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
        } else {
            assert_eq!(result.unwrap(), Some("C"));
            ctx.finish_session().unwrap();
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut empty_storage = ctx.reserve_scoped_limit(0, "empty replacement index").unwrap();
    let empty = std::collections::BTreeMap::<String, String>::new();
    let empty_index = super::ReplacementIndex::build(&ctx, &empty, &mut empty_storage, "empty replacement index").unwrap();
    assert_eq!(empty_index.get(&ctx, "absent", "empty replacement lookup").unwrap(), None);
    let original = ctx.charge_work_limit(1, "original empty replacement refusal").unwrap_err();
    assert_eq!(empty_index.get(&ctx, "absent", "empty replacement lookup").unwrap_err(), original);
    assert!(matches!(super::ReplacementIndex::build(&ctx, &empty, &mut empty_storage, "empty replacement index"), Err(CodecError::ResourceLimit(limit)) if limit == original));
    drop(empty_index);
    drop(empty_storage);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
}

#[test]
fn replacement_index_admits_storage_and_preserves_byte_order() {
    let replacements = std::collections::BTreeMap::from([
        ("é".to_owned(), "third".to_owned()),
        ("alpha".to_owned(), "first".to_owned()),
        ("beta".to_owned(), "second".to_owned()),
    ]);
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems, ResourceDimension::WorkUnits] {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = 0,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 0,
            ResourceDimension::WorkUnits => policy.limits.max_work_units = 0,
            _ => unreachable!(),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut storage = ctx.reserve_scoped_limit(0, "replacement storage").unwrap();
        let CodecError::ResourceLimit(original) = super::ReplacementIndex::build(&ctx, &replacements, &mut storage, "replacement storage").unwrap_err() else { panic!("index must refuse"); };
        assert_eq!(original.dimension, dimension);
        drop(storage);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == original));
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 4096;
    policy.limits.max_collection_items = 3;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped_limit(0, "replacement storage").unwrap();
    let index = super::ReplacementIndex::build(&ctx, &replacements, &mut storage, "replacement storage").unwrap();
    assert_eq!(index.values, [("alpha", "first"), ("beta", "second"), ("é", "third")]);
    assert_eq!(index.get(&ctx, "alpha", "replacement lookup").unwrap(), Some("first"));
    assert_eq!(index.get(&ctx, "é", "replacement lookup").unwrap(), Some("third"));
    assert_eq!(index.get(&ctx, "missing", "replacement lookup").unwrap(), None);
    drop(index);
    drop(storage);
    drop(ctx.reserve_scoped_limit(4096, "replacement storage released").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn replacement_index_checks_exposed_key_order_and_duplicates() {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct TextKey(u8, String);
    impl AsRef<str> for TextKey {
        fn as_ref(&self) -> &str { &self.1 }
    }
    for texts in [["later", "earlier"], ["same", "same"]] {
        let replacements = std::collections::BTreeMap::from([
            (TextKey(0, texts[0].to_owned()), "first".to_owned()),
            (TextKey(1, texts[1].to_owned()), "second".to_owned()),
        ]);
        let ctx = cadmpeg_test_support::service_decode_context();
        let mut storage = ctx.reserve_scoped_limit(0, "replacement order").unwrap();
        assert!(matches!(super::ReplacementIndex::build(&ctx, &replacements, &mut storage, "replacement order"), Err(CodecError::Malformed(message)) if message == "text replacements must have unique keys in byte order"));
        drop(storage);
        ctx.finish_session().unwrap();
    }
}
