// SPDX-License-Identifier: Apache-2.0
//! Value-block parser tests over synthetic CATPart streams.

#![allow(clippy::doc_markdown, clippy::unwrap_used)]

use super::*;
use crate::test_support::{catalog_stream, value_block_stream};

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
                code: 0xea,
                bytes: vec![0x32, 1, 2],
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

    let blocks = crate::value_block::parse(&bytes);
    assert_eq!(blocks.len(), 1);
    assert_eq!(blocks[0].pos, 0);
    assert_eq!(blocks[0].declared_len, 15);
    assert_eq!(blocks[0].total_len(), 16);
    assert_eq!(blocks[0].payload, payload);
}

#[test]
fn native_value_blocks_require_a_complete_adjacent_catalog() {
    let mut bytes = value_block_stream(&[0x81]);
    bytes.extend_from_slice(&[0x7c, 0x02]);

    assert_eq!(crate::value_block::parse(&bytes).len(), 1);
    assert!(crate::native::CatiaNative::decode(&bytes)
        .value_blocks
        .is_empty());
}
