// SPDX-License-Identifier: Apache-2.0
//! Filled, knit, draft, thicken, and result-topology feature recipes.

use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::unique_feature_profile_definition;
use super::axes::model_feature_ids;
use super::dependencies::{surface_merge_quilt_ids, surface_merge_quilt_state_offset};
use super::outputs::CommaList;
use super::round::unique_positive_length;
use super::selections::feature_result_edge_ids;
use crate::container::ContainerScan;
use crate::decode::analytic::equations::PlaneEquation;
use crate::vecmath::dot;
use crate::vecmath::normalize;
use cadmpeg_core::decode::DecodeContext;
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<IrFeatureDefinition, CodecError> {
    let sketch = match unique_feature_profile_definition(
        ctx,
        &scan.features.definitions,
        &scan.features.section_transforms,
        feature_id,
    )? {
        Some(definition) => model_sketch_id(ctx, scan, definition)?,
        None => None,
    };
    let boundary = match sketch {
        Some(sketch) => {
            let sketch_found = ctx.any_by(&ir.model.sketches, |candidate| ctx.equal(
                    &candidate.id,
                    &sketch,
                    "creo model sketch identity comparison",
                ), "creo model sketch lookup")?;
            if sketch_found {
                SurfaceBoundary::Path(PathRef::Sketch(sketch))
            } else {
                SurfaceBoundary::Edges(EdgeSelection::Unresolved)
            }
        }
        None => SurfaceBoundary::Edges(EdgeSelection::Unresolved),
    };
    Ok(IrFeatureDefinition::Operation(
        IrFeatureOperation::FilledSurface {
            boundary,
            support_faces: FaceSelection::Faces(Vec::new()),
            continuity: cadmpeg_ir::features::FilledSurfaceContinuityState::uniform(
                SurfaceContinuity::Contact,
            ),
            merge_result: Some(false),
        },
    ))
}

