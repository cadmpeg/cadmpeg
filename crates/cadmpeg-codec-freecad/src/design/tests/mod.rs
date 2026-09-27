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
fn design_native_parameter_value_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "native-property".into(),
        owner: "feature".into(),
        name: "ProxyState".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><String value=\"native-value\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd native parameter value",
        |ctx| super::native_definition(ctx, "Part::FeaturePython", &[&property]),
    );
}

#[test]
fn design_feature_state_value_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "state-property".into(),
        owner: "feature".into(),
        name: "Visibility".into(),
        type_name: "App::PropertyBool".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Bool value=\"true\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd feature state value",
        |ctx| super::feature_state(ctx, "feature", &[&property]),
    );
}

#[test]
fn design_operation_scalar_expression_refuses_at_retained_limit() {
    let object = crate::native::ObjectRecord {
        id: "fcstd:native:object#Box".into(),
        name: "Box".into(),
        type_name: "PartDesign::AdditiveBox".into(),
        persistent_id: None,
        view_type: None,
        attributes: Default::default(),
        dependencies: Vec::new(),
        dependency_allow_partial: None,
        order: 0,
        data: None,
    };
    let property = crate::native::PropertyRecord {
        id: "length-property".into(),
        owner: object.id.clone(),
        name: "Length".into(),
        type_name: "App::PropertyLength".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><Float value=\"3.5\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd operation scalar expression",
        |ctx| super::append_operation_parameters(ctx, &mut Vec::new(), &object, &[&property]),
    );
}

#[test]
fn design_string_property_value_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "maker-property".into(),
        owner: "feature".into(),
        name: "FaceMakerClass".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><String value=\"Part::FaceMakerBullseye\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd string property value",
        |ctx| super::string_property_value(ctx, &property),
    );
}

#[test]
fn design_numeric_list_refuses_at_collection_limit() {
    let property = crate::native::PropertyRecord {
        id: "numeric-list".into(),
        owner: "pattern".into(),
        name: "Spacings".into(),
        type_name: "App::PropertyFloatList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["numbers.bin".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><FloatList file=\"numbers.bin\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    let mut data = 1_u32.to_le_bytes().to_vec();
    data.extend(2.5_f64.to_le_bytes());
    let entry = crate::native::EntryRecord {
        id: "entry".into(),
        name: "numbers.bin".into(),
        role: cadmpeg_core::container::ContainerRole::Auxiliary,
        referenced_by: Vec::new(),
        data,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::numeric_list(&ctx, &property, &[entry]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "fcstd numeric-list values"));
}

#[test]
fn design_vector_list_refuses_at_collection_limit() {
    let property = crate::native::PropertyRecord {
        id: "vector-list".into(),
        owner: "polygon".into(),
        name: "Nodes".into(),
        type_name: "App::PropertyVectorList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: Vec::new(),
            side_entries: vec!["vectors.bin".into()],
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text(
            "<Property><VectorList file=\"vectors.bin\"/></Property>".into(), 0,
        ).expect("valid XML span"),
    };
    let mut data = 1_u32.to_le_bytes().to_vec();
    for component in [1.0_f64, 2.0, 3.0] {
        data.extend(component.to_le_bytes());
    }
    let entry = crate::native::EntryRecord {
        id: "entry".into(),
        name: "vectors.bin".into(),
        role: cadmpeg_core::container::ContainerRole::Auxiliary,
        referenced_by: Vec::new(),
        data,
    };
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::vector_list_property(&ctx, &[&property], "Nodes", &[entry]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "fcstd vector-list points"));
}

#[test]
fn design_body_output_prefix_refuses_at_retained_limit() {
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
    let property = crate::native::PropertyRecord {
        id: "fcstd:native:property#Body:Shape".into(),
        owner: object.id.clone(),
        name: "Shape".into(),
        type_name: "Part::PropertyPartShape".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let payload = crate::brep::ShapePayloadRecord {
        id: "fcstd:native:shape-payload#Body:Shape".into(),
        property: property.id.clone(),
        entry: "shape.brp".into(),
        payload: crate::brep::ShapePayload::Empty,
    };
    crate::test_support::assert_retained_refusal_at(&[], "fcstd design body output prefix", |ctx| {
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        super::transfer(ctx, &mut ir, &[object.clone()], &[property.clone()], &[payload.clone()], &[], None)
    });
}

#[test]
fn sketch_placement_error_refuses_at_retained_limit() {
    let property = crate::native::PropertyRecord {
        id: "placement-property".into(),
        owner: "sketch".into(),
        name: "Placement".into(),
        type_name: "App::PropertyString".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Transient,
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let expected = "sketch Placement placement carrier has runtime type App::PropertyString";
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::validate_sketch_placement(&ctx, &[&property]),
        Err(cadmpeg_core::CodecError::Malformed(message)) if message == expected));

    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::default();
    policy.limits.max_retained_bytes = u64::try_from(expected.len() - 1).expect("message length fits");
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root");
    assert!(matches!(super::validate_sketch_placement(&ctx, &[&property]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD sketch placement error"));
    assert!(matches!(super::sketch_frame(&ctx, &[&property]),
        Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "FreeCAD sketch placement error"));
}

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
        &ctx, &[object], &Default::default(), &Default::default(),
    ), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "fcstd design ordered objects"));
}

#[test]
fn design_body_member_identity_refuses_at_retained_limit() {
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some("child-object".into()),
        subelements: Vec::new(),
    }).expect("valid link");
    let property = crate::native::PropertyRecord {
        id: "group-property".into(),
        owner: "body".into(),
        name: "Group".into(),
        type_name: "App::PropertyLinkList".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: vec![link],
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let mut feature_ids = std::collections::HashMap::new();
    feature_ids.insert("child-object",
        cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Child")
            .expect("valid feature identity"));
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd body member feature identity",
        |ctx| super::body_definition(ctx, &[&property], &feature_ids),
    );
}

#[test]
fn design_body_tip_identity_refuses_at_retained_limit() {
    let link = crate::native::LinkTarget::optional_from_wire(crate::native::LinkTargetWire {
        document: None,
        document_attribute: None,
        object: Some("child-object".into()),
        subelements: Vec::new(),
    }).expect("valid link");
    let property = crate::native::PropertyRecord {
        id: "tip-property".into(),
        owner: "body".into(),
        name: "Tip".into(),
        type_name: "App::PropertyLink".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        body: crate::native::PropertyBody::Persisted {
            values: Vec::new(),
            links: vec![link],
            side_entries: Vec::new(),
            dynamic: None,
        },
        order: 0,
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0)
            .expect("valid XML span"),
    };
    let mut feature_ids = std::collections::HashMap::new();
    feature_ids.insert("child-object",
        cadmpeg_ir::features::FeatureId::mint("fcstd:design:feature#Child")
            .expect("valid feature identity"));
    crate::test_support::assert_retained_refusal_at(
        &[], "fcstd body tip feature identity",
        |ctx| super::body_definition(ctx, &[&property], &feature_ids),
    );
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
