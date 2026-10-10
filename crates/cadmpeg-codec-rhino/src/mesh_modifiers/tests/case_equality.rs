// SPDX-License-Identifier: Apache-2.0

use crate::chunks::FramingError;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn assert_work_refusal<T>(
    operation: &str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, CodecError>,
) {
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = run(&ctx);
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn xml_attribute_search_preserves_refusal() {
    let document = roxmltree::Document::parse(r#"<root TYPE="bool"/>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino XML attribute search", |ctx| {
        super::super::attribute(ctx, root, "type").map(|_| ())
    });
}

#[test]
fn xml_uuid_braces_and_skipped_hyphens_preserve_refusal() {
    let value = "{{---12345678-1234-5678-90ab-cdef01234567---}}";
    for operation in ["Rhino XML UUID braces", "Rhino XML UUID digits"] {
        assert_work_refusal(operation, |ctx| super::super::parse_uuid(ctx, value));
    }
    assert_eq!(
        super::super::parse_uuid(&cadmpeg_test_support::service_decode_context(), value)
            .unwrap()
            .unwrap()
            .to_string(),
        "12345678-1234-5678-90ab-cdef01234567"
    );
}

#[test]
fn xml_child_search_pays_for_skipped_nodes() {
    let document =
        roxmltree::Document::parse("<root>text<!--skip--><other/><wanted/></root>").unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino XML child search", |ctx| {
        super::super::direct_child(ctx, root, "wanted")
    });
    assert_eq!(
        super::super::direct_child(
            &cadmpeg_test_support::service_decode_context(),
            root,
            "wanted"
        )
        .unwrap()
        .unwrap()
        .tag_name()
        .name(),
        "wanted"
    );
}

#[test]
fn typed_boolean_text_trim_preserves_work_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="bool"> true </value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino typed boolean text trim", |ctx| {
        super::super::field_bool(ctx, root, "value", false)
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid boolean returned {other:?}"),
            })
    });
}

#[test]
fn xml_typed_integer_trim_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="int"> 1 </value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field i32 optional text trim", |ctx| {
        super::super::field_i32_optional(ctx, root, "value")
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid field returned {other:?}"),
            })
    });
}

#[test]
fn xml_typed_real_trim_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="double"> 1.0 </value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field f64 text trim", |ctx| {
        super::super::field_f64(ctx, root, "value", 0.0)
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid field returned {other:?}"),
            })
    });
}

#[test]
fn xml_typed_uuid_trim_preserves_refusal() {
    let document = roxmltree::Document::parse(
        r#"<root><value type="uuid"> 12345678-1234-5678-90ab-cdef01234567 </value></root>"#,
    )
    .unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field uuid text trim", |ctx| {
        super::super::field_uuid(ctx, root, "value").map(|_| ())
    });
}

#[test]
fn xml_untyped_uuid_trim_preserves_refusal() {
    let document = roxmltree::Document::parse(
        r#"<root><value type="uuid"> 12345678-1234-5678-90ab-cdef01234567 </value></root>"#,
    )
    .unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field uuid untyped text trim", |ctx| {
        super::super::field_uuid_untyped(ctx, root, "value").map(|_| ())
    });
}

#[test]
fn xml_untyped_boolean_trim_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="bool"> true </value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field bool untyped text trim", |ctx| {
        super::super::field_bool_untyped(ctx, root, "value", false).map(|_| ())
    });
}

#[test]
fn xml_untyped_integer_trim_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="int"> 1 </value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field i32 untyped text trim", |ctx| {
        super::super::field_i32_untyped(ctx, root, "value", 0)
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid field returned {other:?}"),
            })
    });
}

#[test]
fn xml_untyped_real_trim_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="double"> 1.0 </value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field f64 untyped text trim", |ctx| {
        super::super::field_f64_untyped(ctx, root, "value", cadmpeg_ir::scalar::FiniteReal::ONE)
            .map(|_| ())
    });
}

#[test]
fn xml_cap_type_trim_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="string"> flat </value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field cap type text trim", |ctx| {
        super::super::field_cap_type(ctx, root, "value").map(|_| ())
    });
}

#[test]
fn xml_boolean_integer_fallback_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="string">2</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field bool integer parse", |ctx| {
        super::super::field_bool(ctx, root, "value", false)
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid numeric field returned {other:?}"),
            })
    });
}

#[test]
fn xml_boolean_numeric_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="int">2</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field bool number parse", |ctx| {
        super::super::field_bool(ctx, root, "value", false)
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid numeric field returned {other:?}"),
            })
    });
}

#[test]
fn xml_integer_real_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="double">2.5</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field i32 optional number parse", |ctx| {
        super::super::field_i32_optional(ctx, root, "value")
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid numeric field returned {other:?}"),
            })
    });
}

#[test]
fn xml_integer_string_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="string">2</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field i32 optional number parse", |ctx| {
        super::super::field_i32_optional(ctx, root, "value")
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid numeric field returned {other:?}"),
            })
    });
}

#[test]
fn xml_integer_numeric_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="int">2</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field i32 optional number parse", |ctx| {
        super::super::field_i32_optional(ctx, root, "value")
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid numeric field returned {other:?}"),
            })
    });
}

#[test]
fn xml_real_numeric_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="double">2.5</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field f64 number parse", |ctx| {
        super::super::field_f64(ctx, root, "value", 0.0)
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid numeric field returned {other:?}"),
            })
    });
}

#[test]
fn xml_untyped_boolean_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="int">2</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field bool untyped number parse", |ctx| {
        super::super::field_bool_untyped(ctx, root, "value", false).map(|_| ())
    });
}

