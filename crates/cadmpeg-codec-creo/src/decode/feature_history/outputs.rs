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
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{EdgeSelection, GeneratedEdgeRef};
use cadmpeg_ir::ids::{BodyId, EdgeId};
use cadmpeg_ir::topology::BodyKind;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

pub(in super::super) fn copy_body_id(
    ctx: &DecodeContext<'_>,
    body: &BodyId,
) -> Result<BodyId, CodecError> {
    body.try_clone_for_decode(ctx, "creo feature output body IDs")
}

struct FeatureOutputHistory<'ctx> {
    visiting: BTreeSet<u32>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> FeatureOutputHistory<'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            visiting: BTreeSet::new(),
            storage: ctx.reserve_scoped(0, "Creo feature output history storage")?,
        })
    }
}

pub(in super::super) fn feature_output_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Result<Vec<BodyId>, CodecError> {
    feature_output_bodies_with_history(
        ctx,
        scan,
        ir,
        feature_id,
        &mut FeatureOutputHistory::new(ctx)?,
    )
}

fn feature_output_bodies_with_history(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    history: &mut FeatureOutputHistory<'_>,
) -> Result<Vec<BodyId>, CodecError> {
    let _depth = ctx.enter_nested("creo feature output history")?;
    if history.visiting.contains(&feature_id) {
        return Ok(Vec::new());
    }
    history.storage.with_storage(|| {
        ctx.insert_btree_set(
            &mut history.visiting,
            feature_id,
            "creo feature output visiting nodes",
        )
    })?;
    let affected_geometry = agreed_feature_geometry_ids(
        ctx,
        &scan.features.affected_ids,
        &scan.features.replay_affected_ids,
        feature_id,
    )?;
    let mut outputs = evaluated_sweep_output_bodies(ctx, ir, feature_id)?;
    let edge_outputs = match feature_edge_selection(ctx, scan, ir, feature_id)? {
        Some(EdgeSelection::Resolved { edges, .. }) => bodies_containing_edges(ctx, ir, &edges)?,
        Some(EdgeSelection::Generated { edges, .. }) => {
            generated_edge_output_bodies(ctx, scan, ir, &edges, history)?
        }
        _ => Vec::new(),
    };
    let generated_input_outputs =
        generated_input_output_bodies(ctx, scan, ir, feature_id, history)?;
    let mut add_surface_outputs = |surface_id| -> Result<(), CodecError> {
        let (surface, _reservation) = ctx.format_scoped(
            format_args!("creo:visibgeom:surface#{surface_id}"),
            "creo generated surface lookup",
        )?;
        for face in ctx.admit_iter(&ir.model.faces, "creo generated surface face lookup")? {
            if !ctx.equal(
                face.surface.as_str(),
                surface.as_str(),
                "creo generated surface identity comparison",
            )? {
                continue;
            }
            let mut matching_shell = None;
            for shell in ctx.admit_iter(&ir.model.shells, "creo generated face shell lookup")? {
                if !ctx.equal(
                    &shell.id,
                    &face.shell,
                    "creo generated face shell identity comparison",
                )? {
                    continue;
                }
                if matching_shell.is_some() {
                    matching_shell = None;
                    break;
                }
                matching_shell = Some(shell);
            }
            let Some(shell) = matching_shell else {
                continue;
            };
            let mut matching_region = None;
            for region in ctx.admit_iter(&ir.model.regions, "creo generated shell region lookup")? {
                if !ctx.equal(
                    &region.id,
                    &shell.region,
                    "creo generated shell region identity comparison",
                )? {
                    continue;
                }
                if matching_region.is_some() {
                    matching_region = None;
                    break;
                }
                matching_region = Some(region);
            }
            let Some(region) = matching_region else {
                continue;
            };
            if !ctx.contains(&outputs, &region.body, "creo feature output body lookup")? {
                let body = copy_body_id(ctx, &region.body)?;
                ctx.reserve_vec(&mut outputs, 1, "creo feature output bodies")?;
                outputs.push(body);
            }
        }
        Ok(())
    };
    for row in ctx
        .admit_iter(&scan.surfaces.rows, "creo generated surface rows")?
        .filter(|row| row.feature_id == feature_id)
    {
        add_surface_outputs(row.id)?;
    }
    for table in ctx
        .admit_iter(&scan.features.entity_tables, "creo generated entity tables")?
        .filter(|table| table.feature_id == feature_id)
    {
        for entry in ctx
            .admit_iter(&table.entries, "creo generated entity entries")?
            .filter(|entry| table.contains_surface_id(entry.entity_id))
        {
            add_surface_outputs(entry.entity_id)?;
        }
    }
    if let Some(affected_geometry) = affected_geometry {
        for surface_id in ctx.admit_iter(affected_geometry, "creo affected geometry IDs")? {
            add_surface_outputs(*surface_id)?;
        }
    }
    for body in edge_outputs.into_iter().chain(generated_input_outputs) {
        if !ctx.contains(&outputs, &body, "creo feature output body lookup")? {
            ctx.reserve_vec(&mut outputs, 1, "creo feature output bodies")?;
            outputs.push(body);
        }
    }
    history.visiting.remove(&feature_id);
    Ok(outputs)
}

