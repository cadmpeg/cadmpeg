// SPDX-License-Identifier: Apache-2.0
//! The decode wrapper's give-up of a record a decoder stated (§2.8, §3.3).
//!
//! The wrapper gives up a record it cannot state: an id a decoder emitted
//! twice with different values (§2.8), and a record nested past a bound of
//! §2.5. A record given up this way has no source framing, so it is a part
//! whose kind is its arena's key, whose stream is its path in the document,
//! and whose retained bytes are its canonical JSON. It sits in no record, so
//! its `unknowns` stub links to the document.
//!
//! Giving a record up drops, transitively, each record that needs it, and
//! marks each record that holds it by a parent pointer or as a member whose
//! position states no value of the design (§3.3). The CADIR writer drops a
//! record past a bound by the same removal, without the part (§3.4).

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use cadmpeg_core::decode::{ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use serde::Serialize;

use crate::annotations::Removed;
use crate::design::container::Container;
use crate::design::evaluation::StateDelta;
use crate::design::refs::{EntityRef, Member, RecordRef};
use crate::document::{ArenaHome, ArenaName, CadIr};
use crate::ids::{BodyId, ProductDefinitionId};
use crate::report::loss::LossNote;
use crate::schema::ArenaRecord;
use crate::source_fidelity::SourceFidelity;
use crate::unknown::part::GivenUpPart;

mod restate;

pub(crate) use restate::names;
use restate::{Restate, Schema};

/// The arena that holds a record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Arena {
    /// The arena `field` of the home `home` (`geometry`, `model`, `design`,
    /// or `process`).
    Home {
        home: &'static str,
        field: &'static str,
    },
    /// The arena `arena` of the native namespace `codec`.
    Native { codec: String, arena: String },
}

impl Arena {
    /// The registered arena of `home` whose field is `field`.
    pub(crate) fn home(home: &str, field: &str) -> Option<Self> {
        let home = ArenaHome::parse(home)?;
        ArenaName::ALL
            .iter()
            .find(|arena| arena.home() == home && arena.key() == field)
            .map(|arena| Self::Home {
                home: home.as_str(),
                field: arena.key(),
            })
    }

    /// The part kind of a record of this arena.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Home { field, .. } => field,
            Self::Native { .. } => "native",
        }
    }

    /// The classes of the records this arena holds. A native arena can hold
    /// any.
    fn classes(&self) -> Vec<ArenaClass> {
        match self {
            Self::Home { home, field } => ArenaName::ALL
                .iter()
                .filter(|arena| arena.home().as_str() == *home && arena.key() == *field)
                .map(|arena| arena.class())
                .collect(),
            Self::Native { .. } => ArenaClass::ALL.to_vec(),
        }
    }
}

impl std::fmt::Display for Arena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Home { home, field } => write!(f, "{home}.{field}"),
            Self::Native { codec, arena } => write!(f, "native.{codec}.{arena}"),
        }
    }
}

/// One record the wrapper gives up: every copy of `id` in `arena`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Seed {
    pub(crate) arena: Arena,
    pub(crate) id: String,
}

/// What removing one seed did to the document.
#[derive(Debug)]
pub(crate) struct Released {
    /// The record's path in the document.
    pub(crate) path: String,
    /// The records dropped because they need the removed record, sorted.
    pub(crate) dropped: BTreeSet<String>,
    /// The records kept in a partial form or with a mark, sorted.
    pub(crate) marked: BTreeSet<String>,
    /// The records of the delta of each stored state that the delta dropped
    /// and marked, by the state's arena and id, each under the state's path.
    /// A record of a delta is the state's own: the document's record of the
    /// same id keeps its own outcome.
    pub(crate) states: BTreeMap<(Arena, String), Released>,
    /// The stored states dropped whole, each with the occurrences whose
    /// member entry names a removed definition: no delta states that an
    /// occurrence leaves a state (§5.4, §6.7).
    pub(crate) unstatable: BTreeMap<String, BTreeSet<String>>,
}

/// The part a given-up seed leaves.
#[derive(Debug)]
pub(crate) struct Retained {
    /// The id of the part's `unknowns` stub.
    pub(crate) stub: String,
    /// The length of the retained text.
    pub(crate) length: usize,
}

impl Released {
    fn new(path: String) -> Self {
        Self {
            path,
            dropped: BTreeSet::new(),
            marked: BTreeSet::new(),
            states: BTreeMap::new(),
            unstatable: BTreeMap::new(),
        }
    }

    /// The `dropped=[..]; marked=[..]` tail of the loss, `prefix` naming the
    /// records dropped before the cascade, such as the given-up id itself,
    /// followed by the [`Self::stated`] entries.
    pub(crate) fn tail(&self, prefix: &[&str]) -> String {
        let dropped: BTreeSet<&str> = prefix
            .iter()
            .copied()
            .chain(self.dropped.iter().map(String::as_str))
            .collect();
        let mut parts = vec![
            format!("dropped=[{}]", list(dropped)),
            format!("marked=[{}]", list(self.marked.iter().map(String::as_str))),
        ];
        parts.extend(self.stated());
        parts.join("; ")
    }

    /// One `delta of <state>: dropped=[..]; marked=[..]` entry for the delta
    /// of each stored state that changed, and one entry for each state
    /// dropped because its delta cannot state what leaves it.
    pub(crate) fn stated(&self) -> Vec<String> {
        let mut parts = Vec::new();
        for state in self.states.values() {
            parts.push(format!(
                "delta of {}: dropped=[{}]; marked=[{}]",
                state.path,
                list(state.dropped.iter().map(String::as_str)),
                list(state.marked.iter().map(String::as_str))
            ));
        }
        for (state, occurrences) in &self.unstatable {
            parts.push(format!(
                "state {state} dropped: its delta cannot state that [{}] leave it",
                list(occurrences.iter().map(String::as_str))
            ));
        }
        parts
    }
}

/// The ids of `ids`, separated by commas.
fn list<'a>(ids: impl IntoIterator<Item = &'a str>) -> String {
    ids.into_iter().collect::<Vec<_>>().join(", ")
}

/// One record that names an id the document no longer holds, with each path
/// at which it names one and the id it names there.
#[derive(Debug)]
pub(crate) struct Holder {
    pub(crate) arena: Arena,
    pub(crate) id: String,
    pub(crate) names: Vec<(String, String)>,
}

