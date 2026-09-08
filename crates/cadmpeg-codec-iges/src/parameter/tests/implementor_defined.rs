// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use super::super::{analyze_trailing_pointer_groups, entity_primary_end, TokenValue};
use super::{directory_target, integer_parameter_record, token_parameter_record};

#[test]
fn type406_implementor_defined_forms_use_common_count_boundary() {
    let association = directory_target(3, 212);
    let property = directory_target(5, 406);
    for (form, values, expected_start, expected_associations, expected_properties) in [
        (5557_i64, vec![406, 1, 42, 1, 3, 1, 5], 3, vec![3], vec![5]),
        (
            6007_i64,
            vec![406, 2, 10, 20, 1, 3, 1, 5],
            4,
            vec![3],
            vec![5],
        ),
        (
            9999_i64,
            vec![406, 2, 10, 20, 1, 3, 0],
            4,
            vec![3],
            Vec::new(),
        ),
    ] {
        let mut source = directory_target(1, 406);
        source.form = form;
        let directory = BTreeMap::from([(1, &source), (3, &association), (5, &property)]);
        let record = integer_parameter_record(1, &values);
        assert_eq!(
            entity_primary_end(&record, &directory),
            Some(expected_start)
        );

        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 1, "Form {form}");
        assert_eq!(analysis.valid_candidate_count(), 1, "Form {form}");
        let groups = analysis
            .groups()
            .expect("implementor-defined property table boundary");
        assert_eq!(groups.token_start, expected_start, "Form {form}");
        assert_eq!(
            groups.associations().copied().collect::<Vec<_>>(),
            expected_associations,
            "Form {form}"
        );
        assert_eq!(
            groups.properties().copied().collect::<Vec<_>>(),
            expected_properties,
            "Form {form}"
        );
    }
}

#[test]
fn type406_implementor_defined_malformed_count_or_span_suppresses_generic_recovery() {
    let mut source = directory_target(1, 406);
    source.form = 6007;
    let association = directory_target(3, 212);
    let directory = BTreeMap::from([(1, &source), (3, &association)]);
    for (values, expected_end) in [
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Real(2.0),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(0),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(1),
                TokenValue::Integer(3),
                TokenValue::Integer(0),
            ],
            7,
        ),
        (
            vec![
                TokenValue::Integer(406),
                TokenValue::Integer(2),
                TokenValue::Integer(10),
                TokenValue::Integer(20),
                TokenValue::Integer(2),
                TokenValue::Integer(3),
            ],
            4,
        ),
    ] {
        let record = token_parameter_record(1, values);
        assert_eq!(entity_primary_end(&record, &directory), Some(expected_end));
        let analysis = analyze_trailing_pointer_groups(&record, &directory);
        assert_eq!(analysis.candidate_count(), 0);
        assert_eq!(analysis.valid_candidate_count(), 0);
        assert!(analysis.groups().is_none());
    }
}