fn generated_input_output_bodies(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    history: &mut FeatureOutputHistory<'_>,
) -> Result<Vec<BodyId>, CodecError> {
    let (feature_id_text, reservation) = ctx.format_scoped(
        format_args!("creo:model:feature#{feature_id}"),
        "creo generated input feature lookup",
    )?;
    let mut matching_feature = None;
    for feature in ctx.admit_iter(
        &ir.model.features,
        "creo generated input feature lookup traversal",
    )? {
        if !ctx.equal(
            feature.id.as_str(),
            feature_id_text.as_str(),
            "creo generated input feature identity comparison",
        )? {
            continue;
        }
        if matching_feature.is_some() {
            matching_feature = None;
            break;
        }
        matching_feature = Some(feature);
    }
    let Some(feature) = matching_feature else {
        return Ok(Vec::new());
    };
    drop(feature_id_text);
    drop(reservation);
    let mut outputs = Vec::new();
    let mut dependency_storage = ctx.reserve_scoped(0, "Creo generated producer lookup")?;
    let producers = dependency_storage
        .with_storage(|| feature_generated_dependencies(ctx, feature.evaluation.definition()))?;
    for producer in ctx.admit_iter(&producers, "creo generated feature dependencies")? {
        let Some(producer_id) = producer
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        for body in feature_output_bodies_with_history(ctx, scan, ir, producer_id, history)? {
            if !outputs.contains(&body) {
                ctx.reserve_vec(&mut outputs, 1, "creo generated input output bodies")?;
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
    history: &mut FeatureOutputHistory<'_>,
) -> Result<Vec<BodyId>, CodecError> {
    let mut outputs = Vec::new();
    for edge in ctx.admit_iter(edges, "creo generated edge references")? {
        let Some(producer_id) = edge
            .feature
            .as_str()
            .strip_prefix("creo:model:feature#")
            .and_then(|value| value.parse::<u32>().ok())
        else {
            continue;
        };
        for body in feature_output_bodies_with_history(ctx, scan, ir, producer_id, history)? {
            if !outputs.contains(&body) {
                ctx.reserve_vec(&mut outputs, 1, "creo generated edge output bodies")?;
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
    let mut lookup_storage = ctx.reserve_scoped(0, "Creo selected topology lookup")?;
    let mut selected = BTreeSet::new();
    for edge in ctx.admit_iter(edges, "creo selected edge references")? {
        lookup_storage.with_storage(|| {
            ctx.insert_btree_set(&mut selected, edge, "creo selected edge nodes")
        })?;
    }
    let mut shell_ids = BTreeSet::new();
    for coedge in ctx.admit_iter(&ir.model.coedges, "creo selected edge coedges")? {
        if !ctx.contains_btree_set(&selected, &coedge.edge, "creo selected coedge lookup")? {
            continue;
        }
        let mut matching_loop = None;
        for lp in ctx.admit_iter(&ir.model.loops, "creo selected edge loops")? {
            if !ctx.equal(
                &lp.id,
                &coedge.owner_loop,
                "creo selected edge loop identity comparison",
            )? {
                continue;
            }
            if matching_loop.is_some() {
                matching_loop = None;
                break;
            }
            matching_loop = Some(lp);
        }
        let Some(lp) = matching_loop else {
            continue;
        };
        let mut matching_face = None;
        for face in ctx.admit_iter(&ir.model.faces, "creo selected loop face lookup")? {
            if !ctx.equal(
                &face.id,
                &lp.face,
                "creo selected loop face identity comparison",
            )? {
                continue;
            }
            if matching_face.is_some() {
                matching_face = None;
                break;
            }
            matching_face = Some(face);
        }
        let Some(face) = matching_face else {
            continue;
        };
        lookup_storage.with_storage(|| {
            ctx.insert_btree_set(&mut shell_ids, &face.shell, "creo selected shell nodes")
        })?;
    }
    for shell in ctx.admit_iter(&ir.model.shells, "creo selected shell lookup")? {
        let mut has_selected_wire_edge = false;
        for edge in ctx.admit_iter(shell.wire_edges(), "creo selected shell wire edges")? {
            if ctx.contains_btree_set(&selected, edge, "creo selected shell wire edge lookup")? {
                has_selected_wire_edge = true;
                break;
            }
        }
        if !has_selected_wire_edge {
            continue;
        }
        lookup_storage.with_storage(|| {
            ctx.insert_btree_set(&mut shell_ids, &shell.id, "creo selected shell nodes")
        })?;
    }
    let mut bodies = Vec::new();
    for shell_id in shell_ids {
        let mut matching_shell = None;
        for shell in ctx.admit_iter(&ir.model.shells, "creo selected shell ID lookup")? {
            if !ctx.equal(
                &shell.id,
                shell_id,
                "creo selected shell identity comparison",
            )? {
                continue;
            }
            if matching_shell.is_some() {
                matching_shell = None;
                break;
            }
            matching_shell = Some(shell);
        }
        let Some(shell) = matching_shell else {
            continue;
        };
        let mut matching_region = None;
        for region in ctx.admit_iter(&ir.model.regions, "creo selected shell region lookup")? {
            if !ctx.equal(
                &region.id,
                &shell.region,
                "creo selected shell region identity comparison",
            )? {
                continue;
            }
            if matching_region.is_some() {
                matching_region = None;
                break;
            }
            matching_region = Some(region);
        }
        let Some(region) = matching_region else {
            continue;
        };
        let mut matching_body = None;
        for body in ctx.admit_iter(&ir.model.bodies, "creo selected region body lookup")? {
            if !ctx.equal(
                &body.id,
                &region.body,
                "creo selected region body identity comparison",
            )? {
                continue;
            }
            if matching_body.is_some() {
                matching_body = None;
                break;
            }
            matching_body = Some(body);
        }
        if matching_body.is_none() {
            continue;
        }
        let body = copy_body_id(ctx, &region.body)?;
        if !bodies.contains(&body) {
            ctx.reserve_vec(&mut bodies, 1, "creo bodies containing selected edges")?;
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
        let mut matching_body = None;
        for body in ctx.admit_iter(
            &ir.model.bodies,
            "creo evaluated sweep body lookup traversal",
        )? {
            if !ctx.equal(
                body.id.as_str(),
                candidate.as_str(),
                "creo evaluated sweep body identity comparison",
            )? {
                continue;
            }
            if matching_body.is_some() {
                matching_body = None;
                break;
            }
            matching_body = Some(body);
        }
        if matching_body.is_some() {
            ctx.charge_retained(
                cadmpeg_core::decode::u64_from_index(candidate.len()),
                "creo evaluated sweep body IDs",
            )?;
            ctx.charge_work(
                cadmpeg_core::decode::u64_from_index(candidate.len()),
                "creo evaluated sweep body identity validation",
            )?;
            let body = BodyId::mint(candidate).map_err(CodecError::malformed)?;
            ctx.reserve_vec(&mut outputs, 1, "creo evaluated sweep output bodies")?;
            outputs.push(body);
        }
    }
    Ok(outputs)
}

pub(in super::super) fn evaluated_sweep_body_kind(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    family: &str,
    feature_id: u32,
) -> Result<Option<BodyKind>, CodecError> {
    let prefix = match family {
        "extrusion" => "creo:feature:extrusion#",
        "revolution" => "creo:feature:revolution#",
        _ => return Ok(None),
    };
    Ok(crate::decode::uniqueness::exactly_one_by(
        ctx,
        &ir.model.bodies,
        |body| {
            Ok(body
                .id
                .as_str()
                .strip_suffix(":body")
                .is_some_and(|candidate| {
                    crate::identity::matches_numbered_identity(candidate, prefix, feature_id)
                }))
        },
        "creo evaluated sweep body kind",
    )?
    .map(|body| body.kind))
}

pub(in super::super) fn new_sheet_output_surface_id(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    surface_rows: &[crate::surface::SurfaceRow],
) -> Result<Option<u32>, CodecError> {
    let mut owner_tables = ctx
        .admit_iter(tables, "creo new sheet entity tables")?
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 67);
    let Some(owner_table) = owner_tables.next() else {
        return Ok(None);
    };
    if owner_tables.next().is_some() {
        return Ok(None);
    }
    let [owner] = owner_table.entries.as_slice() else {
        return Ok(None);
    };
    let mut output_tables = ctx
        .admit_iter(tables, "creo new sheet entity tables")?
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 100);
    let Some(output_table) = output_tables.next() else {
        return Ok(None);
    };
    if output_tables.next().is_some() {
        return Ok(None);
    }
    let [output] = output_table.entries.as_slice() else {
        return Ok(None);
    };
    let mut generated_tables = ctx
        .admit_iter(tables, "creo new sheet entity tables")?
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 29);
    let Some(generated) = generated_tables.next() else {
        return Ok(None);
    };
    if generated_tables.next().is_some() {
        return Ok(None);
    }
    if owner.source_entity_id() != Some(feature_id)
        || output.entity_id != owner.entity_id
        || !generated.contains_surface_id(output.class_id())
        || !ctx
            .admit_iter(
                &generated.entries,
                "creo new sheet generated entity entries",
            )?
            .any(|entry| entry.entity_id == output.class_id() && entry.class_id() == 200)
    {
        return Ok(None);
    }
    let mut surfaces = ctx
        .admit_iter(surface_rows, "creo new sheet surface rows")?
        .filter(|row| row.id == output.class_id() && row.feature_id == feature_id);
    let Some(surface) = surfaces.next() else {
        return Ok(None);
    };
    Ok(surfaces.next().is_none().then_some(surface.id))
}

pub(in super::super) fn sweep_output_kind(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &CadIr,
    family: &str,
    feature_id: u32,
) -> Result<Option<BodyKind>, CodecError> {
    if let Some(kind) = evaluated_sweep_body_kind(ctx, ir, family, feature_id)? {
        return Ok(Some(kind));
    }
    if !feature_is_sheet_extrusion(ctx, scan, feature_id)? {
        return Ok(None);
    }
    if new_sheet_output_surface_id(
        ctx,
        feature_id,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    )?
    .is_some()
    {
        return Ok(Some(BodyKind::Sheet));
    }
    Ok(
        current_feature_operation(&scan.features.operations, feature_id)
            .filter(|operation| operation.kind.as_str() == "Surface")
            .map(|_| BodyKind::Sheet),
    )
}

pub(super) fn sweep_solid(output_kind: Option<BodyKind>) -> Option<bool> {
    output_kind.map(|kind| kind == BodyKind::Solid)
}

pub(super) struct CommaList<'a, T>(pub(super) &'a [T]);

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

fn feature_field_text(
    value: &crate::feature::rows::FeatureFieldValue,
) -> Option<FeatureFieldText<'_>> {
    match value {
        crate::feature::rows::FeatureFieldValue::Empty => Some(FeatureFieldText::Empty),
        crate::feature::rows::FeatureFieldValue::CompactInt(value) => {
            Some(FeatureFieldText::CompactInt(*value))
        }
        crate::feature::rows::FeatureFieldValue::CompactIntArray(values) => {
            Some(FeatureFieldText::CompactIntArray(values))
        }
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
    text_storage: &mut ScopedReservation<'_>,
    node_storage: &mut ScopedReservation<'_>,
    parameters: &mut BTreeMap<String, String>,
    base: impl std::fmt::Display,
    value: impl std::fmt::Display,
) -> Result<(), CodecError> {
    let value = text_storage.with_storage(|| {
        ctx.format_retained(format_args!("{value}"), "creo feature parameter value")
    })?;
    let (base, base_reservation) = ctx.format_scoped(
        format_args!("{base}"),
        "creo feature parameter key candidate",
    )?;
    let (key, key_reservation) = if parameters.contains_key(&base) {
        let mut occurrence = 2usize;
        loop {
            ctx.charge_work(1, "creo feature parameter collision candidate")?;
            let candidate = ctx.format_scoped(
                format_args!("{base}#{occurrence}"),
                "creo feature parameter key candidate",
            )?;
            if !parameters.contains_key(&candidate.0) {
                drop(base);
                drop(base_reservation);
                break candidate;
            }
            occurrence = occurrence.checked_add(1).ok_or_else(|| {
                ctx.refuse_codec_limit(
                    "creo feature parameter collision ordinal",
                    u64::MAX,
                    u64::MAX,
                )
            })?;
        }
    } else {
        (base, base_reservation)
    };
    drop(key_reservation);
    text_storage.with_storage(|| {
        ctx.charge_retained(
            cadmpeg_core::decode::u64_from_index(key.len()),
            "creo feature parameter key",
        )
    })?;
    node_storage.with_storage(|| {
        ctx.insert_btree_map(parameters, key, value, "creo feature parameter nodes")
    })?;
    Ok(())
}

fn replace_feature_parameter(
    ctx: &DecodeContext<'_>,
    text_storage: &mut ScopedReservation<'_>,
    node_storage: &mut ScopedReservation<'_>,
    parameters: &mut BTreeMap<String, String>,
    key: &'static str,
    value: impl std::fmt::Display,
) -> Result<(), CodecError> {
    let value = text_storage.with_storage(|| {
        ctx.format_retained(format_args!("{value}"), "creo feature parameter value")
    })?;
    if let Some(existing) = parameters.get_mut(key) {
        *existing = value;
    } else {
        let key = text_storage
            .with_storage(|| ctx.copy_retained_text(key, "creo feature parameter key"))?;
        node_storage.with_storage(|| {
            ctx.insert_btree_map(parameters, key, value, "creo feature parameter nodes")
        })?;
    }
    Ok(())
}

pub(in super::super) fn feature_parameters<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<
    (
        ScopedReservation<'ctx>,
        ScopedReservation<'ctx>,
        BTreeMap<String, String>,
    ),
    CodecError,
> {
    let mut text_storage = ctx.reserve_scoped(0, "creo feature parameter text")?;
    let mut node_storage = ctx.reserve_scoped(0, "creo feature parameter nodes")?;
    let mut parameters = BTreeMap::new();
    for field in ctx
        .admit_iter(&scan.features.choice_fields, "creo feature choice fields")?
        .filter(|field| field.feature_id == feature_id)
    {
        let Some(value) = feature_field_text(&field.value) else {
            continue;
        };
        insert_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            format_args!("choice.{}.{}", field.choice_label, field.name),
            value,
        )?;
    }
    for affected in ctx
        .admit_iter(&scan.features.affected_ids, "creo feature affected IDs")?
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
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            name,
            CommaList(&affected.ids),
        )?;
    }
    for affected in ctx
        .admit_iter(
            &scan.features.replay_affected_ids,
            "creo feature replay affected IDs",
        )?
        .filter(|record| record.feature_id == feature_id)
    {
        insert_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "replay_affected_geometry_ids",
            CommaList(&affected.geometry_ids),
        )?;
        insert_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "replay_affected_edge_ids",
            CommaList(&affected.edge_ids),
        )?;
        insert_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "replay_geometry_extent",
            match affected.geometry_extent {
                crate::feature::rows::ReplayExtentSource::Explicit => "explicit",
                crate::feature::rows::ReplayExtentSource::Inherited => "inherited",
            },
        )?;
        insert_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "replay_edge_extent",
            match affected.edge_extent {
                crate::feature::rows::ReplayExtentSource::Explicit => "explicit",
                crate::feature::rows::ReplayExtentSource::Inherited => "inherited",
            },
        )?;
    }
    for affected in ctx
        .admit_iter(
            &scan.features.surface_merge_replay_affected_ids,
            "creo feature surface merge replay affected IDs",
        )?
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
                &mut text_storage,
                &mut node_storage,
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
                &mut text_storage,
                &mut node_storage,
                &mut parameters,
                name,
                match extent {
                    crate::feature::rows::ReplayExtentSource::Explicit => "explicit",
                    crate::feature::rows::ReplayExtentSource::Inherited => "inherited",
                },
            )?;
        }
    }
    for direction in ctx
        .admit_iter(
            &scan.features.loop_restore_directions,
            "creo feature loop restore directions",
        )?
        .filter(|record| record.feature_id == feature_id)
    {
        let name = match direction.lane {
            crate::feature::rows::LoopRestoreDirectionLane::Primary => "direction",
            crate::feature::rows::LoopRestoreDirectionLane::Secondary => "direction2",
        };
        insert_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            format_args!("loop_restore.{name}"),
            direction.value,
        )?;
    }
    if unique_feature_revolution_extent(ctx, &scan.features.revolution_extents, feature_id)?
        .is_some()
    {
        replace_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "revolution_extent",
            "full_turn",
        )?;
    }
    for table in ctx
        .admit_iter(
            &scan.features.entity_tables,
            "creo feature parameter entity tables",
        )?
        .filter(|table| table.feature_id == feature_id)
    {
        for entry in ctx.admit_iter(&table.entries, "creo feature parameter entity entries")? {
            let Some(source_entity_id) = entry.source_entity_id() else {
                continue;
            };
            insert_feature_parameter(
                ctx,
                &mut text_storage,
                &mut node_storage,
                &mut parameters,
                format_args!(
                    "generated_entity.{}.source_section_entity_id",
                    entry.entity_id
                ),
                source_entity_id,
            )?;
            insert_feature_parameter(
                ctx,
                &mut text_storage,
                &mut node_storage,
                &mut parameters,
                format_args!("generated_entity.{}.entry_class", entry.entity_id),
                entry.class_id(),
            )?;
        }
    }
    if let Some(definition) = exactly_one(
        scan.features
            .definitions
            .iter()
            .filter(|definition| definition.identity.owner_feature_id() == Some(feature_id)),
    ) {
        let sketch_segment_count = match definition.segments.as_ref() {
            Some(segments) => ctx
                .admit_iter(
                    segments.rows.as_slice(),
                    "creo feature parameter sketch segment rows",
                )?
                .filter_map(|row| match row {
                    crate::feature::segment_rows::SegmentRow::Ordinary(segment) => Some(segment),
                    _ => None,
                })
                .count(),
            None => 0,
        };
        replace_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "sketch_segment_count",
            sketch_segment_count,
        )?;
        replace_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "dimension_count",
            definition
                .dimensions
                .as_ref()
                .map_or(0, |dimensions| dimensions.rows.len()),
        )?;
    }
    for transform in ctx
        .admit_iter(
            &scan.features.section_transforms,
            "creo feature parameter section transforms",
        )?
        .filter(|transform| transform.feature_id == Some(feature_id))
    {
        let Some(definition) =
            unique_feature_definition_for_transform(ctx, &scan.features.definitions, transform)?
        else {
            continue;
        };
        let Some(profile_sketch) = model_sketch_id(ctx, scan, definition)? else {
            continue;
        };
        insert_feature_parameter(
            ctx,
            &mut text_storage,
            &mut node_storage,
            &mut parameters,
            "profile_sketch",
            profile_sketch.as_str(),
        )?;
        if feature_recipe(scan, feature_id)
            == Some(crate::feature::operations::FeatureRecipeKind::Extrude)
        {
            insert_feature_parameter(
                ctx,
                &mut text_storage,
                &mut node_storage,
                &mut parameters,
                "sweep_direction",
                CommaList(&transform.normal()),
            )?;
        }
    }
    Ok((text_storage, node_storage, parameters))
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
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<&'a [u8]>, CodecError> {
    let mut records = ctx
        .admit_iter(
            &scan.features.reference_names,
            "creo feature reference names",
        )?
        .filter(|record| record.feature_id == feature_id);
    let Some(record) = records.next() else {
        return Ok(None);
    };
    for candidate in records {
        if !ctx.equal_bytes(
            candidate.name_bytes.as_slice(),
            record.name_bytes.as_slice(),
            "creo feature reference name agreement",
        )? {
            return Ok(None);
        }
    }
    Ok(Some(record.name_bytes.as_slice()))
}

