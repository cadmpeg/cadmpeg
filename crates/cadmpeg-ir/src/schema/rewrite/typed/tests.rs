// SPDX-License-Identifier: Apache-2.0
use super::{IdentityMap, RewriteIdentities};
use crate::ids::PointId;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

#[test]
fn typed_rewrite_maps_repeated_identities_once() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let calls = std::cell::Cell::new(0);
    let mut map = IdentityMap::new(|source: &str| {
        calls.set(calls.get() + 1);
        Ok(format!(
            "test:occurrence:{}",
            source.strip_prefix("test:model:").unwrap()
        ))
    });
    let source = PointId::mint("test:model:point#one").unwrap();
    let rewritten = vec![source.clone(), source]
        .rewrite_identities(&ctx, &mut map)
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(rewritten[0].as_str(), "test:occurrence:point#one");
    assert_eq!(rewritten[0], rewritten[1]);
}

#[test]
fn typed_rewrite_preserves_refusal_before_sequence_recursion() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut map = IdentityMap::new(|source: &str| Ok(source.to_owned()));
    let error = vec![PointId::mint("test:model:point#one").unwrap()]
        .rewrite_identities(&ctx, &mut map)
        .unwrap_err();
    let CodecError::ResourceLimit(limit) = error else {
        panic!("depth must refuse");
    };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.limit, 0);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    drop(map);
    assert!(
        matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit)
    );
}

#[test]
fn typed_text_rewrite_only_changes_owned_identity_text() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let replacements = std::collections::BTreeMap::from([(
        "test:model:point#one".to_owned(),
        "test:occurrence:point#one".to_owned(),
    )]);
    let mut map = IdentityMap::new(|source: &str| Ok(source.to_owned()))
        .with_text_replacements(replacements.iter())
        .unwrap();
    let text = vec![
        "test:model:point#one".to_owned(),
        "test:model:point#external".to_owned(),
        "literal".to_owned(),
    ]
    .rewrite_identities(&ctx, &mut map)
    .unwrap();
    assert_eq!(
        text,
        [
            "test:occurrence:point#one",
            "test:model:point#external",
            "literal"
        ]
    );
    drop(map);
    ctx.finish_session().unwrap();
}

#[test]
fn typed_ordered_map_rewrite_accepts_keys_without_hashing() {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct OrderedKey(u8);
    impl RewriteIdentities for OrderedKey {
        fn visit_identity_references(
            &self,
            ctx: &DecodeContext<'_>,
            visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>,
        ) -> Result<(), CodecError> {
            self.0.visit_identity_references(ctx, visitor)
        }
        fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(
            self,
            ctx: &DecodeContext<'_>,
            map: &mut IdentityMap<'_, F>,
        ) -> Result<Self, CodecError> {
            self.0.rewrite_identities(ctx, map).map(Self)
        }
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let source = std::collections::BTreeMap::from([(OrderedKey(2), 4_u8), (OrderedKey(1), 3)]);
    let mut map = IdentityMap::new(|source: &str| Ok(source.to_owned()));
    let rewritten = source.rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(
        rewritten
            .into_iter()
            .map(|(key, value)| (key.0, value))
            .collect::<Vec<_>>(),
        [(1, 3), (2, 4)]
    );
    drop(map);
    ctx.finish_session().unwrap();
}

#[test]
fn typed_identity_rewrite_retains_its_grammar_proof_through_the_cache() {
    for source in ["a:b:c#one", "a:b:c#é:部"] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_recursion_depth = 0;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let calls = std::cell::Cell::new(0);
        let mut map = IdentityMap::new(|source: &str| {
            calls.set(calls.get() + 1);
            Ok(source.to_owned())
        });
        for _ in 0..2 {
            let identity = crate::ids::Identity::new(source).unwrap();
            assert_eq!(
                identity
                    .rewrite_identities(&ctx, &mut map)
                    .unwrap()
                    .as_str(),
                source
            );
        }
        assert_eq!(calls.get(), 1);
        assert!(map.targets.values().all(|target| target.as_str() == source));
        map.finish().unwrap();
        drop(map);
        ctx.finish_session().unwrap();
    }
}

