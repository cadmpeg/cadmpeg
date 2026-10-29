// SPDX-License-Identifier: Apache-2.0
//! Whole-record removal evidence from an actual helper give-up, never a message.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::ops::Range;

use cadmpeg_ir::report::decode::DecodeReport;
use cadmpeg_ir::source_fidelity::parts::PartState;
use cadmpeg_ir::{CadIr, CodecFormat, SourceFidelity};
use serde_json::Value;

use super::{graph::Document, Allowances};

/// One decoded document and its original, independently retained source facts.
///
/// The document alone cannot prove a B1 or given-up source interval. Keeping the
/// actual fidelity alongside it prevents a report from licensing arbitrary ids.
#[derive(Clone, Copy)]
pub struct DecodeSnapshot<'a> {
    document: &'a CadIr,
    fidelity: &'a SourceFidelity,
}

impl<'a> DecodeSnapshot<'a> {
    /// Borrow the actual decoded document and its matching source fidelity.
    pub const fn new(document: &'a CadIr, fidelity: &'a SourceFidelity) -> Self {
        Self { document, fidelity }
    }
}

impl Allowances {
    /// Build finite whole-record §4.5 allowances from a checked helper outcome.
    ///
    /// Every failure must have its real deterministic unknown stub, exact raw
    /// source image and layout, and one Error `decode/record.unreadable` loss.
    /// Only prior record layouts wholly contained in those original intervals
    /// seed removal. Required reference fields propagate removal; variable list
    /// entries use the comparator's existing list-holder pruning.
    ///
    /// A partial overlap never permits dropping the owner or changing its fields.
    /// Partial-field allowances, export allowances and Cross projection remain
    /// separate requirements. Native dispositions or human loss text grant none.
    ///
    /// # Errors
    /// Refuses inconsistent scope/format, invalid failure homes, missing source
    /// bytes, unmatched unreadable losses or malformed record inventories.
    pub fn given_up<'source>(
        before: DecodeSnapshot<'_>,
        outcome: DecodeSnapshot<'_>,
        report: &DecodeReport,
        root_stream: &str,
        source: impl FnMut(&str, Range<u64>) -> Option<&'source [u8]>,
    ) -> Result<Self, String> {
        before
            .document
            .check_scope()
            .map_err(|error| error.to_string())?;
        outcome
            .document
            .check_scope()
            .map_err(|error| error.to_string())?;
        if report.scope() != outcome.document.scope {
            return Err("given-up report scope differs from the actual document".into());
        }
        let codec = CodecFormat::parse(report.format())
            .ok_or("given-up report has no registered source format")?;
        for snapshot in [before, outcome] {
            if snapshot
                .document
                .source
                .as_ref()
                .is_some_and(|source| source.format() != codec.as_str())
            {
                return Err("given-up report format differs from its document".into());
            }
        }
        crate::contract::parts::check_one_home(
            codec,
            &outcome.document.native,
            outcome.fidelity,
            &report.losses,
            root_stream,
            source,
        )?;
        let extents = outcome
            .fidelity
            .parts()
            .values()
            .filter(|part| matches!(part.state(), PartState::GivenUp {}))
            .map(|part| (part.stream(), part.offset(), part.byte_len()))
            .collect::<BTreeSet<_>>();
        // check_one_home requires one exact loss for every part; reject the
        // converse mismatch as well, including a loss with no actual part.
        for loss in &report.losses {
            if loss.code.to_string() != "decode/record.unreadable" {
                continue;
            }
            let provenance = loss
                .provenance
                .as_ref()
                .ok_or("unreadable loss has no typed source provenance")?;
            let key = (
                provenance.stream_name().unwrap_or(root_stream),
                provenance.offset,
                provenance.byte_len().unwrap_or(0),
            );
            if provenance.format() != codec.as_str() || !extents.contains(&key) {
                return Err("unreadable loss has no exact given-up source part".into());
            }
        }
        let value = serde_json::to_value(before.document).map_err(|error| error.to_string())?;
        let records = Document::read(&value, &BTreeSet::new())?;
        let mut removed = BTreeSet::new();
        for record in &records.records {
            let Some(layout) = before.fidelity.record_layout().get(&record.id) else {
                continue;
            };
            let end = layout
                .offset()
                .checked_add(layout.byte_len())
                .ok_or("prior record extent overflows")?;
            let enclosing = extents
                .range((layout.stream(), 0, 0)..=(layout.stream(), layout.offset(), u64::MAX))
                .next_back();
            if layout.byte_len() != 0
                && enclosing.is_some_and(|(stream, start, length)| {
                    *stream == layout.stream()
                        && layout.offset() >= *start
                        && start.checked_add(*length).is_some_and(|stop| end <= stop)
                })
            {
                removed.insert(record.id.clone());
            }
        }
        // Use §5.3's reference definition: a string equal to an actual record
        // identity. References within a list entry do not drop its holder.
        let mut users = BTreeMap::<String, BTreeSet<String>>::new();
        for record in &records.records {
            let mut required = BTreeSet::new();
            required_references(&record.value, &records.ids, &mut required);
            for target in required {
                users.entry(target).or_default().insert(record.id.clone());
            }
        }
        let mut queue = removed.iter().cloned().collect::<VecDeque<_>>();
        while let Some(target) = queue.pop_front() {
            for owner in users.get(&target).into_iter().flatten() {
                if removed.insert(owner.clone()) {
                    queue.push_back(owner.clone());
                }
            }
        }
        Ok(Self {
            removed,
            removed_fields: BTreeSet::new(),
        })
    }
}

pub(super) fn required_references(
    value: &Value,
    identities: &BTreeMap<String, usize>,
    required: &mut BTreeSet<String>,
) {
    match value {
        Value::String(id) if identities.contains_key(id) => {
            required.insert(id.clone());
        }
        Value::Object(fields) => {
            for (key, value) in fields {
                if identities.contains_key(key) {
                    required.insert(key.clone());
                }
                required_references(value, identities, required);
            }
        }
        // These records list a reference; the existing comparison prunes only
        // the affected entry, retaining independent fields on its list holder.
        Value::Array(_) => {}
        _ => {}
    }
}

#[cfg(test)]
mod tests;
