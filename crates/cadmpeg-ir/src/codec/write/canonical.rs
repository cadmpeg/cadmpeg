// SPDX-License-Identifier: Apache-2.0
//! The document a CADIR writer writes: each id once, only records of the
//! classes the document's scope holds, each procedural construction with one
//! owning carrier, and native-record links that name a written record
//! (§2.1, §2.5, §3.3, §3.4).

use std::collections::BTreeSet;

use cadmpeg_core::decode::{ArenaClass, ContentScope, DecodeContext};
use cadmpeg_core::CodecError;

use super::loss::{value_unwritable, ExportLossCode};
use super::{CadirEncoder, EncoderBackend};
use crate::design::evaluation::StateDelta;
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
pub(super) fn canonical_form(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<Option<(CadIr, Vec<LossNote>)>, CodecError> {
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
        losses.extend(out_of_scope(ctx, &mut written)?);
    }
    losses.extend(without_unowned_constructions(ctx, &mut written)?);
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

/// Each procedural construction of `geometry` without one owning carrier:
/// no carrier, or more than one, names it.
fn unowned_constructions(geometry: &Topology) -> Vec<Seed> {
    let seed = |field: &'static str, id: &str| Seed {
        arena: Arena::Home {
            home: "geometry",
            field,
        },
        id: id.to_owned(),
    };
    let surfaces = crate::hash::procedural_owners(geometry.surfaces.iter().filter_map(|carrier| {
        carrier
            .geometry
            .procedural_construction()
            .map(|construction| (construction.as_str(), &carrier.id))
    }));
    let curves = crate::hash::procedural_owners(geometry.curves.iter().filter_map(|carrier| {
        carrier
            .geometry
            .procedural_construction()
            .map(|construction| (construction.as_str(), &carrier.id))
    }));
    let surfaces = geometry
        .procedural_surfaces
        .iter()
        .filter(|construction| surfaces.get(construction.id.as_str()).is_none())
        .map(|construction| seed("procedural_surfaces", construction.id.as_str()));
    let curves = geometry
        .procedural_curves
        .iter()
        .filter(|construction| curves.get(construction.id.as_str()).is_none())
        .map(|construction| seed("procedural_curves", construction.id.as_str()));
    surfaces.chain(curves).collect()
}

/// Whether a procedural construction of `ir`, of its geometry or of the
/// delta of one of its stored states, has no one owning carrier.
fn holds_unowned_constructions(ir: &CadIr) -> bool {
    std::iter::once(&ir.geometry)
        .chain(
            stored_deltas(ir)
                .into_iter()
                .map(|(_, delta)| &delta.records),
        )
        .any(|geometry| !unowned_constructions(geometry).is_empty())
}

/// The stored state of `field`, `evaluations` of the design or `states` of
/// the process, whose id is `id`.
fn stored_state(field: &'static str, id: &str) -> Seed {
    let home = if field == "evaluations" {
        "design"
    } else {
        "process"
    };
    Seed {
        arena: Arena::Home { home, field },
        id: id.to_owned(),
    }
}

/// The delta of each stored state of `ir`, with the state.
fn stored_deltas(ir: &CadIr) -> Vec<(Seed, &StateDelta)> {
    let evaluations = ir.design.evaluations.iter().filter_map(|evaluation| {
        let content = evaluation.content.as_ref()?;
        Some((
            stored_state("evaluations", evaluation.id.as_str()),
            &content.delta,
        ))
    });
    let states = ir.process.states.iter().filter_map(|stored| {
        let content = stored.content.as_ref()?;
        Some((stored_state("states", stored.id.as_str()), &content.delta))
    });
    evaluations.chain(states).collect()
}

