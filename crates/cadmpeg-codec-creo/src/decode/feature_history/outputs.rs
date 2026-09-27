// SPDX-License-Identifier: Apache-2.0
//! Feature output bodies, sweep kind, and native parameter maps.

use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::{exactly_one, unique_feature_definition_for_transform};
use super::dependencies::feature_generated_dependencies;
use super::draft::feature_is_sheet_extrusion;
use super::selections::{agreed_feature_geometry_ids, feature_edge_selection};
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::recipe::{
    current_feature_operation, current_feature_recipe, feature_recipe, feature_row_schema_classes,
    feature_schema_class, unique_feature_revolution_extent,
};
use crate::feature::schema::SchemaClass;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{EdgeSelection, GeneratedEdgeRef};
use cadmpeg_ir::ids::{BodyId, EdgeId};
use cadmpeg_ir::topology::BodyKind;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn copy_body_id(ctx: &DecodeContext<'_>, body: &BodyId) -> Result<BodyId, CodecError> {
    BodyId::mint(ctx.copy_retained_text(body.as_str(), "creo feature output body IDs")?)
        .map_err(CodecError::malformed)
}

pub(in super::super) fn feature_output_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Vec<BodyId>, CodecError> {
    feature_output_bodies_with_history(ctx, scan, ir, feature_id, &mut BTreeSet::new())
}

fn feature_output_bodies_with_history(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    visiting: &mut BTreeSet<u32>,
) -> Result<Vec<BodyId>, CodecError> {
    let _depth = ctx.enter_nested("creo feature output history")?;
    if visiting.contains(&feature_id) {
        return Ok(Vec::new());
    }
    ctx.charge_collection_items(1, "creo feature output visiting nodes")?;
    visiting.insert(feature_id);
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
        .map(|row| row.id)
        .chain(
            scan.features
                .entity_tables
                .iter()
                .filter(|table| table.feature_id == feature_id)
                .flat_map(crate::feature::entity::FeatureEntityTable::surface_ids_iter),
        )
        .chain(affected_geometry.into_iter().flatten().copied());
    let mut outputs = evaluated_sweep_output_bodies(ctx, ir, feature_id)?;
    let edge_outputs = match feature_edge_selection(ctx, scan, ir, feature_id)? {
        Some(EdgeSelection::Resolved { edges, .. }) => bodies_containing_edges(ctx, ir, &edges)?,
        Some(EdgeSelection::Generated { edges, .. }) => {
            generated_edge_output_bodies(ctx, scan, ir, &edges, visiting)?
        }
        _ => Vec::new(),
    };
    let generated_input_outputs = generated_input_output_bodies(ctx, scan, ir, feature_id, visiting)?;
    for surface_id in generated_surfaces {
        let (surface, _reservation) = ctx.format_scoped(
            format_args!("creo:visibgeom:surface#{surface_id}"),
            "creo generated surface lookup",
        )?;
        for face in ir.model.faces.iter().filter(|face| face.surface.as_str() == surface) {
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
                let body = copy_body_id(ctx, &region.body)?;
                ctx.try_reserve_items(&mut outputs, 1, "creo feature output bodies")?;
                outputs.push(body);
            }
        }
    }
    for body in edge_outputs.into_iter().chain(generated_input_outputs) {
        if !outputs.contains(&body) {
            ctx.try_reserve_items(&mut outputs, 1, "creo feature output bodies")?;
            outputs.push(body);
        }
    }
    visiting.remove(&feature_id);
    Ok(outputs)
}

fn generated_input_output_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    visiting: &mut BTreeSet<u32>,
) -> Result<Vec<BodyId>, CodecError> {
    let (feature_id_text, reservation) = ctx.format_scoped(
        format_args!("creo:model:feature#{feature_id}"),
        "creo generated input feature lookup",
    )?;
    let Some(feature) = exactly_one(
        ir.model
            .features
            .iter()
            .filter(|feature| feature.id.as_str() == feature_id_text),
    ) else {
        return Ok(Vec::new());
    };
    drop(feature_id_text);
    drop(reservation);
    let mut outputs = Vec::new();
    for producer in feature_generated_dependencies(ctx, feature.evaluation.definition())? {
        let Some(producer_id) = producer
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        for body in feature_output_bodies_with_history(ctx, scan, ir, producer_id, visiting)? {
            if !outputs.contains(&body) {
                ctx.try_reserve_items(&mut outputs, 1, "creo generated input output bodies")?;
                outputs.push(body);
            }
        }
    }
    Ok(outputs)
}

