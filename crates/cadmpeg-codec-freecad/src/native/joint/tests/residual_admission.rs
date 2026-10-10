// SPDX-License-Identifier: Apache-2.0
//! Checked property admission and final parameter storage.

use super::super::{parameter_from_property, JointParameters};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

fn property(raw: &str) -> crate::native::PropertyRecord {
    crate::native::PropertyRecord {
        id: "property".into(),
        owner: "fcstd:native:object#Joint".into(),
        name: "Angle".into(),
        type_name: "App::PropertyAngle".into(),
        family: crate::native::PropertyFamily::Unknown,
        status: None,
        order: 0,
        body: crate::native::PropertyBody::Persisted {
            values: vec![crate::native::ValueRecord {
                tag: "Float".into(),
                order: 0,
                attributes: BTreeMap::from([("value".into(), raw.into())]),
                text: None,
                raw_xml: String::new(),
            }],
            links: Vec::new(),
            side_entries: Vec::new(),
            dynamic: None,
        },
        xml: crate::native::RetainedXml::from_text("<Property/>".into(), 0).expect("XML"),
    }
}

#[test]
fn checked_joint_property_keeps_scalar_and_source_spelling() {
    let raw = format!("15.5{}", "0".repeat(4096));
    let property = property(&raw);
    crate::test_support::with_service_context(&[], |ctx| {
        let value = parameter_from_property(ctx, &property)
            .expect("admission")
            .expect("scalar");
        let mut parameters = JointParameters::default();
        parameters
            .insert(ctx, property.name.clone(), value)
            .expect("one final tree");
        assert_eq!(parameters.raw("Angle"), Some(raw.as_str()));
        assert_eq!(
            parameters
                .scalar_value(ctx, "Angle")
                .expect("lookup")
                .map(|value| value.get()),
            Some(15.5)
        );
    });
}

#[test]
fn checked_joint_malformed_property_preserves_diagnostic() {
    let property = property("NaN");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1024;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let error = parameter_from_property(&ctx, &property).expect_err("nonfinite scalar");
    assert!(
        matches!(error, CodecError::Malformed(ref value) if value == "joint parameter property property: joint parameter Angle has an invalid value \"NaN\"")
    );
    assert_eq!(ctx.resource_refusal(), None);
}

#[test]
fn checked_joint_final_parameter_tree_refuses_collection_limit() {
    let property = property("1");
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
    let value = parameter_from_property(&ctx, &property)
        .expect("scalar admission")
        .expect("scalar");
    let CodecError::ResourceLimit(limit) = JointParameters::default()
        .insert(&ctx, property.name.clone(), value)
        .expect_err("map slot")
    else {
        panic!("collection refusal")
    };
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!(ctx.resource_refusal(), Some(limit));
}