impl Holder {
    /// The holder `entity`, a record of `arena`, is, where it names an id
    /// that `lookup` finds, other than its own, as a value or as the key of
    /// a map. A record that has no JSON value names nothing.
    pub(crate) fn of<'k, T: Serialize>(
        arena: &Arena,
        id: &str,
        entity: &T,
        lookup: impl Fn(&str) -> Option<&'k str>,
    ) -> Option<Self> {
        let value = crate::hash::finite_json::to_json_value(entity).ok()?;
        let mut names = Vec::new();
        crate::reference_walk::visit_edges(
            &value,
            "",
            true,
            &|text| lookup(text).into_iter().collect(),
            &mut |path, target: &str| {
                names.push((path.to_owned(), target.to_owned()));
                true
            },
        );
        keys(&value, "", &lookup, &mut names);
        (!names.is_empty()).then(|| Self {
            arena: arena.clone(),
            id: id.to_owned(),
            names,
        })
    }

    /// The holder `entity`, a record of `arena`, is: for a neutral record,
    /// where a field the CADIR schema types as an id, a key of a map keyed
    /// by ids, or a `native_ref` names an id that `lookup` finds (§3.3), so
    /// that a label that equals an id names nothing; for a native record,
    /// by [`Self::of`]. A walk that reads no native record
    /// ([`NativeLinks::Unread`]) asks for no native holder.
    pub(crate) fn named<'k, T: Serialize>(
        arena: &Arena,
        id: &str,
        entity: &T,
        lookup: impl Fn(&str) -> Option<&'k str>,
    ) -> Option<Self> {
        let Arena::Home { home, field } = arena else {
            return Self::of(arena, id, entity, lookup);
        };
        let value = crate::hash::finite_json::to_json_value(entity).ok()?;
        let names = restate::Schema::cadir()?.references(home, field, &value, &lookup);
        (!names.is_empty()).then(|| Self {
            arena: arena.clone(),
            id: id.to_owned(),
            names,
        })
    }

    /// Whether every name lies under one of `prefixes`.
    fn names_only_under(&self, prefixes: &[&str]) -> bool {
        self.names
            .iter()
            .all(|(path, _)| prefixes.iter().any(|prefix| under(path, prefix)))
    }
}

/// Whether `path` is `prefix` or lies under it.
fn under(path: &str, prefix: &str) -> bool {
    path.strip_prefix(prefix)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
}

/// Adds to `names` each key of a map below the root of `value` that
/// `lookup` finds, at the path of the map.
fn keys<'k>(
    value: &serde_json::Value,
    path: &str,
    lookup: &impl Fn(&str) -> Option<&'k str>,
    names: &mut Vec<(String, String)>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map {
                if !path.is_empty() {
                    if let Some(target) = lookup(key) {
                        names.push((path.to_owned(), target.to_owned()));
                    }
                }
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
                keys(child, &child_path, lookup, names);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                keys(item, path, lookup, names);
            }
        }
        _ => {}
    }
}

/// Whether a native record names the ids its values state, for a release.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum NativeLinks {
    /// A native record names nothing (§2): the release of a decode or of a
    /// read, whose codec states what each of its native records needs
    /// (`dropped_with`).
    #[default]
    Unread,
    /// A native record names each id its values state: the drop of a write,
    /// which omits a native record that holds a reference the write does
    /// not resolve (Rule W1).
    Read,
}

/// The references of a document, from one walk of its records: the number
/// of records that hold each id, and the records that name each id.
#[derive(Debug, Default)]
pub(crate) struct References {
    pub(crate) native_links: NativeLinks,
    held: HashMap<String, usize>,
    holders: Vec<(Arena, String)>,
    named_by: HashMap<String, Vec<usize>>,
}

impl References {
    /// The references a walk that reads native records by `native_links`
    /// fills.
    pub(crate) fn new(native_links: NativeLinks) -> Self {
        Self {
            native_links,
            ..Self::default()
        }
    }

    /// Counts one record, neutral or native, that holds `id`.
    pub(crate) fn hold(&mut self, id: &str) {
        *self.held.entry(id.to_owned()).or_default() += 1;
    }

    /// Adds the names `entity`, a record of `arena`, holds of the ids
    /// counted so far. The caller counts every record first.
    pub(crate) fn add<T: Serialize>(&mut self, arena: &Arena, id: &str, entity: &T) {
        let held = &self.held;
        let Some(holder) = Holder::named(arena, id, entity, |text| {
            held.get_key_value(text).map(|(key, _)| key.as_str())
        }) else {
            return;
        };
        let slot = self.holders.len();
        let targets: BTreeSet<String> =
            holder.names.into_iter().map(|(_, target)| target).collect();
        self.holders.push((holder.arena, holder.id));
        for target in targets {
            self.named_by.entry(target).or_default().push(slot);
        }
    }

    /// Names `id`, which no record holds, so that [`Self::add`] finds the
    /// records that name it.
    fn name(&mut self, id: &str) {
        self.held.entry(id.to_owned()).or_default();
    }

    /// Whether a record holds `id`.
    fn holds(&self, id: &str) -> bool {
        self.held.get(id).is_some_and(|held| *held > 0)
    }

    /// Counts `copies` fewer records of `id` and returns whether none is
    /// left.
    fn release(&mut self, id: &str, copies: usize) -> bool {
        self.held.get_mut(id).is_none_or(|held| {
            *held = held.checked_sub(copies).unwrap_or_default();
            *held == 0
        })
    }

    /// The seeds of `candidates` that no record names other than one of
    /// `dropped` or of these seeds: a candidate that another record names
    /// is kept, and so is each candidate that a kept one names.
    fn unnamed(&self, candidates: Vec<Seed>, dropped: &[Seed]) -> Vec<Seed> {
        let dropped: BTreeSet<&str> = dropped.iter().map(|seed| seed.id.as_str()).collect();
        let mut left: BTreeSet<String> = candidates.iter().map(|seed| seed.id.clone()).collect();
        loop {
            let named: Vec<String> = left
                .iter()
                .filter(|id| {
                    self.named_by.get(id.as_str()).is_some_and(|slots| {
                        slots.iter().any(|slot| {
                            let holder = self.holders[*slot].1.as_str();
                            !dropped.contains(holder) && !left.contains(holder)
                        })
                    })
                })
                .cloned()
                .collect();
            if named.is_empty() {
                break;
            }
            for id in named {
                left.remove(&id);
            }
        }
        candidates
            .into_iter()
            .filter(|seed| left.contains(&seed.id))
            .collect()
    }

    /// Each record that names an id of `gone`, once.
    fn naming(&self, gone: &BTreeSet<String>) -> BTreeSet<&(Arena, String)> {
        gone.iter()
            .filter_map(|id| self.named_by.get(id))
            .flatten()
            .map(|slot| &self.holders[*slot])
            .collect()
    }
}

/// The compact canonical JSON text of the copies of one record: the
/// record's own where there is one copy, and the array of the copies
/// otherwise. A value with no canonical JSON keeps its plain JSON text.
pub(crate) fn record_text<T: Serialize>(copies: &[T]) -> String {
    fn text<U: Serialize + ?Sized>(value: &U) -> String {
        crate::hash::finite_json::to_compact_canonical_json_string(value).unwrap_or_else(|_| {
            crate::hash::finite_json::to_json_value(value)
                .map(|value| value.to_string())
                .unwrap_or_default()
        })
    }
    match copies {
        [one] => text(one),
        copies => text(copies),
    }
}

