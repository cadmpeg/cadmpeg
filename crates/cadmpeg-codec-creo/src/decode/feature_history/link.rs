// SPDX-License-Identifier: Apache-2.0
//! Sketch-history links and generated surface identity helpers.

use super::super::sketch_ids::{model_sketch_id, section_owner_feature_id};
use super::super::uniqueness::{
    exactly_one, unique_feature_definition_for_transform, unique_feature_section_transform,
};
use crate::container::ContainerScan;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId as IrFeatureId;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometry, SketchGeometryDefinition};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn link_feature_sketch_history(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    for transform in ctx.admit_iter(
        &scan.features.section_transforms,
        "creo linked section transforms",
    )? {
        if unique_feature_section_transform(
            ctx,
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )?
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        let (owner, _owner_reservation) = crate::identity::compose_scoped::<IrFeatureId>(
            ctx,
            &crate::identity::MODEL_FEATURE,
            feature_id,
            "creo linked feature lookup identity",
        )?;
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let Some(sketch) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        let Some(sketch_feature) =
            section_owner_feature_id(ctx, scan, transform.definition_id, &sketch)?
        else {
            continue;
        };
        if unique_model_feature_index(ctx, ir, &sketch_feature)?.is_none() {
            continue;
        }
        let Some(owner_index) = unique_model_feature_index(ctx, ir, &owner)? else {
            continue;
        };
        ir.model.features[owner_index].dependencies.insert(
            ctx,
            sketch_feature,
            "creo sketch history dependencies",
        )?;
    }
    Ok(())
}

fn unique_model_feature_index(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    feature_id: &IrFeatureId,
) -> Result<Option<usize>, CodecError> {
    let mut matching_index = None;
    for (index, feature) in ctx
        .admit_iter(&ir.model.features, "creo linked model feature lookup")?
        .enumerate()
    {
        if !ctx.equal(
            &feature.id,
            feature_id,
            "creo linked model feature identity comparison",
        )? {
            continue;
        }
        if matching_index.is_some() {
            return Ok(None);
        }
        matching_index = Some(index);
    }
    Ok(matching_index)
}

pub(in super::super) fn surface_kind_for_geometry(
    geometry: &SurfaceGeometry,
) -> Option<crate::surface::SurfaceKind> {
    solved_surface_kind(geometry.solved()?)
}

fn solved_surface_kind(geometry: &SolvedSurfaceGeometry) -> Option<crate::surface::SurfaceKind> {
    match geometry {
        SolvedSurfaceGeometry::Plane(_) => Some(crate::surface::SurfaceKind::Plane),
        SolvedSurfaceGeometry::Cylinder(_) => Some(crate::surface::SurfaceKind::Cylinder),
        SolvedSurfaceGeometry::Cone(_) => Some(crate::surface::SurfaceKind::Cone),
        SolvedSurfaceGeometry::Sphere(_) => Some(crate::surface::SurfaceKind::TorusOrSphere),
        SolvedSurfaceGeometry::Torus(_) => Some(crate::surface::SurfaceKind::TorusOrSphere),
        SolvedSurfaceGeometry::Nurbs(_) => Some(crate::surface::SurfaceKind::Spline),
        SolvedSurfaceGeometry::Transformed(placed) => solved_surface_kind(placed.basis()),
        SolvedSurfaceGeometry::Polygonal(_) | SolvedSurfaceGeometry::Unknown { .. } => None,
    }
}

pub(in super::super) fn generated_surface_id_for_feature(
    ctx: &DecodeContext<'_>,
    tables: &[crate::feature::entity::FeatureEntityTable],
    feature_id: u32,
    source_entity_id: u32,
) -> Result<Option<u32>, CodecError> {
    let mut surface_id = None;
    for table in ctx.admit_iter(tables, "creo generated surface feature tables")? {
        if table.feature_id != feature_id {
            continue;
        }
        for entry in ctx.admit_iter(&table.entries, "creo generated surface feature entries")? {
            if entry.source_entity_id() != Some(source_entity_id)
                || !table.contains_surface_id(entry.entity_id)
            {
                continue;
            }
            if surface_id.is_some() {
                return Ok(None);
            }
            surface_id = Some(entry.entity_id);
        }
    }
    Ok(surface_id)
}

