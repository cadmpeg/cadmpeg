// SPDX-License-Identifier: Apache-2.0
//! Sketch-history links and generated surface identity helpers.

use super::super::sketch_ids::{model_sketch_id, section_owner_feature_id};
use super::super::uniqueness::{
    exactly_one, unique_feature_definition_for_transform, unique_feature_section_transform,
};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FeatureId as IrFeatureId;
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::sketches::{SketchEntityId, SketchEntityUse, SketchGeometry};
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn link_feature_sketch_history(scan: &ContainerScan, ir: &mut CadIr) {
    let links = scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| {
            unique_feature_section_transform(
                &scan.features.section_transforms,
                transform.definition_id,
                transform.offset,
            )
            .is_some()
        })
        .filter_map(|transform| {
            let owner = IrFeatureId(format!("creo:model:feature#{}", transform.feature_id?));
            let definition =
                unique_feature_definition_for_transform(&scan.features.definitions, transform)?;
            let sketch = model_sketch_id(scan, definition);
            let sketch_feature = section_owner_feature_id(scan, transform.definition_id, &sketch);
            exactly_one(
                ir.model
                    .features
                    .iter()
                    .filter(|feature| feature.id == sketch_feature),
            )
            .is_some()
            .then_some((owner, sketch_feature))
        })
        .collect::<Vec<_>>();
    for (owner, sketch_feature) in links {
        let Some(feature) = exactly_one(
            ir.model
                .features
                .iter_mut()
                .filter(|feature| feature.id == owner),
        ) else {
            continue;
        };
        if !feature.dependencies.contains(&sketch_feature) {
            feature.dependencies.push(sketch_feature);
        }
    }
}