/// Gives up each seed, in order, as a part (§3.3): removes it as
/// [`drop_records`] does, and adds its `unknowns` stub to `codec`'s
/// namespace and its canonical JSON to `fidelity` as an `untyped` retained
/// record. The annotations of each record the give-ups remove go with it.
/// `loss` states the loss of each seed from what its give-up did.
///
/// # Errors
///
/// A resource refusal for the retained text or the stub, or
/// [`CodecError::Malformed`] where the stub's id is already present.
pub(crate) fn give_up(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    fidelity: &mut SourceFidelity,
    codec: &'static str,
    seeds: Vec<Seed>,
    mut loss: impl FnMut(&Seed, &Released, &Retained) -> LossNote,
) -> Result<Vec<LossNote>, CodecError> {
    let before = (!fidelity.annotations.is_empty()).then(|| Removed::before(ir));
    let mut cascade = Cascade::new(ctx, ir, NativeLinks::Unread, &BTreeSet::new())?;
    let mut losses = Vec::new();
    for seed in seeds {
        let Some(text) = text(ir, &seed.arena, &seed.id) else {
            continue;
        };
        let Some(released) = remove(ctx, ir, &mut cascade, &seed)? else {
            continue;
        };
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(text.len()),
            "given-up record text",
        )?;
        let kind = seed.arena.kind();
        let retained = Retained {
            stub: GivenUpPart::record_id(codec, kind, &released.path)?
                .as_str()
                .to_owned(),
            length: text.len(),
        };
        let note = loss(&seed, &released, &retained);
        let part = GivenUpPart::of_record(
            codec,
            kind,
            &released.path,
            text.into_bytes(),
            seed.arena.classes(),
            note,
        )?;
        losses.push(part.commit(ctx, ir, fidelity)?);
    }
    if let Some(before) = before {
        fidelity.annotations.remove(&before.after(ir));
    }
    Ok(losses)
}

/// Removes each seed, in order: every copy of it, each record that needs
/// it, transitively, and each mark a record that holds it by a parent
/// pointer or as a member takes (§3.3). A seed an earlier seed's removal
/// dropped is skipped.
///
/// # Errors
///
/// A resource refusal for a rewritten `unknowns` arena or for the walk of
/// the carriers.
pub(crate) fn drop_records(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    seeds: Vec<Seed>,
) -> Result<Vec<(Seed, Released)>, CodecError> {
    let mut cascade = Cascade::new(ctx, ir, NativeLinks::Read, &BTreeSet::new())?;
    let mut removed = Vec::new();
    for seed in seeds {
        if let Some(released) = remove(ctx, ir, &mut cascade, &seed)? {
            removed.push((seed, released));
        }
    }
    Ok(removed)
}

/// Removes from `ir` each record that needs an id of `gone`, which `ir` no
/// longer holds, and each record that needs one of those, transitively, as
/// [`drop_records`] removes the records that need a seed; then removes each
/// seed as [`drop_records`] does, and each record of `unnamed` that no
/// record outside `seeds` and `unnamed` names, directly or through records
/// of `unnamed` it keeps.
///
/// # Errors
///
/// A resource refusal for a rewritten `unknowns` arena or for the walk of
/// the carriers.
pub(crate) fn drop_records_needing(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    gone: BTreeSet<String>,
    seeds: Vec<Seed>,
    unnamed: Vec<Seed>,
) -> Result<(), CodecError> {
    let mut cascade = Cascade::new(ctx, ir, NativeLinks::Read, &gone)?;
    let unnamed = cascade.references.unnamed(unnamed, &seeds);
    let mut released = Released::new(String::new());
    release(
        ctx,
        ir,
        &mut cascade.references,
        &cascade.lazy,
        gone,
        Vec::new(),
        &mut released,
    )?;
    for seed in seeds.into_iter().chain(unnamed) {
        remove(ctx, ir, &mut cascade, &seed)?;
    }
    Ok(())
}

/// States `Source` each expression of the records of each seed whose tree
/// passes the bound of an expression (§2.5), the form §3.3 gives an
/// expression of which nothing is read, and returns the seeds whose records
/// it restated. The arenas are in identity order.
pub(crate) fn restate_past_the_bound(ir: &mut CadIr, seeds: &[Seed]) -> Vec<Seed> {
    let Some(schema) = Schema::cadir() else {
        return Vec::new();
    };
    let gone = BTreeSet::new();
    seeds
        .iter()
        .filter(|seed| {
            let Arena::Home { home, field } = &seed.arena else {
                return false;
            };
            let Some(restate) = Restate::new(schema, &gone, &seed.id, *home == "process") else {
                return false;
            };
            let restate = restate.past_the_bound();
            ir.rewrite_records(&seed.arena, &seed.id, &mut |record| {
                restate.record(home, field, record)
            })
        })
        .cloned()
        .collect()
}

/// The state of one batch of removals: the document's references, what
/// the batch reads once a record needs it, and the carriers no record
/// reached before the batch.
struct Cascade {
    references: References,
    lazy: Lazy,
    unreached: BTreeSet<String>,
}

/// What a batch of removals reads of the document once a record needs it.
#[derive(Default)]
struct Lazy {
    /// The stored states whose delta names each id, by id.
    stated: OnceCell<HashMap<String, BTreeSet<String>>>,
}

impl Cascade {
    /// The state of a batch of removals from `ir`, which it puts in
    /// identity order, walking native records by `native_links`, where
    /// `gone` holds the ids that records of `ir` name and `ir` no longer
    /// holds.
    fn new(
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        native_links: NativeLinks,
        gone: &BTreeSet<String>,
    ) -> Result<Self, CodecError> {
        ir.finalize();
        let mut named = References::new(native_links);
        for id in gone {
            named.name(id);
        }
        Ok(Self {
            references: ir.references_from(named),
            lazy: Lazy::default(),
            unreached: crate::validate::carriers_parameterization::unreached_carriers(ctx, ir)?,
        })
    }

    /// The state of a batch of releases of `lost`, ids that no record of
    /// `ir` may hold, so that the walk of the references finds the records
    /// that name them.
    fn naming(
        ctx: &DecodeContext<'_>,
        ir: &mut CadIr,
        lost: &[String],
    ) -> Result<Self, CodecError> {
        ir.finalize();
        let mut named = References::default();
        for id in lost {
            named.name(id);
        }
        Ok(Self {
            references: ir.references_from(named),
            lazy: Lazy::default(),
            unreached: crate::validate::carriers_parameterization::unreached_carriers(ctx, ir)?,
        })
    }
}

/// Removes every copy of `seed` and releases it, then drops each carrier
/// that the removal leaves with no user, or returns `None` where its arena
/// holds none.
fn remove(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    cascade: &mut Cascade,
    seed: &Seed,
) -> Result<Option<Released>, CodecError> {
    let Cascade {
        references,
        lazy,
        unreached,
    } = cascade;
    let parents = parents(ir, &seed.arena, &seed.id);
    let owned = owned(ir, &seed.arena, &seed.id);
    let copies = take(ir, &seed.arena, &seed.id);
    if copies == 0 {
        return Ok(None);
    }
    let mut released = Released::new(format!("{}/{}", seed.arena, seed.id));
    mark_parents(ir, &parents, &mut released.marked);
    let gone = references.release(&seed.id, copies);
    release(
        ctx,
        ir,
        references,
        lazy,
        gone.then(|| seed.id.clone()).into_iter().collect(),
        owned,
        &mut released,
    )?;
    let geometric = matches!(
        seed.arena,
        Arena::Home {
            home: "geometry" | "model",
            ..
        }
    );
    if geometric || !released.dropped.is_empty() || !released.states.is_empty() {
        drop_orphans(ctx, ir, references, lazy, unreached, &mut released)?;
    }
    Ok(Some(released))
}

