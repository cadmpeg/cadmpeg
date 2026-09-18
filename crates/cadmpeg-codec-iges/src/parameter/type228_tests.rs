// SPDX-License-Identifier: Apache-2.0
//! Type 228 parameter-table boundary tests.
#![allow(clippy::unwrap_used)]

use crate::parameter::tests::directory_target_with_form;
use std::collections::BTreeMap;

use super::{
    analyze_trailing_pointer_groups, entity_primary_end, ParameterRecord, Token, TokenValue,
};

#[test]
fn type228_standard_and_implementor_forms_share_entity_table_boundary() {
    for form in [0, 1, 2, 3, 5001] {
        let association = directory_target_with_form(1, 212, 0);
        let source = directory_target_with_form(5, 228, form);
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
