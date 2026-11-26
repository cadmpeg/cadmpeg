// SPDX-License-Identifier: Apache-2.0
//! Typed IGES entity accessors and neutral projection.

use cadmpeg_core::CodecError;
use std::collections::BTreeSet;
use std::fmt;

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{record_unreadable_kind, GivenUpRecord};

use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;

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

/// Pushes a loss on `entry` whose entity the projection gives up, with the
/// entity's `record.unreadable`.
fn push_given_up_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    code: IgesLossCode,
    message: fmt::Arguments<'_>,
) {
    give_up(losses, entry, attributed_note(entry, code, message));
}

/// Pushes `why` and the `record.unreadable` that gives up `entry`'s entity,
/// spanning its Directory Entry.
fn give_up(losses: &mut Vec<LossNote>, entry: &DirectoryEntry, why: LossNote) {
    let unreadable = GivenUpRecord::caused_by(
        format_args!("iges:entity:directory#{}", entry.sequence),
        &why,
    )
    .spanning(entry.source_length)
    .loss();
    losses.push(why);
    losses.push(unreadable);
}

/// Keeps the first `record.unreadable` of each entity for which the
/// projection holds no record, and removes the others.
fn settle_given_up(losses: &mut Vec<LossNote>, decoded: &BTreeSet<u32>) {
    let mut charged = BTreeSet::new();
    losses.retain(|loss| {
        given_up_entity(loss)
            .is_none_or(|sequence| !decoded.contains(&sequence) && charged.insert(sequence))
    });
}

fn given_up_entity(loss: &LossNote) -> Option<u32> {
    if loss.code != record_unreadable_kind() {
        return None;
    }
    loss.provenance
        .as_ref()?
        .tag
        .as_deref()?
        .strip_prefix("directory_entry:D")?
        .parse()
        .ok()
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

/// Pushes the loss of an entity that the projection gives up, with its
/// `record.unreadable`.
fn push_entity_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) {
    give_up(losses, entry, entity_note(entry, reason));
}

/// Pushes the loss of geometry that the projection gives up, with its
/// `record.unreadable`.
fn push_geometry_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) {
    give_up(losses, entry, geometry_note(entry, reason));
}

/// Pushes the loss of geometry that the projection keeps with less content.
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
mod presentation;
mod splines;
pub(crate) mod structure;
mod surfaces;
mod trimming;

#[cfg(test)]
mod tests;