/// Releases each id of `ids`, records a read gave up before they reached
/// `ir`, as [`remove`] releases a removed seed: drops each record that needs
/// one, transitively, marks each record that holds one, and drops each
/// carrier left with no user (§3.3). An id a record of `ir` holds names that
/// record, so nothing needs the given-up one, and it releases nothing.
///
/// Each release goes into `released` as it runs, so `released` holds what
/// left `ir` also when a release stops on an error.
///
/// # Errors
///
/// A refusal of a rewritten `unknowns` arena, or a resource refusal for the
/// walk of the carriers.
pub(crate) fn release_lost(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    ids: &[String],
    released: &mut Vec<(String, Released)>,
) -> Result<(), CodecError> {
    let mut cascade = Cascade::naming(ctx, ir, ids)?;
    let Cascade {
        references,
        lazy,
        unreached,
    } = &mut cascade;
    for id in ids {
        if references.holds(id) {
            continue;
        }
        let mut release_of = Released::new(id.clone());
        let outcome = release(
            ctx,
            ir,
            references,
            lazy,
            BTreeSet::from([id.clone()]),
            Vec::new(),
            &mut release_of,
        )
        .and_then(|_| {
            if release_of.dropped.is_empty() && release_of.states.is_empty() {
                return Ok(());
            }
            drop_orphans(ctx, ir, references, lazy, unreached, &mut release_of)
        });
        released.push((id.clone(), release_of));
        outcome?;
    }
    Ok(())
}

/// Drops each carrier that the releases so far left with no user, until
/// none is left, recording each in `released`.
fn drop_orphans(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    references: &mut References,
    lazy: &Lazy,
    unreached: &BTreeSet<String>,
    released: &mut Released,
) -> Result<(), CodecError> {
    loop {
        let orphans = orphans(ctx, ir, unreached)?;
        if orphans.is_empty() {
            return Ok(());
        }
        release(
            ctx,
            ir,
            references,
            lazy,
            BTreeSet::new(),
            orphans,
            released,
        )?;
    }
}

/// Each surface, curve, pcurve, and point carrier of `ir` that no record
/// reaches (part 10, carrier reachability), other than those of `unreached`
/// and those a stored state names.
fn orphans(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    unreached: &BTreeSet<String>,
) -> Result<Vec<(Arena, String)>, CodecError> {
    let mut orphans = crate::validate::carriers_parameterization::unreached_carriers(ctx, ir)?;
    orphans.retain(|id| !unreached.contains(id));
    if orphans.is_empty() {
        return Ok(Vec::new());
    }
    for (_, delta) in stored(ir) {
        if let Ok(delta) = crate::hash::finite_json::to_json_value(delta) {
            unname(&delta, &mut orphans);
        }
    }
    let geometry = &ir.geometry;
    Ok(orphans
        .into_iter()
        .filter_map(|id| {
            let field = if !records(&geometry.surfaces, &id).is_empty() {
                "surfaces"
            } else if !records(&geometry.curves, &id).is_empty() {
                "curves"
            } else if !records(&geometry.pcurves, &id).is_empty() {
                "pcurves"
            } else if !records(&geometry.points, &id).is_empty() {
                "points"
            } else {
                return None;
            };
            Some((
                Arena::Home {
                    home: "geometry",
                    field,
                },
                id,
            ))
        })
        .collect())
}

/// The canonical JSON text of the copies of `id` in `arena`, or `None`
/// where the arena holds none.
fn text(ir: &CadIr, arena: &Arena, id: &str) -> Option<String> {
    match arena {
        Arena::Home { home, field } => ir.record_text(home, field, id),
        Arena::Native { codec, arena } => ir.native.record_text(codec, arena, id),
    }
}

/// Removes every copy of `id` from `arena`, returning the number removed.
fn take(ir: &mut CadIr, arena: &Arena, id: &str) -> usize {
    match arena {
        Arena::Home { home, field } => ir.remove_records(home, field, id),
        Arena::Native { codec, arena } => ir.native.remove_records(codec, arena, id),
    }
}

/// The records of `id` in `records`, which are in identity order.
pub(crate) fn records<'a, T: ArenaRecord>(records: &'a [T], id: &str) -> &'a [T] {
    let start = records.partition_point(|record| record.record_identity() < id);
    let length = records[start..]
        .iter()
        .take_while(|record| record.record_identity() == id)
        .count();
    &records[start..start + length]
}

/// The records of `id` in `records`, which are in identity order, to change.
pub(crate) fn records_mut<'a, T: ArenaRecord>(records: &'a mut [T], id: &str) -> &'a mut [T] {
    let start = records.partition_point(|record| record.record_identity() < id);
    let length = records[start..]
        .iter()
        .take_while(|record| record.record_identity() == id)
        .count();
    &mut records[start..start + length]
}

/// The parents a copy of `id` in `arena` names by its parent pointer: a
/// face's shell and a region's body.
fn parents(ir: &CadIr, arena: &Arena, id: &str) -> Vec<String> {
    let Arena::Home {
        home: "geometry",
        field,
    } = arena
    else {
        return Vec::new();
    };
    match *field {
        "faces" => records(&ir.geometry.faces, id)
            .iter()
            .map(|face| face.shell.as_str().to_owned())
            .collect(),
        "regions" => records(&ir.geometry.regions, id)
            .iter()
            .map(|region| region.body.as_str().to_owned())
            .collect(),
        _ => Vec::new(),
    }
}

/// The records that the copies of `id` in `arena` own, which the document
/// states none of without their owner: the procedural construction of a
/// curve or a surface, each member of a container (§5.2), and each root
/// container of a scope with each record that names the scope (§5.1).
fn owned(ir: &CadIr, arena: &Arena, id: &str) -> Vec<(Arena, String)> {
    let Arena::Home { home, field } = arena else {
        return Vec::new();
    };
    let owned: Vec<(&str, &str, String)> = match (*home, *field) {
        ("geometry", "curves") => records(&ir.geometry.curves, id)
            .iter()
            .filter_map(|curve| curve.geometry.procedural_construction())
            .map(|construction| {
                (
                    "geometry",
                    "procedural_curves",
                    construction.as_str().to_owned(),
                )
            })
            .collect(),
        ("geometry", "surfaces") => records(&ir.geometry.surfaces, id)
            .iter()
            .filter_map(|surface| surface.geometry.procedural_construction())
            .map(|construction| {
                (
                    "geometry",
                    "procedural_surfaces",
                    construction.as_str().to_owned(),
                )
            })
            .collect(),
        ("design", "containers") => records(&ir.design.containers, id)
            .iter()
            .flat_map(|container| match container {
                Container::Ordered { members, .. } => members.iter().collect::<Vec<_>>(),
                Container::Unordered { members, .. } => members.iter().collect(),
            })
            .map(|member| match member {
                Member::Feature(feature) => ("design", "features", feature.as_str().to_owned()),
                Member::Container(container) => {
                    ("design", "containers", container.as_str().to_owned())
                }
            })
            .collect(),
        ("design", "scopes") => {
            let design = &ir.design;
            records(&design.scopes, id)
                .iter()
                .flat_map(|scope| &scope.roots)
                .map(|root| ("design", "containers", root.as_str().to_owned()))
                .chain(scoped(
                    "design",
                    "parameters",
                    &design.parameters,
                    id,
                    |record| record.scope.as_str(),
                ))
                .chain(scoped(
                    "design",
                    "programs",
                    &design.programs,
                    id,
                    |record| record.scope.as_str(),
                ))
                .chain(scoped(
                    "design",
                    "sheet_metal_rules",
                    &design.sheet_metal_rules,
                    id,
                    |record| record.scope.as_str(),
                ))
                .chain(scoped(
                    "design",
                    "configurations",
                    &design.configurations,
                    id,
                    |record| record.scope.as_str(),
                ))
                .chain(scoped(
                    "design",
                    "arrangements",
                    &design.arrangements,
                    id,
                    |record| record.scope.as_str(),
                ))
                .chain(scoped(
                    "design",
                    "publications",
                    &design.publications,
                    id,
                    |record| record.scope.as_str(),
                ))
                .chain(scoped(
                    "design",
                    "evaluations",
                    &design.evaluations,
                    id,
                    |record| record.scope.as_str(),
                ))
                .chain(scoped(
                    "process",
                    "setups",
                    &ir.process.setups,
                    id,
                    |record| record.scope.as_str(),
                ))
                .collect()
        }
        _ => Vec::new(),
    };
    owned
        .into_iter()
        .filter_map(|(home, field, id)| Some((Arena::home(home, field)?, id)))
        .collect()
}