fn generated_edge_output_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    edges: &[GeneratedEdgeRef],
    visiting: &mut BTreeSet<u32>,
) -> Result<Vec<BodyId>, CodecError> {
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
        for body in feature_output_bodies_with_history(ctx, scan, ir, producer_id, visiting)? {
            if !outputs.contains(&body) {
                ctx.try_reserve_items(&mut outputs, 1, "creo generated edge output bodies")?;
                outputs.push(body);
            }
        }
    }
    Ok(outputs)
}

fn bodies_containing_edges(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    edges: &[EdgeId],
) -> Result<Vec<BodyId>, CodecError> {
    let mut selected = BTreeSet::new();
    for edge in edges {
        if !selected.contains(edge) {
            ctx.charge_collection_items(1, "creo selected edge nodes")?;
            selected.insert(edge);
        }
    }
    let mut shell_ids = BTreeSet::new();
    for coedge in ir.model.coedges.iter().filter(|coedge| selected.contains(&coedge.edge)) {
            let lp = exactly_one(
                ir.model
                    .loops
                    .iter()
                    .filter(|lp| lp.id == coedge.owner_loop),
            );
            let Some(face) = lp.and_then(|lp| exactly_one(ir.model.faces.iter().filter(|face| face.id == lp.face))) else {
                continue;
            };
            if !shell_ids.contains(&face.shell) {
                ctx.charge_collection_items(1, "creo selected shell nodes")?;
                shell_ids.insert(&face.shell);
            }
    }
    for shell in ir.model.shells.iter().filter(|shell| {
        shell.wire_edges().iter().any(|edge| selected.contains(edge))
    }) {
        if !shell_ids.contains(&shell.id) {
            ctx.charge_collection_items(1, "creo selected shell nodes")?;
            shell_ids.insert(&shell.id);
        }
    }
    let mut bodies = Vec::new();
    for shell_id in shell_ids {
            let Some(shell) = exactly_one(ir.model.shells.iter().filter(|shell| shell.id == *shell_id)) else {
                continue;
            };
            let region = exactly_one(
                ir.model
                    .regions
                    .iter()
                    .filter(|region| region.id == shell.region),
            );
            let Some(region) = region.filter(|region| exactly_one(ir.model.bodies.iter().filter(|body| body.id == region.body)).is_some()) else {
                continue;
            };
            let body = copy_body_id(ctx, &region.body)?;
            if !bodies.contains(&body) {
                ctx.try_reserve_items(&mut bodies, 1, "creo bodies containing selected edges")?;
                bodies.push(body);
            }
    }
    Ok(bodies)
}

pub(in super::super) fn evaluated_sweep_output_bodies(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Vec<BodyId>, CodecError> {
    let mut outputs = Vec::new();
    for namespace in [
        &crate::identity::FEATURE_EXTRUSION,
        &crate::identity::FEATURE_REVOLUTION,
    ] {
        let (candidate, _reservation) = ctx.format_scoped(
            format_args!(
                "{}:{}:{}#{feature_id}:body",
                namespace.format(),
                namespace.scope(),
                namespace.kind(),
            ),
            "creo evaluated sweep body candidate",
        )?;
        if exactly_one(ir.model.bodies.iter().filter(|body| body.id.as_str() == candidate)).is_some() {
            ctx.charge_retained(candidate.len() as u64, "creo evaluated sweep body IDs")?;
            let body = BodyId::mint(candidate).map_err(CodecError::malformed)?;
            ctx.try_reserve_items(&mut outputs, 1, "creo evaluated sweep output bodies")?;
            outputs.push(body);
        }
    }
    Ok(outputs)
}

pub(in super::super) fn evaluated_sweep_body_kind(
    ir: &CadIr,
    family: &str,
    feature_id: u32,
) -> Option<BodyKind> {
    let namespace = match family {
        "extrusion" => &crate::identity::FEATURE_EXTRUSION,
        "revolution" => &crate::identity::FEATURE_REVOLUTION,
        _ => return None,
    };
    let id = BodyId::compose(
        namespace,
        cadmpeg_ir::ids::IdentityKey::from(feature_id).colon(cadmpeg_ir::identity_key!("body")),
    );
    exactly_one(ir.model.bodies.iter().filter(|body| body.id == id)).map(|body| body.kind)
}

pub(in super::super) fn new_sheet_output_surface_id(
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Option<u32> {
    let owned = tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
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
    (owner.source_entity_id() == Some(feature_id)
        && output.entity_id == owner.entity_id
        && generated.contains_surface_id(output.class_id())
        && generated
            .entries
            .iter()
            .any(|entry| entry.entity_id == output.class_id() && entry.class_id() == 200))
    .then_some(())?;
    let mut surfaces = surface_rows
        .iter()
        .filter(|row| row.id == output.class_id() && row.feature_id == feature_id);
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
                .filter(|operation| operation.kind.as_str() == "Surface")
                .map(|_| BodyKind::Sheet)
        })
    })
}

