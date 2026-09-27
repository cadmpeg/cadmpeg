// SPDX-License-Identifier: Apache-2.0
//! Filled, knit, draft, thicken, and result-topology feature recipes.

use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::{exactly_one, unique_feature_profile_definition};
use super::axes::model_feature_ids;
use super::dependencies::{
    surface_merge_quilt_ids, surface_merge_quilt_state_offset,
};
use super::outputs::CommaList;
use super::round::unique_positive_length;
use super::selections::feature_result_edge_ids;
use crate::container::ContainerScan;
use crate::decode::analytic::equations::PlaneEquation;
use crate::vecmath::dot;
use crate::vecmath::normalize;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    EdgeSelection, FaceSelection, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, FeatureOperation as IrFeatureOperation, FeatureResultTopology,
    GeneratedFaceRef, PathRef, SurfaceBoundary, SurfaceContinuity, ThickenSide,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_core::text::NonBlankString;
use std::collections::{BTreeMap, BTreeSet};

const EPS_NORMAL_ALIGNMENT: f64 = 1.0e-9;
const EPS_OFFSET_AGREEMENT: f64 = 1.0e-9;

pub(in super::super) fn filled_surface_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> IrFeatureDefinition {
    let boundary = unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )
    .and_then(|definition| model_sketch_id(scan, definition))
    .filter(|sketch| {
        ir.model
            .sketches
            .iter()
            .any(|candidate| candidate.id == *sketch)
    })
    .map_or(
        SurfaceBoundary::Edges(EdgeSelection::Unresolved),
        |sketch| SurfaceBoundary::Path(PathRef::Sketch(sketch)),
    );
    IrFeatureDefinition::Operation(IrFeatureOperation::FilledSurface {
        boundary,
        support_faces: FaceSelection::Faces(Vec::new()),
        continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
            SurfaceContinuity::Contact,
        ),
        merge_result: Some(false),
    })
}

pub(in super::super) fn knit_class_100_operand_entity_ids(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    for (table_index, table) in tables.iter().enumerate() {
        if table.feature_id != feature_id || table.table_class_id != 100 {
            continue;
        }
        for (entry_index, entry) in table.entries.iter().enumerate() {
            if seen.contains(&entry.entity_id) {
                return Ok(None);
            }
            ctx.charge_collection_items(1, "creo knit consumer identity nodes")?;
            seen.insert(entry.entity_id);
            let consumer_position = (table.offset, entry.offset, table_index, entry_index);
            let mut producer = None;
            for (source_index, source_table) in tables.iter().enumerate() {
                if source_table.feature_id == feature_id {
                    continue;
                }
                for (source_entry_index, source_entry) in source_table.entries.iter().enumerate() {
                    let source_position = (
                        source_table.offset,
                        source_entry.offset,
                        source_index,
                        source_entry_index,
                    );
                    if source_position < consumer_position
                        && source_entry.class_id() == 200
                        && source_entry.entity_id == entry.entity_id
                    {
                        if producer.is_some() {
                            return Ok(None);
                        }
                        producer = Some(source_table.feature_id);
                    }
                }
            }
            if producer.is_none() {
                return Ok(None);
            }
            ctx.try_reserve_items(&mut ids, 1, "creo knit class 100 operand IDs")?;
            ids.push(entry.entity_id);
        }
    }
    Ok((!ids.is_empty()).then_some(ids))
}