/// The identity of each record of `records`, of the arena `field` of `home`,
/// whose scope, as `of` reads it, is `scope`.
fn scoped<'a, T: ArenaRecord>(
    home: &'static str,
    field: &'static str,
    records: &'a [T],
    scope: &'a str,
    of: fn(&T) -> &str,
) -> impl Iterator<Item = (&'static str, &'static str, String)> + 'a {
    records
        .iter()
        .filter(move |record| of(record) == scope)
        .map(move |record| (home, field, record.record_identity().to_owned()))
}

/// Removes from `ids` each id `value` names, as a value or as a key.
fn unname(value: &serde_json::Value, ids: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::String(text) => {
            ids.remove(text.as_str());
        }
        serde_json::Value::Array(items) => {
            for item in items {
                unname(item, ids);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                ids.remove(key.as_str());
                unname(item, ids);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

/// Marks each shell and body of `parents` the document holds: it no longer
/// states each of its children (§5.6, Unstated children).
fn mark_parents(ir: &mut CadIr, parents: &[String], marked: &mut BTreeSet<String>) {
    for parent in parents {
        for shell in records_mut(&mut ir.geometry.shells, parent) {
            shell.unstated = Some(crate::topology::Unstated::Members);
            marked.insert(parent.clone());
        }
        for body in records_mut(&mut ir.geometry.bodies, parent) {
            body.unstated = Some(crate::topology::Unstated::Members);
            marked.insert(parent.clone());
        }
    }
}

/// Drops, transitively, each record that needs `gone`, ids no record of
/// `ir` holds, with `carried` and each construction a dropped carrier owns,
/// and marks each record that holds it in a form that keeps the rest,
/// recording both in `released`. A stored state loses only what of its
/// delta needs an id of `gone` that the delta does not hold itself. Then
/// A scope that loses a root keeps its other roots, and is dropped once it
/// holds no root and no record names it (§5.1, §5.2). Then
/// removes every id it dropped from the collections keyed by id and from
/// the stubs' links, which no record needs, and returns every id that no
/// record holds after.
fn release(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    references: &mut References,
    lazy: &Lazy,
    mut gone: BTreeSet<String>,
    mut carried: Vec<(Arena, String)>,
    released: &mut Released,
) -> Result<BTreeSet<String>, CodecError> {
    let mut all_gone = gone.clone();
    let mut scopes = BTreeSet::new();
    loop {
        if gone.is_empty() && carried.is_empty() {
            carried = emptied(ir, &mut scopes);
            if carried.is_empty() {
                break;
            }
        }
        let mut drops: BTreeMap<Arena, BTreeSet<String>> = BTreeMap::new();
        for (arena, id) in carried.drain(..) {
            drops.entry(arena).or_default().insert(id);
        }
        for (arena, id) in references.naming(&gone) {
            let held = stated_ids(ir, arena, id);
            let unheld: BTreeSet<String>;
            let here = if held.is_empty() {
                &gone
            } else {
                unheld = gone.difference(&held).cloned().collect();
                &unheld
            };
            let Some(mut holder) = ir.holder(references.native_links, arena, id, here) else {
                continue;
            };
            if holder
                .names
                .iter()
                .any(|(path, _)| under(path, STATE_DELTA))
            {
                let unstatable = unstatable_members(ir, &holder, here);
                if !unstatable.is_empty() {
                    released.unstatable.insert(holder.id.clone(), unstatable);
                    drops.entry(holder.arena).or_default().insert(holder.id);
                    continue;
                }
                let changed = release_states(ctx, ir, references, lazy, &holder, here, released)?;
                holder.names.retain(|(path, _)| !under(path, STATE_DELTA));
                if holder.names.is_empty() {
                    if changed {
                        released.marked.insert(holder.id);
                    }
                    continue;
                }
            }
            let kept = match keep_roots(ir, &holder) {
                Some(kept) => {
                    if kept {
                        scopes.insert(holder.id.clone());
                    }
                    kept
                }
                None => {
                    clear_native_refs(ir, &mut holder)
                        || keep_topology(ir, &holder)
                        || restate(ir, &holder, here)
                        || keep_unresolved(ir, &holder)
                }
            };
            if kept {
                released.marked.insert(holder.id);
            } else {
                drops.entry(holder.arena).or_default().insert(holder.id);
            }
        }
        let mut next = BTreeSet::new();
        for (arena, ids) in drops {
            let parents: Vec<String> = ids.iter().flat_map(|id| parents(ir, &arena, id)).collect();
            carried.extend(ids.iter().flat_map(|id| owned(ir, &arena, id)));
            for (id, copies) in ir.remove_ids(&arena, &ids) {
                if references.release(&id, copies) {
                    next.insert(id.clone());
                }
                released.dropped.insert(id);
            }
            mark_parents(ir, &parents, &mut released.marked);
        }
        all_gone.extend(next.iter().cloned());
        gone = next;
    }
    ir.release_keyed(&all_gone);
    release_stub_links(ctx, ir, &all_gone)?;
    released
        .marked
        .retain(|record| !released.dropped.contains(record));
    Ok(all_gone)
}

/// The occurrences whose member entry, in the delta of a copy of `holder`,
/// names an id of `gone` while the occurrence stays. A state holds no
/// occurrence without its owner (§8.1), and no delta states that an
/// occurrence leaves a state (§5.4, §6.7), so such a state has no form.
fn unstatable_members(ir: &CadIr, holder: &Holder, gone: &BTreeSet<String>) -> BTreeSet<String> {
    deltas(ir, &holder.arena, &holder.id)
        .into_iter()
        .flat_map(|delta| &delta.members)
        .filter(|(occurrence, owner)| {
            !gone.contains(occurrence.as_str())
                && owner
                    .as_ref()
                    .is_some_and(|owner| gone.contains(owner.as_str()))
        })
        .map(|(occurrence, _)| occurrence.as_str().to_owned())
        .collect()
}

/// The path in a stored state of its delta.
const STATE_DELTA: &str = "content.delta";

/// The deltas of the copies of `id` in `arena`, where it is an arena of
/// stored states.
fn deltas<'a>(ir: &'a CadIr, arena: &Arena, id: &str) -> Vec<&'a StateDelta> {
    match arena {
        Arena::Home {
            home: "design",
            field: "evaluations",
        } => records(&ir.design.evaluations, id)
            .iter()
            .filter_map(|evaluation| evaluation.content.as_ref())
            .map(|content| &content.delta)
            .collect(),
        Arena::Home {
            home: "process",
            field: "states",
        } => records(&ir.process.states, id)
            .iter()
            .filter_map(|state| state.content.as_ref())
            .map(|content| &content.delta)
            .collect(),
        _ => Vec::new(),
    }
}

/// The deltas of the copies of `id` in `arena`, to change.
fn deltas_mut<'a>(ir: &'a mut CadIr, arena: &Arena, id: &str) -> Vec<&'a mut StateDelta> {
    match arena {
        Arena::Home {
            home: "design",
            field: "evaluations",
        } => records_mut(&mut ir.design.evaluations, id)
            .iter_mut()
            .filter_map(|evaluation| evaluation.content.as_mut())
            .map(|content| &mut content.delta)
            .collect(),
        Arena::Home {
            home: "process",
            field: "states",
        } => records_mut(&mut ir.process.states, id)
            .iter_mut()
            .filter_map(|state| state.content.as_mut())
            .map(|content| &mut content.delta)
            .collect(),
        _ => Vec::new(),
    }
}

