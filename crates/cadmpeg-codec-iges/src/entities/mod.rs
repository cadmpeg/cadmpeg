// SPDX-License-Identifier: Apache-2.0
//! Typed IGES entity accessors and neutral projection.

use cadmpeg_core::decode::{ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;

use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{
    dropped_with, gives_up, needs, Cause, DecodeParts, GivenUpPart, GivenUpRecord, Part,
};
use cadmpeg_ir::{CadIr, SourceFidelity, StreamName};

use crate::directory::{DirectoryEntry, QuarantinedDirectoryRecord};
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, QuarantinedParameterRecord};
use crate::representation::Representation;

/// The part kind of an entity whose neutral projection the decoder gives up:
/// the entity's Directory Entry pair.
const ENTITY_PART: &str = "entity";
/// The part kind of a quarantined Directory Entry record.
pub(crate) const DIRECTORY_ENTRY_PART: &str = "directory-entry";
/// The part kind of an entity's quarantined Parameter Data record.
pub(crate) const PARAMETER_DATA_PART: &str = "parameter-data";

/// The card image the reader frames records in: the source itself for Fixed
/// ASCII input, and the fixed-card image that the reader normalizes Compressed
/// ASCII and Binary input to, in the stream `fixed-card-image`. Every record
/// offset of the reader is an offset in this image.
pub(crate) struct CardImage<'a> {
    bytes: &'a [u8],
    stream: Option<StreamName>,
}

impl<'a> CardImage<'a> {
    pub(crate) fn new(bytes: &'a [u8], representation: Representation) -> Self {
        let stream = (representation != Representation::FixedAscii)
            .then(|| cadmpeg_ir::stream_name!("fixed-card-image"));
        Self { bytes, stream }
    }

    /// The part of `kind` that holds the `length` bytes from `offset`, for
    /// the record of Directory sequence `sequence`.
    pub(crate) fn part(
        &self,
        kind: &'static str,
        sequence: u32,
        offset: u64,
        length: u64,
    ) -> Result<Part<'a>, CodecError> {
        let bytes = usize::try_from(offset)
            .ok()
            .zip(usize::try_from(length).ok())
            .and_then(|(start, length)| self.bytes.get(start..start.checked_add(length)?))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES record D{sequence} lies outside the card image"
                ))
            })?;
        Ok(self
            .placed(Part::at("iges", kind, offset, bytes, [ArenaClass::Model]))
            .named(format!("D{sequence}")))
    }

    /// The stub id of the part of `kind` at `offset`.
    pub(crate) fn part_id(&self, kind: &'static str, offset: u64) -> Result<Identity, CodecError> {
        Ok(self
            .placed(Part::at("iges", kind, offset, &[], []))
            .id()?
            .into())
    }

    fn placed(&self, part: Part<'a>) -> Part<'a> {
        match &self.stream {
            Some(stream) => part.in_stream(stream.clone()),
            None => part,
        }
    }
}

/// Gives up `part` for `cause`, with the error text `error`.
pub(crate) fn give_up_part(
    ctx: &DecodeContext<'_>,
    part: Part<'_>,
    cause: Cause,
    error: impl fmt::Display,
) -> Result<GivenUpPart, CodecError> {
    ctx.give_up_part(part, cause, CodecError::malformed(error))
}

/// The ids by which the decode's give-ups name given-up records, for a pass that drops an entity only because it needs one.
pub(crate) struct GivenUpIds<'a> {
    image: &'a CardImage<'a>,
    quarantined: BTreeMap<u32, Identity>,
}

impl<'a> GivenUpIds<'a> {
    pub(crate) fn new(
        image: &'a CardImage<'a>,
        directory: &[QuarantinedDirectoryRecord],
        parameters: &[QuarantinedParameterRecord],
    ) -> Result<Self, CodecError> {
        let mut quarantined = BTreeMap::new();
        for record in directory {
            quarantined.insert(
                record.sequence,
                image.part_id(DIRECTORY_ENTRY_PART, record.source_offset)?,
            );
        }
        for record in parameters {
            if let std::collections::btree_map::Entry::Vacant(slot) =
                quarantined.entry(record.sequence)
            {
                slot.insert(image.part_id(PARAMETER_DATA_PART, record.source_offset())?);
            }
        }
        Ok(Self { image, quarantined })
    }