fn knit_operand_entity_ids(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<Option<(Vec<u32>, &'static str)>, CodecError> {
    if let Some(ids) = surface_merge_quilt_ids(
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
    ) {
        let mut copied = Vec::new();
        let mut seen = BTreeSet::new();
        for &id in ids {
            if seen.contains(&id) {
                return Ok(None);
            }
            ctx.charge_collection_items(1, "creo knit quilt identity nodes")?;
            seen.insert(id);
            ctx.try_reserve_items(&mut copied, 1, "creo knit quilt IDs")?;
            copied.push(id);
        }
        return Ok(Some((copied, "surface_merge_quilts")));
    }
    Ok(knit_class_100_operand_entity_ids(ctx, feature_id, &scan.features.entity_tables)?
        .map(|ids| (ids, "surface_merge_entities")))
}

pub(in super::super) fn knit_operand_surface_ids(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
    quilt_ids: &[u32],
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(consumer_offset) = surface_merge_quilt_state_offset(
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
        quilt_ids,
    ) else {
        return Ok(None);
    };
    let mut surface_ids = Vec::new();
    let mut seen = BTreeSet::new();
    for quilt_id in quilt_ids {
        let mut producer = None;
        for table in &scan.features.entity_tables {
            for entry in &table.entries {
                if entry.class_id() == 200
                    && entry.entity_id == *quilt_id
                    && entry.offset < consumer_offset
                {
                    if producer.is_some() {
                        return Ok(None);
                    }
                    producer = Some(table.feature_id);
                }
            }
        }
        let Some(producer) = producer.filter(|producer| *producer != feature_id) else {
            return Ok(None);
        };
        let mut surface_id = None;
        for table in &scan.features.entity_tables {
            if table.feature_id != producer
                || table.table_class_id != 100
                || table.offset >= consumer_offset
            {
                continue;
            }
            for entry in &table.entries {
                if entry.entity_id == *quilt_id && entry.offset < consumer_offset {
                    if surface_id.is_some() {
                        return Ok(None);
                    }
                    surface_id = Some(entry.class_id());
                }
            }
        }
        let Some(surface_id) = surface_id else {
            return Ok(None);
        };
        let Some(surface) = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id) else {
            return Ok(None);
        };
        if surface.feature_id != producer || seen.contains(&surface_id) {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "creo knit surface identity nodes")?;
        seen.insert(surface_id);
        ctx.try_reserve_items(&mut surface_ids, 1, "creo knit surface IDs")?;
        surface_ids.push(surface_id);
    }
    Ok(Some(surface_ids))
}