pub(in super::super) fn generated_profile_entry_is_admissible(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    table: &crate::feature::entity::FeatureEntityTable,
    entry: &crate::feature::entity::FeatureEntityTableEntry,
    expected_kinds: &[crate::surface::SurfaceKind],
    rows: &crate::surface::SurfaceRows,
) -> Result<bool, CodecError> {
    if entry.source_entity_id().is_none() {
        return Ok(false);
    }
    if table.contains_surface_id(entry.entity_id) {
        let Some(row) = crate::surface::unique_surface_row(rows, entry.entity_id) else {
            return Ok(false);
        };
        return Ok(row.feature_id == feature_id
            && ctx.any_by(
                expected_kinds,
                |kind| Ok(kind.same_family(row.kind)),
                "creo generated profile expected surface kinds",
            )?);
    }
    if !table.contains_non_surface_entity_id(entry.entity_id)
        || !generated_profile_table_shape(ctx, table)?
    {
        return Ok(false);
    }
    for candidate in ctx
        .admit_iter(&table.entries, "creo generated profile candidate entries")?
        .skip(2)
    {
        if !table.contains_surface_id(candidate.entity_id) {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(rows, candidate.entity_id) else {
            continue;
        };
        if row.feature_id == feature_id
            && ctx.any_by(
                expected_kinds,
                |kind| Ok(kind.same_family(row.kind)),
                "creo generated profile expected surface kinds",
            )?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(in super::super) fn section_entity_is_generated_profile(
    ctx: &DecodeContext<'_>,
    segment_table_complete: bool,
    feature_id: Option<u32>,
    source_entity_id: u32,
    expected_kinds: &[crate::surface::SurfaceKind],
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &crate::surface::SurfaceRows,
) -> Result<bool, CodecError> {
    if !segment_table_complete {
        return Ok(false);
    }
    let Some(feature_id) = feature_id else {
        return Ok(false);
    };
    let direct = match generated_surface_id_for_feature(ctx, tables, feature_id, source_entity_id)?
    {
        Some(surface_id) => match crate::surface::unique_surface_row(rows, surface_id) {
            Some(row) => {
                row.feature_id == feature_id
                    && ctx.any_by(
                        expected_kinds,
                        |kind| Ok(kind.same_family(row.kind)),
                        "creo direct generated profile surface kinds",
                    )?
            }
            None => false,
        },
        None => false,
    };
    if direct {
        return Ok(true);
    }
    let mut rowless_matches = 0usize;
    for table in ctx.admit_iter(tables, "creo generated profile rowless tables")? {
        if table.feature_id != feature_id {
            continue;
        }
        let Some(entry) = exactly_one(
            ctx.admit_iter(&table.entries, "creo generated profile rowless entries")?
                .filter(|entry| entry.source_entity_id() == Some(source_entity_id)),
        ) else {
            continue;
        };
        if !table.contains_surface_id(entry.entity_id)
            && generated_profile_entry_is_admissible(
                ctx,
                feature_id,
                table,
                entry,
                expected_kinds,
                rows,
            )?
        {
            rowless_matches = rowless_matches.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit("creo rowless generated profile matches", u64::MAX, u64::MAX)
            })?;
        }
    }
    if rowless_matches == 1 {
        return Ok(true);
    }
    if !ctx.contains(
        expected_kinds,
        &crate::surface::SurfaceKind::Cylinder,
        "creo blind generated profile surface kind lookup",
    )? {
        return Ok(false);
    }
    let mut found_cylinder = false;
    for table in ctx.admit_iter(tables, "creo blind generated profile tables")? {
        if table.feature_id != feature_id {
            continue;
        }
        let [rowless_cap, cap, profile, cylinder] = table.entries.as_slice() else {
            continue;
        };
        let row_matches =
            crate::surface::unique_surface_row(rows, cylinder.entity_id).is_some_and(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
            });
        let is_candidate = [
            rowless_cap.class_id(),
            cap.class_id(),
            profile.class_id(),
            cylinder.class_id(),
        ] == [204, 203, 200, 200]
            && profile.source_entity_id() == Some(source_entity_id)
            && cylinder.source_entity_id().is_none()
            && table.contains_surface_id(cap.entity_id)
            && table.contains_surface_id(cylinder.entity_id)
            && table.contains_non_surface_entity_id(rowless_cap.entity_id)
            && table.contains_non_surface_entity_id(profile.entity_id)
            && row_matches;
        if is_candidate {
            if found_cylinder {
                return Ok(false);
            }
            found_cylinder = true;
        }
    }
    Ok(found_cylinder)
}

