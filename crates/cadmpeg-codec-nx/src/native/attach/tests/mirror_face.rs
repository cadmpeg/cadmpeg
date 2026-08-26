// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn nx_body_writing_mirror_face_retains_unresolved_family() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let definition =
        super::body_writing_unresolved_feature_definition("MIRROR_FACE", &source_properties);

    assert_eq!(definition, Some(FeatureDefinition::MirrorFaceUnresolved));
    assert_eq!(
        definition.unwrap().body_output_family(),
        Some("mirror face")
    );
}

#[test]
fn nx_non_body_writing_mirror_face_remains_native_for_semantic_review() {
    assert_eq!(
        super::body_writing_unresolved_feature_definition("MIRROR_FACE", &BTreeMap::new()),
        None
    );
}
