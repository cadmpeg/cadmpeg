// SPDX-License-Identifier: Apache-2.0
//! Work-estimate tests for the mesh quotient search.

use super::direction_work_estimate;

#[test]
fn direction_work_estimate_states_no_figure_the_work_counter_cannot_hold() {
    assert_eq!(direction_work_estimate([0usize, 1, 2].into_iter()), Some(7));
    let widest = usize::BITS as usize - 1;
    assert_eq!(
        direction_work_estimate([widest].into_iter()),
        Some(1usize << widest)
    );
    for unknown in [usize::BITS as usize, usize::MAX] {
        assert_eq!(direction_work_estimate([unknown].into_iter()), None);
    }
    assert_eq!(direction_work_estimate([widest, widest].into_iter()), None);
}