    /// The id of `entry`'s entity part: the stub of its give-up, or the id
    /// by which a give-up that drops it names it.
    fn entity(&self, entry: &DirectoryEntry) -> Result<Identity, CodecError> {
        self.image.part_id(ENTITY_PART, entry.source_offset)
    }

    /// The stub of the quarantined record that keeps the entity of
    /// `sequence` from the projection.
    fn quarantined(&self, sequence: u32) -> Option<&Identity> {
        self.quarantined.get(&sequence)
    }
}

fn attributed_note(
    entry: &DirectoryEntry,
    code: IgesLossCode,
    message: fmt::Arguments<'_>,
) -> LossNote {
    code.note(message.to_string())
        .with_provenance(entry.loss_provenance())
}

/// Pushes a loss on `entry` that gives up no record of it: the projection
/// keeps the entity with less content, or the entity's type or form has no
/// neutral record.
fn push_attributed_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    code: IgesLossCode,
    message: fmt::Arguments<'_>,
) {
    losses.push(attributed_note(entry, code, message));
}

/// Gives up `entry`'s entity for `cause`, with the text of the loss `code`
/// and `message`.
fn push_given_up_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    code: IgesLossCode,
    cause: Cause,
    message: fmt::Arguments<'_>,
) {
    give_up(losses, entry, cause, attributed_note(entry, code, message));
}

/// The id of the native record of the entity of Directory sequence
/// `sequence`.
fn entity_record(sequence: u32) -> Identity {
    Identity::compose(
        &cadmpeg_ir::identity_namespace!("iges", "entity", "directory"),
        sequence,
    )
}

/// Pushes `why` and the give-up of `entry`'s entity for `cause`.
/// [`give_up_entities`] makes the pair the one give-up of the entity's part,
/// with `why`'s text.
fn give_up(losses: &mut Vec<LossNote>, entry: &DirectoryEntry, cause: Cause, why: LossNote) {
    let given_up = GivenUpRecord::caused_by(entity_record(entry.sequence), cause, &why);
    losses.push(why);
    losses.push(given_up.loss());
}

/// The id of the neutral record that the projection mints for `entry`'s
/// entity and that names the entity's records in a give-up. `None` where the
/// entity's records take their ids from another entity, from a path, or from
/// a position in a list.
fn projected_record(entry: &DirectoryEntry) -> Option<Identity> {
    let stem = crate::ids::Stem::directory(entry.sequence);
    Some(match entry.entity_type {
        100 | 102 | 104 | 110 | 112 | 126 | 130 => crate::ids::curve(&stem).into(),
        116 | 125 => crate::ids::point(&stem).into(),
        108 | 114 | 118 | 120 | 122 | 128 | 140 | 190 | 192 | 194 | 196 | 198 => {
            crate::ids::surface(&stem).into()
        }
        143 | 144 => crate::ids::face(&stem).into(),
        186 => crate::ids::body(&stem).into(),
        514 if entry.form == 2 => crate::ids::shell(&stem).into(),
        _ => return None,
    })
}

/// Pushes `why` for `entry`'s entity, which the projection drops. Where it
/// drops the entity because the entity of Directory sequence `needs` has no
/// projected record, it pushes the marker that [`give_up_entities`] settles:
/// the entity's records, named by `record`, are dropped with `needs` where
/// the decode gives `needs` up, and otherwise the entity is given up on its
/// own for `cause`, or for [`Cause::Unreadable`] where `needs` has no
/// Directory Entry. `record` is the id of [`projected_record`], or of the
/// record that the pass mints for the entity in its place. The marker's
/// severity is that of `cause`. Where `needs` is `None`, it pushes the
/// entity's own give-up for `cause`.
fn give_up_needing(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    record: Option<Identity>,
    needs: Option<u32>,
    cause: Cause,
    why: LossNote,
) {
    let Some(needs) = needs else {
        give_up(losses, entry, cause, why);
        return;
    };
    losses.push(why);
    losses.push(
        dropped_with(
            record.unwrap_or_else(|| entity_record(entry.sequence)),
            entity_record(needs),
        )
        .with_provenance(entry.loss_provenance())
        .with_severity(cause.severity()),
    );
}