pub(in super::super) fn knit_class_100_operand_entity_ids(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut items = (tables).into_iter().enumerate();
    while let Some((table_index, table)) = ctx.next_charged(&mut items, "creo knit entity tables")? {
        if table.feature_id != feature_id || table.table_class_id != 100 {
            continue;
        }
        let mut items = (&table.entries).into_iter().enumerate();
        while let Some((entry_index, entry)) = ctx.next_charged(&mut items, "creo knit table entries")? {
            if ctx.contains_btree_set(&seen, &entry.entity_id, "creo knit consumer identity lookup")? {
                return Ok(None);
            }
            ctx.insert_btree_set(
                &mut seen,
                entry.entity_id,
                "creo knit consumer identity nodes",
            )?;
            let consumer_position = (table.offset, entry.offset, table_index, entry_index);
            let mut producer = None;
            let mut items = (tables).into_iter().enumerate();
            while let Some((source_index, source_table)) = ctx.next_charged(&mut items, "creo knit source tables")? {
                if source_table.feature_id == feature_id {
                    continue;
                }
                let mut items = (&source_table.entries).into_iter().enumerate();
                while let Some((source_entry_index, source_entry)) = ctx.next_charged(&mut items, "creo knit source table entries")? {
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
            ctx.reserve_vec(&mut ids, 1, "creo knit class 100 operand IDs")?;
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
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    if let Some(ids) = surface_merge_quilt_ids(
        ctx,
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
    )? {
        let mut copied = Vec::new();
        let mut seen = BTreeSet::new();
        for &id in ids {
            if ctx.contains_btree_set(&seen, &id, "creo knit quilt identity lookup")? {
                return Ok(None);
            }
            local_storage.with_storage(|| {
                ctx.insert_btree_set(&mut seen, id, "creo knit quilt identity nodes")
            })?;
            ctx.reserve_vec(&mut copied, 1, "creo knit quilt IDs")?;
            copied.push(id);
        }
        return Ok(Some((copied, "surface_merge_quilts")));
    }
    Ok(
        knit_class_100_operand_entity_ids(ctx, feature_id, &scan.features.entity_tables)?
            .map(|ids| (ids, "surface_merge_entities")),
    )
}

pub(in super::super) fn knit_operand_surface_ids(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
    quilt_ids: &[u32],
) -> Result<Option<Vec<u32>>, CodecError> {
    let Some(consumer_offset) = surface_merge_quilt_state_offset(
        ctx,
        &scan.features.affected_ids,
        &scan.features.surface_merge_replay_affected_ids,
        feature_id,
        quilt_ids,
    )?
    else {
        return Ok(None);
    };
    let mut surface_ids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut quilt_id_iter = (quilt_ids).into_iter();
    while let Some(quilt_id) = ctx.next_charged(&mut quilt_id_iter, "creo knit quilt IDs")? {
        let mut producer = None;
        let mut table_iter = (&scan.features.entity_tables).into_iter();
        while let Some(table) = ctx.next_charged(&mut table_iter, "creo knit entity tables")? {
            let mut entry_iter = (&table.entries).into_iter();
            while let Some(entry) = ctx.next_charged(&mut entry_iter, "creo knit table entries")? {
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
        let mut table_iter = (&scan.features.entity_tables).into_iter();
        while let Some(table) = ctx.next_charged(&mut table_iter, "creo knit entity tables")? {
            if table.feature_id != producer
                || table.table_class_id != 100
                || table.offset >= consumer_offset
            {
                continue;
            }
            let mut entry_iter = (&table.entries).into_iter();
            while let Some(entry) = ctx.next_charged(&mut entry_iter, "creo knit table entries")? {
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
        let Some(surface) = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)
        else {
            return Ok(None);
        };
        if surface.feature_id != producer || ctx.contains_btree_set(&seen, &surface_id, "creo knit surface identity lookup")? {
            return Ok(None);
        }
        ctx.insert_btree_set(&mut seen, surface_id, "creo knit surface identity nodes")?;
        ctx.reserve_vec(&mut surface_ids, 1, "creo knit surface IDs")?;
        surface_ids.push(surface_id);
    }
    Ok(Some(surface_ids))
}

pub(super) fn knit_surface_feature_definition(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<IrFeatureDefinition, CodecError> {
    let faces =
        if let Some((quilt_ids, namespace)) = knit_operand_entity_ids(ctx, scan, feature_id)? {
            let native = ctx.format_retained(
                format_args!(
                    "creo:allfeatur:{namespace}#{feature_id}:{}",
                    CommaList(&quilt_ids)
                ),
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
                    ctx,
                )?
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
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<FaceSelection, CodecError> {
    let mut match_entry = None;
    let mut table_iter = (&scan.features.entity_tables).into_iter();
    while let Some(table) = ctx.next_charged(&mut table_iter, "creo draft entity tables")? {
        if !(table.feature_id == feature_id) { continue; }
        let mut entry_iter = (&table.entries).into_iter();
        while let Some(entry) = ctx.next_charged(&mut entry_iter, "creo draft table entries")? {
            if !(entry.class_id() == 209) { continue; }
            if match_entry.replace((table, entry)).is_some() {
                return Ok(FaceSelection::Unresolved);
            }
        }
    }
    let Some((table, entry)) = match_entry else {
        return Ok(FaceSelection::Unresolved);
    };
    if ctx
        .admit_iter(&table.entries, "creo draft table surface entries")?
        .filter(|entry| table.contains_surface_id(entry.entity_id))
        .map(|entry| entry.entity_id)
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
    surface_rows: &crate::surface::SurfaceRows,
) -> Result<Option<Vec<(u32, u32)>>, CodecError> {
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut outputs = 0usize;
    for table in ctx.admit_iter(tables, "creo transition entity tables")? {
        if table.feature_id == feature_id {
            for entry in ctx.admit_iter(&table.entries, "creo transition table entries")? {
                if entry.class_id() == 210 {
                    outputs = outputs.checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "creo feature transition output count",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
                }
            }
        }
    }
    if outputs == 0 {
        return Ok(None);
    }
    let mut predecessors = 0usize;
    for table in ctx.admit_iter(tables, "creo predecessor entity tables")? {
        if table.feature_id == feature_id {
            for entry in ctx.admit_iter(&table.entries, "creo predecessor table entries")? {
                if entry.class_id() == 214 && entry.related_entity_id().is_some() {
                    predecessors = predecessors.checked_add(1).ok_or_else(|| {
                        ctx.refuse_codec_limit(
                            "creo feature transition predecessor count",
                            u64::MAX,
                            u64::MAX,
                        )
                    })?;
                }
            }
        }
    }
    if predecessors != outputs {
        return Ok(None);
    }

    let mut output_ids = BTreeSet::new();
    let mut intermediate_ids = BTreeSet::new();
    let mut source_ids = BTreeSet::new();
    let mut transitions = Vec::new();
    let mut output_table_iter = (tables).into_iter();
    while let Some(output_table) = ctx.next_charged(&mut output_table_iter, "creo transition output tables")? {
        if output_table.feature_id != feature_id {
            continue;
        }
        let mut output_iter = (&output_table.entries).into_iter();
        while let Some(output) = ctx.next_charged(&mut output_iter, "creo transition output entries")? {
            if !(output.class_id() == 210) { continue; }
            let Some(intermediate_id) = output.related_entity_id() else {
                return Ok(None);
            };
            if output.related_entity_state() != Some(0)
                || ctx
                    .admit_iter(&output_table.entries, "creo transition surface entries")?
                    .filter(|entry| output_table.contains_surface_id(entry.entity_id))
                    .map(|entry| entry.entity_id)
                    .filter(|surface_id| *surface_id == output.entity_id)
                    .count()
                    != 1
                || crate::surface::unique_surface_row(surface_rows, output.entity_id)
                    .is_none_or(|row| row.feature_id != feature_id)
                || ctx.contains_btree_set(&output_ids, &output.entity_id, "creo transition output identity lookup")?
                || ctx.contains_btree_set(&intermediate_ids, &intermediate_id, "creo transition intermediate identity lookup")?
            {
                return Ok(None);
            }
            local_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut output_ids,
                    output.entity_id,
                    "creo transition output identity nodes",
                )
            })?;
            local_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut intermediate_ids,
                    intermediate_id,
                    "creo transition intermediate identity nodes",
                )
            })?;
            let mut matches = ctx
                .admit_iter(&output_table.entries, "creo transition predecessor entries")?
                .filter(|predecessor| {
                    predecessor.class_id() == 214
                        && predecessor.entity_id == intermediate_id
                        && predecessor.related_entity_state() == Some(0)
                        && output_table.contains_non_surface_entity_id(predecessor.entity_id)
                        && crate::surface::unique_surface_row(surface_rows, predecessor.entity_id)
                            .is_none()
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
                || ctx.contains_btree_set(&source_ids, &source_id, "creo transition source identity lookup")?
            {
                return Ok(None);
            }
            local_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut source_ids,
                    source_id,
                    "creo transition source identity nodes",
                )
            })?;
            ctx.reserve_vec(&mut transitions, 1, "creo surface transitions")?;
            transitions.push((source_id, output.entity_id));
        }
    }
    Ok((!ctx.any_by(&output_ids, |id| ctx.contains_btree_set(&source_ids, id, "creo transition identity separation lookup"), "creo transition identity separation")?).then_some(transitions))
}

pub(in super::super) fn surface_transition_dependencies(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &crate::surface::SurfaceRows,
) -> Result<Vec<u32>, CodecError> {
    let mut dependencies = Vec::new();
    let transitions =
        feature_surface_transitions(ctx, feature_id, tables, surface_rows)?.unwrap_or_default();
    for &(source_id, _) in ctx.admit_iter(&transitions, "creo feature surface transitions")? {
        let Some(row) = crate::surface::unique_surface_row(surface_rows, source_id) else {
            continue;
        };
        if !ctx.contains(&dependencies, &row.feature_id, "creo knit dependencies membership")? {
            ctx.reserve_vec(&mut dependencies, 1, "creo transition dependencies")?;
            dependencies.push(row.feature_id);
        }
    }
    Ok(dependencies)
}

pub(in super::super) fn thicken_plane_offset(
    ctx: &DecodeContext<'_>,
    transitions: &[(u32, u32)],
    planes: &BTreeMap<u32, PlaneEquation>,
    rows: &crate::surface::SurfaceRows,
) -> Result<Option<(f64, ThickenSide)>, CodecError> {
    let mut offsets = Vec::new();
    let mut items = (transitions).into_iter();
    while let Some(&(source_id, output_id)) = ctx.next_charged(&mut items, "creo thicken surface transitions")? {
        let (Some(source), Some(output)) = (ctx.get_btree_map(planes, &source_id, "creo thicken source plane lookup")?, ctx.get_btree_map(planes, &output_id, "creo thicken output plane lookup")?) else {
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
            (dot(source_normal, output_normal).abs() >= 1.0 - EPS_NORMAL_ALIGNMENT).then_some(())?;
            let displacement =
                std::array::from_fn(|index| output.origin[index] - source.origin[index]);
            Some(dot(displacement, source_normal))
        })() else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut offsets, 1, "creo thicken plane offsets")?;
        offsets.push(offset);
    }
    let mut magnitudes = Vec::new();
    ctx.reserve_vec(
        &mut magnitudes,
        offsets.len(),
        "creo thicken plane magnitudes",
    )?;
    for offset in ctx.admit_iter(&offsets, "creo thicken plane offsets")? {
        magnitudes.push(offset.abs());
    }
    let Some(magnitude) =
        unique_positive_length(ctx, &magnitudes)?.map(cadmpeg_ir::scalar::PositiveLength::get)
    else {
        return Ok(None);
    };
    let tolerance = EPS_OFFSET_AGREEMENT * magnitude.max(1.0);
    let side = if ctx.all_by(
        &offsets,
        |offset| Ok((*offset - magnitude).abs() <= tolerance),
        "creo thicken plane offsets",
    )? {
        ThickenSide::Forward
    } else if ctx.all_by(
        &offsets,
        |offset| Ok((*offset + magnitude).abs() <= tolerance),
        "creo thicken plane offsets",
    )? {
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
    rows: &crate::surface::SurfaceRows,
    feature_id: u32,
) -> Result<Option<Vec<u32>>, CodecError> {
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut surface_ids = Vec::new();
    let mut seen = BTreeSet::new();
    let mut table_iter = (tables).into_iter();
    while let Some(table) = ctx.next_charged(&mut table_iter, "creo feature result entity tables")? {
        if !(table.feature_id == feature_id) { continue; }
        for surface_id in ctx
            .admit_iter(&table.entries, "creo feature result surface entries")?
            .filter(|entry| table.contains_surface_id(entry.entity_id))
            .map(|entry| entry.entity_id)
        {
            let Some(row) = crate::surface::unique_surface_row(rows, surface_id) else {
                return Ok(None);
            };
            if row.feature_id != feature_id || ctx.contains_btree_set(&seen, &surface_id, "creo knit surface identity lookup")? {
                return Ok(None);
            }
            local_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut seen,
                    surface_id,
                    "creo feature result surface identity nodes",
                )
            })?;
            ctx.reserve_vec(&mut surface_ids, 1, "creo feature result surface IDs")?;
            surface_ids.push(surface_id);
        }
    }
    Ok((!surface_ids.is_empty()).then_some(surface_ids))
}