pub(super) fn sweep_solid(output_kind: Option<BodyKind>) -> Option<bool> {
    output_kind.map(|kind| kind == BodyKind::Solid)
}

struct CommaList<'a, T>(&'a [T]);

impl<T: std::fmt::Display> std::fmt::Display for CommaList<'_, T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, value) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(",")?;
            }
            write!(formatter, "{value}")?;
        }
        Ok(())
    }
}

enum FeatureFieldText<'a> {
    Empty,
    CompactInt(u32),
    CompactIntArray(&'a [u32]),
    EntityReference(u32, bool),
    ScalarArray(&'a [f64]),
}

impl std::fmt::Display for FeatureFieldText<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => formatter.write_str("empty"),
            Self::CompactInt(value) => write!(formatter, "{value}"),
            Self::CompactIntArray(values) => std::fmt::Display::fmt(&CommaList(values), formatter),
            Self::EntityReference(entity_id, terminated) => write!(
                formatter,
                "entity:{entity_id}{}",
                if *terminated { ":terminated" } else { "" }
            ),
            Self::ScalarArray(values) => std::fmt::Display::fmt(&CommaList(values), formatter),
        }
    }
}

fn feature_field_text(value: &crate::feature::rows::FeatureFieldValue) -> Option<FeatureFieldText<'_>> {
    match value {
        crate::feature::rows::FeatureFieldValue::Empty => Some(FeatureFieldText::Empty),
        crate::feature::rows::FeatureFieldValue::CompactInt(value) => Some(FeatureFieldText::CompactInt(*value)),
        crate::feature::rows::FeatureFieldValue::CompactIntArray(values) => Some(FeatureFieldText::CompactIntArray(values)),
        crate::feature::rows::FeatureFieldValue::EntityReference {
            entity_id,
            terminated,
        } => Some(FeatureFieldText::EntityReference(*entity_id, *terminated)),
        crate::feature::rows::FeatureFieldValue::ScalarArray {
            decoded_values: Some(values),
            ..
        } => Some(FeatureFieldText::ScalarArray(values)),
        crate::feature::rows::FeatureFieldValue::ScalarArray {
            decoded_values: None,
            ..
        }
        | crate::feature::rows::FeatureFieldValue::Raw(_) => None,
    }
}

fn insert_feature_parameter(
    ctx: &DecodeContext<'_>,
    parameters: &mut BTreeMap<String, String>,
    base: impl std::fmt::Display,
    value: impl std::fmt::Display,
) -> Result<(), CodecError> {
    let value = ctx.format_retained(value, "creo feature parameter value")?;
    let (base, base_reservation) = ctx.format_scoped(base, "creo feature parameter key candidate")?;
    let (key, key_reservation) = if parameters.contains_key(&base) {
        let mut occurrence = 2usize;
        loop {
            let candidate = ctx.format_scoped(
                format_args!("{base}#{occurrence}"),
                "creo feature parameter key candidate",
            )?;
            if !parameters.contains_key(&candidate.0) {
                break candidate;
            }
            occurrence += 1;
        }
    } else {
        (base, base_reservation)
    };
    ctx.charge_retained(key.len() as u64, "creo feature parameter key")?;
    ctx.charge_collection_items(1, "creo feature parameter nodes")?;
    parameters.insert(key, value);
    drop(key_reservation);
    Ok(())
}

fn replace_feature_parameter(
    ctx: &DecodeContext<'_>,
    parameters: &mut BTreeMap<String, String>,
    key: &'static str,
    value: impl std::fmt::Display,
) -> Result<(), CodecError> {
    let value = ctx.format_retained(value, "creo feature parameter value")?;
    if let Some(existing) = parameters.get_mut(key) {
        *existing = value;
    } else {
        ctx.charge_collection_items(1, "creo feature parameter nodes")?;
        let key = ctx.copy_retained_text(key, "creo feature parameter key")?;
        parameters.insert(key, value);
    }
    Ok(())
}

