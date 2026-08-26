// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use cadmpeg_ir::features::FeatureDefinition;

use super::*;

#[test]
fn nx_body_writing_blend_retains_unresolved_fillet_family() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let definition = super::body_writing_unresolved_feature_definition("BLEND", &source_properties);

    assert_eq!(
        definition,
        Some(FeatureDefinition::Fillet {
            groups: vec![cadmpeg_ir::features::FilletGroup {
                edges: EdgeSelection::Unresolved,
                radius: RadiusSpec::Unresolved { form: None },
                tangency_weight: None,
            }],
        })
    );
    assert_eq!(definition.unwrap().body_output_family(), Some("fillet"));
}

#[test]
fn nx_non_body_writing_blend_remains_native_for_semantic_review() {
    let source_properties = BTreeMap::new();

    assert_eq!(
        super::body_writing_unresolved_feature_definition("BLEND", &source_properties),
        None
    );
}

#[test]
fn nx_body_writing_face_blend_retains_unresolved_face_blend_family() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let definition =
        super::body_writing_unresolved_feature_definition("FACE_BLEND", &source_properties);

    assert_eq!(
        definition,
        Some(FeatureDefinition::FaceBlend {
            first_faces: FaceSelection::Unresolved,
            second_faces: FaceSelection::Unresolved,
            radius: RadiusSpec::Unresolved { form: None },
        })
    );
    assert_eq!(definition.unwrap().body_output_family(), Some("face blend"));
}

#[test]
fn nx_non_body_writing_face_blend_remains_native_for_semantic_review() {
    let source_properties = BTreeMap::new();

    assert_eq!(
        super::body_writing_unresolved_feature_definition("FACE_BLEND", &source_properties),
        None
    );
}
