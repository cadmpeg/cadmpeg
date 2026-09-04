// SPDX-License-Identifier: Apache-2.0
//! Feature output bodies, sweep kind, and native parameter maps.

use super::super::sketch_ids::model_sketch_id;
use super::super::sketch_transfer::{
    current_feature_operation, current_feature_recipe, feature_recipe, feature_row_schema_classes,
    feature_schema_class, unique_feature_revolution_extent_kind,
};
use super::super::uniqueness::{exactly_one, unique_feature_definition_for_transform};
use super::dependencies::feature_generated_dependencies;
use super::{agreed_feature_geometry_ids, feature_edge_selection, feature_is_sheet_extrusion};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{EdgeSelection, GeneratedEdgeRef};
use cadmpeg_ir::ids::{BodyId, EdgeId, SurfaceId};
use cadmpeg_ir::topology::BodyKind;
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn feature_output_bodies(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Vec<BodyId> {
    feature_output_bodies_with_history(scan, ir, feature_id, &mut BTreeSet::new())
}

fn feature_output_bodies_with_history(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    visiting: &mut BTreeSet<u32>,
) -> Vec<BodyId> {
    if !visiting.insert(feature_id) {
        return Vec::new();
    }
    let affected_geometry = agreed_feature_geometry_ids(
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    );
    let generated_surfaces = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .map(|row| {
            SurfaceId::mint(format!("creo:visibgeom:surface#{}", row.id)).expect("identity grammar")
        })
        .chain(
            scan.features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == Some(feature_id))
                .flat_map(|table| &table.surface_ids)
                .map(|surface_id| {
                    SurfaceId::mint(format!("creo:visibgeom:surface#{surface_id}"))
                        .expect("identity grammar")
                }),
        )
        .chain(affected_geometry.into_iter().flatten().map(|surface_id| {
            SurfaceId::mint(format!("creo:visibgeom:surface#{surface_id}"))
                .expect("identity grammar")
        }));
    let mut outputs = evaluated_sweep_output_bodies(ir, feature_id);
    let edge_outputs = match feature_edge_selection(scan, ir, feature_id) {
        Some(EdgeSelection::Resolved { edges, .. }) => bodies_containing_edges(ir, &edges),
        Some(EdgeSelection::Generated { edges, .. }) => {
            generated_edge_output_bodies(scan, ir, &edges, visiting)
        }
        _ => Vec::new(),
    };
    let generated_input_outputs = generated_input_output_bodies(scan, ir, feature_id, visiting);
    for surface in generated_surfaces {
        for face in ir.model.faces.iter().filter(|face| face.surface == surface) {
            let Some(shell) = exactly_one(
                ir.model
                    .shells
                    .iter()
                    .filter(|shell| shell.id == face.shell),
            ) else {
                continue;
            };
            let Some(region) = exactly_one(
                ir.model
                    .regions
                    .iter()
                    .filter(|region| region.id == shell.region),
            ) else {
                continue;
            };
            if !outputs.contains(&region.body) {
                outputs.push(region.body.clone());
            }
        }
    }
    for body in edge_outputs.into_iter().chain(generated_input_outputs) {
        if !outputs.contains(&body) {
            outputs.push(body);
        }
    }
    visiting.remove(&feature_id);
    outputs
}

fn generated_input_output_bodies(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    visiting: &mut BTreeSet<u32>,
) -> Vec<BodyId> {
    let feature_id_text = format!("creo:model:feature#{feature_id}");
    let Some(feature) = exactly_one(
        ir.model
            .features
            .iter()
            .filter(|feature| feature.id.as_str() == feature_id_text),
    ) else {
        return Vec::new();
    };
    feature_generated_dependencies(&feature.definition)
        .into_iter()
        .filter_map(|producer| {
            producer
                .as_str()
                .strip_prefix("creo:model:feature#")
                .and_then(|value| value.parse::<u32>().ok())
        })
        .flat_map(|producer_id| feature_output_bodies_with_history(scan, ir, producer_id, visiting))
        .fold(Vec::new(), |mut outputs, body| {
            if !outputs.contains(&body) {
                outputs.push(body);
            }
            outputs
        })
}

fn generated_edge_output_bodies(
    scan: &ContainerScan,
    ir: &CadIr,
    edges: &[GeneratedEdgeRef],
    visiting: &mut BTreeSet<u32>,
) -> Vec<BodyId> {
    let mut outputs = Vec::new();
    for edge in edges {
        let Some(producer_id) = edge
            .feature
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        for body in feature_output_bodies_with_history(scan, ir, producer_id, visiting) {
            if !outputs.contains(&body) {
                outputs.push(body);
            }
        }
    }
    outputs
}

