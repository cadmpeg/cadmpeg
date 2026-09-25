// SPDX-License-Identifier: Apache-2.0
//! Typed IGES entity accessors and neutral projection.

use std::collections::BTreeSet;

use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;

fn directed_cycle(
    sequence: u32,
    visited: &mut BTreeSet<u32>,
    successors: impl Fn(u32) -> Vec<u32>,
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
        for target in successors(current).into_iter().rev() {
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

fn presentation_loss(entry: &DirectoryEntry, message: impl Into<String>) -> LossNote {
    IgesLossCode::DisplayDataNotProjected
        .note(format!(
            "IGES entity type {} form {} display data was not projected: {}",
            entry.entity_type,
            entry.form,
            message.into()
        ))
        .with_provenance(entry.loss_provenance())
}

pub(crate) fn line_directrix(ir: &CadIr, curve_id: &CurveId) -> bool {
    // `cadmpeg_ir::geometry::PlacedCurve::try_new` bounds the chain, so the
    // walk needs no depth of its own.
    fn is_line(geometry: &SolvedCurveGeometry) -> bool {
        match geometry {
            SolvedCurveGeometry::Line(_) => true,
            SolvedCurveGeometry::Transformed(placed) => is_line(placed.basis()),
            _ => false,
        }
    }

    ir.model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .is_some_and(|curve| curve.geometry.solved().is_some_and(is_line))
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
mod offsets;
mod presentation;
mod splines;
pub(crate) mod structure;
mod surfaces;
mod trimming;

#[cfg(test)]
mod tests;
