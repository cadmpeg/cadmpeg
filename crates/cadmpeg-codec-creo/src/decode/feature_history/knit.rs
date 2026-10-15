// SPDX-License-Identifier: Apache-2.0
//! Filled, knit, draft, thicken, and result-topology feature recipes.

use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::{exactly_one, unique_feature_profile_definition};
use super::axes::model_feature_ids;
use super::dependencies::{surface_merge_quilt_ids, surface_merge_quilt_state_offset};
use super::outputs::CommaList;
use super::round::unique_positive_length;
use super::selections::feature_result_edge_ids_by_feature;
use crate::container::ContainerScan;
use crate::decode::analytic::equations::PlaneEquation;
use crate::vecmath::dot;
use crate::vecmath::normalize;
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{
    EdgeSelection, FaceSelection, FeatureDefinition as IrFeatureDefinition,
    FeatureId as IrFeatureId, FeatureOperation as IrFeatureOperation, FeatureResultTopology,
    GeneratedFaceRef, PathRef, SurfaceBoundary, SurfaceContinuity, ThickenSide,
};
use cadmpeg_ir::ids::FeatureResultTopologyId;
use std::collections::{BTreeMap, BTreeSet};

const EPS_NORMAL_ALIGNMENT: f64 = 1.0e-9;
const EPS_OFFSET_AGREEMENT: f64 = 1.0e-9;

pub(in super::super) fn filled_surface_feature_definition(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> IrFeatureDefinition {
    let sketch = match unique_feature_profile_definition(
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    ) {
        Some(definition) => model_sketch_id(scan, definition),
        None => None,
    };
    let boundary = sketch
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
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
) -> Option<Vec<u32>> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    for (table_index, table) in tables.iter().enumerate() {
        if table.feature_id != feature_id || table.table_class_id != 100 {
            continue;
        }
        for (entry_index, entry) in table.entries.iter().enumerate() {
            if seen.contains(&entry.entity_id) {
                return None;
            }
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
                            return None;
                        }
                        producer = Some(source_table.feature_id);
                    }
                }
            }
            producer?;
            ids.push(entry.entity_id);
        }
    }
    (!ids.is_empty()).then_some(ids)
}

fn knit_operand_entity_ids(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<(Vec<u32>, &'static str)> {
    if let Some(ids) = surface_merge_quilt_ids(
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
    ) {
        let mut copied = Vec::new();
        let mut seen = BTreeSet::new();
        for &id in ids {
            if seen.contains(&id) {
                return None;
            }
            seen.insert(id);
            copied.push(id);
        }
        return Some((copied, "surface_merge_quilts"));
    }
    knit_class_100_operand_entity_ids(feature_id, &scan.features.entity_tables)
        .map(|ids| (ids, "surface_merge_entities"))
}

pub(in super::super) fn knit_operand_surface_ids(
    scan: &ContainerScan,
    feature_id: u32,
    quilt_ids: &[u32],
) -> Option<Vec<u32>> {
    let consumer_offset = surface_merge_quilt_state_offset(
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
        quilt_ids,
    )?;
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
                        return None;
                    }
                    producer = Some(table.feature_id);
                }
            }
        }
        let producer = producer.filter(|producer| *producer != feature_id)?;
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
                        return None;
                    }
                    surface_id = Some(entry.class_id());
                }
            }
        }
        let surface_id = surface_id?;
        let surface = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)?;
        if surface.feature_id != producer || seen.contains(&surface_id) {
            return None;
        }
        seen.insert(surface_id);
        surface_ids.push(surface_id);
    }
    Some(surface_ids)
}

