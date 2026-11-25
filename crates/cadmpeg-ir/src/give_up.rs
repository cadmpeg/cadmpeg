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

use crate::document::{ArenaHome, ArenaName, CadIr};
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
    /// The `dropped=[..]; marked=[..]` tail of the loss, `prefix` naming the
    /// records dropped before the cascade, such as the given-up id itself.
    pub(crate) fn tail(&self, prefix: &[&str]) -> String {
        let dropped: BTreeSet<&str> = prefix
            .iter()
            .copied()
            .chain(self.dropped.iter().map(String::as_str))
            .collect();
        format!(
            "dropped=[{}]; marked=[{}]",
            dropped.into_iter().collect::<Vec<_>>().join(", "),
            self.marked
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
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
    /// that `lookup` finds, other than its own. A record that has no JSON
    /// value names nothing.
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
        (!names.is_empty()).then(|| Self {
            arena: arena.clone(),
            id: id.to_owned(),
            names,
        })
    }

    /// Whether every name lies under one of `prefixes`.
    fn names_only_under(&self, prefixes: &[&str]) -> bool {
        self.names.iter().all(|(path, _)| {
            prefixes
                .iter()
                .any(|prefix| path == prefix || path.starts_with(&format!("{prefix}.")))
        })
    }
}

/// The references of a document, from one walk of its records: the number
/// of records that hold each id, and the records that name each id.
#[derive(Debug, Default)]
pub(crate) struct References {
    held: HashMap<String, usize>,
    holders: Vec<(Arena, String)>,
    named_by: HashMap<String, Vec<usize>>,
}

impl References {
    /// Counts one record, neutral or native, that holds `id`.
    pub(crate) fn hold(&mut self, id: &str) {
        *self.held.entry(id.to_owned()).or_default() += 1;
    }