pub(super) fn feature_result_surface_ids_by_feature(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &crate::surface::SurfaceRows,
) -> Result<BTreeMap<u32, Vec<u32>>, CodecError> {
    let mut local_storage = ctx.reserve_scoped(0, "Creo feature selection workspace")?;
    let mut unique_features = BTreeSet::new();
    let mut by_feature = BTreeMap::new();
    for table in ctx.admit_iter(tables, "creo feature result entity tables")? {
        let feature_id = table.feature_id;
        if ctx.contains_btree_set(&unique_features, &feature_id, "creo feature result feature identity lookup")? {
            continue;
        }
        local_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut unique_features,
                feature_id,
                "creo feature result feature identity nodes",
            )
        })?;
        if let Some(surface_ids) = feature_result_surface_ids(ctx, tables, rows, feature_id)? {
            ctx.insert_btree_map(
                &mut by_feature,
                feature_id,
                surface_ids,
                "creo feature result surface map nodes",
            )?;
        }
    }
    Ok(by_feature)
}

pub(in super::super) fn feature_result_topology(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &crate::surface::SurfaceRows,
    curve_rows: &[crate::curve::CurveTopologyRow],
    feature_id: u32,
) -> Result<Option<FeatureResultTopology>, CodecError> {
    let mut faces = Vec::new();
    let surface_ids =
        feature_result_surface_ids(ctx, tables, surface_rows, feature_id)?.unwrap_or_default();
    for surface_id in ctx.admit_iter(&surface_ids, "creo feature result surface IDs")? {
        let text = ctx.format_retained(
            format_args!("surface#{surface_id}"),
            "creo feature result face local IDs",
        )?;
        let id = NonBlankString::for_decode(ctx, text, "validate nonblank text")?
            .ok_or_else(|| CodecError::Malformed("constructed face local ID is blank".into()))?;
        ctx.reserve_vec(&mut faces, 1, "creo feature result face members")?;
        faces.push(id);
    }
    let mut edges = Vec::new();
    let curve_ids = feature_result_edge_ids(ctx, curve_rows, feature_id)?.unwrap_or_default();
    for curve_id in ctx.admit_iter(&curve_ids, "creo feature result edge IDs")? {
        let text = ctx.format_retained(
            format_args!("curve#{curve_id}"),
            "creo feature result edge local IDs",
        )?;
        let id = NonBlankString::for_decode(ctx, text, "validate nonblank text")?
            .ok_or_else(|| CodecError::Malformed("constructed edge local ID is blank".into()))?;
        ctx.reserve_vec(&mut edges, 1, "creo feature result edge members")?;
        edges.push(id);
    }
    if faces.is_empty() && edges.is_empty() {
        return Ok(None);
    }
    let Ok(members) = cadmpeg_ir::features::FeatureResultMembers::new(
        Vec::new(),
        faces,
        edges,
        Vec::new(),
        ctx,
        "creo feature result member distinctness",
    )?
    else {
        return Ok(None);
    };
    let id_text = ctx.format_retained(
        format_args!("creo:model:feature-result-topology#{feature_id}"),
        "creo feature result topology ID",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(id_text.len()),
        "creo feature result topology identity validation",
    )?;
    let id = FeatureResultTopologyId::mint(id_text)
        .map_err(|_| CodecError::Malformed("constructed result topology ID is invalid".into()))?;
    let output_of_text = ctx.format_retained(
        format_args!("creo:model:feature#{feature_id}"),
        "creo feature result owner ID",
    )?;
    ctx.charge_work(
        cadmpeg_core::decode::u64_from_index(output_of_text.len()),
        "creo feature result owner identity validation",
    )?;
    let output_of = IrFeatureId::mint(output_of_text)
        .map_err(|_| CodecError::Malformed("constructed result owner ID is invalid".into()))?;
    Ok(Some(FeatureResultTopology::new(
        id, output_of, members, None,
    )))
}

