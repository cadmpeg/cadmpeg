// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use super::super::{Affine, OccurrenceDefinition, OccurrenceExpansion, RealPrecision};
use crate::parameter::{ParameterRecord, Token, TokenValue};

#[test]
fn occurrence_expansion_reports_a_missing_instance_directory_entry() {
    let record = ParameterRecord::from_test_tokens(
        1,
        1..2,
        b"408,3,0,0,0,1;".to_vec(),
        6,
        [
            (408, 0..3),
            (3, 4..5),
            (0, 6..7),
            (0, 8..9),
            (0, 10..11),
            (1, 12..13),
        ]
        .into_iter()
        .map(|(value, span)| Token {
            value: TokenValue::Integer(value),
            span,
        })
        .collect(),
        Vec::new(),
    );
    let entries = BTreeMap::new();
    let records = BTreeMap::from([(1, &record)]);
    let definitions = BTreeMap::from([(
        3,
        OccurrenceDefinition {
            members: Vec::new(),
            transform: Affine::identity(),
        },
    )]);
    let neutral_links = BTreeMap::new();
    let expansion = OccurrenceExpansion {
        entries: &entries,
        records: &records,
        definitions: &definitions,
        neutral_links: &neutral_links,
        length_factor: 1.0,
        precision: RealPrecision {
            single_significance: 7,
            double_significance: 15,
        },
        output_limit: 10,
        depth_limit: 10,
        ctx: None,
    };
    let mut path = Vec::new();
    let mut occurrences = Vec::new();
    let mut depth_truncated_at = None;
    let mut malformed = BTreeSet::new();
    let result = expansion
        .expand(
            1,
            Affine::identity(),
            &mut path,
            &mut occurrences,
            &mut depth_truncated_at,
            &mut malformed,
        )
        .unwrap();
    assert_eq!(malformed, BTreeSet::from([1]));
    assert_eq!(result, None);
    assert!(occurrences.is_empty());
    assert!(path.is_empty());
    assert_eq!(depth_truncated_at, None);
}
