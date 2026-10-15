// SPDX-License-Identifier: Apache-2.0

use std::collections::{HashMap, HashSet};

#[test]
fn collect_owned_ids_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use serde_value::Value;

    let value = Value::Map(std::collections::BTreeMap::from([(
        Value::String("id".into()),
        Value::String("f3d:brep:entity#1".into()),
    )]));
    let mut owned = HashSet::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::collect_owned_ids(&ctx, &value, &mut owned)
        .expect_err("one owned id exceeds zero items");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected collection refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
}

#[test]
fn collect_references_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use serde_value::Value;

    let value = Value::String("f3d:brep:entity#1".into());
    let owned = HashSet::from(["f3d:brep:entity#1".into()]);
    let mut references = std::collections::BTreeSet::new();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::collect_references(&ctx, &value, &owned, &mut references)
        .expect_err("one reference exceeds zero items");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected collection refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
}

#[test]
fn collect_entity_adjacency_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use serde_value::Value;

    let entity = Value::Map(std::collections::BTreeMap::from([
        (Value::String("id".into()), Value::String("owner".into())),
        (
            Value::String("peer".into()),
            Value::String("referenced".into()),
        ),
    ]));
    let value = Value::Map(std::collections::BTreeMap::from([(
        Value::String("bodies".into()),
        Value::Seq(vec![entity]),
    )]));
    let owned = HashSet::from(["referenced".into()]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::CollectionItems,
        "ASM adjacency owners",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            super::super::collect_entity_adjacency(&ctx, &value, &owned, &mut HashMap::new())
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected collection refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(limit.operation, "ASM adjacency owners");
}

#[test]
fn remap_owned_ids_refuses_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use serde_value::Value;

    let mut value = Value::Map(std::collections::BTreeMap::from([(
        Value::String("field".into()),
        Value::I64(1),
    )]));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::remap_owned_ids(&ctx, &mut value, &HashMap::new())
        .expect_err("one remapped field exceeds zero items");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected collection refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
}
