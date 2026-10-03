// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::feature_projection::body_writing_unresolved_feature_definition;
use cadmpeg_ir::features::edge_treatments::RadiusSpec;
use cadmpeg_ir::features::EdgeSelection;
use cadmpeg_ir::features::FaceSelection;
use std::collections::BTreeMap;

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

#[test]
fn nx_body_writing_blend_retains_unresolved_fillet_family() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let definition = body_writing_unresolved_feature_definition(
        &cadmpeg_test_support::service_decode_context(),
        "BLEND",
        &source_properties,
    )
    .expect("body-writing projection admission");

    assert_eq!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::Fillet {
            groups: cadmpeg_ir::features::NonEmptyMembers::one(
                cadmpeg_ir::features::edge_treatments::FilletGroup {
                    edges: EdgeSelection::Unresolved,
                    radius: RadiusSpec::Unresolved { form: None },
                    tangency_weight: None,
                }
            ),
        }))
    );
    assert_eq!(definition.unwrap().body_output_family(), Some("fillet"));
}

#[test]
fn nx_non_body_writing_blend_remains_native_for_semantic_review() {
    let source_properties = BTreeMap::new();

    assert_eq!(
        body_writing_unresolved_feature_definition(
            &cadmpeg_test_support::service_decode_context(),
            "BLEND",
            &source_properties
        )
        .expect("body-writing projection admission"),
        None
    );
}

#[test]
fn nx_body_writing_face_blend_retains_unresolved_face_blend_family() {
    let mut source_properties = BTreeMap::new();
    source_properties.insert("body_write.0".to_string(), "witness".to_string());

    let definition = body_writing_unresolved_feature_definition(
        &cadmpeg_test_support::service_decode_context(),
        "FACE_BLEND",
        &source_properties,
    )
    .expect("body-writing projection admission");

    assert_eq!(
        definition,
        Some(FeatureDefinition::Operation(FeatureOperation::FaceBlend {
            operands: cadmpeg_ir::features::FaceBlendOperands::new(
                FaceSelection::Unresolved,
                FaceSelection::Unresolved,
                &cadmpeg_test_support::service_decode_context(),
            )
            .expect("operand admission")
            .unwrap(),

            radius: RadiusSpec::Unresolved { form: None },
        }))
    );
    assert_eq!(definition.unwrap().body_output_family(), Some("face blend"));
}

#[test]
fn nx_non_body_writing_face_blend_remains_native_for_semantic_review() {
    let source_properties = BTreeMap::new();

    assert_eq!(
        body_writing_unresolved_feature_definition(
            &cadmpeg_test_support::service_decode_context(),
            "FACE_BLEND",
            &source_properties
        )
        .expect("body-writing projection admission"),
        None
    );
}

#[test]
fn body_writing_projection_keeps_property_scan_refusals_outside_absence() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let source_properties = BTreeMap::from([("body_write.0".to_owned(), "witness".to_owned())]);
    for cap in [0, 1, 11] {
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error =
            body_writing_unresolved_feature_definition(&ctx, "FACE_BLEND", &source_properties)
                .unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("original resource refusal");
        };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(original)) if original == limit)
        );
    }
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 12;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 0;
    policy.limits.max_recursion_depth = 0;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(matches!(
        body_writing_unresolved_feature_definition(&ctx, "FACE_BLEND", &source_properties).unwrap(),
        Some(FeatureDefinition::Operation(
            FeatureOperation::FaceBlend { .. }
        ))
    ));
    ctx.finish_session()
        .expect("only the property visit and prefix comparison");
}
