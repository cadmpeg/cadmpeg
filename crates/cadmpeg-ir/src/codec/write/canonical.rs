// SPDX-License-Identifier: Apache-2.0
//! The document a CADIR writer writes: each id once, only records of the
//! classes the document's scope holds, each procedural construction with one
//! owning carrier, and native-record links that name a written record
//! (§2.1, §2.5, §3.3, §3.4).

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::{ArenaClass, ContentScope};
use cadmpeg_core::CodecError;

use super::loss::{value_unwritable, ExportLossCode};
use super::{CadirEncoder, EncoderBackend};
use crate::document::procedural::standard_owners;
use crate::document::{ArenaName, CadIr, DifferingCopies, ScopeViolation, Topology};
use crate::give_up::{Arena, Seed};
use crate::report::loss::LossNote;

/// A copy of `ir` that states each id of an arena once, holds no record its
/// scope excludes and no procedural construction without one owning
/// carrier, and links no record the write leaves out, with the losses of
/// what the copy leaves out, or `None` where `ir` is already such a
/// document.
///
/// # Errors
///
/// A native record whose text no longer reads as an object.
pub(super) fn canonical_form(ir: &CadIr) -> Result<Option<(CadIr, Vec<LossNote>)>, CodecError> {
    let breach = holds_out_of_scope(ir);
    if !breach
        && ir.ids_in_order()
        && !holds_unowned_constructions(ir)
        && unwritten_links(ir).is_empty()
    {
        return Ok(None);
    }
    let mut written = ir.clone();
    let mut losses = repeated_ids(&mut written);
    if breach {
        losses.extend(out_of_scope(&mut written)?);
    }
    losses.extend(without_unowned_constructions(&mut written)?);
    let unwritten = unwritten_links(&written);
    if !unwritten.is_empty() {
        let released = written.native.release_links(&unwritten)?;
        losses.extend(value_unwritable(
            CadirEncoder::FORMAT.as_str(),
            "writes each native record without its link to a record of an arena the CADIR \
             wire omits",
            released
                .into_iter()
                .map(|(record, link)| format!("{record} -> {link}")),
        ));
    }
    Ok(Some((written, losses)))
}

/// Whether an arena of `ir` holds a record of a class its scope excludes or
/// a reference into one. The write states the scope as given.
fn holds_out_of_scope(ir: &CadIr) -> bool {
    ir.scope_violations()
        .iter()
        .any(|violation| !matches!(violation, ScopeViolation::Unmet { .. }))
}

/// Each id that a native-record link of `ir` names, that a record of an
/// arena the CADIR wire omits has, and that no written record has.
fn unwritten_links(ir: &CadIr) -> BTreeSet<String> {
    let omitted = ir.model.omitted_record_ids();
    if omitted.is_empty() {
        return BTreeSet::new();
    }
    let linked = ir.native.linked_among(&omitted);
    if linked.is_empty() {
        return BTreeSet::new();
    }
    let written = ir.arenas().written_among(&linked);
    let native = ir.native.held_among(&linked);
    linked
        .into_iter()
        .filter(|id| !written.contains(id) && !native.contains(id))
        .map(str::to_owned)
        .collect()
}

/// Keeps the first copy in arena order of each id an arena states more than
/// once, and charges `export:record.invalid` for each id whose copies differ.
fn repeated_ids(ir: &mut CadIr) -> Vec<LossNote> {
    ir.repeated_ids(DifferingCopies::First)
        .into_iter()
        .map(|repeated| {
            record_invalid_code(&repeated.arena).note(format!(
                "the {} write states id {} of arena {} once: its {} copies differ, and the \
                 write keeps the first in arena order",
                CadirEncoder::FORMAT.as_str(),
                repeated.id,
                repeated.arena,
                repeated.copies
            ))
        })
        .collect()
}

/// The `export:record.invalid` code of a record of `arena`.
pub(super) fn record_invalid_code(arena: &Arena) -> ExportLossCode {
    match arena {
        Arena::Home { home, field } => ArenaName::record_kind(home, field)
            .map_or(ExportLossCode::RuleInvalid, ExportLossCode::RecordInvalid),
        Arena::Native { .. } => ExportLossCode::RuleInvalid,
    }
}