/// Gives up `record`, a neutral record that a pass mints from `entry`'s
/// decoded entity and then drops, for `cause`, with `why`'s text. Where it
/// drops the record because the entity of Directory sequence `needs` has no
/// projected record, it pushes `why` and the marker that
/// [`give_up_entities`] settles, with the severity of `cause`. Neither
/// carries `entry`'s entity tag, so the settlement of the entity's own
/// give-up leaves them.
fn give_up_record(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    record: impl Into<Identity>,
    needs: Option<u32>,
    cause: Cause,
    why: LossNote,
) {
    let mut provenance = entry.loss_provenance();
    provenance.tag = None;
    match needs {
        Some(needs) => {
            losses.push(why);
            losses.push(
                dropped_with(record, entity_record(needs))
                    .with_provenance(provenance)
                    .with_severity(cause.severity()),
            );
        }
        None => losses.push(
            GivenUpRecord::caused_by(record, cause, &why)
                .at(provenance)
                .loss(),
        ),
    }
}

/// Gives up `record`, a neutral record that a pass mints from `entry`'s
/// decoded entity and then drops, for `cause`, with `why`'s text, naming
/// `marked`, the record that states its unstated children for the drop
/// (§5.6).
fn give_up_marking(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    record: impl Into<Identity>,
    marked: Option<impl Into<Identity>>,
    cause: Cause,
    why: &LossNote,
) {
    let mut provenance = entry.loss_provenance();
    provenance.tag = None;
    losses.push(
        GivenUpRecord::caused_by(record, cause, why)
            .at(provenance)
            .marking(marked)
            .loss(),
    );
}

/// The cause of the give-up of the entity of a [`give_up_needing`] marker
/// that the entity gives up on its own.
fn marker_cause(marker: &LossNote) -> Cause {
    if marker.severity == Cause::Unreadable.severity() {
        Cause::Unreadable
    } else {
        Cause::Undecoded
    }
}

/// The index of the `why` that [`give_up`] or [`give_up_needing`] pushed
/// before the give-up or marker at `at` of the entity of `sequence`.
fn why_at(losses: &[LossNote], at: usize, sequence: u32) -> Option<usize> {
    let before = at.checked_sub(1)?;
    let why = losses.get(before)?;
    (entity_tag(why) == Some(sequence) && !gives_up(&why.code) && needs(why).is_none())
        .then_some(before)
}

/// Removes the losses at `removed` from `losses`.
fn remove_at(losses: &mut Vec<LossNote>, removed: &BTreeSet<usize>) {
    let mut at = 0;
    losses.retain(|_| {
        let kept = !removed.contains(&at);
        at += 1;
        kept
    });
}

/// Removes each give-up and `record.dropped-with` marker of an entity that
/// the projection holds, whose `why` stays as a loss that gives up no record.
/// Keeps the first give-up of each other entity and removes the others with
/// their `why`.
fn settle_given_up(losses: &mut Vec<LossNote>, decoded: &BTreeSet<u32>) {
    let mut charged = BTreeSet::new();
    let mut removed = BTreeSet::new();
    for (at, loss) in losses.iter().enumerate() {
        let marker = needs(loss).is_some();
        let Some(sequence) = entity_tag(loss).filter(|_| marker || gives_up(&loss.code)) else {
            continue;
        };
        if decoded.contains(&sequence) {
            removed.insert(at);
        } else if !marker && !charged.insert(sequence) {
            removed.insert(at);
            removed.extend(why_at(losses, at, sequence));
        }
    }
    remove_at(losses, &removed);
}

fn given_up_entity(loss: &LossNote) -> Option<u32> {
    if !gives_up(&loss.code) {
        return None;
    }
    entity_tag(loss)
}

/// The record that a [`give_up_needing`] or [`give_up_record`] marker drops,
/// and the Directory sequence of the entity it names as needed.
fn marker(loss: &LossNote) -> Option<(&Identity, u32)> {
    let (record, needed) = needs(loss)?;
    let directory = needed.as_str().strip_prefix("iges:entity:directory#")?;
    Some((record, directory.parse().ok()?))
}

