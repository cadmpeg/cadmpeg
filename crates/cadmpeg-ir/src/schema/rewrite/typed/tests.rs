// SPDX-License-Identifier: Apache-2.0
use super::{IdentityMap, RewriteIdentities};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::ids::PointId;

#[test]
fn typed_rewrite_maps_repeated_identities_once() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let calls = std::cell::Cell::new(0);
    let mut map = IdentityMap::new(&ctx, "test identity rewrite", |source: &str| {
        calls.set(calls.get() + 1);
        ctx.format_retained(format_args!("test:occurrence:{}", source.strip_prefix("test:model:").unwrap()), "test identity target")
    }).unwrap();
    let source = PointId::mint("test:model:point#one").unwrap();
    let rewritten = vec![source.clone(), source].rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(rewritten[0].as_str(), "test:occurrence:point#one");
    assert_eq!(rewritten[0], rewritten[1]);
}

#[test]
fn typed_rewrite_preserves_refusal_before_cache_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut map = IdentityMap::new(&ctx, "test identity rewrite", |source: &str| ctx.copy_retained_text(source, "test target")).unwrap();
    let error = PointId::mint("test:model:point#one").unwrap().rewrite_identities(&ctx, &mut map).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("cache storage must refuse"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    drop(map);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn typed_rewrite_preserves_refusal_before_sequence_recursion() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut map = IdentityMap::new(&ctx, "test identity rewrite", |source: &str| ctx.copy_retained_text(source, "test target")).unwrap();
    let error = vec![PointId::mint("test:model:point#one").unwrap()].rewrite_identities(&ctx, &mut map).unwrap_err();
    let CodecError::ResourceLimit(limit) = error else { panic!("depth must refuse"); };
    assert_eq!(limit.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(limit.limit, 0);
    assert_eq!(limit.used, 0);
    assert_eq!(limit.additional, 1);
    drop(map);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(original)) if original == limit));
}

#[test]
fn typed_text_rewrite_only_changes_owned_identity_text() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    let replacements = std::collections::BTreeMap::from([("test:model:point#one".to_owned(), "test:occurrence:point#one".to_owned())]);
    let mut map = IdentityMap::new(&ctx, "test text identities", |source: &str| ctx.copy_retained_text(source, "test unchanged identity")).unwrap().with_text_replacements(&replacements);
    let text = vec!["test:model:point#one".to_owned(), "test:model:point#external".to_owned(), "literal".to_owned()].rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(text, ["test:occurrence:point#one", "test:model:point#external", "literal"]);
    drop(map);
    ctx.finish_session().unwrap();
}

#[test]
fn typed_text_rewrite_preserves_its_first_refusal() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let replacements = std::collections::BTreeMap::from([("test:model:point#one".to_owned(), "test:occurrence:point#one".to_owned())]);
    let mut map = IdentityMap::new(&ctx, "test text identities", |source: &str| ctx.copy_retained_text(source, "test identity")).unwrap().with_text_replacements(&replacements);
    let CodecError::ResourceLimit(first) = "test:model:point#one".to_owned().rewrite_identities(&ctx, &mut map).unwrap_err() else { panic!("text replacement must refuse"); };
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(first.additional, "test:occurrence:point#one".len() as u64);
    assert!(matches!("ordinary text".to_owned().rewrite_identities(&ctx, &mut map), Err(CodecError::ResourceLimit(limit)) if limit == first));
    assert!(matches!(map.finish(&ctx), Err(CodecError::ResourceLimit(limit)) if limit == first));
    drop(map);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(limit)) if limit == first));
}


#[test]
fn typed_ordered_map_rewrite_accepts_keys_without_hashing() {
    #[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
    struct OrderedKey(u8);
    impl RewriteIdentities for OrderedKey {
        fn visit_identity_references(&self, ctx: &DecodeContext<'_>, visitor: &mut dyn FnMut(&str) -> Result<(), CodecError>) -> Result<(), CodecError> {
            self.0.visit_identity_references(ctx, visitor)
        }
        fn rewrite_identities<F: FnMut(&str) -> Result<String, CodecError>>(self, ctx: &DecodeContext<'_>, map: &mut IdentityMap<'_, F>) -> Result<Self, CodecError> {
            self.0.rewrite_identities(ctx, map).map(Self)
        }
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let source = std::collections::BTreeMap::from([(OrderedKey(2), 4_u8), (OrderedKey(1), 3)]);
    let mut map = IdentityMap::new(&ctx, "ordered map rewrite", |source: &str| ctx.copy_retained_text(source, "map identity")).unwrap();
    let rewritten = source.rewrite_identities(&ctx, &mut map).unwrap();
    assert_eq!(rewritten.into_iter().map(|(key, value)| (key.0, value)).collect::<Vec<_>>(), [(1, 3), (2, 4)]);
    drop(map);
    ctx.finish_session().unwrap();
}
