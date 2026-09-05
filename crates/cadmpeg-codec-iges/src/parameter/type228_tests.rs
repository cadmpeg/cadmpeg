// SPDX-License-Identifier: Apache-2.0
//! Type 228 parameter-table boundary tests.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use super::{
    analyze_trailing_pointer_groups, entity_primary_end, ParameterRecord, Token, TokenValue,
};
use crate::directory::{DirectoryEntry, Status};

fn directory_target(sequence: u32, entity_type: i64, form: i64) -> DirectoryEntry {
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
        form,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

#[test]
fn type228_standard_and_implementor_forms_share_entity_table_boundary() {
    for form in [0, 1, 2, 3, 5001] {
        let association = directory_target(1, 212, 0);
        let source = directory_target(5, 228, form);
        let directory = BTreeMap::from([(1, &association), (5, &source)]);
        let values = [228, 1, 1, 3, 1, 1, 1, 1, 0];
        let record = ParameterRecord {
            directory_sequence: 5,
            line_range: 1..2,
            bytes: Vec::new(),
            tokens: values
                .into_iter()
                .map(|value| Token {
                    value: TokenValue::Integer(value),
                    span: 0..0,
                })
                .collect(),
            parameter_end: values.len(),
            comment: Vec::new(),
        };

        assert_eq!(entity_primary_end(&record, &directory), Some(6));
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        let groups = analysis.groups().expect("Type 228 table boundary");
        assert_eq!(groups.token_start, 6);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
        assert!(groups.properties().copied().collect::<Vec<_>>().is_empty());
    }
}
