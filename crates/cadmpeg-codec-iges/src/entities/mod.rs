// SPDX-License-Identifier: Apache-2.0
//! Typed IGES entity accessors and neutral projection.

use cadmpeg_core::CodecError;
use std::collections::BTreeSet;
use std::fmt;

use cadmpeg_ir::report::loss::LossNote;

use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;

fn push_attributed_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    code: IgesLossCode,
    message: fmt::Arguments<'_>,
) {
    losses.push(
        code.note(message.to_string())
            .with_provenance(entry.admitted_loss_provenance()),
    );
}

fn push_entity_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) {
    push_attributed_loss(
        losses,
        entry,
        IgesLossCode::EntityNotProjected,
        format_args!(
            "IGES entity type {} form {} was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    );
}

fn push_geometry_loss(
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) {
    push_attributed_loss(
        losses,
        entry,
        IgesLossCode::GeometryNotProjected,
        format_args!(
            "IGES entity type {} form {} geometry was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    );
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