pub(in super::super) fn feature_parameters(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<BTreeMap<String, String>, CodecError> {
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
            ctx,
            &mut parameters,
            format_args!("choice.{}.{}", field.choice_label, field.name),
            value,
        )?;
    }
    for affected in scan
        .features
        .affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match affected.kind {
            crate::feature::rows::AffectedIdKind::Geometry => "affected_geometry_ids",
            crate::feature::rows::AffectedIdKind::Edges => "affected_edge_ids",
            crate::feature::rows::AffectedIdKind::StrongParents => "strong_parent_feature_ids",
            crate::feature::rows::AffectedIdKind::Parents => "parent_feature_ids",
            crate::feature::rows::AffectedIdKind::Contours => "contour_ids",
            crate::feature::rows::AffectedIdKind::Quilts => "affected_quilt_ids",
        };
        insert_feature_parameter(
            ctx,
            &mut parameters,
            name,
            CommaList(&affected.ids),
        )?;
    }
    for affected in scan
        .features
        .replay_affected_ids
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        insert_feature_parameter(
            ctx,
            &mut parameters,
            "replay_affected_geometry_ids",
            CommaList(&affected.geometry_ids),
        )?;
        insert_feature_parameter(
            ctx,
            &mut parameters,
            "replay_affected_edge_ids",
            CommaList(&affected.edge_ids),
        )?;
        insert_feature_parameter(
            ctx,
            &mut parameters,
            "replay_geometry_extent",
            match affected.geometry_extent {
                crate::feature::rows::ReplayExtentSource::Explicit => "explicit",
                crate::feature::rows::ReplayExtentSource::Inherited => "inherited",
            },
        )?;
        insert_feature_parameter(
            ctx,
            &mut parameters,
            "replay_edge_extent",
            match affected.edge_extent {
                crate::feature::rows::ReplayExtentSource::Explicit => "explicit",
                crate::feature::rows::ReplayExtentSource::Inherited => "inherited",
            },
        )?;
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
                ctx,
                &mut parameters,
                name,
                CommaList(ids),
            )?;
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
                ctx,
                &mut parameters,
                name,
                match extent {
                    crate::feature::rows::ReplayExtentSource::Explicit => "explicit",
                    crate::feature::rows::ReplayExtentSource::Inherited => "inherited",
                },
            )?;
        }
    }
    for direction in scan
        .features
        .loop_restore_directions
        .iter()
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match direction.lane {
            crate::feature::rows::LoopRestoreDirectionLane::Primary => "direction",
            crate::feature::rows::LoopRestoreDirectionLane::Secondary => "direction2",
        };
        insert_feature_parameter(
            ctx,
            &mut parameters,
            format_args!("loop_restore.{name}"),
            direction.value,
        )?;
    }
    if unique_feature_revolution_extent(&scan.features.revolution_extents, feature_id).is_some() {
        replace_feature_parameter(ctx, &mut parameters, "revolution_extent", "full_turn")?;
    }
    for table in scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id)
    {
        for entry in &table.entries {
            let Some(source_entity_id) = entry.source_entity_id() else {
                continue;
            };
            insert_feature_parameter(
                ctx,
                &mut parameters,
                format_args!(
                    "generated_entity.{}.source_section_entity_id",
                    entry.entity_id
                ),
                source_entity_id,
            )?;
            insert_feature_parameter(
                ctx,
                &mut parameters,
                format_args!("generated_entity.{}.entry_class", entry.entity_id),
                entry.class_id(),
            )?;
        }
    }
    if let Some(definition) = exactly_one(
        scan.features.definitions.iter()
            .filter(|definition| definition.identity.owner_feature_id() == Some(feature_id)),
    ) {
        replace_feature_parameter(
            ctx,
            &mut parameters,
            "sketch_segment_count",
            definition.segments.as_ref().map_or(0, |segments| segments.rows.ordinary().count()),
        )?;
        replace_feature_parameter(
            ctx,
            &mut parameters,
            "dimension_count",
            definition.dimensions.as_ref().map_or(0, |dimensions| dimensions.rows.len()),
        )?;
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
        let Some(profile_sketch) = model_sketch_id(scan, definition) else {
            continue;
        };
        insert_feature_parameter(
            ctx,
            &mut parameters,
            "profile_sketch",
            profile_sketch.as_str(),
        )?;
        if feature_recipe(scan, feature_id)
            == Some(crate::feature::operations::FeatureRecipeKind::Extrude)
        {
            insert_feature_parameter(
                ctx,
                &mut parameters,
                "sweep_direction",
                CommaList(&transform.normal()),
            )?;
        }
    }
    Ok(parameters)
}