/// The ids of the records the deltas of the copies of `id` in `arena`
/// hold: a reference in a delta names its own record of the id (§5.4).
fn stated_ids(ir: &CadIr, arena: &Arena, id: &str) -> BTreeSet<String> {
    deltas(ir, arena, id)
        .into_iter()
        .flat_map(|delta| delta.records.identities())
        .map(str::to_owned)
        .collect()
}

/// Releases `here` in the delta of each copy of `holder`, a stored state,
/// records what each delta dropped and marked under the state's path in
/// `released`, and returns whether a delta changed.
fn release_states(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    references: &References,
    lazy: &Lazy,
    holder: &Holder,
    here: &BTreeSet<String>,
    released: &mut Released,
) -> Result<bool, CodecError> {
    let naming = lazy.stated.get_or_init(|| states_naming(ir));
    let elsewhere = |id: &str| {
        naming
            .get(id)
            .is_some_and(|states| states.iter().any(|state| *state != holder.id))
    };
    let key = (holder.arena.clone(), holder.id.clone());
    let mut state = released
        .states
        .remove(&key)
        .unwrap_or_else(|| Released::new(format!("{}/{}", holder.arena, holder.id)));
    let plans: Vec<Withdrawn> = deltas(ir, &holder.arena, &holder.id)
        .into_iter()
        .map(|delta| withdrawn(ir, &holder.id, delta, here))
        .collect::<Result<_, _>>()?;
    let mut changed = false;
    for (delta, plan) in deltas_mut(ir, &holder.arena, &holder.id)
        .into_iter()
        .zip(&plans)
    {
        changed |= release_delta(ctx, references, lazy, &elsewhere, delta, plan, &mut state)?;
    }
    if !state.dropped.is_empty() || !state.marked.is_empty() {
        released.states.insert(key, state);
    }
    Ok(changed)
}

/// Drops from the delta of each copy of the stored state `holder` each of
/// its records of `ids` and each record of the delta that needs one, as §3.3
/// drops a record, and returns what the deltas dropped and marked, under the
/// state's path, or `None` where no delta changed.
///
/// # Errors
///
/// A resource refusal for the walk of the carriers.
pub(crate) fn drop_state_records(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    holder: &Seed,
    ids: BTreeSet<String>,
) -> Result<Option<Released>, CodecError> {
    let Cascade {
        references, lazy, ..
    } = Cascade::new(ctx, ir, NativeLinks::Read, &BTreeSet::new())?;
    let naming = lazy.stated.get_or_init(|| states_naming(ir));
    let elsewhere = |id: &str| {
        naming
            .get(id)
            .is_some_and(|states| states.iter().any(|state| *state != holder.id))
    };
    let plan = Withdrawn {
        gone: BTreeSet::new(),
        ids,
        removed: Vec::new(),
        names: Vec::new(),
        owners: Vec::new(),
    };
    let mut state = Released::new(format!("{}/{}", holder.arena, holder.id));
    let mut changed = false;
    for delta in deltas_mut(ir, &holder.arena, &holder.id) {
        changed |= release_delta(
            ctx,
            &references,
            &lazy,
            &elsewhere,
            delta,
            &plan,
            &mut state,
        )?;
    }
    Ok(changed.then_some(state))
}

/// What a stored state withdraws because an owner entry of its delta names
/// a gone definition: the state holds no body without its owner (§8.1).
struct Withdrawn {
    /// The gone ids with the withdrawn ids.
    gone: BTreeSet<String>,
    /// The withdrawn ids: the bodies and the records only they reach.
    ids: BTreeSet<String>,
    /// The withdrawn records that the document geometry or another stored
    /// state holds, which the delta lists as removed.
    removed: Vec<RecordRef>,
    /// The name entries of withdrawn entities that a base can hold.
    names: Vec<EntityRef>,
    /// The owner entries of withdrawn bodies that a base can hold.
    owners: Vec<BodyId>,
}

/// The plan of what `delta`, of the stored state `holder`, withdraws. A
/// body whose owner entry names an id of `gone` is withdrawn with the
/// records it reaches that no other body reaches (§5.6), in the document
/// geometry and the records of every stored state, as `remove_member`
/// removes an extent (§8.1). A base entry is cleared where the document's
/// name table or another stored state holds an entry of its key.
fn withdrawn(
    ir: &CadIr,
    holder: &str,
    delta: &StateDelta,
    gone: &BTreeSet<String>,
) -> Result<Withdrawn, CodecError> {
    use crate::design::edit::states::{ids, reached, record_refs, select, upsert};

    let named = |owner: Option<&ProductDefinitionId>| {
        owner.is_some_and(|owner| gone.contains(owner.as_str()))
    };
    let bodies: BTreeSet<String> = delta
        .owners
        .iter()
        .filter(|(_, owner)| named(owner.as_ref()))
        .map(|(body, _)| body.as_str().to_owned())
        .collect();
    let mut plan = Withdrawn {
        gone: gone.clone(),
        ids: BTreeSet::new(),
        removed: Vec::new(),
        names: Vec::new(),
        owners: Vec::new(),
    };
    if bodies.is_empty() {
        return Ok(plan);
    }
    let others: Vec<&StateDelta> = stored(ir)
        .filter(|(state, _)| *state != holder)
        .map(|(_, delta)| delta)
        .collect();
    let mut every = ir.geometry.clone();
    for other in &others {
        upsert(&mut every, other.records.clone());
    }
    let outside = ids(&every);
    upsert(&mut every, delta.records.clone());
    let (leaving, staying): (Vec<BodyId>, Vec<BodyId>) = every
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .partition(|body| bodies.contains(body.as_str()));
    let malformed =
        |error: crate::design::edit::EditError| CodecError::Malformed(error.to_string());
    let kept = reached(&every, &staying).map_err(malformed)?;
    let leaving: BTreeSet<String> = reached(&every, &leaving)
        .map_err(malformed)?
        .into_iter()
        .filter(|id| !kept.contains(id))
        .collect();
    let held: BTreeSet<String> = leaving.intersection(&outside).cloned().collect();
    plan.removed = record_refs(&select(&every, &held));
    for record in record_refs(&select(&every, &leaving)) {
        let entity = match record {
            RecordRef::Body(id) => EntityRef::Body(id),
            RecordRef::Face(id) => EntityRef::Face(id),
            RecordRef::Edge(id) => EntityRef::Edge(id),
            RecordRef::Vertex(id) => EntityRef::Vertex(id),
            RecordRef::Surface(id) => EntityRef::Surface(id),
            RecordRef::Curve(id) => EntityRef::Curve(id),
            RecordRef::Subd(id) => EntityRef::Subd(id),
            _ => continue,
        };
        if ir.design.names.entries().contains_key(&entity)
            || others.iter().any(|delta| delta.names.contains_key(&entity))
        {
            plan.names.push(entity);
        }
    }
    plan.owners = delta
        .owners
        .keys()
        .filter(|body| bodies.contains(body.as_str()))
        .filter(|body| others.iter().any(|delta| delta.owners.contains_key(*body)))
        .cloned()
        .collect();
    plan.ids = bodies;
    plan.ids.extend(leaving);
    plan.gone.extend(plan.ids.iter().cloned());
    Ok(plan)
}

