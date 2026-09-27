// SPDX-License-Identifier: Apache-2.0
//! Typed IGES entity accessors and neutral projection.

use std::collections::BTreeSet;
use std::fmt;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::decode_resource::{
    format_retained, insert_optional_btree_set, reserve_optional_vec_growth, reserve_vec_growth,
};

use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;

fn push_attributed_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    code: IgesLossCode,
    message: fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    reserve_vec_growth(ctx, losses, 1, "iges entity loss slots")?;
    let message = format_retained(ctx, message, "iges entity loss message")?;
    ctx.charge_retained(4 + code.code().len() as u64, "iges entity loss kind")?;
    losses.push(code.note(message).with_provenance(entry.admitted_loss_provenance(ctx)?));
    Ok(())
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
    ctx: Option<&DecodeContext<'_>>,
    successors: impl Fn(u32) -> I,
) -> Result<bool, CodecError> {
    if visited.contains(&sequence) {
        return Ok(false);
    }
    let mut active = BTreeSet::new();
    let mut stack = Vec::new();
    reserve_optional_vec_growth(ctx, &mut stack, 1, "iges cycle stack")?;
    stack.push((sequence, false));
    while let Some((current, expanded)) = stack.pop() {
        if let Some(ctx) = ctx {
            ctx.charge_work(1, "iges cycle work")?;
        }
        if expanded {
            active.remove(&current);
            insert_optional_btree_set(ctx, visited, current, "iges cycle visited")?;
            continue;
        }
        if visited.contains(&current) {
            continue;
        }
        if !insert_optional_btree_set(ctx, &mut active, current, "iges cycle active")? {
            return Ok(true);
        }
        reserve_optional_vec_growth(ctx, &mut stack, 1, "iges cycle stack")?;
        stack.push((current, true));
        for target in successors(current).rev() {
            if let Some(ctx) = ctx {
                ctx.charge_work(1, "iges cycle work")?;
            }
            if active.contains(&target) {
                return Ok(true);
            }
            if !visited.contains(&target) {
                reserve_optional_vec_growth(ctx, &mut stack, 1, "iges cycle stack")?;
                stack.push((target, false));
            }
        }
    }
    Ok(false)
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