    /// Adds the names `entity`, a record of `arena`, holds of the ids
    /// counted so far. The caller counts every record first.
    pub(crate) fn add<T: Serialize>(&mut self, arena: &Arena, id: &str, entity: &T) {
        let held = &self.held;
        let Some(holder) = Holder::of(arena, id, entity, |text| {
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

    /// Counts `copies` fewer records of `id` and returns whether none is
    /// left.
    fn release(&mut self, id: &str, copies: usize) -> bool {
        self.held.get_mut(id).is_none_or(|held| {
            *held = held.checked_sub(copies).unwrap_or_default();
            *held == 0
        })
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

/// The canonical JSON text of the copies of one record: the record's own
/// where there is one copy, and the array of the copies otherwise. A value
/// with no canonical JSON keeps its plain JSON text.
pub(crate) fn record_text<T: Serialize>(copies: &[T]) -> String {
    fn text<U: Serialize + ?Sized>(value: &U) -> String {
        crate::hash::finite_json::to_canonical_json_string(value).unwrap_or_else(|_| {
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
/// record. `loss` states the loss of each seed from what its give-up did.
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
    ir.finalize();
    let mut references = ir.references();
    let schema = OnceCell::new();
    let mut losses = Vec::new();
    for seed in seeds {
        let Some(text) = text(ir, &seed.arena, &seed.id) else {
            continue;
        };
        let Some(released) = remove(ctx, ir, &mut references, &schema, &seed)? else {
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
    Ok(losses)
}

/// Removes each seed, in order: every copy of it, each record that needs
/// it, transitively, and each mark a record that holds it by a parent
/// pointer or as a member takes (§3.3). A seed an earlier seed's removal
/// dropped is skipped.
///
/// # Errors
///
/// A resource refusal for a rewritten `unknowns` arena.
pub(crate) fn drop_records(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    seeds: Vec<Seed>,
) -> Result<Vec<(Seed, Released)>, CodecError> {
    ir.finalize();
    let mut references = ir.references();
    let schema = OnceCell::new();
    let mut removed = Vec::new();
    for seed in seeds {
        if let Some(released) = remove(ctx, ir, &mut references, &schema, &seed)? {
            removed.push((seed, released));
        }
    }
    Ok(removed)
}

/// Removes every copy of `seed` and releases it, or returns `None` where
/// its arena holds none.
fn remove(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    references: &mut References,
    schema: &OnceCell<Option<Schema>>,
    seed: &Seed,
) -> Result<Option<Released>, CodecError> {
    let parents = parents(ir, &seed.arena, &seed.id);
    let owned = owned(ir, &seed.arena, &seed.id);
    let copies = take(ir, &seed.arena, &seed.id);
    if copies == 0 {
        return Ok(None);
    }
    let mut released = Released {
        path: format!("{}/{}", seed.arena, seed.id),
        dropped: BTreeSet::new(),
        marked: BTreeSet::new(),
    };
    mark_parents(ir, &parents, &mut released.marked);
    let gone = references.release(&seed.id, copies);
    release(
        ctx,
        ir,
        references,
        schema,
        gone.then(|| seed.id.clone()),
        owned,
        &mut released,
    )?;
    Ok(Some(released))
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

/// The procedural constructions that the copies of `id` in `arena` carry:
/// the document states no construction without its owning carrier.
fn owned(ir: &CadIr, arena: &Arena, id: &str) -> Vec<(Arena, String)> {
    let Arena::Home {
        home: "geometry",
        field,
    } = arena
    else {
        return Vec::new();
    };
    let constructions: Vec<(&str, String)> = match *field {
        "curves" => records(&ir.geometry.curves, id)
            .iter()
            .filter_map(|curve| curve.geometry.procedural_construction())
            .map(|construction| ("procedural_curves", construction.as_str().to_owned()))
            .collect(),
        "surfaces" => records(&ir.geometry.surfaces, id)
            .iter()
            .filter_map(|surface| surface.geometry.procedural_construction())
            .map(|construction| ("procedural_surfaces", construction.as_str().to_owned()))
            .collect(),
        _ => Vec::new(),
    };
    constructions
        .into_iter()
        .filter_map(|(field, id)| Some((Arena::home("geometry", field)?, id)))
        .collect()
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

/// Drops, transitively, each record that needs `gone`, an id no record of
/// `ir` holds, with `carried` and each construction a dropped carrier owns,
/// and marks each record that holds it in a form that keeps the rest,
/// recording both in `released`. Then removes every id it
/// dropped from the collections keyed by id and from the stubs' links,
/// which no record needs. `schema` holds the CADIR schema once a record
/// needs it.
fn release(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    references: &mut References,
    schema: &OnceCell<Option<Schema>>,
    gone: Option<String>,
    mut carried: Vec<(Arena, String)>,
    released: &mut Released,
) -> Result<(), CodecError> {
    let mut gone: BTreeSet<String> = gone.into_iter().collect();
    let mut all_gone = gone.clone();
    while !gone.is_empty() || !carried.is_empty() {
        let mut drops: BTreeMap<Arena, BTreeSet<String>> = BTreeMap::new();
        for (arena, id) in carried.drain(..) {
            drops.entry(arena).or_default().insert(id);
        }
        for (arena, id) in references.naming(&gone) {
            let Some(mut holder) = ir.holder(arena, id, &gone) else {
                continue;
            };
            if clear_native_refs(ir, &mut holder)
                || keep_topology(ir, &holder)
                || restate(ir, &holder, &gone, schema)
                || keep_unresolved(ir, &holder)
            {
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
    Ok(())
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
    let cleared = ir.rewrite_records(&holder.arena, &holder.id, |record| {
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
/// none of them after. `schema` is read when first needed.
fn restate(
    ir: &mut CadIr,
    holder: &Holder,
    gone: &BTreeSet<String>,
    schema: &OnceCell<Option<Schema>>,
) -> bool {
    let Arena::Home { home, field } = &holder.arena else {
        return false;
    };
    let Some(schema) = schema.get_or_init(Schema::read) else {
        return false;
    };
    let Some(restate) = Restate::new(schema, gone, &holder.id, *home == "process") else {
        return false;
    };
    ir.rewrite_records(&holder.arena, &holder.id, |record| {
        restate.record(home, field, record)
    })
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
