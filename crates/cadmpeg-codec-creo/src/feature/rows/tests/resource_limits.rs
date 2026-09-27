// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use super::super::{FeatureChoice, FeatureFieldValue, FeatureRow};

fn run<T>(
    input: &[u8],
    items: u64,
    retained: u64,
    parse: impl FnOnce(&DecodeContext<'_>) -> Result<T, CodecError>,
) -> Result<T, CodecError> {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = items;
    policy.limits.max_retained_bytes = retained;
    let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy)
        .expect("root choice input is admitted");
    parse(&ctx)
}

fn item(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::CollectionItems && limit.operation == operation));
}

fn retained(error: CodecError, operation: &'static str) {
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == operation));
}

fn row() -> FeatureRow {
    let body = b"\xe0\x01blend_choice\0\x00".to_vec();
    FeatureRow {
        feature_id: 7,
        root_schema_class: None,
        stream_offset: 0,
        body: body.try_into().expect("two-byte row body"),
        body_offset: 0,
        offset: 0,
    }
}

fn field_choice(payload: Vec<u8>) -> FeatureChoice {
    FeatureChoice {
        feature_id: 7,
        label: "blend_choice".to_string(),
        type_byte: Some(1),
        payload,
        payload_offset: 0,
        offset: 0,
    }
}

#[test]
fn choice_hit_refuses_before_vec_growth() {
    let row = row();
    item(
        run(&row.body, 0, u64::MAX, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect_err("one recognized label needs a hit item"),
        "creo choice label hits",
    );
}

#[test]
fn choice_label_refuses_before_retained_text_copy() {
    let row = row();
    retained(
        run(&row.body, 2, 0, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect_err("choice label needs retained text"),
        "creo feature choice label",
    );
}

#[test]
fn choice_payload_refuses_before_retained_byte_copy() {
    let row = row();
    retained(
        run(&row.body, 2, "blend_choice".len() as u64, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect_err("choice payload needs retained bytes"),
        "creo feature choice payload",
    );
}

#[test]
fn choice_record_refuses_before_vec_growth() {
    let row = row();
    assert_eq!(
        run(&row.body, 2, u64::MAX, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect("one choice admitted")
        .len(),
        1
    );
    item(
        run(&row.body, 1, u64::MAX, |ctx| {
            super::super::choices(ctx, std::slice::from_ref(&row))
        })
        .expect_err("choice result needs another item"),
        "creo feature choices",
    );
}

#[test]
fn raw_feature_field_refuses_before_retained_copy() {
    let payload = [0xff, 0x00];
    retained(
        run(&payload, 0, 0, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("raw bytes need retained admission"),
        "creo feature raw field",
    );
}

#[test]
fn scalar_feature_field_cache_refuses_before_hashset_growth() {
    let payload = [0xf9, 0x01, 0x01, 0x46, 0, 0, 0, 0, 0, 0, 0];
    item(
        run(&payload, 0, u64::MAX, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("scalar image requires a cache item"),
        "creo scalar cache unique images",
    );
}

#[test]
fn scalar_feature_values_refuse_before_vec_growth() {
    let payload = [0xf9, 0x01, 0x01, 0x0f];
    let value = run(&payload, 1, u64::MAX, |ctx| {
        super::super::field_value(ctx, &payload)
    })
    .expect("one scalar value admitted");
    assert!(
        matches!(value, FeatureFieldValue::ScalarArray { decoded_values: Some(values), .. } if values == [0.0])
    );
    item(
        run(&payload, 0, u64::MAX, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("decoded scalar needs one item"),
        "creo feature scalar values",
    );
}

#[test]
fn scalar_feature_body_refuses_before_retained_copy() {
    let payload = [0xf9, 0x01, 0x01, 0x0f];
    retained(
        run(&payload, 1, 0, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("scalar body needs retained bytes"),
        "creo feature scalar field body",
    );
}

#[test]
fn compact_feature_integer_refuses_before_vec_growth() {
    let payload = [0xf8, 0x01, 0x07];
    item(
        run(&payload, 0, u64::MAX, |ctx| {
            super::super::field_value(ctx, &payload)
        })
        .expect_err("compact array needs one value item"),
        "creo feature compact integer values",
    );
}

#[test]
fn choice_field_header_refuses_before_vec_growth() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    item(
        run(&choice.payload, 0, u64::MAX, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect_err("field header needs one item"),
        "creo choice field headers",
    );
}

#[test]
fn choice_field_label_refuses_before_retained_text_copy() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    retained(
        run(&choice.payload, 3, 0, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect_err("copied choice label needs retained bytes"),
        "creo choice field label",
    );
}

#[test]
fn choice_field_name_refuses_before_retained_text_copy() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    retained(
        run(&choice.payload, 3, choice.label.len() as u64, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect_err("field name needs retained bytes"),
        "creo choice field name",
    );
}

#[test]
fn choice_field_record_refuses_before_vec_growth() {
    let choice = field_choice(b"\xe0\x01foo\0\xf8\x01\x07".to_vec());
    assert_eq!(
        run(&choice.payload, 3, u64::MAX, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect("one choice field admitted")
        .len(),
        1
    );
    item(
        run(&choice.payload, 2, u64::MAX, |ctx| {
            super::super::choice_fields(ctx, std::slice::from_ref(&choice))
        })
        .expect_err("choice field result needs one item"),
        "creo choice fields",
    );
}
