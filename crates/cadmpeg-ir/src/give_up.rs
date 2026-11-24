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

use std::collections::BTreeSet;

use cadmpeg_core::decode::{ArenaClass, DecodeContext};
use cadmpeg_core::CodecError;
use serde::Serialize;

use crate::document::{ArenaHome, ArenaName, CadIr};
use crate::report::loss::LossNote;
use crate::source_fidelity::SourceFidelity;
use crate::unknown::part::GivenUpPart;

/// The arena that holds a record.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
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
    /// The copies of the id the arena held.
    pub(crate) copies: usize,
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
    /// The holder `entity` is, where it names an id of `gone` other than its
    /// own. A record that has no JSON value names nothing.
    pub(crate) fn of<T: Serialize>(
        home: &'static str,
        field: &'static str,
        id: &str,
        entity: &T,
        gone: &BTreeSet<String>,
    ) -> Option<Self> {
        let value = crate::hash::finite_json::to_json_value(entity).ok()?;
        let mut names = Vec::new();
        crate::reference_walk::visit_edges(
            &value,
            "",
            true,
            &|text| gone.get(text).map(String::as_str).into_iter().collect(),
            &mut |path, target: &str| {
                names.push((path.to_owned(), target.to_owned()));
                true
            },
        );
        (!names.is_empty()).then(|| Self {
            arena: Arena::Home { home, field },
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
    let mut losses = Vec::new();
    for seed in seeds {
        let Some(text) = text(ir, &seed.arena, &seed.id) else {
            continue;
        };
        let Some(released) = remove(ctx, ir, &seed)? else {
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
    let mut removed = Vec::new();
    for seed in seeds {
        if let Some(released) = remove(ctx, ir, &seed)? {
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
    seed: &Seed,
) -> Result<Option<Released>, CodecError> {
    let parents = parents(ir, &seed.arena, &seed.id);
    let copies = take(ir, &seed.arena, &seed.id);
    if copies == 0 {
        return Ok(None);
    }
    let mut released = Released {
        path: format!("{}/{}", seed.arena, seed.id),
        copies,
        dropped: BTreeSet::new(),
        marked: BTreeSet::new(),
    };
    mark_parents(ir, &parents, &mut released.marked);
    release(ctx, ir, &seed.id, &mut released)?;
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
        "faces" => ir
            .geometry
            .faces
            .iter()
            .filter(|face| face.id.as_str() == id)
            .map(|face| face.shell.as_str().to_owned())
            .collect(),
        "regions" => ir
            .geometry
            .regions
            .iter()
            .filter(|region| region.id.as_str() == id)
            .map(|region| region.body.as_str().to_owned())
            .collect(),
        _ => Vec::new(),
    }
}

/// Marks each shell and body of `parents` the document holds: it no longer
/// states each of its children (§5.6, Unstated children).
fn mark_parents(ir: &mut CadIr, parents: &[String], marked: &mut BTreeSet<String>) {
    for parent in parents {
        for shell in ir
            .geometry
            .shells
            .iter_mut()
            .filter(|shell| shell.id.as_str() == parent)
        {
            shell.unstated = Some(crate::topology::Unstated::Members);
            marked.insert(parent.clone());
        }
        for body in ir
            .geometry
            .bodies
            .iter_mut()
            .filter(|body| body.id.as_str() == parent)
        {
            body.unstated = Some(crate::topology::Unstated::Members);
            marked.insert(parent.clone());
        }
    }
}

/// Drops, transitively, each record that needs `id`, which `ir` no longer
/// holds, and marks each record that only lists it, recording both in
/// `released`. An id another arena still holds names that record, so
/// nothing needs it.
fn release(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    id: &str,
    released: &mut Released,
) -> Result<(), CodecError> {
    let mut gone: BTreeSet<String> = BTreeSet::new();
    if !ir.holds_id(id) {
        gone.insert(id.to_owned());
    }
    while !gone.is_empty() {
        ir.release_keyed(&gone);
        release_stub_links(ctx, ir, &gone)?;
        let holders = ir.arenas().holders(&gone);
        let mut next = BTreeSet::new();
        for holder in holders {
            if released.dropped.contains(&holder.id) {
                continue;
            }
            if keep_partial(ir, &holder) {
                released.marked.insert(holder.id);
                continue;
            }
            let parents = parents(ir, &holder.arena, &holder.id);
            if take(ir, &holder.arena, &holder.id) == 0 {
                continue;
            }
            mark_parents(ir, &parents, &mut released.marked);
            if !ir.holds_id(&holder.id) {
                next.insert(holder.id.clone());
            }
            released.dropped.insert(holder.id);
        }
        gone = next;
    }
    released
        .marked
        .retain(|record| !released.dropped.contains(record));
    Ok(())
}

/// States `holder` in the form that holds what is left where it names the
/// ids it lost only where its type has one, and returns whether it did:
/// a region that loses a shell, a shell that loses a wire edge or a free
/// vertex, and a face that loses a loop are marked (§5.6, Unstated
/// children), and a design feature whose operation names a lost id states
/// its operation `Unresolved` with its family (§6).
fn keep_partial(ir: &mut CadIr, holder: &Holder) -> bool {
    let Arena::Home { home, field } = &holder.arena else {
        return false;
    };
    let id = holder.id.as_str();
    let lost = || holder.names.iter().map(|(_, target)| target.as_str());
    match (*home, *field) {
        ("geometry", "regions") if holder.names_only_under(&["shells", "unstated"]) => {
            for region in ir
                .geometry
                .regions
                .iter_mut()
                .filter(|r| r.id.as_str() == id)
            {
                for shell in lost() {
                    region.lose_shell(shell);
                }
            }
            true
        }
        ("geometry", "shells") if holder.names_only_under(&["members"]) => {
            for shell in ir
                .geometry
                .shells
                .iter_mut()
                .filter(|s| s.id.as_str() == id)
            {
                for member in lost() {
                    shell.lose_member(member);
                }
            }
            true
        }
        ("geometry", "faces") if holder.names_only_under(&["loops"]) => {
            for face in ir.geometry.faces.iter_mut().filter(|f| f.id.as_str() == id) {
                for lost_loop in lost() {
                    face.loops.lose(lost_loop);
                }
            }
            true
        }
        ("design", "features") if holder.names_only_under(&["definition.operation"]) => {
            let mut unresolved = false;
            for feature in ir
                .design
                .features
                .iter_mut()
                .filter(|f| f.id.as_str() == id)
            {
                if let Some(family) = feature.definition.operation.family() {
                    feature.definition.operation =
                        crate::design::feature::FeatureOperation::Unresolved { family };
                    unresolved = true;
                }
            }
            unresolved
        }
        _ => false,
    }
}

/// Removes each id of `gone` from the links of every `unknowns` stub: a
/// stub's links are a set.
fn release_stub_links(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    gone: &BTreeSet<String>,
) -> Result<(), CodecError> {
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
