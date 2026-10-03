// SPDX-License-Identifier: Apache-2.0

use super::FORMAT;
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
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = cadmpeg_core::decode::u64_from_index(
        4 * std::mem::size_of::<cadmpeg_ir::attributes::AttributeValue>(),
    );
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = source_attribute(&ctx, &record, AttributeTarget::Document, FORMAT)
        .expect_err("one attribute string exceeds zero retained bytes");
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
fn unknown_record_kind_refuses_retained_limit() {
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
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = unknown_record_id(&ctx, &record, FORMAT)
        .expect_err("one unknown kind exceeds zero retained bytes");
    let CodecError::ResourceLimit(limit) = error else {
        panic!("expected retained refusal: {error:?}");
    };
    assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
}
