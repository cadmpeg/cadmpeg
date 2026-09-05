// SPDX-License-Identifier: Apache-2.0
//! FEM Parameter Data boundary tests.
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use super::{
    analyze_trailing_pointer_groups, entity_primary_end, groups_for_candidate,
    structural_pointer_group_candidates, ParameterRecord, Token, TokenValue,
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

fn record(sequence: u32, values: &[i64]) -> ParameterRecord {
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

fn token_record(sequence: u32, values: &[TokenValue]) -> ParameterRecord {
    ParameterRecord {
        directory_sequence: sequence,
        line_range: 1..2,
        bytes: Vec::new(),
        tokens: values
            .iter()
            .cloned()
            .map(|value| Token { value, span: 0..0 })
            .collect(),
        parameter_end: values.len(),
        comment: Vec::new(),
    }
}

#[test]
fn fixed_and_counted_fem_boundaries_stop_before_pointer_groups() {
    let association = directory_target(1, 212, 0);
    let node = directory_target(3, 134, 0);
    let element = directory_target(5, 136, 0);
    let displacement = directory_target(7, 138, 0);
    let nodal_results = directory_target(9, 146, 3);
    let element_results = directory_target(11, 148, 3);
    let load = directory_target(13, 418, 0);
    let directory = BTreeMap::from([
        (1, &association),
        (3, &node),
        (5, &element),
        (7, &displacement),
        (9, &nodal_results),
        (11, &element_results),
        (13, &load),
    ]);

    let cases = [
        (&node, record(3, &[134, 1, 2, 3, 0, 1, 1, 0]), 5),
        (&element, record(5, &[136, 1, 1, 3, 0, 1, 1, 0]), 5),
        (
            &displacement,
            record(7, &[138, 1, 1, 1, 3, 1, 1, 2, 3, 4, 5, 6, 1, 1, 0]),
            12,
        ),
        (
            &nodal_results,
            record(9, &[146, 1, 0, 1, 2, 1, 7, 3, 8, 9, 1, 1, 0]),
            10,
        ),
        (
            &element_results,
            record(
                11,
                &[148, 1, 0, 1, 1, 1, 1, 9, 3, 1, 1, 1, 1, 4, 1, 2, 1, 1, 0],
            ),
            16,
        ),
        (&load, record(13, &[418, 2, 1, 3, 5, 7, 1, 1, 0]), 6),
    ];

    for (case_index, (_entry, record, expected_end)) in cases.into_iter().enumerate() {
        assert_eq!(entity_primary_end(&record, &directory), Some(expected_end));
        let groups = analyze_trailing_pointer_groups(&record, &directory)
            .groups()
            .unwrap_or_else(|| panic!("FEM trailing pointer groups case {case_index}"));
        assert_eq!(groups.token_start, expected_end);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    }
}

#[test]
fn fem_table_boundaries_precede_fully_valid_structural_alternatives() {
    let association_entries = (1..=9)
        .map(|sequence| directory_target(sequence, 402, 7))
        .collect::<Vec<_>>();
    let cases = [
        (100, 136, 0, &[136, 1, 1, 3, 1, 1, 1, 0][..], 5),
        (
            101,
            138,
            0,
            &[138, 1, 1, 1, 3, 1, 1, 2, 3, 4, 5, 5, 1, 1, 0][..],
            12,
        ),
        (
            102,
            146,
            3,
            &[146, 1, 0, 1, 2, 1, 7, 4, 1, 1, 1, 1, 0][..],
            10,
        ),
        (
            103,
            148,
            3,
            &[148, 1, 0, 1, 1, 1, 1, 9, 3, 1, 1, 1, 1, 4, 1, 1, 1, 1, 0][..],
            16,
        ),
        (104, 418, 0, &[418, 2, 1, 4, 5, 7, 1, 1, 0][..], 6),
    ];

    for (sequence, entity_type, form, values, expected_end) in cases {
        let target = directory_target(sequence, entity_type, form);
        let mut directory: BTreeMap<u32, &DirectoryEntry> = association_entries
            .iter()
            .map(|entry| (entry.sequence, entry))
            .collect();
        directory.insert(sequence, &target);
        let record = record(sequence, values);
        let candidates = structural_pointer_group_candidates(&record);
        assert!(
            candidates.iter().any(|candidate| {
                candidate.token_start < expected_end
                    && groups_for_candidate(&record, &directory, *candidate)
                        .is_some_and(|groups| groups.fully_valid())
            }),
            "Type {entity_type} Form {form}"
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1);
        assert_eq!(analysis.valid_candidate_count(), 1);
        let groups = analysis.groups().expect("FEM table boundary");
        assert_eq!(groups.token_start, expected_end);
        assert_eq!(groups.associations().copied().collect::<Vec<_>>(), vec![1]);
    }
}

#[test]
fn malformed_fem_counted_spans_do_not_enable_generic_recovery() {
    let association = directory_target(1, 402, 7);
    let cases = [
        (
            200,
            136,
            0,
            vec![
                TokenValue::Integer(136),
                TokenValue::Integer(1),
                TokenValue::Omitted,
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
            ],
        ),
        (
            201,
            138,
            0,
            vec![
                TokenValue::Integer(138),
                TokenValue::Integer(-1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
            ],
        ),
        (
            202,
            146,
            3,
            vec![
                TokenValue::Integer(146),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Real(2.0),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
            ],
        ),
        (
            203,
            148,
            3,
            vec![
                TokenValue::Integer(148),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
            ],
        ),
        (
            204,
            418,
            0,
            vec![
                TokenValue::Integer(418),
                TokenValue::Real(1.0),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(1),
                TokenValue::Integer(0),
            ],
        ),
    ];

    for (sequence, entity_type, form, values) in cases {
        let target = directory_target(sequence, entity_type, form);
        let directory = BTreeMap::from([(1, &association), (sequence, &target)]);
        let record = token_record(sequence, &values);
        assert_eq!(entity_primary_end(&record, &directory), Some(values.len()));
        assert!(analyze_trailing_pointer_groups(&record, &directory)
            .groups()
            .is_none());
    }
}

#[test]
fn element_results_boundary_rejects_an_incomplete_variable_item() {
    let element_results = directory_target(1, 148, 3);
    let directory = BTreeMap::from([(1, &element_results)]);
    let record = record(1, &[148, 1, 0, 1, 1, 1, 9, 3, 1, 1, 0, 1, 1, 4, 1, 2]);

    assert_eq!(
        entity_primary_end(&record, &directory),
        Some(record.tokens.len())
    );
    assert!(analyze_trailing_pointer_groups(&record, &directory)
        .groups()
        .is_none());
}
