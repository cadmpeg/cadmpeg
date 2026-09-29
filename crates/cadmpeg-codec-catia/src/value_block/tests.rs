// SPDX-License-Identifier: Apache-2.0
//! Value-block parser tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::{tokenize, InlineBytes, InlineBytesWire, ValueBlock, ValueField};

#[test]
fn inline_bytes_borrowed_wire_preserves_json_bytes() {
    let value = InlineBytes::try_from(vec![1, 2, 3]).expect("inline bytes");
    let owned: InlineBytesWire = value.clone().into();
    assert_eq!(
        serde_json::to_vec(&value).expect("borrowed inline JSON"),
        serde_json::to_vec(&owned).expect("owned inline JSON")
    );
}

#[test]
fn inline_bytes_retained_limit_refuses_json_record() {
    let value = InlineBytes::try_from(vec![1, 2, 3]).expect("inline bytes");
    #[derive(serde::Serialize)]
    struct Record<'a> {
        id: &'static str,
        #[serde(flatten)]
        value: &'a InlineBytes,
    }
    let record = Record { id: "catia:test:inline-bytes#0", value: &value };
    let arena_name = "inline_values";
    let json_len = serde_json::to_vec(&record).expect("inline JSON").len();
    let limit = u64::try_from(json_len + arena_name.len() - 1).expect("small JSON");
    let refused = crate::test_support::with_retained_limit(limit, |ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace.set_arena(ctx, arena_name, std::slice::from_ref(&record))
    });
    let error = refused.expect_err("record exceeds retained-byte limit");
    assert!(error.to_string().contains("RetainedBytes"), "{error}");
    crate::test_support::with_service_context(|ctx| {
        let mut namespace = cadmpeg_ir::NativeNamespace::default();
        namespace.set_arena(ctx, arena_name, std::slice::from_ref(&record))
            .expect("service profile admits inline bytes");
    });
}
use crate::test_support::test_object_graph::{catalog_stream, value_block_stream};

fn parse(bytes: &[u8]) -> Vec<ValueBlock> {
    crate::test_support::with_service_context(|ctx| super::parse(ctx, bytes))
        .expect("value block fixture fits the service limits")
}

