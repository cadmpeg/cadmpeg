// SPDX-License-Identifier: Apache-2.0

use super::DesignParameterScopeDraft;
use crate::records::{Located, ReferenceRun};

impl DesignParameterScopeDraft {
    pub(crate) fn with_fixture_layout(mut self) -> Self {
        self.paired_byte_offset = self.byte_offset + self.frame_length;
        self.locate_fixture_references();
        self.kind_offset =
            self.reference_count_offset + 12 + 11 * self.reference_members.len() as u64;
        self.layout_fixture_tail();
        self
    }

    pub(crate) fn locate_fixture_references(&mut self) {
        self.reference_members = ReferenceRun::located(
            self.reference_members
                .values()
                .copied()
                .enumerate()
                .map(|(ordinal, value)| Located {
                    value,
                    offset: self.reference_count_offset + 5 + 11 * ordinal as u64,
                })
                .collect(),
        );
    }

    pub(crate) fn layout_fixture_tail(&mut self) {
        self.feature_ordinal_offset = self.paired_byte_offset - 72;
        self.previous_history_state_id_offset = self.previous_history_state_id.map(|previous| {
            self.history_state_id.get_or_insert(previous + 1);
            self.feature_ordinal_offset + 30
        });
    }
}
