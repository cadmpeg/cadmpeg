// SPDX-License-Identifier: Apache-2.0
//! The stored states of one ASM history as topology records, for the design
//! states of a timeline (§5.4).
//!
//! The walk from the head undoes the changes of each `delta_state` in turn,
//! so each visit holds the record table of one stored state. A state's
//! records are those its table decodes to, as the model blob is decoded,
//! confined to the bodies of the current geometry, the bodies the head
//! does not hold that are no sketch region, and the consumed bodies that
//! the timeline position of the state keeps (§5.5). A state whose table
//! takes the revision of a slot from the board of a later state that updates
//! that slot twice is not read. A record table whose estimated retained
//! bytes do not fit the retained-byte allowance left is not read.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::{u64_from_index, DecodeContext, WorkBudget};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, Topology};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::units::Tolerances;
use cadmpeg_ir::unknown::UnknownRecord;

use crate::brep::Brep;
use crate::decode::geometry_admission::{self, Note};
use crate::history_records::{AsmDeltaState, AsmEntityChangeKind};

#[cfg(test)]
mod tests;

/// The least record work, in table entries, that the stored states of one
/// decode may spend.
pub(crate) const MAX_STATE_WORK: usize = 1 << 22;

/// The record work, in table entries, that each byte of the model blobs
/// with a history adds to [`MAX_STATE_WORK`].
pub(crate) const STATE_WORK_PER_BYTE: usize = 4;

/// What the stored states of one decode may spend: record-table entries,
/// and the retained bytes that the session allows.
pub(crate) struct Budget {
    work: WorkBudget,
    /// The most retained bytes that one record-table entry of a decoded
    /// state has cost.
    rate: Cell<u64>,
}

impl Budget {
    /// A budget of `work` table entries.
    pub(crate) fn new(work: usize) -> Self {
        Self {
            work: WorkBudget::new(work),
            rate: Cell::new(0),
        }
    }

    /// Whether a record table of `entries` entries fits the retained-byte
    /// allowance left, at the highest cost per entry seen.
    fn admits(&self, ctx: &DecodeContext<'_>, entries: usize) -> bool {
        let remaining = ctx.retained_allowance();
        let Some(estimate) = self.rate.get().checked_mul(u64_from_index(entries)) else {
            ctx.observe_retained_allowance(remaining, u64::MAX);
            return false;
        };
        ctx.observe_retained_allowance(remaining, estimate);
        estimate < remaining
    }

    /// Takes into the cost per entry the bytes retained since `before`, the
    /// available retained bytes before a decode of `entries` table entries.
    fn spend(&self, ctx: &DecodeContext<'_>, before: u64, entries: usize) {
        if let Some(rate) = before
            .checked_sub(ctx.retained_bytes_available())
            .and_then(|cost| cost.checked_div(u64_from_index(entries)))
            .and_then(|rate| rate.checked_add(1))
        {
            self.rate.set(self.rate.get().max(rate));
        }
    }
}

/// The model blob of one ASM history.
pub(crate) struct Blob<'a> {
    pub(crate) bytes: &'a [u8],
    /// The entry name of the blob.
    pub(crate) stream: &'a str,
    pub(crate) width: RefWidth,
    /// The namespace that qualifies the blob's record ids, where the decode
    /// selects several model blobs.
    pub(crate) namespace: Option<&'a str>,
}

/// One state of a history, with the consumed bodies it keeps.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Want {
    pub(crate) state: i64,
    /// The body-map keys of the consumed bodies of the head that the state
    /// holds.
    pub(crate) consumed: BTreeSet<u64>,
}

/// The records of one stored state.
pub(crate) struct StateRecords {
    pub(crate) records: Topology,
    /// The native records of the state that its records leave undecoded.
    pub(crate) unknowns: Vec<UnknownRecord>,
    /// What the admission of its records leaves unstated.
    pub(crate) notes: Vec<Note>,
}