pub(super) fn knit_surface_feature_definition(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<IrFeatureDefinition, CodecError> {
    let faces = if let Some((quilt_ids, namespace)) = knit_operand_entity_ids(ctx, scan, feature_id)? {
            let native = ctx.format_retained(
                format_args!("creo:allfeatur:{namespace}#{feature_id}:{}", CommaList(&quilt_ids)),
                "creo knit native selection",
            )?;
            let available_features = model_feature_ids(ctx, scan)?;
            let result_surface_ids = feature_result_surface_ids_by_feature(
                ctx,
                &scan.features.entity_tables,
                &scan.surfaces.rows,
            )?;
            let generated = match knit_operand_surface_ids(ctx, scan, feature_id, &quilt_ids)? {
                Some(surface_ids) => generated_surface_face_refs(
                        ctx,
                        &surface_ids,
                        &scan.surfaces.rows,
                        &result_surface_ids,
                        &available_features,
                    )?,
                None => None,
            };
            match generated {
                Some(faces) => FaceSelection::generated(
                    faces,
                    ctx.copy_retained_text(&native, "creo knit generated native selection")?,
                )
                    .unwrap_or(FaceSelection::Native(native)),
                None => FaceSelection::Native(native),
            }
    } else {
        FaceSelection::Unresolved
    };
    Ok(IrFeatureDefinition::Operation(IrFeatureOperation::KnitSurface {
        faces,
        merge_entities: Some(true),
        create_solid: Some(false),
        gap_tolerance: None,
    }))
}

/// Select the neutral plane carried by a Draft feature's class-209 entity.
///
/// The class is a neutral-plane carrier only when it has one unambiguous
/// feature-owned surface row and that row is a plane. The table class is not
/// part of the rule: Draft records use more than one enclosing table class.
pub(in super::super) fn draft_neutral_plane_selection(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<FaceSelection, CodecError> {
    let Some((table, entry)) = exactly_one(
        scan.features
            .entity_tables
            .iter()
            .filter(|table| table.feature_id == feature_id)
            .flat_map(|table| {
                table
                    .entries
                    .iter()
                    .filter(|entry| entry.class_id() == 209)
                    .map(move |entry| (table, entry))
            }),
    ) else {
        return Ok(FaceSelection::Unresolved);
    };
    if table
        .surface_ids_iter()
        .filter(|surface_id| *surface_id == entry.entity_id)
        .count()
        != 1
    {
        return Ok(FaceSelection::Unresolved);
    }
    let Some(surface) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
        .filter(|surface| {
            surface.feature_id == feature_id && surface.kind == crate::surface::SurfaceKind::Plane
        })
    else {
        return Ok(FaceSelection::Unresolved);
    };
    Ok(FaceSelection::Native(ctx.format_retained(
        format_args!("creo:visibgeom:surface#{}", surface.id),
        "creo draft neutral plane native",
    )?))
}

pub(in super::super) fn feature_surface_transitions(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Result<Option<Vec<(u32, u32)>>, CodecError> {
    let outputs = tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .flat_map(|table| table.entries.iter())
        .filter(|entry| entry.class_id() == 210)
        .count();
    if outputs == 0 {
        return Ok(None);
    }
    let predecessors = tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .flat_map(|table| table.entries.iter())
        .filter(|entry| entry.class_id() == 214 && entry.related_entity_id().is_some())
        .count();
    if predecessors != outputs {
        return Ok(None);
    }

    let mut output_ids = BTreeSet::new();
    let mut intermediate_ids = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    let mut transitions = Vec::new();
    for (output_table, output) in tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .flat_map(|table| {
            table
                .entries
                .iter()
                .filter(|entry| entry.class_id() == 210)
                .map(move |entry| (table, entry))
        })
    {
        let Some(intermediate_id) = output.related_entity_id() else {
            return Ok(None);
        };
        if output.related_entity_state() != Some(0)
            || output_table
                .surface_ids_iter()
                .filter(|surface_id| *surface_id == output.entity_id)
                .count()
                != 1
            || crate::surface::unique_surface_row(surface_rows, output.entity_id)
                .is_none_or(|row| row.feature_id != feature_id)
            || output_ids.contains(&output.entity_id)
            || intermediate_ids.contains(&intermediate_id)
        {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "creo transition output identity nodes")?;
        output_ids.insert(output.entity_id);
        ctx.charge_collection_items(1, "creo transition intermediate identity nodes")?;
        intermediate_ids.insert(intermediate_id);
        let mut matches = output_table.entries.iter().filter(|predecessor| {
            predecessor.class_id() == 214
                && predecessor.entity_id == intermediate_id
                && predecessor.related_entity_state() == Some(0)
                && output_table.contains_non_surface_entity_id(predecessor.entity_id)
                && crate::surface::unique_surface_row(surface_rows, predecessor.entity_id).is_none()
        });
        let Some(predecessor) = matches.next() else {
            return Ok(None);
        };
        if matches.next().is_some() {
            return Ok(None);
        }
        let Some(source_id) = predecessor.related_entity_id() else {
            return Ok(None);
        };
        if crate::surface::unique_surface_row(surface_rows, source_id)
            .is_none_or(|row| row.feature_id == feature_id)
            || source_ids.contains(&source_id)
        {
            return Ok(None);
        }
        ctx.charge_collection_items(1, "creo transition source identity nodes")?;
        source_ids.insert(source_id);
        ctx.try_reserve_items(&mut transitions, 1, "creo surface transitions")?;
        transitions.push((source_id, output.entity_id));
    }
    Ok(output_ids.is_disjoint(&source_ids).then_some(transitions))
}

pub(in super::super) fn surface_transition_dependencies(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Result<Vec<u32>, CodecError> {
    let mut dependencies = Vec::new();
    for (source_id, _) in feature_surface_transitions(ctx, feature_id, tables, surface_rows)?
        .unwrap_or_default()
    {
        let Some(row) = crate::surface::unique_surface_row(surface_rows, source_id) else {
            continue;
        };
        if !dependencies.contains(&row.feature_id) {
            ctx.try_reserve_items(&mut dependencies, 1, "creo transition dependencies")?;
            dependencies.push(row.feature_id);
        }
    }
    Ok(dependencies)
}

pub(in super::super) fn thicken_plane_offset(
    ctx: &DecodeContext<'_>,
    transitions: &[(u32, u32)],
    planes: &BTreeMap<u32, PlaneEquation>,
    rows: &[crate::surface::SurfaceRow],
) -> Result<Option<(f64, ThickenSide)>, CodecError> {
    let mut offsets = Vec::new();
    for &(source_id, output_id) in transitions {
        let (Some(source), Some(output)) = (planes.get(&source_id), planes.get(&output_id)) else {
            continue;
        };
        let Some(offset) = (|| {
            let source_row = crate::surface::unique_surface_row(rows, source_id)?;
            let output_row = crate::surface::unique_surface_row(rows, output_id)?;
            (source_row.reversed != output_row.reversed).then_some(())?;
            let source_normal = normalize(source.normal)?.map(|component| {
                if source_row.reversed {
                    -component
                } else {
                    component
                }
            });
            let output_normal = normalize(output.normal)?;
            (dot(source_normal, output_normal).abs() >= 1.0 - EPS_NORMAL_ALIGNMENT)
                .then_some(())?;
            let displacement =
                std::array::from_fn(|index| output.origin[index] - source.origin[index]);
            Some(dot(displacement, source_normal))
        })() else {
            return Ok(None);
        };
        ctx.try_reserve_items(&mut offsets, 1, "creo thicken plane offsets")?;
        offsets.push(offset);
    }
    let mut magnitudes = Vec::new();
    ctx.try_reserve_items(&mut magnitudes, offsets.len(), "creo thicken plane magnitudes")?;
    for offset in &offsets {
        magnitudes.push(offset.abs());
    }
    let Some(magnitude) = unique_positive_length(&magnitudes).map(|value| value.get()) else {
        return Ok(None);
    };
    let tolerance = EPS_OFFSET_AGREEMENT * magnitude.max(1.0);
    let side = if offsets
        .iter()
        .all(|offset| (*offset - magnitude).abs() <= tolerance)
    {
        ThickenSide::Forward
    } else if offsets
        .iter()
        .all(|offset| (*offset + magnitude).abs() <= tolerance)
    {
        ThickenSide::Reverse
    } else {
        return Ok(None);
    };
    Ok(Some((magnitude, side)))
}

/// Return the materialized surface identities that one feature can expose as
/// faces in its regenerated result.
///
/// Every materialized surface in an owned generated-entity table is a
/// result-face identity when its surface row is unique and names the same
/// owning feature. Duplicate identifiers or malformed materialized rows
/// invalidate the complete result state for that feature.
pub(in super::super) fn feature_result_surface_ids(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut surface_ids = Vec::new();
    let mut seen = BTreeSet::new();
    for table in tables.iter().filter(|table| table.feature_id == feature_id) {
        for surface_id in table.surface_ids_iter() {
            let Some(row) = crate::surface::unique_surface_row(rows, surface_id) else {
                return Ok(None);
            };
            if row.feature_id != feature_id || seen.contains(&surface_id) {
                return Ok(None);
            }
            ctx.charge_collection_items(1, "creo feature result surface identity nodes")?;
            seen.insert(surface_id);
            ctx.try_reserve_items(&mut surface_ids, 1, "creo feature result surface IDs")?;
            surface_ids.push(surface_id);
        }
    }
    Ok((!surface_ids.is_empty()).then_some(surface_ids))
}

pub(super) fn feature_result_surface_ids_by_feature(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Result<BTreeMap<u32, Vec<u32>>, CodecError> {
    let mut unique_features = BTreeSet::new();
    let mut by_feature = BTreeMap::new();
    for table in tables {
        let feature_id = table.feature_id;
        if unique_features.contains(&feature_id) {
            continue;
        }
        ctx.charge_collection_items(1, "creo feature result feature identity nodes")?;
        unique_features.insert(feature_id);
        if let Some(surface_ids) = feature_result_surface_ids(ctx, tables, rows, feature_id)? {
            ctx.charge_collection_items(1, "creo feature result surface map nodes")?;
            by_feature.insert(feature_id, surface_ids);
        }
    }
    Ok(by_feature)
}

pub(in super::super) fn feature_result_topology(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
    curve_rows: &[crate::curve::CurveTopologyRow],
    feature_id: u32,
) -> Result<Option<FeatureResultTopology>, CodecError> {
    let mut faces = Vec::new();
    for surface_id in feature_result_surface_ids(ctx, tables, surface_rows, feature_id)?
        .unwrap_or_default() {
        let text = ctx.format_retained(
            format_args!("surface#{surface_id}"),
            "creo feature result face local IDs",
        )?;
        let id = NonBlankString::new(text)
            .ok_or_else(|| CodecError::Malformed("constructed face local ID is blank".into()))?;
        ctx.try_reserve_items(&mut faces, 1, "creo feature result face members")?;
        faces.push(id);
    }
    let mut edges = Vec::new();
    for curve_id in feature_result_edge_ids(ctx, curve_rows, feature_id)?
        .unwrap_or_default() {
        let text = ctx.format_retained(
            format_args!("curve#{curve_id}"),
            "creo feature result edge local IDs",
        )?;
        let id = NonBlankString::new(text)
            .ok_or_else(|| CodecError::Malformed("constructed edge local ID is blank".into()))?;
        ctx.try_reserve_items(&mut edges, 1, "creo feature result edge members")?;
        edges.push(id);
    }
    if faces.is_empty() && edges.is_empty() {
        return Ok(None);
    }
    for values in [&faces, &edges] {
        for (index, _) in values.iter().enumerate() {
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(index),
                "creo feature result member distinctness",
            )?;
        }
    }
    let id = FeatureResultTopologyId::mint(ctx.format_retained(
        format_args!("creo:model:feature-result-topology#{feature_id}"),
        "creo feature result topology ID",
    )?)
    .map_err(|_| CodecError::Malformed("constructed result topology ID is invalid".into()))?;
    let output_of = IrFeatureId::mint(ctx.format_retained(
        format_args!("creo:model:feature#{feature_id}"),
        "creo feature result owner ID",
    )?)
    .map_err(|_| CodecError::Malformed("constructed result owner ID is invalid".into()))?;
    Ok(FeatureResultTopology::new(
        id,
        output_of,
        Vec::new(),
        faces,
        edges,
        Vec::new(),
        None,
    )
    .ok())
}

pub(in super::super) fn generated_surface_face_refs(
    ctx: &DecodeContext<'_>,
    source_ids: &[u32],
    rows: &[crate::surface::SurfaceRow],
    result_surface_ids: &BTreeMap<u32, Vec<u32>>,
    available_features: &BTreeSet<IrFeatureId>,
) -> Result<Option<Vec<GeneratedFaceRef>>, CodecError> {
    let mut generated = Vec::new();
    for surface_id in source_ids {
        let Some(row) = crate::surface::unique_surface_row(rows, *surface_id) else {
            return Ok(None);
        };
        let feature_text = ctx.format_retained(
            format_args!("creo:model:feature#{}", row.feature_id),
            "creo generated surface feature IDs",
        )?;
        let feature = IrFeatureId::mint(feature_text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        if !available_features.contains(&feature)
            || !result_surface_ids
                .get(&row.feature_id)
                .is_some_and(|ids| ids.contains(surface_id))
        {
            return Ok(None);
        }
        let local_id = ctx.format_retained(
            format_args!("surface#{surface_id}"),
            "creo generated surface local IDs",
        )?;
        let Some(face) = GeneratedFaceRef::new(feature, local_id).ok() else {
            return Ok(None);
        };
        ctx.try_reserve_items(&mut generated, 1, "creo generated surface face references")?;
        generated.push(face);
    }
    Ok(Some(generated))
}

pub(in super::super) fn emit_feature_result_topologies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
) -> Result<usize, cadmpeg_core::CodecError> {
    let mut emitted = 0;
    for feature in &ir.model.features {
        let Some(feature_id) = feature
            .id
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        let Some(state) = feature_result_topology(
            ctx,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
            &scan.curves.topology_rows,
            feature_id,
        )? else {
            continue;
        };
        ctx.try_reserve_items(
            &mut ir.model.feature_result_topologies,
            1,
            "creo model feature result topologies",
        )?;
        ctx.charge_entities(1, "admit Creo model feature_result_topologies")?;
        ir.model.feature_result_topologies.push(state);
        emitted += 1;
    }
    Ok(emitted)
}

#[cfg(test)]
mod tests;
