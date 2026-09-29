// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::body_writing_unresolved_feature_definition;
use crate::native::attach::brep_feature_definition;
use std::collections::BTreeMap;

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};
use cadmpeg_ir::ids::BodyId;

#[test]
fn nx_brep_projects_to_stored_geometry_only_with_unique_result_bodies() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let body = BodyId::mint("test:model:entity#body%231").expect("identity grammar");
    assert!(matches!(
        brep_feature_definition(&ctx, std::slice::from_ref(&body)).unwrap(),
        Some(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {}
        ))
    ));
    assert!(brep_feature_definition(&ctx, &[]).unwrap().is_none());
    assert!(brep_feature_definition(&ctx, &[body.clone(), body]).unwrap().is_none());
}

#[test]
fn nx_brep_output_uniqueness_refuses_work_limit() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let body_a = BodyId::mint("test:model:entity#body-a").unwrap();
    let body_b = BodyId::mint("test:model:entity#body-b").unwrap();
    let error = brep_feature_definition(&ctx, &[body_a, body_b]).unwrap_err();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits));
}

#[test]
fn nx_body_writing_brep_retains_unresolved_family() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    assert_eq!(
        body_writing_unresolved_feature_definition("BREP", &source_properties),
        Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::Brep
        }))
    );
}

#[test]
fn nx_non_body_writing_brep_remains_native_for_result_review() {
    assert_eq!(
        body_writing_unresolved_feature_definition("BREP", &BTreeMap::new()),
        None
    );
}
