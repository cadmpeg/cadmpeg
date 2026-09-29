// SPDX-License-Identifier: Apache-2.0

use super::emitted_entity_views;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::sketches::{SketchEntity, SketchEntityId, SketchGeometry, SketchId};

fn fixture() -> SketchEntity {
    let sketch = SketchId::mint("creo:model:sketch#1").expect("sketch id");
    let entity = SketchEntityId::mint("creo:model:sketch_entity#1").expect("entity id");
    SketchEntity::new(
        entity,
        sketch,
        SketchGeometry::native(NonBlankString::new("native").expect("kind")),
    )
}

fn views_with_policy(
    policy: &DecodePolicy,
) -> Result<usize, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)?;
    let (_, geometry) = emitted_entity_views(&ctx, &[fixture()])?;
    Ok(geometry.len())
}

#[test]
fn emitted_entity_views_refuse_each_tree_node() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let error = views_with_policy(&policy).expect_err("first node exceeds zero items");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo emitted sketch entity ID nodes"));
    policy.limits.max_collection_items = 1;
    let error = views_with_policy(&policy).expect_err("second node exceeds one item");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == "creo emitted sketch geometry nodes"));
    assert_eq!(views_with_policy(&DecodePolicy::service()).expect("service views"), 1);
}

#[test]
fn emitted_entity_views_refuse_nested_identity_and_geometry_copies() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = "creo:model:sketch_entity#1".len() as u64 - 1;
    let error = views_with_policy(&policy).expect_err("first identity copy exceeds cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo emitted sketch entity IDs"));
    policy.limits.max_retained_bytes = ("creo:model:sketch_entity#1".len() * 2 + "native".len() - 1) as u64;
    let error = views_with_policy(&policy).expect_err("native text exceeds remaining cap");
    assert!(matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::RetainedBytes
            && resource.operation == "creo emitted sketch geometry"));
    assert_eq!(views_with_policy(&DecodePolicy::service()).expect("service views"), 1);
}
