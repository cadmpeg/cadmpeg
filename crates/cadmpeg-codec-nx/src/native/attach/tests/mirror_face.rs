// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::feature_projection::body_writing_unresolved_feature_definition;
use std::collections::BTreeMap;

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, UnresolvedFamily};

#[test]
fn nx_body_writing_mirror_face_retains_unresolved_family() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let definition = body_writing_unresolved_feature_definition(&cadmpeg_test_support::service_decode_context(), "MIRROR_FACE", &source_properties).expect("body-writing projection admission");

    assert_eq!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::Unresolved {
            family: UnresolvedFamily::MirrorFace
        }))
    );
    assert_eq!(
        definition.unwrap().body_output_family(),
        Some("mirror face")
    );
}

#[test]
fn nx_non_body_writing_mirror_face_remains_native_for_semantic_review() {
    assert_eq!(
        body_writing_unresolved_feature_definition(&cadmpeg_test_support::service_decode_context(), "MIRROR_FACE", &BTreeMap::new()).expect("body-writing projection admission"),
        None
    );
}
