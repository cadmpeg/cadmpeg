// SPDX-License-Identifier: Apache-2.0
//! History binding work-estimate unit tests.
#![allow(clippy::unwrap_used)]

use crate::history::{
    complete_table_binding_cost, TableBindingCost, HISTORY_BINDING_MAX_WORK_UNITS,
    HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY,
};

#[test]
fn history_binding_estimate_skips_past_the_codec_bound() {
    let entries =
        usize::try_from(HISTORY_BINDING_MAX_WORK_UNITS / HISTORY_TOPOLOGY_WORK_UNITS_PER_ENTRY)
            .expect("entry bound fits usize");
    assert_eq!(
        complete_table_binding_cost([entries]),
        TableBindingCost::Admitted
    );
    assert_eq!(
        complete_table_binding_cost([entries, 1]),
        TableBindingCost::Exceeded
    );
    assert_eq!(
        complete_table_binding_cost([usize::MAX, 1]),
        TableBindingCost::Exceeded
    );
}