/// The states of one history that a timeline's states use.
pub(crate) struct Read {
    /// The state id of the head.
    pub(crate) head: i64,
    /// The records of the head that the current bodies reach.
    pub(crate) head_records: Topology,
    /// The body of the head that each key of the read's consumed keys
    /// selects.
    pub(crate) head_consumed: BTreeMap<u64, BodyId>,
    /// The bodies of the head that are not current, that no consumed key
    /// selects, and that are no sketch region.
    pub(crate) head_unnamed: BTreeSet<BodyId>,
    /// Each state of the wanted states, newest first, with its records, or
    /// why they are not read.
    pub(crate) states: Vec<(Want, Result<StateRecords, String>)>,
}

/// The states `wanted` of the history of `blob`, whose head holds the
/// `current` bodies and the consumed bodies that `keys` select. The outer
/// error is a decode ceiling. The inner error says why the history gives no
/// states. The decoded states charge `budget`.
pub(crate) fn read(
    ctx: &DecodeContext<'_>,
    blob: &Blob<'_>,
    current: &BTreeSet<BodyId>,
    keys: &BTreeSet<u64>,
    wanted: &BTreeSet<Want>,
    tolerances: &Tolerances,
    budget: &Budget,
) -> Result<Result<Read, String>, CodecError> {
    let Some(header) = cadmpeg_asm::asm_header::parse(blob.bytes) else {
        return Ok(Err("the blob has no ASM header".to_owned()));
    };
    let before = ctx.retained_bytes_available();
    let unbound = super::decode_unbound(ctx, blob.bytes, blob.stream, blob.width)?;
    budget.spend(ctx, before, 0);
    let Some((states, archive)) = unbound else {
        return Ok(Err(
            "the history or its record archive does not decode".to_owned()
        ));
    };
    let Some(active_count) = super::archived_active_record_count(&states)?
        .or_else(|| super::insert_only_active_record_count(&states))
        .and_then(|count| i64::try_from(count).ok())
    else {
        return Ok(Err("the active record count is not stated".to_owned()));
    };
    let archived_ids = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let subtypes =
        super::history_bodies::archive_subtypes(&archive, header.metadata.save_format_version);
    let decoder = StateDecoder {
        ctx,
        blob,
        header: &header.metadata,
        subtypes: &subtypes,
        archive: &archive,
        tolerances,
        budget,
    };
    let mut head = None::<Result<(i64, Head), String>>;
    let mut read = Vec::new();
    let mut stopped = None::<String>;
    let mut failure = None::<CodecError>;
    let mut undone = None::<usize>;
    let mut ambiguous = Ambiguous::default();
    let complete = super::walk_entity_versions_hashed(
        &states,
        active_count,
        &archived_ids,
        |index, versions| {
            if failure.is_some() {
                return;
            }
            if let Some(later) = undone.replace(index) {
                ambiguous.undo(&states[later]);
            }
            let state_id = states[index].state_id;
            if head.is_none() {
                match decoder.measured(versions.len(), || decoder.head(versions, current, keys)) {
                    Ok(found) => head = Some(found.map(|found| (state_id, found))),
                    Err(error) => {
                        failure = Some(error);
                        return;
                    }
                }
            }
            let Some(Ok((_, found))) = &head else {
                return;
            };
            for want in wanted.iter().filter(|want| want.state == state_id) {
                if let Some(reason) = &stopped {
                    read.push((want.clone(), Err(reason.clone())));
                    continue;
                }
                if let Some((slot, by)) = ambiguous.first() {
                    let reason = format!("slot {slot} is updated twice by state {by}");
                    stopped = Some(format!("a later state is not read ({reason})"));
                    read.push((want.clone(), Err(reason)));
                    continue;
                }
                match decoder.measured(versions.len(), || {
                    decoder.state(versions, current, found, &want.consumed)
                }) {
                    Ok(records) => {
                        if let Err(reason) = &records {
                            stopped = Some(format!("a later state is not read ({reason})"));
                        }
                        read.push((want.clone(), records));
                    }
                    Err(error) => {
                        failure = Some(error);
                        return;
                    }
                }
            }
        },
    );
    if let Some(error) = failure {
        return Err(error);
    }
    if !complete {
        return Ok(Err("the walk of its states is incomplete".to_owned()));
    }
    match head {
        Some(Ok((head, found))) => Ok(Ok(Read {
            head,
            head_records: found.records,
            head_consumed: found.consumed,
            head_unnamed: found.unnamed,
            states: read,
        })),
        Some(Err(reason)) => Ok(Err(reason)),
        None => Ok(Err("the history stores no state".to_owned())),
    }
}