fn entity_tag(loss: &LossNote) -> Option<u32> {
    loss.provenance
        .as_ref()?
        .tag
        .as_deref()?
        .strip_prefix("directory_entry:D")?
        .parse()
        .ok()
}

/// Settles the [`give_up_needing`] markers of the projection, then gives up
/// the part of each entity whose give-up is in `losses`, for the give-up's
/// cause and with the text of its `why`, and puts the part's give-up in
/// place of the pair. The part is the entity's Directory Entry pair. The
/// native entity record and the `cards` arena keep its bytes, so the part
/// keeps their digest. Returns the Directory sequences of the given-up
/// entities, of the entities dropped with them, and of the quarantined
/// records, and the consequence that the give-up of each quarantined record
/// states for the subfigure and network entities dropped with it.
pub(crate) fn give_up_entities(
    losses: &mut Vec<LossNote>,
    directory: &[DirectoryEntry],
    ids: &GivenUpIds<'_>,
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    fidelity: &mut SourceFidelity,
) -> Result<(BTreeSet<u32>, BTreeMap<u32, String>), CodecError> {
    let mut entries = BTreeMap::new();
    for entry in directory {
        entries.insert(entry.sequence, entry);
    }
    let (mut given_up, mut unplaced) = settle_needs(losses, &entries, ids)?;
    given_up.extend(ids.quarantined.keys().copied());
    let mut whys = BTreeSet::new();
    for at in 0..losses.len() {
        let Some(sequence) = given_up_entity(&losses[at]) else {
            continue;
        };
        let (entry, before) = entries
            .get(&sequence)
            .zip(why_at(losses, at, sequence))
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES give-up of D{sequence} has no Directory Entry or cause"
                ))
            })?;
        let cause = if losses[at].code == Cause::Unreadable.kind() {
            Cause::Unreadable
        } else {
            Cause::Undecoded
        };
        let why = &losses[before];
        let length = entry.source_length.ok_or_else(|| {
            CodecError::malformed(format_args!(
                "IGES Directory Entry D{sequence} has no card span"
            ))
        })?;
        let mut part = ids
            .image
            .part(ENTITY_PART, sequence, entry.source_offset, length)?
            .digest_only();
        if ids.quarantined(sequence).is_none() {
            if let Some(dependents) = unplaced.remove(&sequence) {
                part = part.costing(unplaced_costing(&dependents));
            }
        }
        let loss = give_up_part(
            ctx,
            part,
            cause,
            format_args!("{}: {}", why.code, why.message),
        )?
        .commit(ctx, ir, fidelity)?;
        losses[at] = loss;
        whys.insert(before);
        given_up.insert(sequence);
    }
    remove_at(losses, &whys);
    let costing = unplaced
        .iter()
        .map(|(sequence, dependents)| (*sequence, unplaced_costing(dependents)))
        .collect();
    Ok((given_up, costing))
}

/// The consequence that a give-up states for the subfigure and network
/// entities, named in `dependents`, that it drops: the document keeps their
/// native records and states none of their structure.
fn unplaced_costing(dependents: &[String]) -> String {
    let named = dependents.join(", ");
    format!(
        "subfigure and network entities that need it lose their neutral meaning, \
         and their native records stay: {named}"
    )
}

/// The names (`D<n> (Type <t>)`) of the dropped subfigure and network
/// entities, by the Directory sequence of the entity whose give-up drops
/// them.
type Unplaced = BTreeMap<u32, Vec<String>>;

