// SPDX-License-Identifier: Apache-2.0
//! Constructive-solid primitive validation and native semantic ownership.

use super::geometry::{
    declared_orthogonal_vectors, declared_unit_vector, resolve_transform, ProjectionOutcome,
};
use super::pointer;

use crate::directory::{DirectoryEntry, UseFlag};
use crate::global::ProjectedGlobal;
use crate::parameter::ParameterRecord;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::CadIr;
use std::collections::{BTreeMap, BTreeSet};

fn vector_or(record: &ParameterRecord, start: usize, default: Vector3) -> Option<Vector3> {
    Some(Vector3::new(
        record.number_or(start, default.x)?,
        record.number_or(start + 1, default.y)?,
        record.number_or(start + 2, default.z)?,
    ))
}

fn profile_closed(
    index: &ModelIndex<'_>,
    edges: &[&Edge],
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<bool>, CodecError> {
    let point = |vertex: &cadmpeg_ir::ids::VertexId| -> Result<Option<cadmpeg_ir::math::Point3>, CodecError> {
        let Some(vertex) = index.vertices(vertex.as_str(), ctx)? else { return Ok(None) };
        Ok(index.points(vertex.point.as_str(), ctx)?.map(|point| point.position().get()))
    };
    let mut result = None;
    let mut edges = edges.iter();
    while let Some(edge) = ctx.next_charged(&mut edges, "iges solid profile edges")? {
        let Some(start) = point(&edge.start)? else {
            return Ok(None);
        };
        let Some(end) = point(&edge.end)? else {
            return Ok(None);
        };
        let closed = cadmpeg_ir::math::Point3::distance(start, end) <= tolerance;
        if result.is_some_and(|previous| previous != closed) {
            return Ok(None);
        }
        result = Some(closed);
    }
    Ok(result)
}

#[derive(Clone, Copy)]
enum BooleanTerm {
    Operand(u32),
    Operation,
}

struct BooleanValidation<'ctx> {
    path: BTreeSet<u32>,
    memo: BTreeMap<u32, bool>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn boolean_tree_is_valid(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    boolean_definitions: &BTreeMap<u32, Vec<BooleanTerm>>,
    validation: &mut BooleanValidation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let _depth = ctx.enter_nested("iges boolean tree validation")?;
    if let Some(valid) = validation.memo.get(&sequence) {
        return Ok(*valid);
    }
    let mut path_storage = ctx.reserve_scoped(0, "iges Boolean path frame")?;
    if !path_storage.with_storage(|| {
        ctx.insert_btree_set(
            &mut validation.path,
            sequence,
            "iges boolean validation path",
        )
    })? {
        return Ok(false);
    }
    let Some(entry) = entries.get(&sequence) else {
        validation.path.remove(&sequence);
        return Ok(false);
    };
    let Some(terms) = boolean_definitions.get(&sequence) else {
        validation.path.remove(&sequence);
        return Ok(false);
    };
    let mut has_direct_brep = false;
    let mut operands_valid = true;
    let mut terms = terms.iter();
    while let Some(term) = ctx.next_charged(&mut terms, "iges boolean term validation")? {
        has_direct_brep |= matches!(term, BooleanTerm::Operand(target) if entries.get(target).is_some_and(|target| target.entity_type == 186));
        let valid = match term {
            BooleanTerm::Operation => true,
            BooleanTerm::Operand(target_sequence) => match entries.get(target_sequence) {
                Some(target)
                    if matches!(
                        target.entity_type,
                        150 | 152 | 154 | 156 | 158 | 160 | 162 | 164 | 168 | 430
                    ) =>
                {
                    true
                }
                Some(target) if target.entity_type == 180 => boolean_tree_is_valid(
                    *target_sequence,
                    entries,
                    boolean_definitions,
                    validation,
                    ctx,
                )?,
                Some(target) => entry.form == 1 && target.entity_type == 186,
                None => false,
            },
        };
        if !valid {
            operands_valid = false;
            break;
        }
    }
    let valid = operands_valid && has_direct_brep == (entry.form == 1);
    validation.path.remove(&sequence);
    validation.storage.with_storage(|| {
        ctx.insert_btree_map(
            &mut validation.memo,
            sequence,
            valid,
            "iges boolean validity memo",
        )
    })?;
    Ok(valid)
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    global: &ProjectedGlobal,
    ctx: &DecodeContext<'_>,
) -> Result<ProjectionOutcome, CodecError> {
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();

    for entry in ctx
        .admit_iter(directory, "iges csg directory traversal")?
        .filter(|entry| {
            matches!(entry.entity_type, 150 | 152 | 154 | 156 | 158 | 160 | 168) && entry.form == 0
        })
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let factor = global.length_factor_mm();
        if let Err(error) = resolve_transform(
            entry.transform,
            entries,
            records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            error.non_resource()?;
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "primitive placement is invalid"),
            )?;
            continue;
        }
        let dimension_count = match entry.entity_type {
            150 | 156 | 168 => 3,
            152 => 4,
            154 | 160 => 2,
            158 => 1,
            _ => 0,
        };
        let mut dimensions = [0.0; 4];
        let mut dimensions_present = true;
        for (index, dimension) in dimensions.iter_mut().enumerate().take(dimension_count) {
            let value = if entry.entity_type == 156 && index == 2 {
                record.number_or(index + 1, 0.0)
            } else {
                record.number(index + 1)
            };
            if let Some(value) = value {
                *dimension = value;
            } else {
                dimensions_present = false;
                break;
            }
        }
        if !dimensions_present {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "primitive dimensions are not numeric"),
            )?;
            continue;
        }
        let dimensions = &dimensions[..dimension_count];
        let dimensions_valid = match entry.entity_type {
            150 => dimensions
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
            152 => {
                dimensions[..3]
                    .iter()
                    .all(|value| value.is_finite() && *value > 0.0)
                    && dimensions[3].is_finite()
                    && dimensions[3] >= 0.0
                    && dimensions[3] < dimensions[0]
            }
            154 | 158 => dimensions
                .iter()
                .all(|value| value.is_finite() && *value > 0.0),
            156 => {
                dimensions[0] > 0.0
                    && dimensions[1] > dimensions[2]
                    && dimensions[2] >= 0.0
                    && dimensions.iter().all(|value| value.is_finite())
            }
            160 => {
                dimensions[0] > dimensions[1]
                    && dimensions[1] > 0.0
                    && dimensions.iter().all(|value| value.is_finite())
            }
            168 => {
                dimensions[0] >= dimensions[1]
                    && dimensions[1] >= dimensions[2]
                    && dimensions[2] > 0.0
                    && dimensions.iter().all(|value| value.is_finite())
            }
            _ => false,
        };
        if !dimensions_valid {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "primitive dimension invariant is violated"),
            )?;
            continue;
        }
        let (origin_start, x_axis_start, z_axis_start) = match entry.entity_type {
            150 => (4, Some(7), Some(10)),
            152 => (5, Some(8), Some(11)),
            154 => (3, None, Some(6)),
            156 => (4, None, Some(7)),
            158 => (2, None, None),
            160 => (3, None, Some(6)),
            168 => (4, Some(7), Some(10)),
            _ => {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "primitive solid type is unsupported"),
                )?;
                continue;
            }
        };
        let Some(origin) = vector_or(record, origin_start, Vector3::new(0.0, 0.0, 0.0)) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "primitive origin is invalid"),
            )?;
            continue;
        };
        if !origin.is_finite() {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "primitive origin is non-finite"),
            )?;
            continue;
        }
        let x_axis =
            x_axis_start.and_then(|start| vector_or(record, start, Vector3::new(1.0, 0.0, 0.0)));
        let z_axis =
            z_axis_start.and_then(|start| vector_or(record, start, Vector3::new(0.0, 0.0, 1.0)));
        let precision = global.real_precision();
        if x_axis_start.is_some() != x_axis.is_some()
            || z_axis_start.is_some() != z_axis.is_some()
            || x_axis_start.zip(x_axis).is_some_and(|(start, axis)| {
                declared_unit_vector(record, start, axis, precision).is_none()
            })
            || z_axis_start.zip(z_axis).is_some_and(|(start, axis)| {
                declared_unit_vector(record, start, axis, precision).is_none()
            })
            || x_axis_start
                .zip(x_axis)
                .zip(z_axis_start.zip(z_axis))
                .is_some_and(|((x_start, x_axis), (z_start, z_axis))| {
                    !declared_orthogonal_vectors(
                        record, x_start, x_axis, z_start, z_axis, precision,
                    )
                })
        {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "primitive axes are not orthonormal"),
            )?;
            continue;
        }
        ctx.insert_btree_set(&mut decoded, entry.sequence, "iges csg decoded sequences")?;
    }

    let mut profile_index = None;
    let mut profile_edges = None;
    for entry in ctx
        .admit_iter(directory, "iges csg directory traversal")?
        .filter(|entry| {
            (entry.entity_type == 162 && matches!(entry.form, 0 | 1))
                || (entry.entity_type == 164 && entry.form == 0)
        })
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let factor = global.length_factor_mm();
        let Some(profile) = pointer(record, 1) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid profile curve pointer is invalid"),
            )?;
            continue;
        };
        let mut profile_storage = [0_u8; 64];
        let profile_id = crate::ids::directory_lookup_key(
            "iges:model:curve#D",
            profile,
            &mut profile_storage,
            ctx,
        )?;
        let profile = match profile_id {
            Some(profile_id) => {
                let index = match &mut profile_index {
                    Some(index) => index,
                    slot @ None => {
                        slot.insert(ModelIndex::new_model_only(ir, ctx).map_err(CodecError::from)?)
                    }
                };
                if index.curves(profile_id, ctx)?.is_some() {
                    Some((profile_id, index))
                } else {
                    None
                }
            }
            None => None,
        };
        let Some((profile_id, index)) = profile else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid profile curve pointer is invalid"),
            )?;
            continue;
        };
        let Some(amount) = record
            .number_or(2, 1.0)
            .filter(|value| value.is_finite() && *value > 0.0)
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid sweep amount is invalid"),
            )?;
            continue;
        };
        if entry.entity_type == 162 && amount > 1.0 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid revolution fraction is greater than one"),
            )?;
            continue;
        }
        let (origin, direction_start) = if entry.entity_type == 162 {
            (vector_or(record, 3, Vector3::new(0.0, 0.0, 0.0)), 6)
        } else {
            (Some(Vector3::new(0.0, 0.0, 0.0)), 3)
        };
        let direction = vector_or(record, direction_start, Vector3::new(0.0, 0.0, 1.0));
        if origin.is_none_or(|origin| !origin.is_finite())
            || direction.is_none_or(|direction| {
                declared_unit_vector(record, direction_start, direction, global.real_precision())
                    .is_none()
            })
        {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid sweep axis is invalid"),
            )?;
            continue;
        }
        let groups = match &mut profile_edges {
            Some(groups) => groups,
            slot @ None => slot.insert(
                ctx.collect_scoped_btree_groups(
                    ctx.admit_iter(&ir.model.edges, "iges solid profile edge indexing")?
                        .filter_map(|edge| edge.curve().map(|curve| (curve.as_str(), edge))),
                    "iges solid profile edge groups",
                )?,
            ),
        };
        let edges = ctx
            .get_btree_map(&groups.0, &profile_id, "iges solid profile edge lookup")?
            .map_or(&[][..], Vec::as_slice);
        let Some(closed) = profile_closed(index, edges, global.minimum_resolution_mm(), ctx)?
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid profile endpoints are unavailable"),
            )?;
            continue;
        };
        if (entry.entity_type == 162 && entry.form == 0 && closed)
            || (entry.entity_type == 164 && !closed)
        {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid sweep form disagrees with profile closure"),
            )?;
            continue;
        }
        if let Err(error) = resolve_transform(
            entry.transform,
            entries,
            records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            error.non_resource()?;
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "solid sweep placement is invalid"),
            )?;
            continue;
        }
        ctx.insert_btree_set(&mut decoded, entry.sequence, "iges csg decoded sequences")?;
    }

    drop(profile_edges);
    drop(profile_index);

    let mut boolean_storage = ctx.reserve_scoped(0, "iges Boolean scratch")?;
    let mut boolean_definitions = BTreeMap::new();
    for entry in ctx
        .admit_iter(directory, "iges csg directory traversal")?
        .filter(|entry| entry.entity_type == 180 && matches!(entry.form, 0 | 1))
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(count) = record.count(1).filter(|count| *count > 2) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Boolean postfix length is not greater than two"),
            )?;
            continue;
        };
        let mut terms = boolean_storage
            .with_storage(|| ctx.collection_vec(count, "iges Boolean postfix terms"))?;
        let mut terms_valid = true;
        let mut indices = 0..count;
        while let Some(index) = ctx.next_charged(&mut indices, "iges Boolean postfix parsing")? {
            let term = (|| {
                let value = record.integer(2 + index)?;
                if value < 0 {
                    let sequence = u32::try_from(value.checked_neg()?).ok()?;
                    (sequence % 2 == 1).then_some(BooleanTerm::Operand(sequence))
                } else if matches!(value, 1..=3) {
                    Some(BooleanTerm::Operation)
                } else {
                    None
                }
            })();
            if let Some(term) = term {
                terms.push(term);
            } else {
                terms_valid = false;
                break;
            }
        }
        if !terms_valid {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Boolean postfix term is invalid"),
            )?;
            continue;
        }
        let mut depth = 0_usize;
        let valid_stack = ctx.all_by(
            &terms,
            |term| {
                Ok(match term {
                    BooleanTerm::Operand(_) => {
                        depth += 1;
                        true
                    }
                    BooleanTerm::Operation if depth >= 2 => {
                        depth -= 1;
                        true
                    }
                    BooleanTerm::Operation => false,
                })
            },
            "iges Boolean postfix stack",
        )?;
        if !valid_stack || depth != 1 {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Boolean postfix stack is unbalanced"),
            )?;
            continue;
        }
        boolean_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut boolean_definitions,
                entry.sequence,
                terms,
                "iges Boolean definition nodes",
            )
        })?;
    }
    let mut validation = BooleanValidation {
        path: BTreeSet::new(),
        memo: BTreeMap::new(),
        storage: ctx.reserve_scoped(0, "iges Boolean validation scratch")?,
    };
    for (sequence, _) in
        ctx.admit_iter(&boolean_definitions, "iges Boolean definition validation")?
    {
        let entry = entries[sequence];
        let operands_valid = boolean_tree_is_valid(
            *sequence,
            entries,
            &boolean_definitions,
            &mut validation,
            ctx,
        )?;
        if !operands_valid {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "Boolean operands, form, or reference acyclicity is invalid"
                ),
            )?;
            continue;
        }
        let factor = global.length_factor_mm();
        if let Err(error) = resolve_transform(
            entry.transform,
            entries,
            records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            error.non_resource()?;
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Boolean result placement is invalid"),
            )?;
            continue;
        }
        ctx.insert_btree_set(&mut decoded, *sequence, "iges csg decoded sequences")?;
    }

    for entry in ctx
        .admit_iter(directory, "iges csg directory traversal")?
        .filter(|entry| entry.entity_type == 182 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(_tree) = pointer(record, 1).filter(|sequence| {
            decoded.contains(sequence)
                && entries
                    .get(sequence)
                    .is_some_and(|target| target.entity_type == 180)
        }) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "selected-component Boolean tree pointer is invalid"),
            )?;
            continue;
        };
        let point_valid = (2..=4).all(|index| record.number(index).is_some());
        if !point_valid || entry.status.use_flag(global.global_table()) != Some(UseFlag::Other) {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "selected-component point or entity-use flag is invalid"
                ),
            )?;
            continue;
        }
        let factor = global.length_factor_mm();
        if let Err(error) = resolve_transform(
            entry.transform,
            entries,
            records,
            factor,
            global.real_precision(),
            &mut BTreeSet::new(),
            ctx,
        ) {
            error.non_resource()?;
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "selected-component placement is invalid"),
            )?;
            continue;
        }
        ctx.insert_btree_set(&mut decoded, entry.sequence, "iges csg decoded sequences")?;
    }

    Ok(ProjectionOutcome { decoded, losses })
}

#[cfg(test)]
mod tests;