pub(super) fn knit_surface_feature_definition(
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<IrFeatureDefinition, CodecError> {
    let faces = if let Some((quilt_ids, namespace)) = knit_operand_entity_ids(scan, feature_id) {
        let native = format!(
            "creo:allfeatur:{namespace}#{feature_id}:{}",
            CommaList(&quilt_ids)
        );
        let available_features = model_feature_ids(scan)?;
        let result_surface_ids = feature_result_surface_ids_by_feature(
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        );
        let generated = match knit_operand_surface_ids(scan, feature_id, &quilt_ids) {
            Some(surface_ids) => generated_surface_face_refs(
                &surface_ids,
                &scan.surfaces.rows,
                &result_surface_ids,
                &available_features,
            )?,
            None => None,
        };
        match generated {
            Some(faces) => FaceSelection::generated(faces, native.clone())
                .unwrap_or(FaceSelection::Native(native)),
            None => FaceSelection::Native(native),
        }
    } else {
        FaceSelection::Unresolved
    };
    Ok(IrFeatureDefinition::Operation(
        IrFeatureOperation::KnitSurface {
            faces,
            merge_entities: Some(true),
            create_solid: Some(false),
            gap_tolerance: None,
        },
    ))
}

/// Select the neutral plane carried by a Draft feature's class-209 entity.
///
/// The class is a neutral-plane carrier only when it has one unambiguous
/// feature-owned surface row and that row is a plane. The table class is not
/// part of the rule: Draft records use more than one enclosing table class.
pub(in super::super) fn draft_neutral_plane_selection(
    scan: &ContainerScan,
    feature_id: u32,
) -> FaceSelection {
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
        return FaceSelection::Unresolved;
    };
    if table
        .surface_ids_iter()
        .filter(|surface_id| *surface_id == entry.entity_id)
        .count()
        != 1
    {
        return FaceSelection::Unresolved;
    }
    let Some(surface) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
        .filter(|surface| {
            surface.feature_id == feature_id && surface.kind == crate::surface::SurfaceKind::Plane
        })
    else {
        return FaceSelection::Unresolved;
    };
    FaceSelection::Native(format!("creo:visibgeom:surface#{}", surface.id))
}

pub(in super::super) fn feature_surface_transitions(
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Option<Vec<(u32, u32)>> {
    let outputs = tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .flat_map(|table| table.entries.iter())
        .filter(|entry| entry.class_id() == 210)
        .count();
    if outputs == 0 {
        return None;
    }
    let predecessors = tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
        .flat_map(|table| table.entries.iter())
        .filter(|entry| entry.class_id() == 214 && entry.related_entity_id().is_some())
        .count();
    if predecessors != outputs {
        return None;
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
        let intermediate_id = output.related_entity_id()?;
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
            return None;
        }
        output_ids.insert(output.entity_id);
        intermediate_ids.insert(intermediate_id);
        let mut matches = output_table.entries.iter().filter(|predecessor| {
            predecessor.class_id() == 214
                && predecessor.entity_id == intermediate_id
                && predecessor.related_entity_state() == Some(0)
                && output_table.contains_non_surface_entity_id(predecessor.entity_id)
                && crate::surface::unique_surface_row(surface_rows, predecessor.entity_id).is_none()
        });
        let predecessor = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
        let source_id = predecessor.related_entity_id()?;
        if crate::surface::unique_surface_row(surface_rows, source_id)
            .is_none_or(|row| row.feature_id == feature_id)
            || source_ids.contains(&source_id)
        {
            return None;
        }
        source_ids.insert(source_id);
        transitions.push((source_id, output.entity_id));
    }
    output_ids.is_disjoint(&source_ids).then_some(transitions)
}

pub(in super::super) fn surface_transition_dependencies(
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Vec<u32> {
    let mut dependencies = Vec::new();
    for (source_id, _) in
        feature_surface_transitions(feature_id, tables, surface_rows).unwrap_or_default()
    {
        let Some(row) = crate::surface::unique_surface_row(surface_rows, source_id) else {
            continue;
        };
        if !dependencies.contains(&row.feature_id) {
            dependencies.push(row.feature_id);
        }
    }
    dependencies
}

pub(in super::super) fn thicken_plane_offset(
    transitions: &[(u32, u32)],
    planes: &BTreeMap<u32, PlaneEquation>,
    rows: &[crate::surface::SurfaceRow],
) -> Option<(f64, ThickenSide)> {
    let mut offsets = Vec::new();
    for &(source_id, output_id) in transitions {
        let (Some(source), Some(output)) = (planes.get(&source_id), planes.get(&output_id)) else {
            continue;
        };
        let offset = (|| {
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
            (dot(source_normal, output_normal).abs() >= 1.0 - EPS_NORMAL_ALIGNMENT).then_some(())?;
            let displacement =
                std::array::from_fn(|index| output.origin[index] - source.origin[index]);
            Some(dot(displacement, source_normal))
        })()?;
        offsets.push(offset);
    }
    let mut magnitudes = Vec::new();
    for offset in &offsets {
        magnitudes.push(offset.abs());
    }
    let magnitude =
        unique_positive_length(&magnitudes).map(cadmpeg_ir::scalar::PositiveLength::get)?;
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
        return None;
    };
    Some((magnitude, side))
}