pub(in super::super) fn bodies_containing_edges(ir: &CadIr, edges: &[EdgeId]) -> Vec<BodyId> {
    let selected = edges.iter().collect::<BTreeSet<_>>();
    let mut shell_ids = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| selected.contains(&coedge.edge))
        .filter_map(|coedge| {
            let lp = exactly_one(
                ir.model
                    .loops
                    .iter()
                    .filter(|lp| lp.id == coedge.owner_loop),
            )?;
            exactly_one(ir.model.faces.iter().filter(|face| face.id == lp.face))
                .map(|face| face.shell.clone())
        })
        .collect::<BTreeSet<_>>();
    shell_ids.extend(
        ir.model
            .shells
            .iter()
            .filter(|shell| shell.wire_edges.iter().any(|edge| selected.contains(edge)))
            .map(|shell| shell.id.clone()),
    );
    shell_ids
        .into_iter()
        .filter_map(|shell_id| {
            let shell = exactly_one(ir.model.shells.iter().filter(|shell| shell.id == shell_id))?;
            let region = exactly_one(
                ir.model
                    .regions
                    .iter()
                    .filter(|region| region.id == shell.region),
            )?;
            exactly_one(ir.model.bodies.iter().filter(|body| body.id == region.body))
                .is_some()
                .then(|| region.body.clone())
        })
        .fold(Vec::new(), |mut bodies, body| {
            if !bodies.contains(&body) {
                bodies.push(body);
            }
            bodies
        })
}

pub(in super::super) fn evaluated_sweep_output_bodies(ir: &CadIr, feature_id: u32) -> Vec<BodyId> {
    ["extrusion", "revolution"]
        .into_iter()
        .map(|family| {
            BodyId::mint(format!("creo:feature:{family}#{feature_id}:body"))
                .expect("identity grammar")
        })
        .filter(|id| exactly_one(ir.model.bodies.iter().filter(|body| body.id == *id)).is_some())
        .collect()
}

pub(in super::super) fn evaluated_sweep_body_kind(
    ir: &CadIr,
    family: &str,
    feature_id: u32,
) -> Option<BodyKind> {
    let id =
        BodyId::mint(format!("creo:feature:{family}#{feature_id}:body")).expect("identity grammar");
    exactly_one(ir.model.bodies.iter().filter(|body| body.id == id)).map(|body| body.kind)
}