/// The records of the delta of each stored state of `ir`, with the state.
fn stored_deltas_mut(ir: &mut CadIr) -> Vec<(Seed, &mut Topology)> {
    let evaluations = ir.design.evaluations.iter_mut().filter_map(|evaluation| {
        let state = stored_state("evaluations", evaluation.id.as_str());
        let content = evaluation.content.as_mut()?;
        Some((state, &mut content.delta.records))
    });
    let states = ir.process.states.iter_mut().filter_map(|stored| {
        let state = stored_state("states", stored.id.as_str());
        let content = stored.content.as_mut()?;
        Some((state, &mut content.delta.records))
    });
    evaluations.chain(states).collect()
}

/// Copies into the delta of each stored state, for each procedural
/// construction of the delta that no carrier of the delta names, the first
/// carrier in identity order of the state's base that names it and that the
/// delta neither holds nor removes. The delta then replaces that carrier
/// with an equal copy, so the state holds what the source stored, and the
/// construction has its one owning carrier in the delta (§5.4).
fn adopt_base_owners(ir: &mut CadIr) {
    use crate::design::edit::states::{ids, record_ref_id};

    let mut adopted: Vec<(Seed, Topology)> = Vec::new();
    for (state, delta) in stored_deltas(ir) {
        let unowned = unowned_constructions(&delta.records);
        if unowned.is_empty() {
            continue;
        }
        let base = if matches!(state.arena, Arena::Home { home: "design", .. }) {
            ir.evaluation_base_records(&state.id)
        } else {
            ir.process_base_records(&state.id)
        };
        let Some(base) = base else {
            continue;
        };
        let mut excluded = ids(&delta.records);
        excluded.extend(
            delta
                .removed
                .iter()
                .map(|record| record_ref_id(record).to_owned()),
        );
        let mut carriers = Topology::default();
        for construction in &unowned {
            let field = match &construction.arena {
                Arena::Home { field, .. } => *field,
                Arena::Native { .. } => continue,
            };
            if field == "procedural_surfaces" {
                carriers.surfaces.extend(first_owner(
                    &base.surfaces,
                    &construction.id,
                    &excluded,
                    |carrier| {
                        carrier
                            .geometry
                            .procedural_construction()
                            .map(crate::ids::ProceduralSurfaceId::as_str)
                    },
                ));
            } else {
                carriers.curves.extend(first_owner(
                    &base.curves,
                    &construction.id,
                    &excluded,
                    |carrier| {
                        carrier
                            .geometry
                            .procedural_construction()
                            .map(crate::ids::ProceduralCurveId::as_str)
                    },
                ));
            }
        }
        if !carriers.surfaces.is_empty() || !carriers.curves.is_empty() {
            adopted.push((state, carriers));
        }
    }
    for (state, records) in stored_deltas_mut(ir) {
        if let Some((_, carriers)) = adopted.iter_mut().find(|(held, _)| *held == state) {
            records.surfaces.append(&mut carriers.surfaces);
            records.curves.append(&mut carriers.curves);
        }
    }
}

/// The first carrier of `carriers` in identity order that names
/// `construction` through `names` and whose id `excluded` does not hold.
fn first_owner<T: crate::schema::ArenaRecord + Clone>(
    carriers: &[T],
    construction: &str,
    excluded: &BTreeSet<String>,
    names: impl Fn(&T) -> Option<&str>,
) -> Option<T> {
    carriers
        .iter()
        .filter(|carrier| {
            !excluded.contains(carrier.record_identity()) && names(carrier) == Some(construction)
        })
        .min_by(|left, right| left.record_identity().cmp(right.record_identity()))
        .cloned()
}