/// Whether the JSON of `value` names an id of `gone`.
fn mentions<T: Serialize + ?Sized>(value: &T, gone: &BTreeSet<String>) -> bool {
    crate::hash::finite_json::to_json_value(value).is_ok_and(|value| names(&value, gone))
}

/// The stored states of `ir` whose delta names each id, as a value or as a
/// key, by id.
fn states_naming(ir: &CadIr) -> HashMap<String, BTreeSet<String>> {
    let mut naming: HashMap<String, BTreeSet<String>> = HashMap::new();
    for (state, delta) in stored(ir) {
        let Ok(delta) = crate::hash::finite_json::to_json_value(delta) else {
            continue;
        };
        let mut ids = BTreeSet::new();
        named(&delta, &mut ids);
        for id in ids {
            naming.entry(id).or_default().insert(state.to_owned());
        }
    }
    naming
}

/// The id and delta of each stored state of `ir`: each evaluation and process
/// state with content.
fn stored(ir: &CadIr) -> impl Iterator<Item = (&str, &StateDelta)> {
    let evaluations = ir.design.evaluations.iter().filter_map(|evaluation| {
        let content = evaluation.content.as_ref()?;
        Some((evaluation.id.as_str(), &content.delta))
    });
    let states = ir.process.states.iter().filter_map(|state| {
        let content = state.content.as_ref()?;
        Some((state.id.as_str(), &content.delta))
    });
    evaluations.chain(states)
}

