// SPDX-License-Identifier: Apache-2.0
//! Source-scoped addresses of consolidated allocations.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::wire::bytes::{allocation_reference, AllocationReference, AllocationReferenceEncoding};
use crate::wire::records::{
    parse_consolidated_allocation_record, records_are_contiguous, ConsolidatedFamily,
    ConsolidatedRecord, SourceExtent,
};

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
    #[cfg(test)]
    pub(crate) fn new(records: &'a [ConsolidatedRecord]) -> Self {
        Self::build(None, records)
    }

    /// Build an allocation journal that bridges skipped width-4 allocation
    /// frames (such as `Flag13` governing roots) across logical gaps inside
    /// a source stream while keeping the same source-prefix address scope.
    pub(crate) fn with_sources(
        data: &[u8],
        sources: &[Vec<SourceExtent>],
        records: &'a [ConsolidatedRecord],
    ) -> Self {
        Self::build(Some((data, sources)), records)
    }

    fn build(
        backing: Option<(&[u8], &[Vec<SourceExtent>])>,
        records: &'a [ConsolidatedRecord],
    ) -> Self {
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
                let bridged_increment = if index > 0
                    && records[index - 1].source_index() == record.source_index()
                    && records[index - 1].source_range().end < record.source_range().start
                {
                    backing.and_then(|(data, sources)| {
                        bridge_allocation_gap(
                            data,
                            sources.get(record.source_index())?,
                            records[index - 1].source_range().end,
                            record.source_range().start,
                        )
                    })
                } else {
                    None
                };
                if let Some(delta) = bridged_increment {
                    address = address.and_then(|value| apply_signed_delta(value, delta));
                } else {
                    scope = index;
                    address = Some(0);
                    if seen_sources.insert(record.source_index()) {
                        source_prefix_scopes.insert(scope);
                    }
                }
            }
            let delta = signed_token_delta(record.width(), record.header_token());
            address = address
                .zip(delta)
                .and_then(|(value, delta)| apply_signed_delta(value, delta));
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

fn resolve_journal_reference<'a>(
    journal: &AllocationJournal<'a>,
    origin: usize,
    reference: AllocationReference,
) -> Option<(usize, &'a ConsolidatedRecord)> {
    let record = match reference.encoding {
        AllocationReferenceEncoding::BackwardDistance | AllocationReferenceEncoding::WidthCoded => {
            journal.resolve_weighted_distance(origin, reference.value)?
        }
        AllocationReferenceEncoding::OwnedChild
        | AllocationReferenceEncoding::TaggedU8
        | AllocationReferenceEncoding::TaggedU16
        | AllocationReferenceEncoding::TaggedU24 => journal.resolve_address(
            origin,
            journal
                .address(origin)?
                .checked_add(u64::from(reference.value))?
                .checked_add(1)?,
        )?,
        AllocationReferenceEncoding::Selector2 => return None,
    };
    let target = journal.index(record.source_index(), record.source_range().start)?;
    Some((target, record))
}

/// Extract exact `[start_vertex, end_vertex]` allocation-journal identities from
/// consolidated `b2 03 5e` edge records whose endpoint references resolve to
/// `b2 03 5d` vertex records in the same allocation scope.
#[cfg(test)]
pub(crate) fn consolidated_edge_vertex_references(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    records: &[ConsolidatedRecord],
) -> BTreeMap<u32, [u32; 2]> {
    let journal = AllocationJournal::with_sources(data, sources, records);
    consolidated_edge_vertex_references_with_journal(data, sources, records, &journal)
}