pub(in super::super) fn surface_kind_for_geometry(
    geometry: &SurfaceGeometry,
) -> Option<crate::surface::SurfaceKind> {
    match geometry {
        SurfaceGeometry::Plane { .. } => Some(crate::surface::SurfaceKind::Plane),
        SurfaceGeometry::Cylinder { .. } => Some(crate::surface::SurfaceKind::Cylinder),
        SurfaceGeometry::Cone { .. } => Some(crate::surface::SurfaceKind::Cone),
        SurfaceGeometry::Sphere { .. } | SurfaceGeometry::Torus { .. } => {
            Some(crate::surface::SurfaceKind::TorusOrSphere)
        }
        SurfaceGeometry::Nurbs(_) => Some(crate::surface::SurfaceKind::Spline),
        SurfaceGeometry::Transformed { basis, .. } => surface_kind_for_geometry(basis),
        SurfaceGeometry::Polygonal(_)
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

pub(in super::super) fn generated_surface_id_for_feature(
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    source_entity_id: u32,
) -> Option<u32> {
    let mut matches = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| {
            table
                .entries
                .iter()
                .filter(|entry| {
                    entry.class_id == 200 && entry.source_entity_id == Some(source_entity_id)
                })
                .filter(|entry| table.surface_ids.contains(&entry.entity_id))
                .map(|entry| entry.entity_id)
        });
    let surface_id = matches.next()?;
    matches.next().is_none().then_some(surface_id)
}

pub(in super::super) fn generated_profile_entry_is_admissible(
    feature_id: u32,
    table: &crate::feature::FeatureEntityTable,
    entry: &crate::feature::FeatureEntityTableEntry,
    expected_kinds: &[crate::surface::SurfaceKind],
    rows: &[crate::surface::SurfaceRow],
) -> bool {
    if entry.class_id != 200 || entry.source_entity_id.is_none() {
        return false;
    }
    if table.surface_ids.contains(&entry.entity_id) {
        return crate::surface::unique_surface_row(rows, entry.entity_id)
            .is_some_and(|row| row.feature_id == feature_id && expected_kinds.contains(&row.kind));
    }
    table.non_surface_entity_ids.contains(&entry.entity_id)
        && generated_profile_table_shape(table)
        && table.entries.iter().skip(2).any(|candidate| {
            candidate.class_id == 200
                && table.surface_ids.contains(&candidate.entity_id)
                && crate::surface::unique_surface_row(rows, candidate.entity_id).is_some_and(
                    |row| row.feature_id == feature_id && expected_kinds.contains(&row.kind),
                )
        })
}

pub(in super::super) fn section_entity_is_generated_profile(
    segment_table_complete: bool,
    feature_id: Option<u32>,
    source_entity_id: u32,
    expected_kinds: &[crate::surface::SurfaceKind],
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> bool {
    if !segment_table_complete {
        return false;
    }
    let Some(feature_id) = feature_id else {
        return false;
    };
    let direct = generated_surface_id_for_feature(tables, feature_id, source_entity_id)
        .is_some_and(|surface_id| {
            crate::surface::unique_surface_row(rows, surface_id).is_some_and(|row| {
                row.feature_id == feature_id && expected_kinds.contains(&row.kind)
            })
        });
    if direct {
        return true;
    }
    let rowless_matches = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter_map(|table| {
            let matching = table
                .entries
                .iter()
                .filter(|entry| {
                    entry.class_id == 200 && entry.source_entity_id == Some(source_entity_id)
                })
                .collect::<Vec<_>>();
            let [entry] = matching.as_slice() else {
                return None;
            };
            (!table.surface_ids.contains(&entry.entity_id)
                && generated_profile_entry_is_admissible(
                    feature_id,
                    table,
                    entry,
                    expected_kinds,
                    rows,
                ))
            .then_some(())
        })
        .count();
    if rowless_matches == 1 {
        return true;
    }
    if !expected_kinds.contains(&crate::surface::SurfaceKind::Cylinder) {
        return false;
    }
    let mut blind_cylinders = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .filter_map(|table| {
            let [rowless_cap, cap, profile, cylinder] = table.entries.as_slice() else {
                return None;
            };
            ([
                rowless_cap.class_id,
                cap.class_id,
                profile.class_id,
                cylinder.class_id,
            ] == [204, 203, 200, 200]
                && profile.source_entity_id == Some(source_entity_id)
                && cylinder.source_entity_id.is_none()
                && table.surface_ids.contains(&cap.entity_id)
                && table.surface_ids.contains(&cylinder.entity_id)
                && table
                    .non_surface_entity_ids
                    .contains(&rowless_cap.entity_id)
                && table.non_surface_entity_ids.contains(&profile.entity_id)
                && crate::surface::unique_surface_row(rows, cylinder.entity_id).is_some_and(
                    |row| {
                        row.feature_id == feature_id
                            && row.kind == crate::surface::SurfaceKind::Cylinder
                    },
                ))
            .then_some(cylinder.entity_id)
        });
    blind_cylinders.next().is_some() && blind_cylinders.next().is_none()
}

fn generated_profile_table_shape(table: &crate::feature::FeatureEntityTable) -> bool {
    let [first, second, rest @ ..] = table.entries.as_slice() else {
        return false;
    };
    if table.table_class_id != 29
        || first.class_id != 204
        || second.class_id != 203
        || rest.is_empty()
        || !rest
            .iter()
            .all(|entry| entry.class_id == 200 && entry.source_entity_id.is_some())
    {
        return false;
    }
    let entry_ids = table
        .entries
        .iter()
        .map(|entry| entry.entity_id)
        .collect::<BTreeSet<_>>();
    let roster = table
        .surface_ids
        .iter()
        .chain(&table.non_surface_entity_ids)
        .copied()
        .collect::<BTreeSet<_>>();
    table.entry_ids.len() == entry_ids.len()
        && table.entry_ids.iter().copied().collect::<BTreeSet<_>>() == entry_ids
        && roster == entry_ids
        && table
            .surface_ids
            .iter()
            .all(|id| !table.non_surface_entity_ids.contains(id))
        && table.surface_ids.iter().collect::<BTreeSet<_>>().len() == table.surface_ids.len()
        && table
            .non_surface_entity_ids
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            == table.non_surface_entity_ids.len()
}

pub(in super::super) fn section_generated_profile_surface_kinds(
    geometry: &SketchGeometry,
) -> Option<&'static [crate::surface::SurfaceKind]> {
    match geometry {
        SketchGeometry::Line { .. } => Some(&[crate::surface::SurfaceKind::Plane]),
        SketchGeometry::Arc { .. } | SketchGeometry::Circle { .. } => {
            Some(&[crate::surface::SurfaceKind::Cylinder])
        }
        SketchGeometry::Nurbs { .. } => Some(&[
            crate::surface::SurfaceKind::Spline,
            crate::surface::SurfaceKind::Extrusion,
        ]),
        _ => None,
    }
}

pub(in super::super) fn ordered_analytic_surface_id_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    order: &crate::feature::FeatureOrderTable,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Option<u32> {
    order.internal_id(external_id)?;
    analytic_surface_id_for_feature(surface_rows, tables, feature_id, external_id, geometry)
}

pub(in super::super) fn analytic_surface_id_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    tables: &[crate::feature::FeatureEntityTable],
    feature_id: u32,
    external_id: u32,
    geometry: &SurfaceGeometry,
) -> Option<u32> {
    let surface_id = generated_surface_id_for_feature(tables, feature_id, external_id)?;
    let expected_kind = surface_kind_for_geometry(geometry)?;
    crate::surface::unique_surface_row(surface_rows, surface_id)
        .is_some_and(|row| row.feature_id == feature_id && row.kind == expected_kind)
        .then_some(surface_id)
}

pub(in super::super) fn ordered_family_surface_bindings_for_feature(
    surface_rows: &[crate::surface::SurfaceRow],
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    order: &crate::feature::FeatureOrderTable,
    external_ids: impl IntoIterator<Item = u32>,
    expected_kind: crate::surface::SurfaceKind,
) -> BTreeMap<u32, u32> {
    let mut bindings = BTreeMap::new();
    let mut bound_surfaces = BTreeSet::new();
    for external_id in external_ids {
        if order.internal_id(external_id).is_none() {
            return BTreeMap::new();
        }
        let Some(surface_id) = generated_surface_id_for_feature(tables, feature_id, external_id)
        else {
            return BTreeMap::new();
        };
        if !crate::surface::unique_surface_row(surface_rows, surface_id)
            .is_some_and(|row| row.feature_id == feature_id && row.kind == expected_kind)
            || !bound_surfaces.insert(surface_id)
        {
            return BTreeMap::new();
        }
        bindings.insert(external_id, surface_id);
    }
    bindings
}

pub(in super::super) fn profile_segment_ids(
    definition_id: u32,
    segments: &[crate::feature::FeatureSegment],
    profiles: &[Vec<SketchEntityUse>],
) -> BTreeSet<u32> {
    segments
        .iter()
        .filter(|segment| {
            let entity_id = SketchEntityId(format!(
                "creo:featdefs:sketch_entity#{definition_id}:{}",
                segment.external_id
            ));
            profiles
                .iter()
                .flatten()
                .any(|entity_use| entity_use.entity == entity_id)
        })
        .map(|segment| segment.external_id)
        .collect()
}

#[cfg(test)]
mod tests;
