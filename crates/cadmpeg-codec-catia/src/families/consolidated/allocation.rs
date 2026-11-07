// SPDX-License-Identifier: Apache-2.0
//! Source-scoped addresses of consolidated allocations.

use std::collections::{HashMap, HashSet};

use crate::wire::records::{records_are_contiguous, ConsolidatedRecord, WidthCodedToken};

#[derive(Debug, Clone, Copy)]
enum AddressTarget {
    Unique(usize),
    Ambiguous,
}

#[derive(Debug, Clone, Copy)]
struct AllocationLocation {
    scope: usize,
    address: Option<u64>,
}

pub(crate) struct AllocationJournal<'a> {
    records: &'a [ConsolidatedRecord],
    locations: Vec<AllocationLocation>,
    targets: HashMap<(usize, u64), AddressTarget>,
    indices: HashMap<(usize, usize), AddressTarget>,
    source_prefix_scopes: HashSet<usize>,
}

impl<'a> AllocationJournal<'a> {
    pub(crate) fn new(records: &'a [ConsolidatedRecord]) -> Self {
        let mut locations = Vec::with_capacity(records.len());
        let mut targets = HashMap::new();
        let mut indices = HashMap::new();
        let mut source_prefix_scopes = HashSet::new();
        let mut seen_sources = HashSet::new();
        let mut address = Some(0_u64);
        let mut scope = 0;
        for (index, record) in records.iter().enumerate() {
            indices
                .entry((record.source_index(), record.source_range().start))
                .and_modify(|target| *target = AddressTarget::Ambiguous)
                .or_insert(AddressTarget::Unique(index));
            if index == 0 || !records_are_contiguous(&records[index - 1..=index]) {
                scope = index;
                address = Some(0);
                if seen_sources.insert(record.source_index()) {
                    source_prefix_scopes.insert(scope);
                }
            }
            let increment = WidthCodedToken::new(record.width(), record.header_token())
                .ok()
                .and_then(WidthCodedToken::compact_operand);
            address = address
                .zip(increment)
                .and_then(|(value, increment)| value.checked_add(u64::from(increment.value())));
            let Some(value) = address else {
                locations.push(AllocationLocation {
                    scope,
                    address: None,
                });
                continue;
            };
            locations.push(AllocationLocation {
                scope,
                address: Some(value),
            });
            targets
                .entry((scope, value))
                .and_modify(|target| *target = AddressTarget::Ambiguous)
                .or_insert(AddressTarget::Unique(index));
        }
        Self {
            records,
            locations,
            targets,
            indices,
            source_prefix_scopes,
        }
    }

    pub(crate) fn index(&self, source: usize, source_offset: usize) -> Option<usize> {
        match self.indices.get(&(source, source_offset))? {
            AddressTarget::Unique(index) => Some(*index),
            AddressTarget::Ambiguous => None,
        }
    }

    pub(crate) fn record(&self, index: usize) -> Option<&'a ConsolidatedRecord> {
        self.records.get(index)
    }

    pub(crate) fn address(&self, index: usize) -> Option<u64> {
        let location = self.locations.get(index)?;
        let record = self.records.get(index)?;
        if self.index(record.source_index(), record.source_range().start)? != index {
            return None;
        }
        let address = location.address?;
        matches!(
            self.targets.get(&(location.scope, address)),
            Some(AddressTarget::Unique(target)) if *target == index
        )
        .then_some(address)
    }

    pub(crate) fn absolute_address(&self, index: usize) -> Option<u64> {
        self.source_prefix_scopes
            .contains(&self.locations.get(index)?.scope)
            .then(|| self.address(index))?
    }

    pub(crate) fn resolve_address(
        &self,
        origin: usize,
        address: u64,
    ) -> Option<&'a ConsolidatedRecord> {
        let location = self.locations.get(origin)?;
        self.address(origin)?;
        match self.targets.get(&(location.scope, address))? {
            AddressTarget::Unique(index) => self.records.get(*index),
            AddressTarget::Ambiguous => None,
        }
    }

    pub(crate) fn resolve_weighted_distance(
        &self,
        origin: usize,
        distance: u32,
    ) -> Option<&'a ConsolidatedRecord> {
        self.resolve_address(
            origin,
            self.address(origin)?.checked_sub(u64::from(distance))?,
        )
    }

    pub(crate) fn resolve_record_distance(
        &self,
        origin: usize,
        distance: u32,
    ) -> Option<&'a ConsolidatedRecord> {
        self.record_in_scope(origin, origin.checked_sub(usize::try_from(distance).ok()?)?)
    }

    pub(crate) fn resolve_owned_child(
        &self,
        origin: usize,
        ordinal: u32,
    ) -> Option<&'a ConsolidatedRecord> {
        let target = origin
            .checked_add(1)?
            .checked_add(usize::try_from(ordinal).ok()?)?;
        self.record_in_scope(origin, target)
    }

    fn record_in_scope(&self, origin: usize, target: usize) -> Option<&'a ConsolidatedRecord> {
        (self.locations.get(origin)?.scope == self.locations.get(target)?.scope)
            .then(|| self.records.get(target))?
    }
}

#[cfg(test)]
mod tests;