/// Settles each [`give_up_needing`] marker of an entity that has no give-up
/// of its own. The entity is dropped with a needed entity that the decode
/// gives up: one with a give-up, a quarantined one, or one that is itself
/// dropped with such an entity. Its marker becomes the `record.dropped-with`
/// note of the entity's projected record, which names the needed entity as
/// the decode's give-ups name it: the stub of its part, or its projected
/// record where it is dropped. The marker's `why` goes. An entity without a
/// projected record keeps no loss, and an entity that needs it names what it
/// is dropped with. Otherwise the entity is given up on its own, once: none
/// of the entities it needs is given up, or it needs itself through a cycle
/// of such entities, of which the walk gives up one. The cause is
/// [`Cause::Unreadable`] where a sequence that the entity needs has no
/// Directory Entry in `entries` and no quarantined record, because the
/// Directory section is read in full; otherwise it is the marker's cause. A
/// marker that settles nothing goes with its `why`. Each [`give_up_record`]
/// marker is settled the same way, as a record that no entity needs.
/// Returns the Directory sequences of the dropped entities, and each dropped
/// Type 308 or Type 320 definition and Type 408 or Type 420 instance by the
/// entity whose give-up drops it: the end of its chain of needed entities,
/// as the decode's give-ups name it.
fn settle_needs(
    losses: &mut Vec<LossNote>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    ids: &GivenUpIds<'_>,
) -> Result<(BTreeSet<u32>, Unplaced), CodecError> {
    let charged = losses
        .iter()
        .filter_map(given_up_entity)
        .collect::<BTreeSet<_>>();
    let mut markers = Vec::new();
    let mut records = Vec::new();
    let mut heads = BTreeMap::<u32, Identity>::new();
    let mut needs = BTreeMap::<u32, Vec<u32>>::new();
    let mut needed_by = BTreeMap::<u32, Vec<u32>>::new();
    for (at, loss) in losses.iter().enumerate() {
        let Some((record, needed)) = marker(loss) else {
            continue;
        };
        let Some(entity) = entity_tag(loss) else {
            records.push((at, record.clone(), needed));
            continue;
        };
        markers.push((at, entity, needed));
        if record.as_str() != entity_record(entity).as_str() {
            heads.entry(entity).or_insert_with(|| record.clone());
        }
        if !charged.contains(&entity) {
            needs.entry(entity).or_default().push(needed);
            needed_by.entry(needed).or_default().push(entity);
        }
    }
    if markers.is_empty() && records.is_empty() {
        return Ok((BTreeSet::new(), BTreeMap::new()));
    }
    let absent =
        |sequence: u32| !entries.contains_key(&sequence) && ids.quarantined(sequence).is_none();
    let cause = |marker: &LossNote, needed: &[u32]| {
        if needed.iter().any(|sequence| absent(*sequence)) {
            Cause::Unreadable
        } else {
            marker_cause(marker)
        }
    };
    // `Some(needed)`: dropped with `needed`; `None`: given up on its own.
    let mut settled = BTreeMap::<u32, Option<u32>>::new();
    let mut gone = needed_by
        .keys()
        .copied()
        .filter(|needed| charged.contains(needed) || ids.quarantined(*needed).is_some())
        .collect::<VecDeque<_>>();
    drop_dependents(&mut gone, &needed_by, &mut settled);
    for &entity in needs.keys() {
        if settled.contains_key(&entity) {
            continue;
        }
        let mut walked = BTreeSet::new();
        let mut at = entity;
        let own = loop {
            walked.insert(at);
            let next = needs.get(&at).and_then(|targets| {
                targets
                    .iter()
                    .copied()
                    .find(|target| needs.contains_key(target) && !settled.contains_key(target))
            });
            match next {
                None => break at,
                Some(next) if walked.contains(&next) => break next,
                Some(next) => at = next,
            }
        };
        settled.insert(own, None);
        gone.push_back(own);
        drop_dependents(&mut gone, &needed_by, &mut settled);
    }
    let entity_id = |sequence: u32| {
        entries
            .get(&sequence)
            .ok_or_else(|| {
                CodecError::malformed(format_args!(
                    "IGES entity D{sequence} needed by the projection has no Directory Entry"
                ))
            })
            .and_then(|entry| ids.entity(entry))
    };
    let gone = |sequence: u32| {
        charged.contains(&sequence)
            || ids.quarantined(sequence).is_some()
            || settled.contains_key(&sequence)
    };
    // The id by which the decode's give-ups name the gone entity `sequence`:
    // an entity dropped without a projected record is named by what it is
    // dropped with.
    let given_up_id = |mut sequence: u32| -> Result<Identity, CodecError> {
        loop {
            if let Some(stub) = ids.quarantined(sequence) {
                return Ok(stub.clone());
            }
            match settled.get(&sequence) {
                Some(Some(with)) => match heads.get(&sequence) {
                    Some(head) => return Ok(head.clone()),
                    None => sequence = *with,
                },
                _ => return entity_id(sequence),
            }
        }
    };
    let mut written = BTreeSet::new();
    let mut removed = BTreeSet::new();
    for (at, entity, needed) in markers {
        let settlement = settled.get(&entity).copied();
        let why = why_at(losses, at, entity);
        if charged.contains(&entity) || !written.insert(entity) {
            removed.insert(at);
            removed.extend(why);
            continue;
        }
        losses[at] = match settlement {
            Some(Some(with)) if with == needed => {
                let Some(head) = heads.get(&entity) else {
                    removed.insert(at);
                    removed.extend(why);
                    continue;
                };
                let mut note = dropped_with(head.clone(), given_up_id(with)?);
                note.provenance.clone_from(&losses[at].provenance);
                removed.extend(why);
                note
            }
            Some(Some(_)) => {
                written.remove(&entity);
                removed.insert(at);
                removed.extend(why);
                continue;
            }
            _ => {
                let why = why.and_then(|before| losses.get(before)).ok_or_else(|| {
                    CodecError::malformed(format_args!("IGES give-up of D{entity} has no cause"))
                })?;
                let needed = needs.get(&entity).map_or(&[][..], Vec::as_slice);
                GivenUpRecord::caused_by(entity_record(entity), cause(&losses[at], needed), why)
                    .loss()
            }
        };
    }
    for (at, record, needed) in records {
        let provenance = losses[at].provenance.clone();
        let why = at.checked_sub(1).filter(|before| {
            losses.get(*before).is_some_and(|why| {
                !gives_up(&why.code) && cadmpeg_ir::unknown::part::needs(why).is_none()
            })
        });
        losses[at] = if gone(needed) {
            let mut note = dropped_with(record, given_up_id(needed)?);
            note.provenance = provenance;
            note
        } else {
            let cause = cause(&losses[at], &[needed]);
            let why = why.and_then(|before| losses.get(before)).ok_or_else(|| {
                CodecError::malformed(format_args!("IGES give-up of {record} has no cause"))
            })?;
            GivenUpRecord::caused_by(record, cause, why)
                .at(provenance)
                .loss()
        };
        removed.extend(why);
    }
    remove_at(losses, &removed);
    // A subfigure or network entity has no neutral record of its own to
    // name, so the give-up at the end of its chain states it.
    let mut unplaced = Unplaced::new();
    for (&entity, &with) in &settled {
        let (Some(mut at), Some(entry)) = (with, entries.get(&entity)) else {
            continue;
        };
        if !matches!(entry.entity_type, 308 | 320 | 408 | 420) {
            continue;
        }
        while ids.quarantined(at).is_none() {
            match settled.get(&at) {
                Some(Some(next)) => at = *next,
                _ => break,
            }
        }
        unplaced
            .entry(at)
            .or_default()
            .push(format!("D{entity} (Type {})", entry.entity_type));
    }
    let dropped = settled
        .into_iter()
        .filter_map(|(entity, with)| with.map(|_| entity))
        .collect();
    Ok((dropped, unplaced))
}

