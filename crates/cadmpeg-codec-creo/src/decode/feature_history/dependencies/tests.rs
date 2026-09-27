// SPDX-License-Identifier: Apache-2.0

use super::add_surface_prototype_feature_dependencies;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn dependency_result(limit: u64) -> Result<BTreeMap<u32, Vec<u32>>, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("root input is admitted");
    let mut dependencies = BTreeMap::new();
    add_surface_prototype_feature_dependencies(&ctx, &mut dependencies, 40, &[286])?;
    Ok(dependencies)
}

#[test]
fn prototype_dependency_consumer_node_refuses_before_insertion() {
    assert_eq!(
        dependency_result(2).expect("service limit admits the consumer and producer"),
        BTreeMap::from([(286, vec![40])])
    );
    let error = dependency_result(0).expect_err("one consumer requires a map node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo prototype dependency consumers"
    ));
}

#[test]
fn prototype_dependency_producer_vec_refuses_before_growth() {
    let error = dependency_result(1).expect_err("producer follows its consumer node");
    assert!(matches!(
        error,
        CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo prototype dependency producers"
    ));
}