pub(in super::super) fn decoded_feature_reference_name<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Cow<'a, str>, CodecError> {
    match std::str::from_utf8(bytes) {
        Ok(name) => Ok(Cow::Borrowed(name)),
        Err(_) => ctx
            .copy_retained_lossy_utf8(bytes, "creo decoded feature reference name")
            .map(Cow::Owned),
    }
}

pub(in super::super) fn owned_section_feature_id(
    scan: &ContainerScan,
    definition_id: u32,
) -> Option<u32> {
    let definition = exactly_one(
        scan.features
            .definitions
            .iter()
            .filter(|definition| definition.identity.id() == definition_id),
    )?;
    let row = exactly_one(scan.features.rows.iter().filter(|row| {
        row.root_schema_class == Some(SchemaClass::Section)
            && definition.offset >= row.body_offset
            && row
                .body_offset
                .checked_add(row.body.len())
                .is_some_and(|end| definition.offset < end)
    }))?;
    Some(row.feature_id)
}

pub(super) fn section_definition_for_history_feature<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<&'a crate::feature::definitions::FeatureDefinition> {
    let row = exactly_one(scan.features.rows.iter().filter(|row| {
        row.feature_id == feature_id && row.root_schema_class == Some(SchemaClass::Section)
    }))?;
    let definition = exactly_one(scan.features.definitions.iter().filter(|definition| {
        definition.offset >= row.body_offset
            && row
                .body_offset
                .checked_add(row.body.len())
                .is_some_and(|end| definition.offset < end)
    }))?;
    Some(definition)
}

