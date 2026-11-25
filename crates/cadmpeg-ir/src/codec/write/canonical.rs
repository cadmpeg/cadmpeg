// SPDX-License-Identifier: Apache-2.0
//! The document a CADIR writer writes: each id once, only records of the
//! classes the document's scope holds, each procedural construction with one
//! owning carrier, feature parent relations the model admits, and
//! native-record links that name a written record (§2.1, §2.5, §3.3, §3.4).

use std::collections::{BTreeSet, HashMap};

use cadmpeg_core::decode::{ArenaClass, ContentScope};
use cadmpeg_core::CodecError;

use super::loss::{value_unwritable, ExportLossCode};
use super::{CadirEncoder, EncoderBackend};
use crate::document::procedural::standard_owners;
use crate::document::{feature_parents, ArenaName, CadIr, DifferingCopies};
use crate::give_up::{Arena, Seed};
use crate::report::loss::LossNote;

/// A copy of `ir` that states each id of an arena once, holds no record its
/// scope excludes, no procedural construction without one owning carrier,
/// and no feature parent relation the model refuses, and links no record the
/// write leaves out, with the losses of what the copy leaves out, or `None`
/// where `ir` is already such a document.
///
/// # Errors
///
/// A native record whose text no longer reads as an object.
pub(super) fn canonical_form(ir: &CadIr) -> Result<Option<(CadIr, Vec<LossNote>)>, CodecError> {
    let breach = ir.check_scope().is_err();
    if !breach
        && ir.ids_in_order()
        && unowned_constructions(ir).is_empty()
        && feature_parents::validate(&[&ir.model]).is_ok()
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
    losses.extend(without_invalid_feature_parents(&mut written));
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

/// Each procedural construction of `ir` without one owning carrier, with
/// the carriers that name it: none, or more than one.
fn unowned_constructions(ir: &CadIr) -> Vec<(Seed, &'static str)> {
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
    let geometry = &ir.geometry;
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

/// Drops each procedural construction of `ir` without one owning carrier and
/// each record that needs it, as §3.3 drops a record, and states each
/// carrier that named it without the construction (§3.4). Each construction
/// charges one `export:record.invalid` that names it and the records dropped
/// and marked with it.
fn without_unowned_constructions(ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    loop {
        let unowned: std::collections::BTreeMap<Seed, &str> =
            unowned_constructions(ir).into_iter().collect();
        if unowned.is_empty() {
            return Ok(losses);
        }
        let removed = drop_seeds(ir, unowned.keys().cloned().collect())?;
        if removed.is_empty() {
            return Ok(losses);
        }
        for (seed, released) in removed {
            losses.push(record_invalid_code(&seed.arena).note(format!(
                "the {} write drops procedural construction {} of arena {}, which {}, \
                 with each record that needs it: a construction has one owning carrier; \
                 dropped=[{}], marked=[{}]",
                CadirEncoder::FORMAT.as_str(),
                seed.id,
                seed.arena,
                unowned.get(&seed).copied().unwrap_or_default(),
                listed(&released.dropped),
                listed(&released.marked)
            )));
        }
    }
}

/// The ids of `ids`, in order, separated by commas.
pub(super) fn listed(ids: &BTreeSet<String>) -> String {
    ids.iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Drops each feature parent relation of `ir` the model refuses, and charges
/// one `export:record.invalid` that names the child feature of each.
fn without_invalid_feature_parents(ir: &mut CadIr) -> Vec<LossNote> {
    let released = feature_parents::release_invalid(&mut ir.model);
    debug_assert!(
        feature_parents::validate(&[&ir.model]).is_ok(),
        "a model that states each feature id once and holds no refused relation admits its \
         relations"
    );
    released
        .into_iter()
        .map(|(_, why)| {
            ExportLossCode::RecordInvalid(
                <crate::features::Feature as crate::schema::ArenaRecord>::KIND,
            )
            .note(format!("the {} write {why}", CadirEncoder::FORMAT.as_str()))
        })
        .collect()
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
        ir.check_scope().is_ok(),
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
