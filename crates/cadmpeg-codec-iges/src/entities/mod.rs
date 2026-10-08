// SPDX-License-Identifier: Apache-2.0
//! Typed IGES entity accessors and neutral projection.

use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::CurveId;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;

use crate::directory::DirectoryEntry;
use crate::loss::IgesLossCode;
use crate::parameter::ParameterRecord;

/// Equality identity for property text within one projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PropertyTextId(usize);

/// Keys borrow immutable parameter text. Equal byte strings have one identity.
/// Record identities prevent repeated comparisons of the same parameter text.
struct PropertyTextIndex<'text, 'budget> {
    records: BTreeMap<u32, PropertyTextId>,
    values: BTreeMap<&'text [u8], PropertyTextId>,
    storage: ScopedReservation<'budget>,
}

impl<'text, 'budget> PropertyTextIndex<'text, 'budget> {
    fn new(ctx: &'budget DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            records: BTreeMap::new(),
            values: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "iges property text index")?,
        })
    }

    fn id(
        &mut self,
        sequence: u32,
        text: &'text [u8],
        ctx: &DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<PropertyTextId, CodecError> {
        if let Some(id) = ctx.get_btree_map(
            &self.records,
            &sequence,
            "iges property text record lookup",
        )? {
            return Ok(*id);
        }
        let id = if let Some(id) = ctx.get_btree_map(&self.values, text, operation)? {
            *id
        } else {
            let id = PropertyTextId(self.values.len());
            self.storage
                .with_storage(|| ctx.insert_btree_map(&mut self.values, text, id, operation))?;
            id
        };
        self.storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut self.records,
                sequence,
                id,
                "iges property text record identities",
            )
        })?;
        Ok(id)
    }
}

fn push_attributed_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    code: IgesLossCode,
    message: fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    ctx.reserve_vec(losses, 1, "iges entity loss slots")?;
    let message = ctx.format_retained(message, "iges entity loss message")?;
    ctx.charge_retained(
        4 + cadmpeg_core::decode::u64_from_index(code.code().len()),
        "iges entity loss kind",
    )?;
    losses.push(
        code.note(message)
            .with_provenance(entry.admitted_loss_provenance(ctx)?),
    );
    Ok(())
}

fn push_entity_loss(
    ctx: &DecodeContext<'_>,
    losses: &mut Vec<LossNote>,
    entry: &DirectoryEntry,
    reason: fmt::Arguments<'_>,
) -> Result<(), CodecError> {
    push_attributed_loss(
        ctx,
        losses,
        entry,
        IgesLossCode::EntityNotProjected,
        format_args!(
            "IGES entity type {} form {} was not projected: {reason}",
            entry.entity_type, entry.form
        ),
    )
}

fn non_resource_error(error: CodecError, ctx: &DecodeContext<'_>) -> Result<String, CodecError> {
    match error {
        CodecError::ResourceLimit(_) => Err(error),
        other => ctx.format_retained(format_args!("{other}"), "iges diagnostic error text"),
    }
}

fn directed_cycle<I: DoubleEndedIterator<Item = u32>>(
    sequence: u32,
    visited: &mut BTreeSet<u32>,
    ctx: &DecodeContext<'_>,
    successors: impl Fn(u32) -> Result<I, CodecError>,
) -> Result<bool, CodecError> {
    if ctx.contains_btree_set(visited, &sequence, "iges cycle visited lookup")? {
        return Ok(false);
    }
    let mut search_storage = ctx.reserve_scoped(0, "IGES directed cycle search")?;
    let mut active = BTreeSet::new();
    let mut stack = Vec::new();
    search_storage.with_storage(|| ctx.reserve_vec(&mut stack, 1, "iges cycle stack"))?;
    stack.push((sequence, false));
    while let Some((current, expanded)) =
        ctx.next_charged(&mut std::iter::from_fn(|| stack.pop()), "iges cycle work")?
    {
        if expanded {
            ctx.remove_btree_set(&mut active, &current, "iges cycle active removal")?;
            ctx.insert_btree_set(visited, current, "iges cycle visited")?;
            continue;
        }
        if ctx.contains_btree_set(visited, &current, "iges cycle visited lookup")? {
            continue;
        }
        if !search_storage
            .with_storage(|| ctx.insert_btree_set(&mut active, current, "iges cycle active"))?
        {
            return Ok(true);
        }
        search_storage.with_storage(|| ctx.reserve_vec(&mut stack, 1, "iges cycle stack"))?;
        stack.push((current, true));
        let mut targets = successors(current)?.rev();
        while let Some(target) = ctx.next_charged(&mut targets, "iges cycle work")? {
            if ctx.contains_btree_set(&active, &target, "iges cycle active lookup")? {
                return Ok(true);
            }
            if !ctx.contains_btree_set(visited, &target, "iges cycle visited lookup")? {
                search_storage
                    .with_storage(|| ctx.reserve_vec(&mut stack, 1, "iges cycle stack"))?;
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