pub(in super::super) fn schema_operation_kind(schema_class: SchemaClass) -> Option<&'static str> {
    match schema_class {
        SchemaClass::Hole => Some("Hole"),
        SchemaClass::Round => Some("Round"),
        SchemaClass::Chamfer => Some("Chamfer"),
        SchemaClass::Cut => Some("Cut"),
        SchemaClass::Protrusion => Some("Protrusion"),
        SchemaClass::DatumPlane => Some("Datum Plane"),
        SchemaClass::Section => Some("Section"),
        SchemaClass::Draft => Some("Draft"),
        SchemaClass::SurfaceMerge => Some("Surface Merge"),
        _ => None,
    }
}

pub(in super::super) fn feature_reference_name<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<Cow<'a, str>> {
    let mut records = scan
        .features
        .reference_names
        .iter()
        .filter(|record| record.feature_id == feature_id);
    let record = records.next()?;
    records
        .all(|candidate| candidate.name_bytes.as_slice() == record.name_bytes.as_slice())
        .then(|| record.name())
}

pub(in super::super) fn owned_section_feature_id(
    scan: &ContainerScan,
    definition_id: u32,
) -> Option<u32> {
    let definition = exactly_one(scan
        .features
        .definitions
        .iter()
        .filter(|definition| definition.identity.id() == definition_id))?;
    let row = exactly_one(scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.root_schema_class == Some(SchemaClass::Section)
                && definition.offset >= row.body_offset
                && definition.offset < row.body_offset.saturating_add(row.body.len())
        }))?;
    Some(row.feature_id)
}

pub(super) fn section_definition_for_history_feature<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<&'a crate::feature::definitions::FeatureDefinition> {
    let row = exactly_one(scan
        .features
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.root_schema_class == Some(SchemaClass::Section)
        }))?;
    let definition = exactly_one(scan
        .features
        .definitions
        .iter()
        .filter(|definition| {
            definition.offset >= row.body_offset
                && definition.offset < row.body_offset.saturating_add(row.body.len())
        }))?;
    Some(definition)
}

pub(in super::super) fn feature_source_properties(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut properties = BTreeMap::new();
    if let Some(recipe) = current_feature_recipe(&scan.features.operations, feature_id) {
        insert_feature_source_property(ctx, &mut properties, "recipe", recipe.name())?;
    }
    let schema_class = feature_schema_class(scan, feature_id);
    if let Some(schema_class) = schema_class {
        insert_feature_source_property(ctx, &mut properties, "featdefs_schema_class", schema_class)?;
    }
    let row_schema_classes = feature_row_schema_classes(ctx, scan, feature_id)?;
    if !row_schema_classes.is_empty() {
        insert_feature_source_property(ctx, &mut properties, "featdefs_row_schema_classes", SchemaClassList(&row_schema_classes))?;
    }
    if schema_class.is_none() && !row_schema_classes.is_empty() {
        insert_feature_source_property(ctx, &mut properties, "featdefs_schema_state", "ambiguous")?;
    }
    Ok(properties)
}

pub(in super::super) struct SchemaClassList<'a>(pub &'a BTreeSet<SchemaClass>);

impl std::fmt::Display for SchemaClassList<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, schema_class) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(",")?;
            }
            write!(formatter, "{schema_class}")?;
        }
        Ok(())
    }
}

pub(in super::super) fn insert_feature_source_property(
    ctx: &DecodeContext<'_>,
    properties: &mut BTreeMap<String, String>,
    key: impl std::fmt::Display,
    value: impl std::fmt::Display,
) -> Result<(), CodecError> {
    let key = ctx.format_retained(key, "creo feature source property key")?;
    let value = ctx.format_retained(value, "creo feature source property value")?;
    if !properties.contains_key(&key) {
        ctx.charge_collection_items(1, "creo feature source property nodes")?;
    }
    properties.insert(key, value);
    Ok(())
}

#[cfg(test)]
mod tests;