/// Each procedural construction of `geometry` without one owning carrier,
/// with the carriers that name it: none, or more than one.
fn unowned_constructions(geometry: &Topology) -> Vec<(Seed, &'static str)> {
    fn unowned<T: crate::schema::ArenaRecord, O>(
        field: &'static str,
        constructions: &[T],
        owners: &HashMap<&str, Option<O>>,
        found: &mut Vec<(Seed, &'static str)>,
    ) {
        for construction in constructions {
            let id = construction.record_identity();
            let named = match owners.get(id) {
                Some(Some(_)) => continue,
                Some(None) => "more than one carrier names",
                None => "no carrier names",
            };
            let arena = Arena::Home {
                home: "geometry",
                field,
            };
            found.push((
                Seed {
                    arena,
                    id: id.to_owned(),
                },
                named,
            ));
        }
    }
    let mut found = Vec::new();
    if !geometry.procedural_surfaces.is_empty() {
        let owners = standard_owners(geometry.surfaces.iter().filter_map(|carrier| {
            carrier
                .geometry
                .procedural_construction()
                .map(|construction| (construction.as_str(), &carrier.id))
        }));
        unowned(
            "procedural_surfaces",
            &geometry.procedural_surfaces,
            &owners,
            &mut found,
        );
    }
    if !geometry.procedural_curves.is_empty() {
        let owners = standard_owners(geometry.curves.iter().filter_map(|carrier| {
            carrier
                .geometry
                .procedural_construction()
                .map(|construction| (construction.as_str(), &carrier.id))
        }));
        unowned(
            "procedural_curves",
            &geometry.procedural_curves,
            &owners,
            &mut found,
        );
    }
    found
}

/// Whether a procedural construction of `ir`, of its geometry or of the
/// delta of one of its stored states, has no one owning carrier.
fn holds_unowned_constructions(ir: &CadIr) -> bool {
    let evaluations = ir
        .design
        .evaluations
        .iter()
        .filter_map(|evaluation| evaluation.content.as_ref())
        .map(|content| &content.delta.records);
    let states = ir
        .process
        .states
        .iter()
        .filter_map(|state| state.content.as_ref())
        .map(|content| &content.delta.records);
    std::iter::once(&ir.geometry)
        .chain(evaluations)
        .chain(states)
        .any(|geometry| !unowned_constructions(geometry).is_empty())
}

/// The records of the delta of each stored state of `ir`, with the state.
fn stored_deltas_mut(ir: &mut CadIr) -> Vec<(Seed, &mut Topology)> {
    let evaluations = ir.design.evaluations.iter_mut().filter_map(|evaluation| {
        let state = Seed {
            arena: Arena::Home {
                home: "design",
                field: "evaluations",
            },
            id: evaluation.id.as_str().to_owned(),
        };
        let content = evaluation.content.as_mut()?;
        Some((state, &mut content.delta.records))
    });
    let states = ir.process.states.iter_mut().filter_map(|stored| {
        let state = Seed {
            arena: Arena::Home {
                home: "process",
                field: "states",
            },
            id: stored.id.as_str().to_owned(),
        };
        let content = stored.content.as_mut()?;
        Some((state, &mut content.delta.records))
    });
    evaluations.chain(states).collect()
}

/// Keeps each procedural construction of `geometry` that more than one
/// carrier names with the first of them in identity order, and states each
/// other carrier without it (§3.3). Each construction charges one
/// `export:record.invalid` that names it and the carriers, with `place`,
/// where `geometry` is not the document's own.
fn shared_constructions(geometry: &mut Topology, place: &str) -> Vec<LossNote> {
    let loss = |carriers: &'static str, constructions: &'static str, shared: Shared| {
        let (construction, kept, released) = shared;
        record_invalid_code(&Arena::Home {
            home: "geometry",
            field: carriers,
        })
        .note(format!(
            "the {} write keeps procedural construction {construction} of arena \
             geometry.{constructions}{place} with carrier {kept}, the first in identity \
             order of the carriers that name it, and states carriers [{}] without it, \
             each as its retained cache where it holds one and as `Unknown` otherwise: \
             a construction has one owning carrier",
            CadirEncoder::FORMAT.as_str(),
            released.join(", ")
        ))
    };
    let mut losses: Vec<LossNote> = release_shared(
        &mut geometry.surfaces,
        |carrier| {
            carrier
                .geometry
                .procedural_construction()
                .map(crate::ids::ProceduralSurfaceId::as_str)
        },
        |carrier| carrier.geometry.release_construction(),
    )
    .into_iter()
    .map(|shared| loss("surfaces", "procedural_surfaces", shared))
    .collect();
    losses.extend(
        release_shared(
            &mut geometry.curves,
            |carrier| {
                carrier
                    .geometry
                    .procedural_construction()
                    .map(crate::ids::ProceduralCurveId::as_str)
            },
            |carrier| carrier.geometry.release_construction(),
        )
        .into_iter()
        .map(|shared| loss("curves", "procedural_curves", shared)),
    );
    losses
}

/// A construction that more than one carrier names, the carrier that keeps
/// it, and the carriers stated without it.
type Shared = (String, String, Vec<String>);

/// Applies `release` to each carrier of `carriers` that names, through
/// `construction`, a construction that a carrier before it in identity
/// order names, and returns each construction so shared.
fn release_shared<T: crate::schema::ArenaRecord>(
    carriers: &mut [T],
    construction: impl Fn(&T) -> Option<&str>,
    release: impl Fn(&mut T),
) -> Vec<Shared> {
    let mut named: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for (slot, carrier) in carriers.iter().enumerate() {
        if let Some(construction) = construction(carrier) {
            named.entry(construction.to_owned()).or_default().push(slot);
        }
    }
    let mut shared = Vec::new();
    for (construction, mut slots) in named {
        if slots.len() < 2 {
            continue;
        }
        slots.sort_by(|left, right| {
            carriers[*left]
                .record_identity()
                .cmp(carriers[*right].record_identity())
        });
        let kept = carriers[slots[0]].record_identity().to_owned();
        let released = slots[1..]
            .iter()
            .map(|&slot| {
                release(&mut carriers[slot]);
                carriers[slot].record_identity().to_owned()
            })
            .collect();
        shared.push((construction, kept, released));
    }
    shared
}

/// Keeps each procedural construction that more than one carrier names with
/// one of them, then drops each construction that no carrier names and each
/// record that needs it, as §3.3 drops a record: of the geometry of `ir`,
/// and of the delta of each stored state, from that delta alone (§3.4).
/// Each dropped construction charges one `export:record.invalid` that names
/// it and the records dropped and marked with it.
fn without_unowned_constructions(ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let format = CadirEncoder::FORMAT.as_str();
    let mut losses = shared_constructions(&mut ir.geometry, "");
    loop {
        let unowned: std::collections::BTreeMap<Seed, &str> =
            unowned_constructions(&ir.geometry).into_iter().collect();
        if unowned.is_empty() {
            break;
        }
        let removed = drop_seeds(ir, unowned.keys().cloned().collect())?;
        if removed.is_empty() {
            break;
        }
        for (seed, released) in removed {
            losses.push(record_invalid_code(&seed.arena).note(format!(
                "the {format} write drops procedural construction {} of arena {}, which {}, \
                 with each record that needs it: a construction has one owning carrier; \
                 dropped=[{}], marked=[{}]",
                seed.id,
                seed.arena,
                unowned.get(&seed).copied().unwrap_or_default(),
                listed(&released.dropped),
                listed(&released.marked)
            )));
        }
    }
    let mut unowned = Vec::new();
    for (state, records) in stored_deltas_mut(ir) {
        let place = format!(" in the delta of {}/{}", state.arena, state.id);
        losses.extend(shared_constructions(records, &place));
        let mut by_arena: std::collections::BTreeMap<Arena, BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for (construction, _) in unowned_constructions(records) {
            by_arena
                .entry(construction.arena)
                .or_default()
                .insert(construction.id);
        }
        unowned.extend(
            by_arena
                .into_iter()
                .map(|(arena, ids)| (state.clone(), arena, ids)),
        );
    }
    for (state, arena, ids) in unowned {
        let constructions = listed(&ids);
        let Some(released) = drop_state_records(ir, &state, ids)? else {
            continue;
        };
        losses.push(record_invalid_code(&arena).note(format!(
            "the {format} write drops procedural constructions [{constructions}] of arena \
             {arena} from the delta of {}/{}, which no carrier of the delta names, with \
             each record of the delta that needs one: a construction has one owning \
             carrier; dropped=[{}], marked=[{}]",
            state.arena,
            state.id,
            listed(&released.dropped),
            listed(&released.marked)
        )));
    }
    Ok(losses)
}

/// The ids of `ids`, in order, separated by commas.
pub(super) fn listed(ids: &BTreeSet<String>) -> String {
    ids.iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Drops from `ir` each record of `seeds` and each record that needs one, as
/// §3.3 drops a record, and returns what each drop removed.
pub(super) fn drop_seeds(
    ir: &mut CadIr,
    seeds: Vec<Seed>,
) -> Result<Vec<(Seed, crate::give_up::Released)>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    crate::give_up::drop_records(&ctx, ir, seeds)
}

/// Drops from the delta of the stored state `state` each of its records of
/// `ids` and each record of the delta that needs one, as §3.3 drops a
/// record, and returns what the delta dropped and marked.
fn drop_state_records(
    ir: &mut CadIr,
    state: &Seed,
    ids: BTreeSet<String>,
) -> Result<Option<crate::give_up::Released>, CodecError> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    crate::give_up::drop_state_records(&ctx, ir, state, ids)
}

