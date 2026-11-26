// SPDX-License-Identifier: Apache-2.0
//! The decode wrapper's names for the records each give-up drops (§3.3).

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;

use super::loss::DecodeLossCode;
use super::DecodeResult;
use crate::unknown::part::{
    dropped_ids, dropped_with_ids, extend_tail, given_up_id, record_unreadable_kind, GivenUpRecord,
};

/// One `record.unreadable` of the report and the records it drops.
struct Charge {
    /// The loss's index in the report.
    at: usize,
    /// The record or stub the loss names.
    named: String,
    dropped: BTreeSet<String>,
    marked: BTreeSet<String>,
    stated: Vec<String>,
    /// Whether the wrapper adds to the loss's tail.
    changed: bool,
}

impl DecodeResult {
    /// Names in each `record.unreadable` every record dropped with its
    /// give-up, once for the whole report (§3.3): each record a decoder
    /// dropped only because it needs a given-up record, as its
    /// `record.dropped-with` note states, and each record of the document
    /// that needs a given-up record, which the wrapper drops or marks as the
    /// CADIR reader does. A record reached from two give-ups is named by the
    /// first in the report. A record with a `record.unreadable` of its own
    /// keeps it. A `record.dropped-with` note whose needed record no give-up
    /// reaches becomes a `record.unreadable` of its own record. The notes
    /// leave the report.
    ///
    /// # Errors
    ///
    /// A resource refusal for the release or for the restated loss text.
    pub(super) fn name_dropped_records(
        &mut self,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        let with = DecodeLossCode::RecordDroppedWith.kind();
        let mut edges = Vec::new();
        self.report.losses.retain(|loss| {
            let edge = (loss.code == with)
                .then(|| dropped_with_ids(&loss.message))
                .flatten();
            if let Some((record, needs)) = edge {
                edges.push((record.to_owned(), needs.to_owned()));
            }
            edge.is_none()
        });
        let unreadable = record_unreadable_kind();
        let mut charges = self
            .report
            .losses
            .iter()
            .enumerate()
            .filter(|(_, loss)| loss.code == unreadable)
            .filter_map(|(at, loss)| {
                Some(Charge {
                    at,
                    named: given_up_id(&loss.message)?.to_owned(),
                    dropped: dropped_ids(&loss.message)
                        .into_iter()
                        .map(str::to_owned)
                        .collect(),
                    marked: BTreeSet::new(),
                    stated: Vec::new(),
                    changed: false,
                })
            })
            .collect::<Vec<_>>();
        if charges.is_empty() && edges.is_empty() {
            return Ok(());
        }
        let mut needed_by = BTreeMap::<String, Vec<String>>::new();
        for (record, needs) in &edges {
            needed_by
                .entry(needs.clone())
                .or_default()
                .push(record.clone());
        }
        let mut own = charges
            .iter()
            .map(|charge| charge.named.clone())
            .collect::<BTreeSet<_>>();
        let mut owner = BTreeMap::<String, usize>::new();
        for (index, charge) in charges.iter().enumerate() {
            for id in std::iter::once(&charge.named).chain(&charge.dropped) {
                owner.entry(id.clone()).or_insert(index);
            }
        }
        for index in 0..charges.len() {
            reach(&mut charges, index, &needed_by, &own, &mut owner);
        }
        // A record whose need no give-up reaches is given up for that need,
        // from the first such record whose own need is no such record.
        let dependents = edges
            .iter()
            .map(|(record, _)| record.as_str())
            .collect::<BTreeSet<_>>();
        let unreached = |record: &str, owner: &BTreeMap<String, usize>, own: &BTreeSet<String>| {
            !owner.contains_key(record) && !own.contains(record)
        };
        for pass in [false, true] {
            for (record, needs) in &edges {
                if !unreached(record, &owner, &own)
                    || (!pass
                        && dependents.contains(needs.as_str())
                        && unreached(needs, &owner, &own))
                {
                    continue;
                }
                self.report.losses.push(
                    GivenUpRecord::new(
                        record,
                        format_args!("it needs {needs}, which no give-up of the decode names"),
                    )
                    .loss(),
                );
                own.insert(record.clone());
                owner.insert(record.clone(), charges.len());
                charges.push(Charge {
                    at: self.report.losses.len() - 1,
                    named: record.clone(),
                    dropped: BTreeSet::new(),
                    marked: BTreeSet::new(),
                    stated: Vec::new(),
                    changed: false,
                });
                let index = charges.len() - 1;
                reach(&mut charges, index, &needed_by, &own, &mut owner);
            }
        }
        let mut ids = Vec::new();
        let mut listed = BTreeSet::new();
        for charge in &charges {
            for id in std::iter::once(&charge.named).chain(&charge.dropped) {
                if listed.insert(id.clone()) {
                    ids.push(id.clone());
                }
            }
        }
        for (id, released) in crate::give_up::release_lost(ctx, &mut self.ir, &ids)? {
            let Some(&index) = owner.get(&id) else {
                continue;
            };
            let stated = released.stated();
            let charge = &mut charges[index];
            for record in released.dropped {
                if !owner.contains_key(&record) && !own.contains(&record) {
                    owner.insert(record.clone(), index);
                    charge.dropped.insert(record);
                    charge.changed = true;
                }
            }
            charge.changed |= !released.marked.is_empty() || !stated.is_empty();
            charge.marked.extend(released.marked);
            charge.stated.extend(stated);
        }
        for charge in charges.into_iter().filter(|charge| charge.changed) {
            let message = &mut self.report.losses[charge.at].message;
            extend_tail(message, charge.dropped, charge.marked, charge.stated);
            ctx.retain_bytes(u64_from_index(message.len()), "given-up record loss")?;
        }
        Ok(())
    }
}

/// Names in the charge at `index` each record that needs, transitively, a
/// record it names, other than one with a give-up of its own or one an
/// earlier charge names.
fn reach(
    charges: &mut [Charge],
    index: usize,
    needed_by: &BTreeMap<String, Vec<String>>,
    own: &BTreeSet<String>,
    owner: &mut BTreeMap<String, usize>,
) {
    let charge = &mut charges[index];
    let mut frontier = std::iter::once(charge.named.clone())
        .chain(charge.dropped.iter().cloned())
        .collect::<Vec<_>>();
    while let Some(id) = frontier.pop() {
        for record in needed_by.get(&id).into_iter().flatten() {
            if own.contains(record) || owner.contains_key(record) {
                continue;
            }
            owner.insert(record.clone(), index);
            charge.dropped.insert(record.clone());
            charge.changed = true;
            frontier.push(record.clone());
        }
    }
}