pub(in super::super) fn new_sheet_output_surface_id(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Option<u32> {
    let owned = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let unique_table = |class_id| {
        let mut matches = owned
            .iter()
            .copied()
            .filter(|table| table.table_class_id == class_id);
        let table = matches.next()?;
        matches.next().is_none().then_some(table)
    };
    let [owner] = unique_table(67)?.entries.as_slice() else {
        return None;
    };
    let [output] = unique_table(100)?.entries.as_slice() else {
        return None;
    };
    let generated = unique_table(29)?;
    (owner.class_id == 200
        && owner.source_entity_id == Some(feature_id)
        && output.entity_id == owner.entity_id
        && generated.surface_ids.contains(&output.class_id)
        && generated
            .entries
            .iter()
            .any(|entry| entry.entity_id == output.class_id && entry.class_id == 200))
    .then_some(())?;
    let mut surfaces = surface_rows
        .iter()
        .filter(|row| row.id == output.class_id && row.feature_id == feature_id);
    let surface = surfaces.next()?;
    surfaces.next().is_none().then_some(surface.id)
}

pub(in super::super) fn sweep_output_kind(
    scan: &ContainerScan,
    ir: &CadIr,
    family: &str,
    feature_id: u32,
) -> Option<BodyKind> {
    evaluated_sweep_body_kind(ir, family, feature_id).or_else(|| {
        feature_is_sheet_extrusion(scan, feature_id).then_some(())?;
        new_sheet_output_surface_id(
            feature_id,
            &scan.features.entity_tables,
            &scan.surfaces.rows,
        )
        .map(|_| BodyKind::Sheet)
        .or_else(|| {
            current_feature_operation(&scan.features.operations, feature_id)
                .filter(|operation| operation.kind == "Surface")
                .map(|_| BodyKind::Sheet)
        })
    })
}

pub(in super::super) fn sweep_solid(output_kind: Option<BodyKind>) -> Option<bool> {
    output_kind.map(|kind| kind == BodyKind::Solid)
}

pub(in super::super) fn feature_field_text(
    value: &crate::feature::FeatureFieldValue,
) -> Option<String> {
    match value {
        crate::feature::FeatureFieldValue::Empty => Some("empty".to_string()),
        crate::feature::FeatureFieldValue::CompactInt(value) => Some(value.to_string()),
        crate::feature::FeatureFieldValue::CompactIntArray(values) => Some(
            values
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        crate::feature::FeatureFieldValue::EntityReference {
            entity_id,
            terminated,
        } => Some(format!(
            "entity:{entity_id}{}",
            if *terminated { ":terminated" } else { "" }
        )),
        crate::feature::FeatureFieldValue::ScalarArray {
            decoded_values: Some(values),
            ..
        } => Some(
            values
                .iter()
                .map(f64::to_string)
                .collect::<Vec<_>>()
                .join(","),
        ),
        crate::feature::FeatureFieldValue::ScalarArray {
            decoded_values: None,
            ..
        }
        | crate::feature::FeatureFieldValue::Raw(_) => None,
    }
}

pub(in super::super) fn insert_feature_parameter(
    parameters: &mut BTreeMap<String, String>,
    base: &str,
    value: String,
) {
    if let std::collections::btree_map::Entry::Vacant(entry) = parameters.entry(base.to_string()) {
        entry.insert(value);
        return;
    }
    let mut occurrence = 2;
    loop {
        let name = format!("{base}#{occurrence}");
        if let std::collections::btree_map::Entry::Vacant(entry) = parameters.entry(name) {
            entry.insert(value);
            return;
        }
        occurrence += 1;
    }
}

pub(in super::super) fn feature_parameters(
    scan: &ContainerScan,
    feature_id: u32,
) -> BTreeMap<String, String> {
    let mut parameters = BTreeMap::new();
    for field in scan
        .features
        .choice_fields
        .iter()
        .filter(|field| field.feature_id == feature_id)
    {
        let Some(value) = feature_field_text(&field.value) else {
            continue;
        };
        insert_feature_parameter(
            &mut parameters,
            &format!("choice.{}.{}", field.choice_label, field.name),
            value,
        );
    }
    for affected in scan
        .features
        .affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match affected.kind {
            crate::feature::AffectedIdKind::Geometry => "affected_geometry_ids",
            crate::feature::AffectedIdKind::Edges => "affected_edge_ids",
            crate::feature::AffectedIdKind::StrongParents => "strong_parent_feature_ids",
            crate::feature::AffectedIdKind::Parents => "parent_feature_ids",
            crate::feature::AffectedIdKind::Contours => "contour_ids",
            crate::feature::AffectedIdKind::Quilts => "affected_quilt_ids",
        };
        insert_feature_parameter(
            &mut parameters,
            name,
            affected
                .ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    for affected in scan
        .features
        .replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_geometry_ids",
            affected
                .geometry_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_affected_edge_ids",
            affected
                .edge_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_geometry_extent",
            match affected.geometry_extent {
                crate::feature::ReplayExtentSource::Explicit => "explicit",
                crate::feature::ReplayExtentSource::Inherited => "inherited",
            }
            .to_string(),
        );
        insert_feature_parameter(
            &mut parameters,
            "replay_edge_extent",
            match affected.edge_extent {
                crate::feature::ReplayExtentSource::Explicit => "explicit",
                crate::feature::ReplayExtentSource::Inherited => "inherited",
            }
            .to_string(),
        );
    }
    for affected in scan
        .features
        .surface_merge_replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        for (name, ids) in [
            (
                "surface_merge_replay_affected_geometry_ids",
                &affected.geometry_ids,
            ),
            ("surface_merge_replay_affected_edge_ids", &affected.edge_ids),
            (
                "surface_merge_replay_affected_quilt_ids",
                &affected.quilt_ids,
            ),
        ] {
            insert_feature_parameter(
                &mut parameters,
                name,
                ids.iter().map(u32::to_string).collect::<Vec<_>>().join(","),
            );
        }
        for (name, extent) in [
            (
                "surface_merge_replay_geometry_extent",
                affected.geometry_extent,
            ),
            ("surface_merge_replay_edge_extent", affected.edge_extent),
            ("surface_merge_replay_quilt_extent", affected.quilt_extent),
        ] {
            insert_feature_parameter(
                &mut parameters,
                name,
                match extent {
                    crate::feature::ReplayExtentSource::Explicit => "explicit",
                    crate::feature::ReplayExtentSource::Inherited => "inherited",
                }
                .to_string(),
            );
        }
    }
    for direction in scan
        .features
        .loop_restore_directions
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match direction.lane {
            crate::feature::LoopRestoreDirectionLane::Primary => "direction",
            crate::feature::LoopRestoreDirectionLane::Secondary => "direction2",
        };
        insert_feature_parameter(
            &mut parameters,
            &format!("loop_restore.{name}"),
            direction.value.to_string(),
        );
    }
    if let Some(extent) =
        unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id)
    {
        parameters.insert(
            "revolution_extent".to_string(),
            match extent {
                crate::feature::FeatureRevolutionExtentKind::FullTurn => "full_turn",
            }
            .to_string(),
        );
    }
    for table in scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
    {
        for entry in &table.entries {
            let Some(source_entity_id) = entry.source_entity_id else {
                continue;
            };
            insert_feature_parameter(
                &mut parameters,
                &format!(
                    "generated_entity.{}.source_section_entity_id",
                    entry.entity_id
                ),
                source_entity_id.to_string(),
            );
            insert_feature_parameter(
                &mut parameters,
                &format!("generated_entity.{}.entry_class", entry.entity_id),
                entry.class_id.to_string(),
            );
        }
    }
    let owned_definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.owner_feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    if let [definition] = owned_definitions.as_slice() {
        parameters.insert(
            "sketch_segment_count".to_string(),
            definition
                .segments
                .as_ref()
                .map_or(0, |segments| segments.rows.len())
                .to_string(),
        );
        parameters.insert(
            "dimension_count".to_string(),
            definition
                .dimensions
                .as_ref()
                .map_or(0, |dimensions| dimensions.rows.len())
                .to_string(),
        );
    }
    for transform in scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
    {
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        insert_feature_parameter(
            &mut parameters,
            "profile_sketch",
            model_sketch_id(scan, definition).0,
        );
        if feature_recipe(scan, feature_id) == Some(crate::feature::FeatureRecipeKind::Extrude) {
            insert_feature_parameter(
                &mut parameters,
                "sweep_direction",
                transform
                    .normal
                    .iter()
                    .map(f64::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
    }
    parameters
}

pub(in super::super) fn schema_operation_kind(schema_class: u32) -> Option<&'static str> {
    match schema_class {
        911 => Some("Hole"),
        913 => Some("Round"),
        914 => Some("Chamfer"),
        916 => Some("Cut"),
        917 => Some("Protrusion"),
        923 => Some("Datum Plane"),
        926 => Some("Section"),
        927 => Some("Draft"),
        946 => Some("Surface Merge"),
        _ => None,
    }
}

pub(in super::super) fn feature_reference_name<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<&'a str> {
    let mut records = scan
        .features
        .reference_names
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let record = records.next()?;
    records
        .all(|candidate| candidate.name_bytes.as_slice() == record.name_bytes.as_slice())
        .then_some(record.name.as_str())
}