/// Keeps each procedural construction of `geometry` that more than one
/// carrier names with the first of them in identity order, and states each
/// other carrier without it (§3.3). A construction `geometry` does not hold
/// has no row to keep: the wire writes each carrier that names it from its
/// cache. Each construction charges one
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
    let surfaces: BTreeSet<String> = geometry
        .procedural_surfaces
        .iter()
        .map(|construction| construction.id.as_str().to_owned())
        .collect();
    let curves: BTreeSet<String> = geometry
        .procedural_curves
        .iter()
        .map(|construction| construction.id.as_str().to_owned())
        .collect();
    let mut losses: Vec<LossNote> = release_shared(
        &mut geometry.surfaces,
        &surfaces,
        |carrier| {
            carrier
                .geometry
                .procedural_construction()
                .map(crate::ids::ProceduralSurfaceId::as_str)
        },
        |carrier| carrier.geometry.release(true, false),
    )
    .into_iter()
    .map(|shared| loss("surfaces", "procedural_surfaces", shared))
    .collect();
    losses.extend(
        release_shared(
            &mut geometry.curves,
            &curves,
            |carrier| {
                carrier
                    .geometry
                    .procedural_construction()
                    .map(crate::ids::ProceduralCurveId::as_str)
            },
            |carrier| carrier.geometry.release(true, false),
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
/// `construction`, a construction of `held` that a carrier before it in
/// identity order names, and returns each construction so shared.
fn release_shared<T: crate::schema::ArenaRecord>(
    carriers: &mut [T],
    held: &BTreeSet<String>,
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
        if slots.len() < 2 || !held.contains(&construction) {
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
/// and of the delta of each stored state, from that delta alone, where the
/// state's base holds no carrier that names it (§3.4). Each dropped
/// construction charges one `export:record.invalid` that names it and the
/// records dropped and marked with it.
fn without_unowned_constructions(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
) -> Result<Vec<LossNote>, CodecError> {
    let format = CadirEncoder::FORMAT.as_str();
    let mut losses = shared_constructions(&mut ir.geometry, "");
    loop {
        let unowned = unowned_constructions(&ir.geometry);
        if unowned.is_empty() {
            break;
        }
        let removed = crate::give_up::drop_records(ctx, ir, unowned)?;
        if removed.is_empty() {
            break;
        }
        for (seed, released) in removed {
            losses.push(record_invalid_code(&seed.arena).note(format!(
                "the {format} write drops procedural construction {} of arena {}, which no \
                 carrier names, with each record that needs it: a construction has one \
                 owning carrier; dropped=[{}], marked=[{}]",
                seed.id,
                seed.arena,
                listed(&released.dropped),
                listed(&released.marked)
            )));
        }
    }
    for (state, records) in stored_deltas_mut(ir) {
        let place = format!(" in the delta of {}/{}", state.arena, state.id);
        losses.extend(shared_constructions(records, &place));
    }
    adopt_base_owners(ir);
    let mut unowned = Vec::new();
    for (state, records) in stored_deltas_mut(ir) {
        let mut by_arena: std::collections::BTreeMap<Arena, BTreeSet<String>> =
            std::collections::BTreeMap::new();
        for construction in unowned_constructions(records) {
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
        let Some(released) = crate::give_up::drop_state_records(ctx, ir, &state, ids)? else {
            continue;
        };
        losses.push(record_invalid_code(&arena).note(format!(
            "the {format} write drops procedural constructions [{constructions}] of arena \
             {arena} from the delta of {}/{}, which no carrier of the delta or of the \
             state's base names, with each record of the delta that needs one: a \
             construction has one owning carrier; dropped=[{}], marked=[{}]",
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

/// Drops each record of a class the scope of `ir` excludes and each record
/// that needs one, as §3.3 drops a record. Where the scope excludes the
/// design class, each model reference into it is absent and each attribute
/// whose target is a design record is left out (§2.1).
fn out_of_scope(ctx: &DecodeContext<'_>, ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let format = CadirEncoder::FORMAT.as_str();
    let before: Vec<(ArenaName, _, BTreeSet<String>)> = ir
        .arenas()
        .record_ids()
        .into_iter()
        .map(|(arena, (kind, ids))| (arena, kind, ids.into_iter().map(str::to_owned).collect()))
        .collect();
    let scope = ir.scope();
    let marked = drop_excluded(ctx, ir, scope)?;
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
fn drop_excluded(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    held: ContentScope,
) -> Result<BTreeSet<String>, CodecError> {
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
    for (_, released) in crate::give_up::drop_records(ctx, ir, seeds)? {
        marked.extend(released.marked);
    }
    Ok(marked)
}
