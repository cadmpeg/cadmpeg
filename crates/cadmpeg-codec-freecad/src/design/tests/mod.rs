// SPDX-License-Identifier: Apache-2.0
//! Design-history unit tests over synthesized `FCStd` archives.

pub(crate) mod booleans_patterns;
pub(crate) mod construction;
mod history;
pub(crate) mod holes_extrude;
pub(crate) mod primitives;
pub(crate) mod sketches;
mod taper;

use cadmpeg_ir::features::FeatureDefinition;

#[test]
fn design_ordered_objects_refuse_at_caller_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Body".into(),
        name: "Body".into(),
        type_name: "PartDesign::Body".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::feature_ordinals(
        &ctx, &[object], &Default::default(), &Default::default(), &Default::default(),
    ), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "fcstd design ordered objects"));
}

#[test]
fn design_expression_copy_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "expression-property".into(),
        owner: "owner".into(),
        name: "ExpressionEngine".into(),
        type_name: "App::PropertyExpressionEngine".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: vec![crate::native::ValueRecord {
                tag: "Expression".into(),
                order: 0,
                attributes: [
                    ("path".into(), "Length".into()),
                    ("expression".into(), "Sheet.A1".into()),
                ].into(),
                text: None,
                raw_xml: String::new(),
            }],
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is within policy");
    assert!(matches!(super::expression_binding(&ctx, &[&property], "Length"),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "fcstd expression engine reference"));
}

/// Feature definition selected by exact feature name.
fn definition<'a>(
    result: &'a cadmpeg_ir::codec::DecodeResult,
    name: &str,
) -> &'a FeatureDefinition {
    result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing {name}"))
        .evaluation
        .definition()
}

/// Definition of the single `Extrusion` feature.
fn extrusion_definition(result: &cadmpeg_ir::codec::DecodeResult) -> &FeatureDefinition {
    result
        .ir()
        .model
        .features
        .iter()
        .find(|feature| feature.name.as_deref() == Some("Extrusion"))
        .expect("extrusion feature")
        .evaluation
        .definition()
}