/// The head of a history.
struct Head {
    bodies: BTreeSet<BodyId>,
    /// Its body of each consumed body-map key that selects one.
    consumed: BTreeMap<u64, BodyId>,
    /// Its bodies that are not current, that no consumed key selects, and
    /// that are no sketch region.
    unnamed: BTreeSet<BodyId>,
    /// The records its current bodies reach.
    records: Topology,
}

/// What decodes the record table of one state of a history.
struct StateDecoder<'a, 'c> {
    ctx: &'a DecodeContext<'c>,
    blob: &'a Blob<'a>,
    header: &'a cadmpeg_asm::kernel_header::KernelHeader,
    subtypes: &'a cadmpeg_asm::nurbs::toks::SubtypeTable,
    archive: &'a super::HistoricalRecordArchive,
    tolerances: &'a Tolerances,
    budget: &'a Budget,
}

impl StateDecoder<'_, '_> {
    /// Runs `decode` of a record table of `entries` entries and charges the
    /// bytes it retains to the budget.
    fn measured<T>(
        &self,
        entries: usize,
        decode: impl FnOnce() -> Result<T, CodecError>,
    ) -> Result<T, CodecError> {
        let before = self.ctx.retained_bytes_available();
        let out = decode();
        self.budget.spend(self.ctx, before, entries);
        out
    }

    /// The head, or why it is not read.
    fn head(
        &self,
        versions: &super::binding::FastMap<i64, i64>,
        current: &BTreeSet<BodyId>,
        consumed: &BTreeSet<u64>,
    ) -> Result<Result<Head, String>, CodecError> {
        let brep = match self.brep(versions)? {
            Ok(brep) => brep,
            Err(reason) => return Ok(Err(format!("its head is not read ({reason})"))),
        };
        let bodies = brep
            .asm
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect::<BTreeSet<_>>();
        if let Some(body) = current.iter().find(|body| !bodies.contains(*body)) {
            return Ok(Err(format!("its head holds no body {body}")));
        }
        let regions = brep.sketch_region_bodies();
        let mut keyed = BTreeMap::new();
        for key in consumed {
            match crate::brep::resolve_body_selector(brep.asm.body_native_keys.iter(), *key) {
                Ok(Some(body)) => {
                    keyed.insert(*key, body.clone());
                }
                Ok(None) => {}
                Err(error) => {
                    return Ok(Err(format!("its head does not select key {key}: {error}")))
                }
            }
        }
        let selected = keyed.values().collect::<BTreeSet<_>>();
        let unnamed = bodies
            .iter()
            .filter(|body| {
                !(current.contains(*body) || selected.contains(body) || regions.contains(*body))
            })
            .cloned()
            .collect();
        Ok(self
            .records(brep, current.iter().cloned().collect())?
            .map(|records| Head {
                bodies,
                consumed: keyed,
                unnamed,
                records: records.records,
            }))
    }

    /// The records of one state that is not the head: those its `current`
    /// bodies reach, those of the consumed bodies of `head` that `consumed`
    /// keys, and those of each body the head does not hold that is no sketch
    /// region.
    fn state(
        &self,
        versions: &super::binding::FastMap<i64, i64>,
        current: &BTreeSet<BodyId>,
        head: &Head,
        consumed: &BTreeSet<u64>,
    ) -> Result<Result<StateRecords, String>, CodecError> {
        let brep = match self.brep(versions)? {
            Ok(brep) => brep,
            Err(reason) => return Ok(Err(reason)),
        };
        let regions = brep.sketch_region_bodies();
        let kept_consumed = consumed
            .iter()
            .filter_map(|key| head.consumed.get(key))
            .collect::<BTreeSet<_>>();
        let kept = brep
            .asm
            .bodies
            .iter()
            .map(|body| &body.id)
            .filter(|body| {
                current.contains(*body)
                    || kept_consumed.contains(body)
                    || !(head.bodies.contains(*body) || regions.contains(*body))
            })
            .cloned()
            .collect();
        self.records(brep, kept)
    }

