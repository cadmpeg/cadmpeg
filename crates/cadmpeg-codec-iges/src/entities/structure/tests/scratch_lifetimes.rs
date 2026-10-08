// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::directory::{DirectoryEntry, SourceStatus};
use crate::parameter::{ParameterRecord, Token, TokenValue};
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;
use std::collections::BTreeMap;

const SCRATCH_CAP_BYTES: u64 = 8 * 1024;

// The shared B-tree admission formula bounds 32 Type 322 type keys to seven
// 232-byte nodes. Its descriptor vector holds at most 32 `(i64, usize)` pairs
// (512 bytes at exact capacity). Three Type 316 unit types use one 320-byte
// B-tree node. Each one-record peak fits below this cap; repeating the record
// would exceed it if either temporary index stayed live across iterations.

fn record(values: Vec<TokenValue>, parameter_end: usize) -> ParameterRecord {
    let tokens = values
        .into_iter()
        .map(|value| Token { value, span: 0..0 })
        .collect();
    ParameterRecord::from_test_tokens(1, 0..0, Vec::new(), parameter_end, tokens, Vec::new())
}

fn directory_entry(entity_type: i64, form: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence: 1,
        entity_type,
        parameter_start: 0,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: SourceStatus::from_codes([0, 0, 0, 0]),
        line_weight: 0,
        color: 0,
        parameter_line_count: 0,
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

fn bounded_policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_CAP_BYTES;
    policy
}

#[test]
fn type316_unit_type_index_storage_is_released_per_record() {
    let record = record(
        vec![
            TokenValue::Integer(316),
            TokenValue::Integer(3),
            TokenValue::String(b"LENGTH".to_vec()),
            TokenValue::String(b"KN".to_vec()),
            TokenValue::real(1.0),
            TokenValue::String(b"MASS".to_vec()),
            TokenValue::String(b"KG".to_vec()),
            TokenValue::real(1.0),
            TokenValue::String(b"TIME".to_vec()),
            TokenValue::String(b"S".to_vec()),
            TokenValue::real(1.0),
        ],
        11,
    );
    let policy = bounded_policy();

    let result = crate::test_support::with_policy_context(&[], &policy, |ctx| {
        for _ in 0..64 {
            assert!(super::super::unit_values_valid(&record, ctx)?);
        }
        Ok::<_, CodecError>(())
    });

    result.unwrap();
}

#[test]
fn type322_descriptor_and_type_indexes_are_released_for_rejected_record() {
    let mut values = vec![
        TokenValue::Integer(322),
        TokenValue::Integer(7),
        TokenValue::Integer(0),
        TokenValue::Integer(32),
    ];
    for attribute_type in 100..132 {
        values.extend([
            TokenValue::Integer(attribute_type),
            TokenValue::Integer(1),
            TokenValue::Integer(1),
        ]);
    }
    let record = record(values, 4 + 32 * 3);
    let entry = directory_entry(322, 0);
    let entries = BTreeMap::new();
    let policy = bounded_policy();

    let result = crate::test_support::with_policy_context(&[], &policy, |ctx| {
        for _ in 0..16 {
            let (definition_valid, shape) = super::super::attribute_definition_valid_and_shape(
                &entry,
                &record,
                &entries,
                crate::global::GlobalTable::V5Later,
                ctx,
            )?;
            assert!(!definition_valid);
            assert_eq!(shape.descriptors.len(), 32);
            drop(shape);
        }
        Ok::<_, CodecError>(())
    });

    result.unwrap();
}
