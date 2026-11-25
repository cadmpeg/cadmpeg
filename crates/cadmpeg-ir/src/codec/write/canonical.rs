// SPDX-License-Identifier: Apache-2.0
//! The document a CADIR writer writes: each id once, and only records of the
//! classes the document's scope holds (§2.1, §2.5, §3.4).

use std::collections::BTreeSet;

use cadmpeg_core::decode::{ArenaClass, ContentScope, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;

use super::loss::{value_unwritable, ExportLossCode};
use super::{CadirEncoder, EncoderBackend};
use crate::document::{ArenaName, CadIr, DifferingCopies};
use crate::give_up::Arena;
use crate::report::loss::LossNote;

/// A copy of `ir` that states each id of an arena once and holds no record
/// its scope excludes, with the losses of what the copy leaves out, or
/// `None` where `ir` is already such a document.
///
/// # Errors
///
/// A resource refusal for a rewritten `unknowns` arena.
pub(super) fn canonical_form(ir: &CadIr) -> Result<Option<(CadIr, Vec<LossNote>)>, CodecError> {
    let breach = ir.check_scope().is_err();
    if !breach && ir.ids_in_order() {
        return Ok(None);
    }
    let mut written = ir.clone();
    let mut losses = repeated_ids(&mut written);
    if breach {
        losses.extend(out_of_scope(&mut written)?);
    }
    Ok(Some((written, losses)))
}

/// Keeps the first copy in arena order of each id an arena states more than
/// once, and charges `export:record.invalid` for each id whose copies differ.
fn repeated_ids(ir: &mut CadIr) -> Vec<LossNote> {
    ir.repeated_ids(DifferingCopies::First)
        .into_iter()
        .map(|repeated| {
            let code = match &repeated.arena {
                Arena::Home { home, field } => ArenaName::record_kind(home, field)
                    .map_or(ExportLossCode::RuleInvalid, ExportLossCode::RecordInvalid),
                Arena::Native { .. } => ExportLossCode::RuleInvalid,
            };
            code.note(format!(
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

/// Drops each record of a class the scope of `ir` excludes, each model
/// record that references an excluded design class, and each record that
/// needs one, as §3.3 drops a record.
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
        "a document without the records of the classes its scope excludes and without the \
         model records that reference an excluded design class meets its scope"
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
        "states in its partial form each record that held a record out of the document's scope",
        marked.difference(&dropped).cloned(),
    ));
    Ok(losses)
}

/// Drops the records `held` excludes and each record that needs one, and
/// returns the records it states in their partial forms.
fn drop_excluded(ir: &mut CadIr, held: ContentScope) -> Result<BTreeSet<String>, CodecError> {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    let seeds = ArenaClass::ALL
        .into_iter()
        .filter(|class| !held.holds(*class))
        .flat_map(|class| ir.arenas().seeds_of(class))
        .collect();
    let mut marked = BTreeSet::new();
    for (_, released) in crate::give_up::drop_records(&ctx, ir, seeds)? {
        marked.extend(released.marked);
    }
    if !held.holds(ArenaClass::Design) {
        let seeds = ir.design_reference_seeds();
        for (_, released) in crate::give_up::drop_records(&ctx, ir, seeds)? {
            marked.extend(released.marked);
        }
    }
    Ok(marked)
}
