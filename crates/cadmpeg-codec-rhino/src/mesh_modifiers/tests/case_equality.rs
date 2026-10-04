// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::chunks::FramingError;

#[test]
fn xml_attribute_name_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root TYPE="bool"/>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino XML attribute name case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::attribute(&ctx, root, "type").map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_node_name_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root/>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino XML node name case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::same_name(&ctx, root, "ROOT").map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_boolean_text_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<TRUE/>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino parse bool text case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::parse_bool_text(&ctx, root.tag_name().name()).map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_typed_boolean_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value type="bool">true</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino field bool case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_bool(&ctx, root, "value", false).map(|_| ()).map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid field returned {other:?}"),
            });
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_typed_integer_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value type="string">true</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino field i32 optional case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_i32_optional(&ctx, root, "value").map(|_| ()).map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid field returned {other:?}"),
            });
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_typed_real_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value type="bool">true</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino field f64 case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_f64(&ctx, root, "value", 0.0).map(|_| ()).map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid field returned {other:?}"),
            });
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_typed_uuid_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value type="uuid">12345678-1234-5678-90ab-cdef01234567</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino field uuid case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_uuid(&ctx, root, "value").map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_untyped_boolean_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value>true</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino field bool untyped case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_bool_untyped(&ctx, root, "value", false).map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_untyped_integer_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value>true</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino field i32 untyped case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_i32_untyped(&ctx, root, "value", 0).map(|_| ()).map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid field returned {other:?}"),
            });
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_cap_type_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value type="string">flat</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino field cap type case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_cap_type(&ctx, root, "value").map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_untyped_real_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value>1.0</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino XML node name case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_f64_untyped(&ctx, root, "value", cadmpeg_ir::scalar::FiniteReal::ONE).map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn xml_untyped_uuid_case_equality_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root><value>12345678-1234-5678-90ab-cdef01234567</value></root>"#).unwrap();
    let root = document.root_element();
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino XML node name case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
        let result = super::super::field_uuid_untyped(&ctx, root, "value").map(|_| ());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}

#[test]
fn optional_modifier_case_equality_preserves_refusal_without_warning() {
    let payload = super::v2_payload("<xml><new-displacement-object-data><on type=\"bool\">true</on></new-displacement-object-data></xml>");
    let descriptors = [super::descriptor(&payload, Some(super::MESH_MODIFIER_PLUGIN))];
    cadmpeg_test_support::refusal::resource_limit_at(ResourceDimension::WorkUnits, "Rhino field bool case equality", |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
        let mut warnings = crate::loss::Diagnostics::new();
        let result = super::super::parse_attribute_userdata(&ctx, &payload, &descriptors, crate::chunks::ArchiveVersion::V6, &mut warnings)
            .map(|_| ()).map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid modifier returned {other:?}"),
            });
        assert!(warnings.is_empty());
        if let Err(CodecError::ResourceLimit(refusal)) = &result { assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal)); }
        result
    });
}