#[test]
fn copied_value_fields_refuse_nested_retained_and_outer_collection_limits() {
    let fields = [ValueField::Inline {
        bytes: InlineBytes(vec![1, 2]),
        offset: 0,
    }];
    let retained =
        crate::test_support::with_retained_limit(1, |ctx| super::copy_fields_charged(ctx, &fields));
    assert!(
        matches!(retained, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_value_inline_bytes")
    );
    let collection = crate::test_support::with_collection_limit(2, |ctx| {
        super::copy_fields_charged(ctx, &fields)
    });
    assert!(
        matches!(collection, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_native_value_fields")
    );
    let copied =
        crate::test_support::with_service_context(|ctx| super::copy_fields_charged(ctx, &fields))
            .expect("service profile admits nested value field");
    assert_eq!(copied, fields);
}

#[test]
fn value_block_payload_refuses_retained_and_collection_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let mut bytes = value_block_stream(&[0x81]);
    bytes.extend_from_slice(&[0x7c, 0x02]);
    for dimension in [
        ResourceDimension::RetainedBytes,
        ResourceDimension::CollectionItems,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if dimension == ResourceDimension::RetainedBytes {
            policy.limits.max_retained_bytes = 0;
        } else {
            policy.limits.max_collection_items = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .expect("value block fixture fits the input limit");
        let error = super::parse(&ctx, &bytes)
            .expect_err("the retained payload exceeds the selected limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == dimension && limit.operation == "catia_value_block_payload"));
    }
    assert_eq!(parse(&bytes).len(), 1);
}

#[test]
fn value_tokenizer_refuses_field_and_retained_byte_limits() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;

    let inline = [0x8e, 0xe8, 0x84, b'A'];
    let cases = [
        (
            &[0x81][..],
            ResourceDimension::CollectionItems,
            "catia_value_fields",
        ),
        (
            &inline[..],
            ResourceDimension::RetainedBytes,
            "catia_value_field_bytes",
        ),
    ];
    for (payload, dimension, operation) in cases {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        if dimension == ResourceDimension::CollectionItems {
            policy.limits.max_collection_items = 0;
        } else {
            policy.limits.max_retained_bytes = 0;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(payload, &arena, &policy)
            .expect("token fixture fits the input limit");
        let error = super::tokenize_charged(&ctx, payload)
            .expect_err("one token exceeds the selected limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == dimension && limit.operation == operation));
        let charged =
            crate::test_support::with_service_context(|ctx| super::tokenize_charged(ctx, payload))
                .expect("service budget admits the token");
        assert_eq!(charged, tokenize(payload));
    }
}

#[test]
fn typed_payloads_hide_embedded_schema_marker_bytes() {
    let payload = [
        0x32, 5, 0, 0, 0, 0x87, 0xe6, 0, 0, 0, 0, 0, 0x32, 0, 0, 0x8e, 0xea, 0x84, 0x32, 1, 2,
        0x87, 0xe8,
    ];
    assert_eq!(
        tokenize(&payload),
        vec![
            ValueField::SchemaSelector {
                ordinal: 5,
                offset: 0,
            },
            ValueField::Binary64 {
                bits: 0x0000_3200_0000_0000,
                offset: 5,
            },
            ValueField::Inline {
                bytes: vec![0x32, 1, 2].try_into().unwrap(),
                offset: 15,
            },
            ValueField::Marker {
                code: 0xe8,
                offset: 21,
            },
        ]
    );
}

#[test]
fn length_framed_byte_strings_hide_marker_shaped_payload_bytes() {
    let payload = [0xe5, 5, 0, 0, 0, 0x32, 0xe8, 0x37, 0xfe, 0x80, 0xfe];
    assert_eq!(
        tokenize(&payload),
        vec![
            ValueField::ByteString {
                bytes: vec![0x32, 0xe8, 0x37, 0xfe, 0x80],
                offset: 0,
            },
            ValueField::Terminator { offset: 10 },
        ]
    );
}

#[test]
fn truncated_length_framed_byte_string_is_not_assigned() {
    let fields = tokenize(&[0xe5, 5, 0, 0, 0, 1]);
    assert!(matches!(
        fields.first(),
        Some(ValueField::Literal {
            value: 0xe5,
            offset: 0
        })
    ));
    assert!(fields
        .iter()
        .all(|field| !matches!(field, ValueField::ByteString { .. })));
}

#[test]
fn truncated_multi_byte_forms_remain_literal() {
    assert_eq!(
        tokenize(&[0x8e, 0xef, 0x84, 1]),
        vec![
            ValueField::Literal {
                value: 0x8e,
                offset: 0,
            },
            ValueField::Literal {
                value: 0xef,
                offset: 1,
            },
            ValueField::Atom {
                value: 4,
                width: 1,
                offset: 2,
            },
            ValueField::Literal {
                value: 1,
                offset: 3,
            },
        ]
    );
}

#[test]
fn untagged_value_opcodes_and_terminators_remain_distinct() {
    assert_eq!(
        tokenize(&[0xe6, 0xe7, 0xe8, 0xe9, 0xfe]),
        vec![
            ValueField::Opcode {
                code: 0xe6,
                offset: 0,
            },
            ValueField::Opcode {
                code: 0xe7,
                offset: 1,
            },
            ValueField::Opcode {
                code: 0xe8,
                offset: 2,
            },
            ValueField::Opcode {
                code: 0xe9,
                offset: 3,
            },
            ValueField::Terminator { offset: 4 },
        ]
    );
}

#[test]
fn value_block_parser_reads_length_to_terminator_boundary() {
    let payload = [0x81, 0x83, 0x32, 4, 0, 0, 0, 0x83, 0x82];
    let mut bytes = value_block_stream(&payload);
    bytes.extend(catalog_stream(&[
        "CATCatalogManager",
        "catalogManager",
        "catalogLinks",
        "",
        "Sketch",
    ]));

    let blocks = parse(&bytes);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].pos, 0);
    assert_eq!(blocks[0].declared_len(), 15);
    assert_eq!(blocks[0].total_len(), 16);
    assert_eq!(blocks[0].payload, payload);
}

#[test]
fn native_value_blocks_require_a_complete_adjacent_catalog() {
    let mut bytes = value_block_stream(&[0x81]);
    bytes.extend_from_slice(&[0x7c, 0x02]);

    assert_eq!(parse(&bytes).len(), 1);
    assert!(crate::native::CatiaNative::decode(&bytes)
        .value_blocks
        .is_empty());
}

#[test]
fn serialized_length_must_match_payload() {
    let block = ValueBlock {
        pos: 12,
        payload: vec![0x87, 0xe8],
    };
    let mut wire = serde_json::to_value(&block).unwrap();
    assert_eq!(wire["declared_len"], 8);
    assert_eq!(
        serde_json::from_value::<ValueBlock>(wire.clone()).unwrap(),
        block
    );
    wire["declared_len"] = serde_json::json!(7);
    assert!(serde_json::from_value::<ValueBlock>(wire).is_err());
}

#[test]
fn serialized_inline_code_must_match_the_inline_byte_count() {
    let bytes = InlineBytes::try_from(vec![1, 2, 3]).unwrap();
    let mut wire = serde_json::to_value(&bytes).unwrap();
    assert_eq!(wire["code"], serde_json::json!(0xea));
    assert_eq!(
        serde_json::from_value::<InlineBytes>(wire.clone()).unwrap(),
        bytes
    );

    wire["code"] = serde_json::json!(0xeb);
    let error = serde_json::from_value::<InlineBytes>(wire)
        .expect_err("inline code disagreeing with the byte count");
    assert!(error.to_string().contains("code"), "{error}");
}