#[test]
fn xml_untyped_integer_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="double">2.5</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field i32 untyped number parse", |ctx| {
        super::super::field_i32_untyped(ctx, root, "value", 0)
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid numeric field returned {other:?}"),
            })
    });
}

#[test]
fn xml_untyped_real_number_parse_preserves_refusal() {
    let document =
        roxmltree::Document::parse(r#"<root><value type="double">2.5</value></root>"#).unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino field f64 untyped number parse", |ctx| {
        super::super::field_f64_untyped(ctx, root, "value", cadmpeg_ir::scalar::FiniteReal::ONE)
            .map(|_| ())
    });
}

#[test]
fn xml_untyped_real_child_search_preserves_refusal() {
    let document = roxmltree::Document::parse(r"<root><value>1.0</value></root>").unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino XML child search", |ctx| {
        super::super::field_f64_untyped(ctx, root, "value", cadmpeg_ir::scalar::FiniteReal::ONE)
            .map(|_| ())
    });
}

#[test]
fn xml_untyped_uuid_child_search_preserves_refusal() {
    let document = roxmltree::Document::parse(
        r"<root><value>12345678-1234-5678-90ab-cdef01234567</value></root>",
    )
    .unwrap();
    let root = document.root_element();
    assert_work_refusal("Rhino XML child search", |ctx| {
        super::super::field_uuid_untyped(ctx, root, "value").map(|_| ())
    });
}

#[test]
fn optional_modifier_child_search_preserves_refusal_without_warning() {
    let payload = super::v2_payload("<xml><new-displacement-object-data><on type=\"bool\">true</on></new-displacement-object-data></xml>");
    let descriptors = [super::descriptor(
        &payload,
        Some(super::MESH_MODIFIER_PLUGIN),
    )];
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "Rhino XML child search",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
            let mut warnings = crate::loss::Diagnostics::new();
            let result = super::super::parse_attribute_userdata(
                &ctx,
                &payload,
                &descriptors,
                crate::chunks::ArchiveVersion::V6,
                &mut warnings,
            )
            .map(|_| ())
            .map_err(|error| match error {
                FramingError::Resource(refusal) => CodecError::ResourceLimit(refusal),
                other => panic!("valid modifier returned {other:?}"),
            });
            assert!(warnings.is_empty());
            if let Err(CodecError::ResourceLimit(refusal)) = &result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
            }
            result
        },
    );
}

#[test]
fn dropped_modifier_does_not_retain_its_numeric_array() {
    let xml = "<xml><new-displacement-object-data><sub/><on type=\"bool\">bad</on></new-displacement-object-data></xml>";
    let payload = super::v2_payload(xml);
    let descriptors = [super::descriptor(
        &payload,
        Some(super::MESH_MODIFIER_PLUGIN),
    )];
    for operation in [
        "Rhino displacement sub-items",
        "Rhino optional modifier output",
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            ResourceDimension::RetainedBytes,
            operation,
            None,
        );
        let mut warnings = crate::loss::Diagnostics::new();
        let modifier = super::super::parse_attribute_userdata(
            &ctx,
            &payload,
            &descriptors,
            crate::chunks::ArchiveVersion::V6,
            &mut warnings,
        )
        .unwrap();
        assert!(modifier.is_none());
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("XML field `on` has invalid bool value"));
        assert!(ctx.finish_session().is_ok());
    }
}

#[test]
fn optional_modifier_array_refuses_speculative_and_retained_boundaries() {
    let xml = "<xml><new-displacement-object-data><sub/></new-displacement-object-data></xml>";
    let payload = super::v2_payload(xml);
    let descriptors = [super::descriptor(
        &payload,
        Some(super::MESH_MODIFIER_PLUGIN),
    )];
    for (dimension, operation) in [
        (
            ResourceDimension::MaterializedBytes,
            "Rhino displacement sub-items",
        ),
        (
            ResourceDimension::RetainedBytes,
            "Rhino optional modifier output",
        ),
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            match dimension {
                ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
                ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
                _ => panic!("storage dimension"),
            }
            let (ctx, _) = DecodeContext::from_root_bytes(&payload, &arena, &policy).unwrap();
            let mut warnings = crate::loss::Diagnostics::new();
            let result = super::super::parse_attribute_userdata(
                &ctx,
                &payload,
                &descriptors,
                crate::chunks::ArchiveVersion::V6,
                &mut warnings,
            );
            assert!(warnings.is_empty());
            if let Err(FramingError::Resource(limit)) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*limit));
            }
            result.map_err(|error| match error {
                FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
                error => panic!("unexpected modifier error: {error:?}"),
            })
        });
    }
    let ctx = cadmpeg_test_support::service_decode_context();
    let mut warnings = crate::loss::Diagnostics::new();
    let modifier = super::super::parse_attribute_userdata(
        &ctx,
        &payload,
        &descriptors,
        crate::chunks::ArchiveVersion::V6,
        &mut warnings,
    )
    .unwrap()
    .unwrap();
    assert_eq!(modifier.displacement.unwrap().sub_items.len(), 1);
    assert!(warnings.is_empty());
}

#[test]
fn direct_modifier_array_retained_probe_is_active() {
    let xml = "<xml><new-displacement-object-data><sub/></new-displacement-object-data></xml>";
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "Rhino displacement sub-items",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &policy).unwrap();
            let result = super::super::parse_xml(&ctx, xml, 2, crate::chunks::ArchiveVersion::V6);
            if let Err(FramingError::Resource(limit)) = &result {
                assert_eq!(ctx.resource_refusal(), Some(*limit));
            }
            result.map_err(|error| match error {
                FramingError::Resource(limit) => CodecError::ResourceLimit(limit),
                error => panic!("valid modifier: {error:?}"),
            })
        },
    );
}