fn generated_profile_table_shape(
    ctx: &DecodeContext<'_>,
    table: &crate::feature::entity::FeatureEntityTable,
) -> Result<bool, CodecError> {
    let [first, second, rest @ ..] = table.entries.as_slice() else {
        return Ok(false);
    };
    if table.table_class_id != 29
        || first.class_id() != 204
        || second.class_id() != 203
        || rest.is_empty()
        || !ctx.all_by(
            rest,
            |entry| Ok(entry.source_entity_id().is_some()),
            "creo generated profile remaining entries",
        )?
    {
        return Ok(false);
    }
    for (index, entry) in ctx
        .admit_iter(&table.entries, "creo generated profile unique entry IDs")?
        .enumerate()
    {
        if ctx.any_by(
            &table.entries[..index],
            |prior| Ok(prior.entity_id == entry.entity_id),
            "creo generated profile prior entry IDs",
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(in super::super) fn section_generated_profile_surface_kinds(
    geometry: &SketchGeometry,
) -> Option<&'static [crate::surface::SurfaceKind]> {
    match geometry.definition() {
        SketchGeometryDefinition::Line { .. } => Some(&[crate::surface::SurfaceKind::Plane]),
        SketchGeometryDefinition::Arc { .. } | SketchGeometryDefinition::Circle { .. } => {
            Some(&[crate::surface::SurfaceKind::Cylinder])
        }
        SketchGeometryDefinition::Nurbs { .. } => Some(&[
            crate::surface::SurfaceKind::Spline,
            crate::surface::SurfaceKind::Extrusion(crate::surface::ExtrusionVariant::Linear),
        ]),
        _ => None,
    }
}

pub(in super::super) fn ordered_analytic_surface_id_for_feature(
    ctx: &DecodeContext<'_>,
    surface_rows: &crate::surface::SurfaceRows,
    tables: &[crate::feature::entity::FeatureEntityTable],
    feature_id: u32,
    order: &crate::feature::definitions::FeatureOrderTable,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Result<Option<u32>, CodecError> {
    if order.internal_id(external_id).is_none() {
        return Ok(None);
    }
    analytic_surface_id_for_feature(ctx, surface_rows, tables, feature_id, external_id, geometry)
}

pub(in super::super) fn analytic_surface_id_for_feature(
    ctx: &DecodeContext<'_>,
    surface_rows: &crate::surface::SurfaceRows,
    tables: &[crate::feature::entity::FeatureEntityTable],
    feature_id: u32,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Result<Option<u32>, CodecError> {
    let Some(surface_id) = generated_surface_id_for_feature(ctx, tables, feature_id, external_id)?
    else {
        return Ok(None);
    };
    let Some(expected_kind) = surface_kind_for_geometry(geometry) else {
        return Ok(None);
    };
    let Some(row) = crate::surface::unique_surface_row(surface_rows, surface_id) else {
        return Ok(None);
    };
    Ok((row.feature_id == feature_id && row.kind.same_family(expected_kind)).then_some(surface_id))
}

pub(in super::super) fn insert_ordered_family_surface_binding(
    ctx: &DecodeContext<'_>,
    surface_rows: &crate::surface::SurfaceRows,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    order: &crate::feature::definitions::FeatureOrderTable,
    external_id: u32,
    expected_kind: crate::surface::SurfaceKind,
    bindings: &mut BTreeMap<u32, u32>,
    bound_surfaces: &mut BTreeSet<u32>,
) -> Result<bool, CodecError> {
    if order.internal_id(external_id).is_none() {
        return Ok(false);
    }
    let Some(surface_id) = generated_surface_id_for_feature(ctx, tables, feature_id, external_id)?
    else {
        return Ok(false);
    };
    if !crate::surface::unique_surface_row(surface_rows, surface_id)
        .is_some_and(|row| row.feature_id == feature_id && row.kind.same_family(expected_kind))
        || bound_surfaces.contains(&surface_id)
    {
        return Ok(false);
    }
    ctx.insert_btree_set(
        bound_surfaces,
        surface_id,
        "creo bound generated surface IDs",
    )?;
    ctx.insert_btree_map(
        bindings,
        external_id,
        surface_id,
        "creo ordered generated surface bindings",
    )?;
    Ok(true)
}

pub(in super::super) fn ordered_family_surface_bindings_for_feature(
    ctx: &DecodeContext<'_>,
    surface_rows: &crate::surface::SurfaceRows,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    order: &crate::feature::definitions::FeatureOrderTable,
    external_ids: impl IntoIterator<Item = u32>,
    expected_kind: crate::surface::SurfaceKind,
) -> Result<BTreeMap<u32, u32>, CodecError> {
    let mut bindings = BTreeMap::new();
    let mut bound_surfaces = BTreeSet::new();
    for external_id in external_ids {
        if !insert_ordered_family_surface_binding(
            ctx,
            surface_rows,
            feature_id,
            tables,
            order,
            external_id,
            expected_kind,
            &mut bindings,
            &mut bound_surfaces,
        )? {
            return Ok(BTreeMap::new());
        }
    }
    Ok(bindings)
}

pub(in super::super) fn profile_segment_ids(
    ctx: &DecodeContext<'_>,
    definition_id: u32,
    segments: &[&crate::feature::definitions::FeatureSegment],
    profiles: &[Vec<SketchEntityUse>],
) -> Result<BTreeSet<u32>, CodecError> {
    let mut ids = BTreeSet::new();
    for segment in ctx.admit_iter(segments, "creo profile segment rows")? {
        let mut matches = false;
        'profiles: for profile in ctx.admit_iter(profiles, "creo sketch profiles")? {
            for entity_use in ctx.admit_iter(profile, "creo sketch profile entities")? {
                let Some(suffix) = entity_use
                    .entity
                    .as_str()
                    .strip_prefix("creo:featdefs:sketch_entity#")
                else {
                    continue;
                };
                let Some((scope, external)) = suffix.split_once(':') else {
                    continue;
                };
                if crate::identity::matches_numbered_identity(scope, "", definition_id)
                    && crate::identity::matches_numbered_identity(external, "", segment.external_id)
                {
                    matches = true;
                    break 'profiles;
                }
            }
        }
        if matches {
            ctx.insert_btree_set(
                &mut ids,
                segment.external_id,
                "creo profile segment ID nodes",
            )?;
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests;
