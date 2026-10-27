// SPDX-License-Identifier: Apache-2.0
//! Counts and coverage borrowed from the sequential Deltas owner.

use crate::records::deltas::{DeltasRecords, EventContent};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub(crate) struct Census<'r, 's> {
    records: &'r DeltasRecords<'s>,
}

impl<'r, 's> Census<'r, 's> {
    pub(crate) fn new(records: &'r DeltasRecords<'s>) -> Self {
        Self { records }
    }

    pub(crate) fn bytes_decoded(self) -> usize {
        self.records.suffix().start
    }

    pub(crate) fn instance_count(self) -> usize {
        self.records
            .events()
            .iter()
            .filter(|event| matches!(event.content(), EventContent::Instance(_)))
            .count()
    }

    pub(crate) fn schema_count(self) -> usize {
        self.records.definitions().len()
    }

    pub(crate) fn full_counts(self) -> BTreeMap<&'static str, usize> {
        let mut counts = BTreeMap::new();
        for event in self.records.events() {
            let EventContent::Instance(instance) = event.content() else {
                continue;
            };
            let family =
                super::record_kind::RecordKind::try_from(instance.definition().plan().kind)
                    .map_or("CUSTOM", |kind| kind.name());
            *counts.entry(family).or_default() += 1;
        }
        counts
    }
}