    /// The B-rep graph of the record table `versions` maps to, with the ids
    /// of the model blob.
    fn brep(
        &self,
        versions: &super::binding::FastMap<i64, i64>,
    ) -> Result<Result<Brep, String>, CodecError> {
        if !self.budget.admits(self.ctx, versions.len()) {
            return Ok(Err(
                "its record table does not fit the retained-byte allowance left".to_owned(),
            ));
        }
        if !self.budget.work.charge_by(versions.len()) {
            return Ok(Err("the state work bound is reached".to_owned()));
        }
        let Some(table) = super::history_bodies::materialize_hashed_table(versions, self.archive)
        else {
            return Ok(Err("its record table is incomplete".to_owned()));
        };
        let mut brep = match crate::brep::decode_state(
            self.ctx,
            &table,
            self.blob.bytes,
            (self.header, self.subtypes),
            self.blob.stream,
        ) {
            Ok(brep) => brep,
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => return Ok(Err(format!("its record table does not decode: {error}"))),
        };
        if let Some(namespace) = self.blob.namespace {
            brep.qualify_ids(self.ctx, crate::ids::ID_FORMAT, namespace)?;
        }
        Ok(Ok(brep))
    }

    /// The topology records of `brep` that the bodies `kept` reach, as the
    /// model geometry admits them, and the native records they leave
    /// undecoded.
    fn records(
        &self,
        mut brep: Brep,
        kept: Vec<BodyId>,
    ) -> Result<Result<StateRecords, String>, CodecError> {
        brep.retain_bodies(self.ctx, kept)?;
        let mut scratch = CadIr::empty();
        scratch.tolerances = *self.tolerances;
        let remainder = cadmpeg_asm::brep::transfer::transfer_into_ir(
            self.ctx,
            &mut scratch,
            cadmpeg_ir::codec_id!("f3d"),
            brep.asm,
        )
        .map(|(_, remainder)| remainder);
        let mut notes = Vec::new();
        let admitted = remainder.and_then(|remainder| {
            geometry_admission::admit_noted(self.ctx, &mut scratch, &mut notes)
                .map(|()| remainder.unknowns)
        });
        match admitted {
            Ok(unknowns) => Ok(Ok(StateRecords {
                records: scratch.geometry,
                unknowns,
                notes,
            })),
            Err(error @ CodecError::ResourceLimit(_)) => Err(error),
            Err(error) => Ok(Err(format!("its records do not transfer: {error}"))),
        }
    }
}

/// The state id of each `delta_state` of the history of `blob` and the
/// native id of its record, read from the state headers.
pub(crate) fn state_ids(blob: &Blob<'_>) -> Option<BTreeMap<i64, String>> {
    let history = super::decode_header_index(blob.bytes, blob.stream, blob.width)?;
    Some(
        history
            .states()
            .iter()
            .map(|state| (state.state_id, state.id.clone()))
            .collect(),
    )
}

/// The slots whose revision a state of a walk does not fix: each slot that
/// the board of a later state updates twice, and that no board between them
/// changes, with the id of that later state.
#[derive(Default)]
struct Ambiguous(BTreeMap<i64, i64>);

impl Ambiguous {
    /// Adds the reversal of the board of `state`, the earliest state undone.
    fn undo(&mut self, state: &AsmDeltaState) {
        let mut updated = BTreeSet::new();
        let mut twice = BTreeSet::new();
        for change in state
            .bulletin_boards
            .iter()
            .flat_map(|board| &board.changes)
        {
            if let AsmEntityChangeKind::Update { new, .. } = change.kind {
                if !updated.insert(new) {
                    twice.insert(new);
                }
            }
        }
        for change in state
            .bulletin_boards
            .iter()
            .flat_map(|board| &board.changes)
        {
            let slot = match change.kind {
                AsmEntityChangeKind::Insert { new } | AsmEntityChangeKind::Update { new, .. } => {
                    new
                }
                AsmEntityChangeKind::Delete { old } => old,
            };
            if twice.contains(&slot) {
                self.0.insert(slot, state.state_id);
            } else {
                self.0.remove(&slot);
            }
        }
    }

    /// The first ambiguous slot, with the state that updates it twice.
    fn first(&self) -> Option<(i64, i64)> {
        self.0.iter().next().map(|(slot, by)| (*slot, *by))
    }
}
