// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use super::{
    analyze_trailing_pointer_groups, analyze_trailing_pointer_groups_for_global_table,
    analyze_trailing_pointer_groups_with_records, entity_primary_end,
    entity_primary_end_for_global_table, entity_primary_end_with_records, groups_for_candidate,
    structural_pointer_group_candidates, ParameterRecord, Token, TokenValue,
};
use crate::card::{scan, Section};
use crate::directory::{DirectoryEntry, Status};
use crate::global::GlobalTable;
use crate::loss::IgesLossCode;
use crate::test_support::*;
use crate::IgesCodec;

mod advanced_entity_boundaries;
mod curve_surface_boundaries;
mod drawing_associativity;
mod entity_table_boundaries;
mod entity_table_forms;
mod envelope_boundaries;
mod fixed_entity_boundaries;
mod implementor_defined;
mod later_entity_boundaries;
mod legacy_entities;
mod legacy_type402;
mod lexical;
mod macros;
mod presentation_forms;

fn parameter_owner(field: [u8; 8]) -> Option<u32> {
    let mut bytes = owned_test_file(&[OwnedTestEntity {
        entity_type: 116,
        form: 0,
        label: "POINT".into(),
        status: "00010000",
        parameters: "116,1,2,3,0;".into(),
    }]);
    let marker = bytes
        .windows(8)
        .position(|window| window == b"P      1")
        .expect("Parameter Data card");
    let card_start = marker - 72;
    bytes[card_start + 64..card_start + 72].copy_from_slice(&field);
    let scan = scan(&bytes).unwrap();
    let line = scan
        .lines
        .iter()
        .find(|line| line.section == Some(Section::Parameter))
        .expect("Parameter Data line");
    super::back_pointer(line)
}

impl From<i64> for TokenValue {
    fn from(value: i64) -> Self {
        Self::Integer(value)
    }
}

impl From<f64> for TokenValue {
    fn from(value: f64) -> Self {
        Self::Real(value)
    }
}

fn directory_target(sequence: u32, entity_type: i64) -> DirectoryEntry {
    DirectoryEntry {
        source_offset: 0,
        sequence,
        entity_type,
        parameter_start: 1,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: Status {
            blank: 0,
            subordinate: 0,
            use_flag: 0,
            hierarchy: 0,
        },
        line_weight: 0,
        color: 0,
        parameter_line_count: 1,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

#[test]
fn parameter_owner_field_uses_blank_column_65_and_right_aligned_seven_digits() {
    for (field, expected) in [
        (*b"       1", Some(1)),
        (*b" 0000001", Some(1)),
        (*b" 9999999", Some(9_999_999)),
        (*b"1       ", None),
        (*b" 123456 ", None),
        (*b"        ", None),
        (*b" 0000000", None),
        (*b"  123456", Some(123_456)),
    ] {
        assert_eq!(parameter_owner(field), expected, "{field:?}");
    }
}

fn integer_parameter_record(sequence: u32, values: &[i64]) -> ParameterRecord {
    ParameterRecord {
        directory_sequence: sequence,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .iter()
            .copied()
            .map(|value| Token {
                value: TokenValue::Integer(value),
                span: 0..0,
            })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    }
}

fn token_parameter_record(sequence: u32, values: Vec<TokenValue>) -> ParameterRecord {
    let parameter_end = values.len();
    ParameterRecord {
        directory_sequence: sequence,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .into_iter()
            .map(|value| Token { value, span: 0..0 })
            .collect(),
        parameter_end,
        comment: Vec::new(),
    }
}