#[test]
fn typed_rewrite_skips_identity_free_point_grids() {
    let rows = (0..4096)
        .map(|_| vec![crate::features::FinitePoint3::ZERO; 4])
        .collect::<Vec<_>>();
    let outer = rows.as_ptr();
    let inner = rows[0].as_ptr();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut map = IdentityMap::new(|_| panic!("no identity fields"));
    let rows = rows.rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(rows.as_ptr(), outer);
    assert_eq!(rows[0].as_ptr(), inner);
    assert_eq!(rows.len(), 4096);
    assert!(rows
        .iter()
        .flatten()
        .all(|point| *point == crate::features::FinitePoint3::ZERO));
    map.finish().unwrap();
    drop(map);
    ctx.finish_session().unwrap();
}

#[test]
fn typed_hash_index_qualifies_thousands_of_long_identities_and_rejects_collisions() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let prefix = "x".repeat(128);
    let mut map = IdentityMap::new(|source: &str| {
        Ok(format!(
            "test:qualified:point#{}",
            source.rsplit('#').next().unwrap()
        ))
    });
    for index in 0..4096 {
        let source = format!("test:model:point#{prefix}{index:04}");
        let expected = format!("test:qualified:point#{prefix}{index:04}");
        assert_eq!(map.identity(&source).unwrap(), expected);
        assert_eq!(map.identity(&source).unwrap(), expected);
    }
    assert!(
        matches!(map.identity(&format!("test:other:point#{prefix}0000")), Err(CodecError::Malformed(message)) if message.contains("collides"))
    );
    assert!(
        matches!(map.finish(), Err(CodecError::Malformed(message)) if message.contains("collides"))
    );
    drop(map);
    ctx.finish_session().unwrap();
}

#[test]
fn a_callback_resource_refusal_fuses_later_identity_and_text_rewrites() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let limit = cadmpeg_core::decode::ResourceLimit::allocation_failed(
        ResourceDimension::RetainedBytes,
        4096,
        64,
        "test identity callback",
    );
    let replacements = std::collections::BTreeMap::from([(
        "test:model:point#one".to_owned(),
        "test:occurrence:point#one".to_owned(),
    )]);
    let calls = std::cell::Cell::new(0);
    let mut map = IdentityMap::new(|_: &str| {
        calls.set(calls.get() + 1);
        Err(CodecError::ResourceLimit(limit))
    })
    .with_text_replacements(replacements.iter())
    .unwrap();
    assert!(matches!(
        PointId::mint("test:model:point#one")
            .unwrap()
            .rewrite_identities(&ctx, &mut map),
        Err(CodecError::ResourceLimit(first)) if first == limit
    ));
    assert!(matches!(
        PointId::mint("test:model:point#two")
            .unwrap()
            .rewrite_identities(&ctx, &mut map),
        Err(CodecError::ResourceLimit(first)) if first == limit
    ));
    assert_eq!(calls.get(), 1);
    assert!(matches!(
        "ordinary text".to_owned().rewrite_identities(&ctx, &mut map),
        Err(CodecError::ResourceLimit(first)) if first == limit
    ));
    assert!(matches!(map.finish(), Err(CodecError::ResourceLimit(first)) if first == limit));
}

#[test]
fn typed_rewrite_reuses_owned_sequence_storage() {
    let mut values = (0..4096)
        .map(|index| PointId::mint(format!("test:model:point#{index:04}")).unwrap())
        .collect::<Vec<_>>();
    values.try_reserve(128).unwrap();
    let pointer = values.as_ptr();
    let capacity = values.capacity();
    assert!(capacity > values.len());
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut map = IdentityMap::new(|source: &str| Ok(source.to_owned()));
    let values = values.rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(values.len(), 4096);
    assert_eq!(values.as_ptr(), pointer);
    assert_eq!(values.capacity(), capacity);
    for (index, value) in values.iter().enumerate() {
        assert_eq!(value.as_str(), format!("test:model:point#{index:04}"));
    }
    map.finish().unwrap();
}