pub(in super::super) fn owned_section_feature_id(
    scan: &ContainerScan,
    definition_id: u32,
) -> Option<u32> {
    let definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.id == definition_id)
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    let rows = scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.root_schema_class == Some(926)
                && definition.offset >= row.body_offset
                && definition.offset < row.body_offset.saturating_add(row.body.len())
        })
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    Some(row.feature_id)
}

pub(in super::super) fn section_definition_for_history_feature<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<&'a crate::feature::FeatureDefinition> {
    let rows = scan
        .features
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id && row.root_schema_class == Some(926))
        .collect::<Vec<_>>();
    let [row] = rows.as_slice() else {
        return None;
    };
    let definitions = scan
        .features
        .definitions
        .iter()
        .filter(|definition| {
            definition.offset >= row.body_offset
                && definition.offset < row.body_offset.saturating_add(row.body.len())
        })
        .collect::<Vec<_>>();
    let [definition] = definitions.as_slice() else {
        return None;
    };
    Some(*definition)
}

pub(in super::super) fn feature_source_properties(
    scan: &ContainerScan,
    feature_id: u32,
) -> BTreeMap<String, String> {
    let mut properties = BTreeMap::new();
    if let Some(recipe) = current_feature_recipe(&scan.features.operations, feature_id) {
        properties.insert("recipe".to_string(), recipe.name().to_string());
    }
    let schema_class = feature_schema_class(scan, feature_id);
    if let Some(schema_class) = schema_class {
        properties.insert(
            "featdefs_schema_class".to_string(),
            schema_class.to_string(),
        );
    }
    let row_schema_classes = feature_row_schema_classes(scan, feature_id);
    if !row_schema_classes.is_empty() {
        properties.insert(
            "featdefs_row_schema_classes".to_string(),
            row_schema_classes
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    if schema_class.is_none() && !row_schema_classes.is_empty() {
        properties.insert("featdefs_schema_state".to_string(), "ambiguous".to_string());
    }
    properties
}

#[cfg(test)]
mod tests;