/// Drops each record of a class the scope of `ir` excludes and each record
/// that needs one, as §3.3 drops a record. Where the scope excludes the
/// design class, each model reference into it is absent and each attribute
/// whose target is a design record is left out (§2.1).
fn out_of_scope(ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let format = CadirEncoder::FORMAT.as_str();
    let before: Vec<(ArenaName, _, BTreeSet<String>)> = ir
        .arenas()
        .record_ids()
        .into_iter()
        .map(|(arena, (kind, ids))| (arena, kind, ids.into_iter().map(str::to_owned).collect()))
        .collect();
    let scope = ir.scope();
    let marked = drop_excluded(ir, scope)?;
    debug_assert!(
        !holds_out_of_scope(ir),
        "a document without the records of the classes its scope excludes and without model \
         references into an excluded design class meets its scope"
    );
    let after = ir.arenas().record_ids();
    let mut losses = Vec::new();
    let mut dropped = BTreeSet::new();
    for (arena, kind, ids) in before {
        let kept = after.get(&arena).map(|(_, kept)| kept);
        let omitted: Vec<&str> = ids
            .iter()
            .map(String::as_str)
            .filter(|id| kept.is_none_or(|kept| !kept.contains(id)))
            .collect();
        if omitted.is_empty() {
            continue;
        }
        losses.push(ExportLossCode::ArenaOmitted(kind).note(format!(
            "the {format} write drops the records of arena `{arena}` that the document's \
             scope excludes or that need one: {} record(s) omitted: {}",
            omitted.len(),
            omitted.join(", ")
        )));
        dropped.extend(omitted.into_iter().map(str::to_owned));
    }
    losses.extend(value_unwritable(
        format,
        "writes without its reference to a record out of the document's scope each record \
         that held one",
        marked.difference(&dropped).cloned(),
    ));
    Ok(losses)
}

/// Drops the records `held` excludes and each record that needs one, and
/// returns the records it states without a reference or in their partial
/// forms.
fn drop_excluded(ir: &mut CadIr, held: ContentScope) -> Result<BTreeSet<String>, CodecError> {
    let mut marked = BTreeSet::new();
    let mut seeds = Vec::new();
    if !held.holds(ArenaClass::Design) {
        let (kept, left_out) = ir.leave_design_out();
        marked.extend(kept);
        seeds = left_out;
    }
    seeds.extend(
        ArenaClass::ALL
            .into_iter()
            .filter(|class| !held.holds(*class))
            .flat_map(|class| ir.arenas().seeds_of(class)),
    );
    for (_, released) in drop_seeds(ir, seeds)? {
        marked.extend(released.marked);
    }
    Ok(marked)
}
