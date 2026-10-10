// SPDX-License-Identifier: Apache-2.0
//! Source construction ownership survives failure of a neutral owner projection.

use crate::{directory::DirectoryEntry, parameter::ParameterRecord};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{CadIr, SourceGeometryRole};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn mark_supports(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let mut storage = ctx.reserve_scoped(0, "IGES source support ownership")?;
    let mut records = BTreeMap::new();
    for record in parameters {
        storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut records,
                record.directory_sequence,
                record,
                "IGES support parameter index",
            )
        })?;
    }
    let mut supports = BTreeSet::new();
    for owner in directory {
        ctx.charge_work(1, "IGES source support owners")?;
        let Some(record) = records.get(&owner.sequence) else {
            continue;
        };
        visit_support_fields(owner, record, &mut |index| {
            ctx.charge_work(1, "IGES declared support pointer")?;
            if let Some(target) = record
                .integer(index)
                .and_then(|value| u32::try_from(value).ok())
                .filter(|value| *value > 0 && value % 2 == 1)
            {
                storage.with_storage(|| {
                    ctx.insert_btree_set(&mut supports, target, "IGES support identity index")
                })?;
            }
            Ok(())
        })?;
    }
    for source in ir
        .model
        .surfaces
        .iter_mut()
        .filter_map(|surface| surface.source_object.as_mut())
        .chain(
            ir.model
                .curves
                .iter_mut()
                .filter_map(|curve| curve.source_object.as_mut()),
        )
        .chain(
            ir.model
                .points
                .iter_mut()
                .filter_map(|point| point.source_object.as_mut()),
        )
    {
        ctx.charge_work(1, "IGES support association")?;
        if source
            .object_id
            .as_str()
            .strip_prefix('D')
            .and_then(|value| value.parse::<u32>().ok())
            .is_some_and(|sequence| supports.contains(&sequence))
        {
            source.geometry_role = Some(SourceGeometryRole::Support);
        }
    }
    Ok(())
}

/// Read only entity-defined geometry pointer slots, including a readable prefix
/// of a failed construction. Groups and presentation do not establish ownership.
fn visit_support_fields(
    owner: &DirectoryEntry,
    record: &ParameterRecord,
    visit: &mut impl FnMut(usize) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let fixed: &[usize] = match (owner.entity_type, owner.form) {
        (108, -1..=1) => &[5],
        (118, 0 | 1) | (120, 0) => &[1, 2],
        (122 | 162 | 164 | 144, 0) | (162 | 510, 1) => &[1],
        (130, 0) if record.integer(2) == Some(3) => &[1, 3],
        (130, 0) => &[1],
        (140, 0) => &[5],
        (141, 0) => &[3],
        (142, 0) => &[2, 3, 4],
        (143, 0) => &[2],
        (190 | 192 | 194 | 196 | 198, 0 | 1) => &[1],
        _ => &[],
    };
    for index in fixed {
        visit(*index)?;
    }
    match (owner.entity_type, owner.form) {
        (102, 0) => visit_fixed_items(record, 1, 2, 1, visit),
        (504, 1) => visit_fixed_items(record, 1, 2, 5, visit),
        (141, 0) => visit_variable_items(record, 4, 5, 3, 1, 0, visit),
        (508, 1) => visit_variable_items(record, 1, 2, 5, 2, 1, visit),
        _ => Ok(()),
    }
}

// Ownership needs a readable declaration, not proof that the whole owner fits.
// Every caller caps iteration against the physically present primary span.
fn declared_count(record: &ParameterRecord, index: usize) -> Option<usize> {
    record
        .integer(index)
        .and_then(|value| usize::try_from(value).ok())
}

fn visit_fixed_items(
    record: &ParameterRecord,
    count_index: usize,
    start: usize,
    stride: usize,
    visit: &mut impl FnMut(usize) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let Some(available) = record.parameter_end().checked_sub(start) else {
        return Ok(());
    };
    let count = declared_count(record, count_index)
        .unwrap_or(0)
        .min(available.div_ceil(stride));
    for item in 0..count {
        visit(start + item * stride)?;
    }
    Ok(())
}

fn visit_variable_items(
    record: &ParameterRecord,
    count_index: usize,
    mut start: usize,
    prefix: usize,
    pcurve_stride: usize,
    pointer_offset: usize,
    visit: &mut impl FnMut(usize) -> Result<(), CodecError>,
) -> Result<(), CodecError> {
    let count = declared_count(record, count_index)
        .unwrap_or(0)
        .min(record.parameter_end() / prefix);
    for _ in 0..count {
        let Some(available) = record.parameter_end().checked_sub(start) else {
            break;
        };
        if available < prefix {
            break;
        }
        visit(start + pointer_offset)?;
        let Some(pcurves) = declared_count(record, start + prefix - 1) else {
            break;
        };
        start += prefix;
        let available = (available - prefix) / pcurve_stride;
        for item in 0..pcurves.min(available) {
            visit(start + item * pcurve_stride + pcurve_stride - 1)?;
        }
        if pcurves > available {
            break;
        }
        start += pcurves * pcurve_stride;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
