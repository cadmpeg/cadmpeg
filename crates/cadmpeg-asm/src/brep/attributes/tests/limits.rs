// SPDX-License-Identifier: Apache-2.0

use crate::ids::IdFormat;

const FORMAT: IdFormat = crate::asm_format!("f3d");
use crate::sab::{Record, Token};

#[test]
fn source_attribute_string_refuses_retained_limit() {
    use crate::brep::attributes::source_attribute;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::attributes::AttributeTarget;

    let record = Record {
        index: 1,
        name: "string-st-attrib".into(),
        tokens: vec![Token::Str("value".into())].into(),
        offset: 0,
        len: 0,
    };
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes, "ASM attribute string", |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            source_attribute(&ctx, &record, AttributeTarget::Document, FORMAT)
        },
    );
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected retained refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "ASM attribute string");
}

#[test]
fn source_attribute_record_name_refuses_retained_limit() {
    use crate::brep::attributes::source_attribute;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::attributes::AttributeTarget;

    let record = Record {
        index: 1,
        name: "empty-st-attrib".into(),
        tokens: Vec::new().into(),
        offset: 0,
        len: 0,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = source_attribute(&ctx, &record, AttributeTarget::Document, FORMAT)
        .expect_err("one attribute name exceeds zero retained bytes");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected retained refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(limit.operation, "ASM attribute record name");
}

#[test]
fn unknown_record_kind_refuses_materialized_limit() {
    use crate::brep::attributes::unknown_record_id;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let record = Record {
        index: 1,
        name: "unknown".into(),
        tokens: Vec::new().into(),
        offset: 0,
        len: 0,
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = unknown_record_id(&ctx, &record, FORMAT)
        .expect_err("one unknown kind exceeds zero materialized bytes");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected materialized refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
}

#[test]
fn decimal_attribute_color_refuses_work_before_parsing() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    use std::collections::HashMap;

    let entity = Record {
        index: 0,
        name: "face".into(),
        tokens: vec![Token::Ref(1)].into(),
        offset: 0,
        len: 0,
    };
    let decimal = Record {
        index: 1,
        name: "entatt_color-bt-attrib".into(),
        tokens: vec![
            Token::Ref(-1),
            Token::Long(-1),
            Token::Ref(-1),
            Token::Ref(-1),
            Token::Ref(0),
            Token::Str("4227264".into()),
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let by_index = HashMap::from([(1, &decimal)]);
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "parse ASM decimal color",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let result = crate::brep::attributes::attribute_chain_color_carrier(
                &ctx, &entity, by_index.len(), |index| by_index.get(&index).copied(),
            ).map(|_| ());
            if let Err(CodecError::ResourceLimit(ref limit)) = result {
                assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
            }
            result
        },
    );
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("expected work refusal, got {error:?}");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "parse ASM decimal color");
    assert_eq!(refusal.additional, 7);
}