pub(crate) fn consolidated_edge_vertex_references_with_journal(
    data: &[u8],
    sources: &[Vec<SourceExtent>],
    records: &[ConsolidatedRecord],
    journal: &AllocationJournal<'_>,
) -> BTreeMap<u32, [u32; 2]> {
    let mut resolved = BTreeMap::<u32, Option<[u32; 2]>>::new();
    for (index, record) in records.iter().enumerate() {
        if record.family() != ConsolidatedFamily::B || record.class() != 0x5e {
            continue;
        }
        let Some(edge_tag) = journal
            .absolute_address(index)
            .and_then(|addr| u32::try_from(addr).ok())
        else {
            continue;
        };
        let Some(payload) = record.payload_bytes(data, sources) else {
            continue;
        };
        let mut at = 0usize;
        let mut refs = Vec::new();
        let mut terminal_len = 0usize;
        while at < payload.len() && refs.len() < 6 {
            let start = at;
            let Some(reference) = allocation_reference(&payload, &mut at) else {
                refs.clear();
                break;
            };
            terminal_len = at - start;
            refs.push(reference);
        }
        if at != payload.len() || terminal_len != 1 {
            continue;
        }
        let [start_ref, end_ref] = match refs.as_slice() {
            [_, start, end, _, _, _] => [*start, *end],
            [start, end, _, _, _] => [*start, *end],
            _ => continue,
        };
        let resolve_vertex = |reference: AllocationReference| -> Option<u32> {
            let (target_idx, target) = resolve_journal_reference(journal, index, reference)?;
            if target.family() != ConsolidatedFamily::B || target.class() != 0x5d {
                return None;
            }
            u32::try_from(journal.absolute_address(target_idx)?).ok()
        };
        let (Some(start_tag), Some(end_tag)) = (resolve_vertex(start_ref), resolve_vertex(end_ref))
        else {
            continue;
        };
        let pair = [start_tag, end_tag];
        resolved
            .entry(edge_tag)
            .and_modify(|existing| {
                if *existing != Some(pair) {
                    *existing = None;
                }
            })
            .or_insert(Some(pair));
    }
    resolved
        .into_iter()
        .filter_map(|(edge, pair)| pair.map(|pair| (edge, pair)))
        .collect()
}

/// Walk a logical byte gap inside one record source and return the signed sum
/// of its allocation deltas when every byte of the gap is closed by valid
/// consolidated allocation frames.
fn bridge_allocation_gap(
    data: &[u8],
    extents: &[SourceExtent],
    start: usize,
    end: usize,
) -> Option<i64> {
    if start >= end {
        return None;
    }
    let gap_len = end - start;
    // Copy only the bounded logical gap bytes (handles extent-spanning gaps too).
    let mut logical = 0usize;
    let mut gap = Vec::new();
    let mut borrowed: Option<&[u8]> = None;
    for extent in extents {
        let range = extent.range();
        let ext_len = range.end.checked_sub(range.start)?;
        let ext_end = logical.checked_add(ext_len)?;
        if ext_end > start && logical < end {
            let copy_start = start.max(logical) - logical;
            let copy_end = end.min(ext_end) - logical;
            let slice = data.get(range.start + copy_start..range.start + copy_end)?;
            if borrowed.is_none() && gap.is_empty() && slice.len() == gap_len {
                borrowed = Some(slice);
                break;
            }
            if gap.is_empty() {
                gap.reserve(gap_len);
            }
            gap.extend_from_slice(slice);
        }
        logical = ext_end;
        if logical >= end {
            break;
        }
    }
    let bytes = match borrowed {
        Some(slice) => slice,
        None if gap.len() == gap_len => gap.as_slice(),
        None => return None,
    };
    let mut pos = 0usize;
    let mut total = 0_i64;
    while pos < bytes.len() {
        let rec = parse_consolidated_allocation_record(bytes, pos, bytes.len())?;
        let delta = signed_token_delta(rec.width(), rec.header_token())?;
        total = total.checked_add(delta)?;
        let next = rec.range()?.end;
        if next <= pos {
            return None;
        }
        pos = next;
    }
    Some(total)
}

fn signed_token_delta(
    width: crate::wire::records::ConsolidatedFrameWidth,
    header_token: u32,
) -> Option<i64> {
    let raw = header_token.to_le_bytes();
    let bytes = raw.get(..usize::from(u8::from(width)))?;
    if bytes.len() == 4 && bytes[0] == 0x0e {
        let value = crate::wire::bytes::u32_le_24(bytes, 1)?;
        return Some(-(i64::from(value) + 1));
    }
    let mut at = 0;
    let reference = crate::wire::bytes::allocation_reference(bytes, &mut at)?;
    if at != bytes.len() {
        return None;
    }
    match reference.encoding {
        crate::wire::bytes::AllocationReferenceEncoding::BackwardDistance
        | crate::wire::bytes::AllocationReferenceEncoding::WidthCoded => {
            Some(i64::from(reference.value))
        }
        crate::wire::bytes::AllocationReferenceEncoding::OwnedChild
        | crate::wire::bytes::AllocationReferenceEncoding::TaggedU8
        | crate::wire::bytes::AllocationReferenceEncoding::TaggedU16
        | crate::wire::bytes::AllocationReferenceEncoding::TaggedU24 => {
            Some(-(i64::from(reference.value) + 1))
        }
        crate::wire::bytes::AllocationReferenceEncoding::Selector2 => None,
    }
}

fn apply_signed_delta(address: u64, delta: i64) -> Option<u64> {
    if delta >= 0 {
        address.checked_add(u64::try_from(delta).ok()?)
    } else {
        address.checked_sub(delta.unsigned_abs())
    }
}

#[cfg(test)]
mod tests;
