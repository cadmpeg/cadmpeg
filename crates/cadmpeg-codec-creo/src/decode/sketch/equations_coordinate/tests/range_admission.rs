// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

#[test]
fn section_remaining_variable_range_refuses_work_and_preserves_result() {
    let variables = crate::test_support::assert_work_boundaries(
        &["creo section remaining variable scan"],
        |ctx| super::super::section_remaining_variables(ctx, 3),
    );
    assert_eq!(variables, BTreeSet::from([0, 1, 2]));
}

fn solved_linear_system(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
) -> Result<Option<Vec<(usize, f64)>>, cadmpeg_core::CodecError> {
    let mut matrix = vec![
        super::super::SectionLinearRow {
            coefficients: BTreeMap::from([(0, 1.0)]),
            rhs: 1.0,
        },
        super::super::SectionLinearRow {
            coefficients: BTreeMap::from([(0, 2.0)]),
            rhs: 2.0,
        },
    ];
    super::super::uniquely_solved_linear_variables(ctx, &mut matrix, 1)
}

#[test]
fn section_pivot_column_range_refuses_work_and_preserves_solution() {
    let solution = crate::test_support::assert_work_boundaries(
        &["creo section pivot column scan"],
        solved_linear_system,
    );
    assert_eq!(solution, Some(vec![(0, 1.0)]));
}

#[test]
fn section_free_column_range_refuses_work_and_preserves_solution() {
    let solution = crate::test_support::assert_work_boundaries(
        &["creo section free column scan"],
        solved_linear_system,
    );
    assert_eq!(solution, Some(vec![(0, 1.0)]));
}