/// Adds to `ids` each text `value` holds, as a value or as a key.
fn named(value: &serde_json::Value, ids: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::String(text) => {
            ids.insert(text.clone());
        }
        serde_json::Value::Array(items) => {
            for item in items {
                named(item, ids);
            }
        }
        serde_json::Value::Object(map) => {
            for (key, item) in map {
                ids.insert(key.clone());
                named(item, ids);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

/// Drops, transitively, each record of `delta` that needs a gone or
/// withdrawn id of `plan`, as [`release`] drops a record of the document,
/// and marks each that holds one in a form that keeps the rest, recording
/// both in `state`. Each record of the delta that `plan` withdraws is dropped with
/// them. A record the delta drops is gone for it where no record of the
/// document, as `references` counts them, holds its id. Then drops each
/// carrier of the delta that the drops leave with no user, other than one
/// the document holds or another stored state names (`elsewhere`). Then
/// removes each removal, name, owner, member, instance body, and pose of
/// the delta that names a gone or withdrawn id: each is an entry of a
/// collection keyed by its members' own ids (§3.3). An entry of the name
/// table whose name names one keeps its native name alone (§5.7). Then
/// lists the records `plan` withdraws as removed and clears the base
/// entries it names (§5.4). Returns whether the delta changed.
fn release_delta(
    ctx: &DecodeContext<'_>,
    references: &References,
    lazy: &Lazy,
    elsewhere: &dyn Fn(&str) -> bool,
    delta: &mut StateDelta,
    plan: &Withdrawn,
    state: &mut Released,
) -> Result<bool, CodecError> {
    let mut stated = CadIr::empty();
    stated.geometry = std::mem::take(&mut delta.records);
    stated.geometry.finalize();
    let unreached = crate::validate::carriers_parameterization::unreached_carriers(ctx, &stated)?;
    let mut assigned = Vec::new();
    for field in [
        "bodies",
        "regions",
        "shells",
        "faces",
        "loops",
        "coedges",
        "edges",
        "vertices",
        "points",
        "surfaces",
        "curves",
        "subds",
        "pcurves",
        "procedural_surfaces",
        "procedural_curves",
    ] {
        let arena = Arena::Home {
            home: "geometry",
            field,
        };
        for id in &plan.ids {
            if text(&stated, &arena, id).is_some() {
                assigned.push((arena.clone(), id.clone()));
            }
        }
    }
    let mut local = References::new(references.native_links);
    for id in &plan.gone {
        local.name(id);
    }
    for id in stated.geometry.identities() {
        if references.holds(id) {
            local.hold(id);
        }
    }
    let mut local = stated.references_from(local);
    let mut inner = Released::new(String::new());
    let mut all_gone = release(
        ctx,
        &mut stated,
        &mut local,
        lazy,
        plan.gone.clone(),
        assigned.clone(),
        &mut inner,
    )?;
    loop {
        let mut orphans = orphans(ctx, &stated, &unreached)?;
        orphans.retain(|(_, id)| !references.holds(id) && !elsewhere(id));
        if orphans.is_empty() {
            break;
        }
        all_gone.extend(release(
            ctx,
            &mut stated,
            &mut local,
            lazy,
            BTreeSet::new(),
            orphans,
            &mut inner,
        )?);
    }
    delta.records = stated.geometry;
    let changed = !inner.dropped.is_empty() || !inner.marked.is_empty() || !plan.ids.is_empty();
    state.dropped.extend(inner.dropped);
    state.marked.extend(inner.marked);
    let pruned = prune(delta, &all_gone);
    for record in &plan.removed {
        delta.removed.push(record.clone());
        state
            .dropped
            .insert(crate::design::edit::states::record_ref_id(record).to_owned());
    }
    delta
        .names
        .extend(plan.names.iter().map(|entity| (entity.clone(), None)));
    delta
        .owners
        .extend(plan.owners.iter().map(|body| (body.clone(), None)));
    state
        .marked
        .retain(|record| !state.dropped.contains(record));
    Ok(pruned || changed)
}

/// Removes each entry of `delta` outside its records that names an id of
/// `gone`, and returns whether it removed or changed one. An entry of the
/// name table whose name names one keeps its native name alone, and is
/// removed where it has none.
fn prune(delta: &mut StateDelta, gone: &BTreeSet<String>) -> bool {
    let before = [
        delta.removed.len(),
        delta.names.len(),
        delta.owners.len(),
        delta.members.len(),
        delta.instance_bodies.len(),
        delta.poses.len(),
    ];
    let mut renamed = false;
    delta.removed.retain(|record| !mentions(record, gone));
    delta.names.retain(|entity, entry| {
        if gone.contains(entity.id()) {
            return false;
        }
        let Some(entry) = entry else {
            return true;
        };
        if !entry.name.as_ref().is_some_and(|name| mentions(name, gone)) {
            return true;
        }
        if entry.native.is_none() {
            return false;
        }
        entry.name = None;
        renamed = true;
        true
    });
    delta.owners.retain(|body, _| !gone.contains(body.as_str()));
    delta
        .members
        .retain(|occurrence, _| !gone.contains(occurrence.as_str()));
    delta
        .instance_bodies
        .retain(|path, bodies| !mentions(path, gone) && !mentions(bodies, gone));
    delta
        .poses
        .retain(|path, pose| !mentions(path, gone) && !mentions(pose, gone));
    let after = [
        delta.removed.len(),
        delta.names.len(),
        delta.owners.len(),
        delta.members.len(),
        delta.instance_bodies.len(),
        delta.poses.len(),
    ];
    renamed || before != after
}

/// Clears each `native_ref` of `holder`'s records that names a lost record:
/// the field states the source record a record projects, which no value of
/// the design needs. Removes those names from `holder` and returns whether
/// it names nothing else. A record whose JSON does not read back keeps its
/// names.
fn clear_native_refs(ir: &mut CadIr, holder: &mut Holder) -> bool {
    if !holder.names.iter().any(|(path, _)| path == "native_ref") {
        return false;
    }
    let cleared = ir.rewrite_records(&holder.arena, &holder.id, &mut |record| {
        if let Some(record) = record.as_object_mut() {
            record.remove("native_ref");
        }
        true
    });
    if !cleared {
        return false;
    }
    holder.names.retain(|(path, _)| path != "native_ref");
    holder.names.is_empty()
}

/// States `holder` in its topology's partial form where it names the ids it
/// lost only where that form leaves them unstated, and returns whether it
/// did: a region that loses a shell, a shell that loses a wire edge or a
/// free vertex, and a face that loses a loop are marked (§5.6, Unstated
/// children).
fn keep_topology(ir: &mut CadIr, holder: &Holder) -> bool {
    let Arena::Home {
        home: "geometry",
        field,
    } = &holder.arena
    else {
        return false;
    };
    let id = holder.id.as_str();
    let lost = || holder.names.iter().map(|(_, target)| target.as_str());
    match *field {
        "regions" if holder.names_only_under(&["shells", "unstated"]) => {
            for region in records_mut(&mut ir.geometry.regions, id) {
                for shell in lost() {
                    region.lose_shell(shell);
                }
            }
            true
        }
        "shells" if holder.names_only_under(&["members"]) => {
            for shell in records_mut(&mut ir.geometry.shells, id) {
                for member in lost() {
                    shell.lose_member(member);
                }
            }
            true
        }
        "faces" if holder.names_only_under(&["loops"]) => {
            for face in records_mut(&mut ir.geometry.faces, id) {
                for lost_loop in lost() {
                    face.loops.lose(lost_loop);
                }
            }
            true
        }
        _ => false,
    }
}

/// Restates `holder`, a record that names an id of `gone`, through its JSON
/// in the partial forms of its values (§3.3), and returns whether it names
/// none of them after.
fn restate(ir: &mut CadIr, holder: &Holder, gone: &BTreeSet<String>) -> bool {
    let Arena::Home { home, field } = &holder.arena else {
        return false;
    };
    let Some(schema) = Schema::cadir() else {
        return false;
    };
    let Some(restate) = Restate::new(schema, gone, &holder.id, *home == "process") else {
        return false;
    };
    ir.rewrite_records(&holder.arena, &holder.id, &mut |record| {
        restate.record(home, field, record)
    })
}

/// Removes from `holder`, a scope that names a lost id only as a root, each
/// lost root, and keeps its other roots in their order (§5.2). Returns
/// whether the scope still holds a root or a record names it (§5.1), or
/// `None` where `holder` is no such scope.
fn keep_roots(ir: &mut CadIr, holder: &Holder) -> Option<bool> {
    if holder.arena
        != (Arena::Home {
            home: "design",
            field: "scopes",
        })
        || !holder.names_only_under(&["roots"])
    {
        return None;
    }
    let lost: BTreeSet<&str> = holder
        .names
        .iter()
        .map(|(_, target)| target.as_str())
        .collect();
    for scope in records_mut(&mut ir.design.scopes, &holder.id) {
        scope.roots.retain(|root| !lost.contains(root.as_str()));
    }
    Some(!owned(ir, &holder.arena, &holder.id).is_empty())
}

/// Each scope of `kept` that the document holds with no root and no record
/// that names it, which it removes from `kept`.
fn emptied(ir: &CadIr, kept: &mut BTreeSet<String>) -> Vec<(Arena, String)> {
    let arena = Arena::Home {
        home: "design",
        field: "scopes",
    };
    let empty: Vec<String> = kept
        .iter()
        .filter(|id| !records(&ir.design.scopes, id).is_empty() && owned(ir, &arena, id).is_empty())
        .cloned()
        .collect();
    kept.retain(|id| !empty.contains(id));
    empty.into_iter().map(|id| (arena.clone(), id)).collect()
}

/// States the operation of `holder`, a design feature that names a lost id
/// only in its operation, `Unresolved` with its family (§6), and returns
/// whether it did.
fn keep_unresolved(ir: &mut CadIr, holder: &Holder) -> bool {
    if holder.arena
        != (Arena::Home {
            home: "design",
            field: "features",
        })
        || !holder.names_only_under(&["definition.operation"])
    {
        return false;
    }
    let features = records_mut(&mut ir.design.features, &holder.id);
    let families: Vec<_> = features
        .iter()
        .map(|feature| feature.definition.operation.family())
        .collect();
    let Some(families) = families.into_iter().collect::<Option<Vec<_>>>() else {
        return false;
    };
    for (feature, family) in features.iter_mut().zip(families) {
        feature.definition.operation =
            crate::design::feature::FeatureOperation::Unresolved { family };
    }
    true
}

/// Removes each id of `gone` from the links of every `unknowns` stub: a
/// stub's links are a set.
fn release_stub_links(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    gone: &BTreeSet<String>,
) -> Result<(), CodecError> {
    if gone.is_empty() {
        return Ok(());
    }
    let namespaces: Vec<String> = ir
        .native
        .0
        .iter()
        .filter(|(_, namespace)| namespace.arenas().contains_key("unknowns"))
        .map(|(codec, _)| codec.as_str().to_owned())
        .collect();
    for codec in namespaces {
        let Ok(mut stubs) = ir.native_unknowns(&codec) else {
            continue;
        };
        let mut changed = false;
        for stub in &mut stubs {
            let before = stub.links.len();
            stub.links.retain(|link| !gone.contains(link.as_str()));
            changed |= stub.links.len() != before;
        }
        if changed {
            ir.set_native_unknowns(ctx, &codec, &stubs)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