pub(in super::super) fn feature_source_properties<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<(ScopedReservation<'ctx>, BTreeMap<String, String>), CodecError> {
    let mut node_storage = ctx.reserve_scoped(0, "creo feature source property nodes")?;
    let mut properties = BTreeMap::new();
    if let Some(recipe) = current_feature_recipe(&scan.features.operations, feature_id) {
        insert_feature_source_property(
            ctx,
            &mut node_storage,
            &mut properties,
            "recipe",
            recipe.name(),
        )?;
    }
    let schema_class = feature_schema_class(ctx, scan, feature_id)?;
    if let Some(schema_class) = schema_class {
        insert_feature_source_property(
            ctx,
            &mut node_storage,
            &mut properties,
            "featdefs_schema_class",
            schema_class,
        )?;
    }
    let row_schema_classes = feature_row_schema_classes(ctx, scan, feature_id)?;
    if !row_schema_classes.is_empty() {
        insert_feature_source_property(
            ctx,
            &mut node_storage,
            &mut properties,
            "featdefs_row_schema_classes",
            SchemaClassList(&row_schema_classes),
        )?;
    }
    if schema_class.is_none() && !row_schema_classes.is_empty() {
        insert_feature_source_property(
            ctx,
            &mut node_storage,
            &mut properties,
            "featdefs_schema_state",
            "ambiguous",
        )?;
    }
    Ok((node_storage, properties))
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
    node_storage: &mut ScopedReservation<'_>,
    properties: &mut BTreeMap<String, String>,
    key: impl std::fmt::Display,
    value: impl std::fmt::Display,
) -> Result<(), CodecError> {
    let key = ctx.format_retained(format_args!("{key}"), "creo feature source property key")?;
    let value = ctx.format_retained(
        format_args!("{value}"),
        "creo feature source property value",
    )?;
    node_storage.with_storage(|| {
        ctx.insert_btree_map(properties, key, value, "creo feature source property nodes")
    })?;
    Ok(())
}

#[cfg(test)]
mod tests;