/// Settles as dropped each entity that needs an entity of `gone`, and
/// then each entity that needs one of those, until none is left.
fn drop_dependents(
    gone: &mut VecDeque<u32>,
    needed_by: &BTreeMap<u32, Vec<u32>>,
    settled: &mut BTreeMap<u32, Option<u32>>,
) {
    while let Some(needed) = gone.pop_front() {
        for &entity in needed_by.get(&needed).into_iter().flatten() {
            if let std::collections::btree_map::Entry::Vacant(slot) = settled.entry(entity) {
                slot.insert(Some(needed));
                gone.push_back(entity);
            }
        }
    }
}

fn entity_note(entry: &DirectoryEntry, reason: fmt::Arguments<'_>) -> LossNote {
    attributed_note(
        entry,
        IgesLossCode::EntityNotProjected,
        format_args!(
            "IGES entity type {} form {} was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    )
}

fn geometry_note(entry: &DirectoryEntry, reason: fmt::Arguments<'_>) -> LossNote {
    attributed_note(
        entry,
        IgesLossCode::GeometryNotProjected,
        format_args!(
            "IGES entity type {} form {} geometry was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    )
}

/// Pushes the loss of an entity that the projection gives up for `cause`.
fn push_entity_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    cause: Cause,
    reason: fmt::Arguments<'_>,
) {
    give_up(losses, entry, cause, entity_note(entry, reason));
}

/// Pushes the loss of an entity that the projection drops, as
/// [`give_up_needing`] does.
fn push_entity_loss_needing(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    needs: Option<u32>,
    cause: Cause,
    reason: fmt::Arguments<'_>,
) {
    give_up_needing(
        losses,
        entry,
        projected_record(entry),
        needs,
        cause,
        entity_note(entry, reason),
    );
}

/// Pushes the finding of a pass that only validates `entry`. The document
/// keeps the entity's native record whether or not it passes, so the
/// finding gives up no record.
fn push_entity_finding(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) {
    losses.push(entity_note(entry, reason));
}

/// Pushes the loss of geometry that the projection gives up for `cause`.
fn push_geometry_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    cause: Cause,
    reason: fmt::Arguments<'_>,
) {
    give_up(losses, entry, cause, geometry_note(entry, reason));
}

/// Pushes the loss of geometry that the projection drops: where it drops it
/// because the entity of Directory sequence `needs` has no projected record,
/// with the marker of [`give_up_needing`], and otherwise with its own
/// give-up for `cause`.
fn push_geometry_loss_needing(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    needs: Option<u32>,
    cause: Cause,
    reason: fmt::Arguments<'_>,
) {
    give_up_needing(
        losses,
        entry,
        projected_record(entry),
        needs,
        cause,
        geometry_note(entry, reason),
    );
}

/// Pushes a geometry loss that gives up no record: the projection keeps the
/// geometry with less content, or the pass only validates an entity whose
/// native record the document keeps.
fn push_retained_geometry_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) {
    losses.push(geometry_note(entry, reason));
}

fn non_resource_error(error: CodecError) -> Result<String, CodecError> {
    match error {
        CodecError::ResourceLimit(_) => Err(error),
        other => Ok(other.to_string()),
    }
}

fn directed_cycle<I: DoubleEndedIterator<Item = u32>>(
    sequence: u32,
    visited: &mut BTreeSet<u32>,
    successors: impl Fn(u32) -> I,
) -> bool {
    if visited.contains(&sequence) {
        return false;
    }
    let mut active = BTreeSet::new();
    let mut stack = vec![(sequence, false)];
    while let Some((current, expanded)) = stack.pop() {
        if expanded {
            active.remove(&current);
            visited.insert(current);
            continue;
        }
        if visited.contains(&current) {
            continue;
        }
        if !active.insert(current) {
            return true;
        }
        stack.push((current, true));
        for target in successors(current).rev() {
            if active.contains(&target) {
                return true;
            }
            if !visited.contains(&target) {
                stack.push((target, false));
            }
        }
    }
    false
}

fn pointer(record: &ParameterRecord, index: usize) -> Option<u32> {
    record.integer(index).and_then(|value| {
        let sequence = u32::try_from(value).ok()?;
        (sequence % 2 == 1).then_some(sequence)
    })
}

fn mirror_flag_valid(value: i64) -> bool {
    matches!(value, 0..=2)
}

fn vertical_text_flag_valid(value: i64) -> bool {
    matches!(value, 0..=1)
}

pub(crate) fn affine_parameter_map(source: [f64; 2], target: [f64; 2]) -> Option<(f64, f64)> {
    let source = cadmpeg_ir::topology::IncreasingParameterInterval::new(source)?;
    let target = cadmpeg_ir::topology::IncreasingParameterInterval::new(target)?;
    let (scale, offset) = source.affine_coefficients_to(target)?;
    Some((scale.get(), offset.get()))
}

mod analytic_surfaces;
pub(crate) mod annotation;
mod brep;
mod composite;
mod conics;
pub(crate) mod copious;
mod csg;
pub(crate) mod curve_conversion;
pub(crate) mod drawing;
pub(crate) mod geometry;
pub(crate) mod nurbs_controls;
mod offsets;
pub(crate) mod ownership;
pub(crate) mod presentation;
mod splines;
pub(crate) mod structure;
mod surfaces;
mod trimming;

#[cfg(test)]
mod tests;