/// Return the materialized surface identities that one feature can expose as
/// faces in its regenerated result.
///
/// Every materialized surface in an owned generated-entity table is a
/// result-face identity when its surface row is unique and names the same
/// owning feature. Duplicate identifiers or malformed materialized rows
/// invalidate the complete result state for that feature.
pub(in super::super) fn feature_result_surface_ids(
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
) -> Option<Vec<u32>> {
    let mut surface_ids = Vec::new();
    let mut seen = BTreeSet::new();
    for table in tables.iter().filter(|table| table.feature_id == feature_id) {
        for surface_id in table.surface_ids_iter() {
            let row = crate::surface::unique_surface_row(rows, surface_id)?;
            if row.feature_id != feature_id || seen.contains(&surface_id) {
                return None;
            }
            seen.insert(surface_id);
            surface_ids.push(surface_id);
        }
    }
    (!surface_ids.is_empty()).then_some(surface_ids)
}

pub(super) fn feature_result_surface_ids_by_feature(
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> BTreeMap<u32, Vec<u32>> {
    let mut unique_features = BTreeSet::new();
    let mut by_feature = BTreeMap::new();
    for table in tables {
        let feature_id = table.feature_id;
        if unique_features.contains(&feature_id) {
            continue;
        }
        unique_features.insert(feature_id);
        if let Some(surface_ids) = feature_result_surface_ids(tables, rows, feature_id) {
            by_feature.insert(feature_id, surface_ids);
        }
    }
    by_feature
}

pub(in super::super) fn feature_result_topology(
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
    edge_ids: &[u32],
    feature_id: u32,
) -> Result<Option<FeatureResultTopology>, CodecError> {
    let mut faces = Vec::new();
    for surface_id in
        feature_result_surface_ids(tables, surface_rows, feature_id).unwrap_or_default()
    {
        let text = format!("surface#{surface_id}");
        let id = NonBlankString::new(text)
            .ok_or_else(|| CodecError::Malformed("constructed face local ID is blank".into()))?;
        faces.push(id);
    }
    let mut edges = Vec::new();
    for curve_id in edge_ids {
        let text = format!("curve#{curve_id}");
        let id = NonBlankString::new(text)
            .ok_or_else(|| CodecError::Malformed("constructed edge local ID is blank".into()))?;
        edges.push(id);
    }
    if faces.is_empty() && edges.is_empty() {
        return Ok(None);
    }
    let Ok(members) =
        cadmpeg_ir::features::FeatureResultMembers::new(Vec::new(), faces, edges, Vec::new())
    else {
        return Ok(None);
    };
    let id = FeatureResultTopologyId::mint(format!(
        "creo:model:feature-result-topology#{feature_id}"
    ))
    .map_err(|_| CodecError::Malformed("constructed result topology ID is invalid".into()))?;
    let output_of = IrFeatureId::mint(format!("creo:model:feature#{feature_id}"))
        .map_err(|_| CodecError::Malformed("constructed result owner ID is invalid".into()))?;
    Ok(Some(FeatureResultTopology::new(
        id, output_of, members, None,
    )))
}

pub(in super::super) fn generated_surface_face_refs(
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
        let feature_text = format!("creo:model:feature#{}", row.feature_id);
        let feature = IrFeatureId::mint(feature_text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        if !available_features.contains(&feature)
            || !result_surface_ids
                .get(&row.feature_id)
                .is_some_and(|ids| ids.contains(surface_id))
        {
            return Ok(None);
        }
        let local_id = format!("surface#{surface_id}");
        let Some(face) = GeneratedFaceRef::new(feature, local_id).ok() else {
            return Ok(None);
        };
        generated.push(face);
    }
    Ok(Some(generated))
}

pub(in super::super) fn emit_feature_result_topologies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
) -> Result<usize, cadmpeg_core::CodecError> {
    let result_edges = feature_result_edge_ids_by_feature(&scan.curves.topology_rows);
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
            &scan.features.entity_tables,
            &scan.surfaces.rows,
            result_edges.get(&feature_id).map_or(&[], Vec::as_slice),
            feature_id,
        )?
        else {
            continue;
        };
        ctx.charge_entities(1, "admit Creo model feature_result_topologies")?;
        ir.model.feature_result_topologies.push(state);
        emitted += 1;
    }
    Ok(emitted)
}

#[cfg(test)]
mod tests;