pub(in super::super) fn generated_surface_face_refs(
    ctx: &DecodeContext<'_>,
    source_ids: &[u32],
    rows: &crate::surface::SurfaceRows,
    result_surface_ids: &BTreeMap<u32, Vec<u32>>,
    available_features: &BTreeSet<IrFeatureId>,
) -> Result<Option<Vec<GeneratedFaceRef>>, CodecError> {
    let mut generated = Vec::new();
    let mut surface_id_iter = (source_ids).into_iter();
    while let Some(surface_id) = ctx.next_charged(&mut surface_id_iter, "creo generated surface IDs")? {
        let Some(row) = crate::surface::unique_surface_row(rows, *surface_id) else {
            return Ok(None);
        };
        let feature_text = ctx.format_retained(
            format_args!("creo:model:feature#{}", row.feature_id),
            "creo generated surface feature IDs",
        )?;
        ctx.charge_work(
            cadmpeg_core::decode::u64_from_index(feature_text.len()),
            "creo generated surface feature identity validation",
        )?;
        let feature = IrFeatureId::mint(feature_text)
            .map_err(|_| CodecError::Malformed("constructed Creo feature ID is invalid".into()))?;
        if !ctx.contains_btree_set(
            available_features,
            &feature,
            "creo generated surface feature lookup",
        )? {
            return Ok(None);
        }
        let Some(ids) = ctx.get_btree_map(result_surface_ids, &row.feature_id, "creo generated surface result roster lookup")? else {
            return Ok(None);
        };
        if !ctx.contains(ids, surface_id, "creo generated surface result ID lookup")? {
            return Ok(None);
        }
        let local_id = ctx.format_retained(
            format_args!("surface#{surface_id}"),
            "creo generated surface local IDs",
        )?;
        let Some(face) = GeneratedFaceRef::new(feature, local_id, ctx)?.ok() else {
            return Ok(None);
        };
        ctx.reserve_vec(&mut generated, 1, "creo generated surface face references")?;
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
    for feature in ctx.admit_iter(&ir.model.features, "creo model features")? {
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
        )?
        else {
            continue;
        };
        ctx.reserve_vec(
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
