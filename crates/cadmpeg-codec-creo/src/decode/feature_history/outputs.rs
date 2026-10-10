// SPDX-License-Identifier: Apache-2.0
//! Feature output bodies, sweep kind, and native parameter maps.

use super::super::sketch_ids::model_sketch_id;
use super::super::uniqueness::unique_feature_definition_for_transform;
use super::dependencies::feature_generated_dependencies;
use super::draft::feature_is_sheet_extrusion;
use super::selections::{agreed_feature_geometry_ids, feature_edge_selection};
use crate::container::ContainerScan;
use crate::decode::sketch_transfer::recipe::{
    feature_row_schema_classes, feature_schema_class, unique_feature_revolution_extent,
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

struct FeatureOutputCandidates<'ir, 'ctx> {
    bodies: Vec<&'ir BodyId>,
    storage: ScopedReservation<'ctx>,
}

impl<'ir, 'ctx> FeatureOutputCandidates<'ir, 'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            bodies: Vec::new(),
            storage: ctx.reserve_scoped(0, "creo feature output candidate rows")?,
        })
    }
}

pub(super) struct FeatureOutputHistory<'ir, 'ctx> {
    visiting: BTreeSet<u32>,
    surface_outputs: super::output_rows::SurfaceOutputs<'ir, 'ctx>,
    storage: ScopedReservation<'ctx>,
}

impl<'ir, 'ctx> FeatureOutputHistory<'ir, 'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>, ir: &'ir CadIr) -> Result<Self, CodecError> {
        Ok(Self {
            visiting: BTreeSet::new(),
            surface_outputs: super::output_rows::SurfaceOutputs::new(ctx, ir)?,
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
        &mut FeatureOutputHistory::new(ctx, ir)?,
    )
}

pub(super) fn feature_output_bodies_with_history<'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &'ir CadIr,
    feature_id: u32,
    history: &mut FeatureOutputHistory<'ir, 'ctx>,
) -> Result<Vec<BodyId>, CodecError> {
    let candidates =
        feature_output_body_candidates_with_history(ctx, scan, ir, feature_id, history)?;
    let mut outputs = Vec::new();
    for candidate in ctx.admit_iter(
        &candidates.bodies,
        "creo feature output candidate references",
    )? {
        let body = *candidate;
        ctx.reserve_vec(&mut outputs, 1, "creo feature output bodies")?;
        outputs.push(copy_body_id(ctx, body)?);
    }
    Ok(outputs)
}

fn feature_output_body_candidates_with_history<'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &'ir CadIr,
    feature_id: u32,
    history: &mut FeatureOutputHistory<'ir, 'ctx>,
) -> Result<FeatureOutputCandidates<'ir, 'ctx>, CodecError> {
    let _depth = ctx.enter_nested("creo feature output history")?;
    if ctx.contains_btree_set(
        &history.visiting,
        &feature_id,
        "creo feature output visiting lookup",
    )? {
        return FeatureOutputCandidates::new(ctx);
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
    let mut outputs = FeatureOutputCandidates::new(ctx)?;
    append_evaluated_sweep_output_body_candidates(ctx, ir, feature_id, &mut outputs)?;
    let mut edge_selection_storage =
        ctx.reserve_scoped(0, "creo output edge selection evidence")?;
    let edge_outputs = match edge_selection_storage
        .with_storage(|| feature_edge_selection(ctx, scan, ir, feature_id))?
    {
        Some(EdgeSelection::Resolved { edges, .. }) => bodies_containing_edges(ctx, ir, &edges)?,
        Some(EdgeSelection::Generated { edges, .. }) => {
            generated_edge_output_bodies(ctx, scan, ir, &edges, history)?
        }
        _ => FeatureOutputCandidates::new(ctx)?,
    };
    let generated_input_outputs =
        generated_input_output_bodies(ctx, scan, ir, feature_id, history)?;
    let mut add_surface_outputs = |surface_id| -> Result<(), CodecError> {
        let surface_storage = ctx.format_scoped(
            format_args!("creo:visibgeom:surface#{surface_id}"),
            "creo generated surface lookup",
        )?;
        let bodies = history
            .surface_outputs
            .bodies(ctx, &surface_storage.0)?
            .unwrap_or(&[]);
        for body in ctx.admit_iter(bodies, "creo generated surface body references")? {
            if !ctx.contains(&outputs.bodies, body, "creo feature output body lookup")? {
                outputs.storage.with_storage(|| {
                    ctx.push_vec(&mut outputs.bodies, body, "creo feature output bodies")
                })?;
            }
        }
        Ok(())
    };
    for row in ctx
        .admit_iter(&*scan.surfaces.rows, "creo generated surface rows")?
        .filter(|row| row.feature_id == feature_id)
    {
        add_surface_outputs(row.id)?;
    }
    for table in ctx
        .admit_iter(&scan.features.entity_tables, "creo generated entity tables")?
        .filter(|table| table.feature_id == feature_id)
    {
        for entry in ctx
            .admit_iter(table.entries.as_slice(), "creo generated entity entries")?
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
    for candidate in ctx.admit_iter(
        &edge_outputs.bodies,
        "creo feature edge output body candidates",
    )? {
        let body = *candidate;
        if !ctx.contains(&outputs.bodies, &body, "creo feature output body lookup")? {
            outputs.storage.with_storage(|| {
                ctx.push_vec(&mut outputs.bodies, body, "creo feature output bodies")
            })?;
        }
    }
    for candidate in ctx.admit_iter(
        &generated_input_outputs.bodies,
        "creo feature input output body candidates",
    )? {
        let body = *candidate;
        if !ctx.contains(&outputs.bodies, &body, "creo feature output body lookup")? {
            outputs.storage.with_storage(|| {
                ctx.push_vec(&mut outputs.bodies, body, "creo feature output bodies")
            })?;
        }
    }
    ctx.remove_btree_set(
        &mut history.visiting,
        &feature_id,
        "creo feature output visiting removal",
    )?;
    Ok(outputs)
}

fn generated_input_output_bodies<'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &'ir CadIr,
    feature_id: u32,
    history: &mut FeatureOutputHistory<'ir, 'ctx>,
) -> Result<FeatureOutputCandidates<'ir, 'ctx>, CodecError> {
    let feature_id_storage = ctx.format_scoped(
        format_args!("creo:model:feature#{feature_id}"),
        "creo generated input feature lookup",
    )?;
    let matching_feature = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &ir.model.features,
        |feature| {
            ctx.equal(
                feature.id.as_str(),
                feature_id_storage.0.as_str(),
                "creo generated input feature identity comparison",
            )
        },
        "creo generated input feature lookup traversal",
    )?;
    let Some(feature) = matching_feature else {
        return FeatureOutputCandidates::new(ctx);
    };
    drop(feature_id_storage);
    let mut outputs = FeatureOutputCandidates::new(ctx)?;
    let mut dependency_storage = ctx.reserve_scoped(0, "Creo generated producer lookup")?;
    let producers = dependency_storage
        .with_storage(|| feature_generated_dependencies(ctx, feature.evaluation.definition()))?;
    for producer in ctx.admit_iter(&producers, "creo generated feature dependencies")? {
        let Some(suffix) = producer.as_str().strip_prefix("creo:model:feature#") else {
            continue;
        };
        let Ok(producer_id) =
            ctx.parse_text::<u32>(suffix, "creo generated producer identity number")?
        else {
            continue;
        };
        let producer_outputs =
            feature_output_body_candidates_with_history(ctx, scan, ir, producer_id, history)?;
        for candidate in ctx.admit_iter(
            &producer_outputs.bodies,
            "creo generated producer body candidates",
        )? {
            let body = *candidate;
            if !ctx.contains(&outputs.bodies, &body, "creo output body membership")? {
                outputs.storage.with_storage(|| {
                    ctx.push_vec(
                        &mut outputs.bodies,
                        body,
                        "creo generated input output bodies",
                    )
                })?;
            }
        }
    }
    Ok(outputs)
}

fn generated_edge_output_bodies<'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    ir: &'ir CadIr,
    edges: &[GeneratedEdgeRef],
    history: &mut FeatureOutputHistory<'ir, 'ctx>,
) -> Result<FeatureOutputCandidates<'ir, 'ctx>, CodecError> {
    let mut outputs = FeatureOutputCandidates::new(ctx)?;
    for edge in ctx.admit_iter(edges, "creo generated edge references")? {
        let Some(suffix) = edge.feature.as_str().strip_prefix("creo:model:feature#") else {
            continue;
        };
        let Ok(producer_id) =
            ctx.parse_text::<u32>(suffix, "creo generated edge producer identity number")?
        else {
            continue;
        };
        let producer_outputs =
            feature_output_body_candidates_with_history(ctx, scan, ir, producer_id, history)?;
        for candidate in ctx.admit_iter(
            &producer_outputs.bodies,
            "creo generated producer body candidates",
        )? {
            let body = *candidate;
            if !ctx.contains(&outputs.bodies, &body, "creo output body membership")? {
                outputs.storage.with_storage(|| {
                    ctx.push_vec(
                        &mut outputs.bodies,
                        body,
                        "creo generated edge output bodies",
                    )
                })?;
            }
        }
    }
    Ok(outputs)
}

fn bodies_containing_edges<'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &'ir CadIr,
    edges: &[EdgeId],
) -> Result<FeatureOutputCandidates<'ir, 'ctx>, CodecError> {
    let mut lookup_storage = ctx.reserve_scoped(0, "Creo selected topology lookup")?;
    let mut selected = BTreeSet::new();
    for edge in ctx.admit_iter(edges, "creo selected edge references")? {
        lookup_storage.with_storage(|| {
            ctx.insert_btree_set(&mut selected, edge, "creo selected edge nodes")
        })?;
    }
    if selected.is_empty() {
        return FeatureOutputCandidates::new(ctx);
    }
    let mut selected_coedges = Vec::new();
    for coedge in ctx.admit_iter(&ir.model.coedges, "creo selected edge coedges")? {
        if ctx.contains_btree_set(&selected, &coedge.edge, "creo selected coedge lookup")? {
            lookup_storage.with_storage(|| {
                ctx.push_vec(
                    &mut selected_coedges,
                    coedge,
                    "creo selected coedge references",
                )
            })?;
        }
    }
    let mut shell_ids = BTreeSet::new();
    if !selected_coedges.is_empty() {
        let owners = super::output_rows::CoedgeOwners::new(ctx, ir)?;
        for coedge in ctx.admit_iter(&selected_coedges, "creo selected coedge ownership")? {
            let Some(face) = owners.face(ctx, coedge.owner_loop.as_str())? else {
                continue;
            };
            lookup_storage.with_storage(|| {
                ctx.insert_btree_set(&mut shell_ids, &face.shell, "creo selected shell nodes")
            })?;
        }
    }
    for shell in ctx.admit_iter(&ir.model.shells, "creo selected shell lookup")? {
        let mut has_selected_wire_edge = false;
        let mut edge_iter = shell.wire_edges().iter();
        while edge_iter.len() != 0 {
            let Some(edge) = ctx.next_charged(&mut edge_iter, "creo selected shell wire edges")?
            else {
                break;
            };
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
    let mut bodies = FeatureOutputCandidates::new(ctx)?;
    if shell_ids.is_empty() {
        return Ok(bodies);
    }
    let owners = super::output_rows::ShellRegions::new(ctx, ir)?;
    let mut body_presence = BTreeMap::new();
    for body in ctx.admit_iter(&ir.model.bodies, "creo selected region body lookup")? {
        match lookup_storage.with_storage(|| {
            ctx.entry_btree_map(
                &mut body_presence,
                body.id.as_str(),
                "creo output body presence nodes",
            )
        })? {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(true);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                entry.insert(false);
            }
        }
    }
    for shell_id in ctx.admit_iter(shell_ids, "creo selected shell identity traversal")? {
        let Some(body) = owners.body(ctx, shell_id.as_str())? else {
            continue;
        };
        if ctx.get_btree_map(
            &body_presence,
            body.as_str(),
            "creo selected region body identity comparison",
        )? != Some(&true)
        {
            continue;
        }
        if !ctx.contains(&bodies.bodies, &body, "creo selected body membership")? {
            bodies.storage.with_storage(|| {
                ctx.push_vec(
                    &mut bodies.bodies,
                    body,
                    "creo bodies containing selected edges",
                )
            })?;
        }
    }
    Ok(bodies)
}

fn append_evaluated_sweep_output_body_candidates<'ir, 'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    ir: &'ir CadIr,
    feature_id: u32,
    outputs: &mut FeatureOutputCandidates<'ir, 'ctx>,
) -> Result<(), CodecError> {
    for namespace in [
        &crate::identity::FEATURE_EXTRUSION,
        &crate::identity::FEATURE_REVOLUTION,
    ] {
        let candidate_storage = ctx.format_scoped(
            format_args!(
                "{}:{}:{}#{feature_id}:body",
                namespace.format(),
                namespace.scope(),
                namespace.kind(),
            ),
            "creo evaluated sweep body candidate",
        )?;
        let matching_body = crate::decode::uniqueness::exactly_one_by(
            ctx,
            &ir.model.bodies,
            |body| {
                ctx.equal(
                    body.id.as_str(),
                    candidate_storage.0.as_str(),
                    "creo evaluated sweep body identity comparison",
                )
            },
            "creo evaluated sweep body lookup traversal",
        )?;
        if let Some(body) = matching_body {
            outputs.storage.with_storage(|| {
                ctx.push_vec(
                    &mut outputs.bodies,
                    &body.id,
                    "creo evaluated sweep output bodies",
                )
            })?;
        }
    }
    Ok(())
}

pub(in super::super) fn evaluated_sweep_body_kind(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    family: &str,
    feature_id: u32,
) -> Result<Option<BodyKind>, CodecError> {
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
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
    if let Some(refusal) = ctx.resource_refusal() {
        return Err(refusal.into());
    }
    let mut selected = [None; 3];
    let mut rows = tables.iter();
    while rows.len() != 0 {
        let Some(table) = ctx.next_charged(&mut rows, "creo new sheet entity tables")? else {
            break;
        };
        if table.feature_id != feature_id {
            continue;
        }
        let slot = match table.table_class_id {
            67 => 0,
            100 => 1,
            29 => 2,
            _ => continue,
        };
        if selected[slot].replace(table).is_some() {
            return Ok(None);
        }
    }
    let [Some(owner_table), Some(output_table), Some(generated)] = selected else {
        return Ok(None);
    };
    let [owner] = owner_table.entries.as_slice() else {
        return Ok(None);
    };
    let [output] = output_table.entries.as_slice() else {
        return Ok(None);
    };
    if owner.source_entity_id() != Some(feature_id)
        || output.entity_id != owner.entity_id
        || !generated.contains_surface_id(output.class_id())
        || !ctx.any_by(
            &generated.entries,
            |entry| Ok(entry.entity_id == output.class_id() && entry.class_id() == 200),
            "creo new sheet generated entity entries",
        )?
    {
        return Ok(None);
    }
    Ok(crate::decode::uniqueness::exactly_one_by(
        ctx,
        surface_rows,
        |row| Ok(row.id == output.class_id() && row.feature_id == feature_id),
        "creo new sheet surface rows",
    )?
    .map(|surface| surface.id))
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
    Ok(crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.features.operations,
        |operation| Ok(operation.feature_id == feature_id),
        "creo current feature operation lookup",
    )?
    .filter(|operation| operation.kind.as_str() == "Surface")
    .map(|_| BodyKind::Sheet))
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
    let base_storage = ctx.format_scoped(
        format_args!("{base}"),
        "creo feature parameter key candidate",
    )?;
    let (key, key_reservation) = if ctx.contains_key_btree_map(
        parameters,
        &base_storage.0,
        "creo feature parameter key lookup",
    )? {
        let mut occurrence = 2usize;
        loop {
            let candidate = ctx.format_scoped(
                format_args!("{}#{occurrence}", base_storage.0),
                "creo feature parameter key candidate",
            )?;
            if !ctx.contains_key_btree_map(
                parameters,
                &candidate.0,
                "creo feature parameter key lookup",
            )? {
                drop(base_storage);
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
        base_storage
    };
    let key = text_storage.with_storage(|| key_reservation.commit_value(key))?;
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
    if let Some(existing) =
        ctx.get_mut_btree_map(parameters, key, "creo feature parameter replacement lookup")?
    {
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
        BTreeMap<String, String>,
        ScopedReservation<'ctx>,
        ScopedReservation<'ctx>,
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
        for entry in ctx.admit_iter(
            table.entries.as_slice(),
            "creo feature parameter entity entries",
        )? {
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
    if let Some(definition) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.features.definitions,
        |definition| Ok(definition.identity.owner_feature_id() == Some(feature_id)),
        "creo feature parameter definition lookup",
    )? {
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
    let mut recipe_kind = None;
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
        let mut sketch_storage = ctx.reserve_scoped(0, "creo history sketch lookup")?;
        let Some(profile_sketch) =
            sketch_storage.with_storage(|| model_sketch_id(ctx, scan, definition))?
        else {
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
        let kind = match recipe_kind {
            Some(kind) => kind,
            None => {
                let kind = super::operations::feature_recipe(ctx, scan, feature_id)?
                    .map(crate::feature::operations::FeatureRecipe::kind);
                recipe_kind = Some(kind);
                kind
            }
        };
        if kind == Some(crate::feature::operations::FeatureRecipeKind::Extrude) {
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
    Ok((parameters, text_storage, node_storage))
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
    let records = &scan.features.reference_names;
    let Some(first) = ctx.position_by(
        records,
        |record| Ok(record.feature_id == feature_id),
        "creo feature reference names",
    )?
    else {
        return Ok(None);
    };
    let bytes = records[first].name_bytes.as_slice();
    Ok(ctx
        .all_by(
            &records[first + 1..],
            |record| {
                if record.feature_id != feature_id {
                    return Ok(true);
                }
                ctx.equal_bytes(
                    record.name_bytes.as_slice(),
                    bytes,
                    "creo feature reference name agreement",
                )
            },
            "creo feature reference names",
        )?
        .then_some(bytes))
}

pub(in super::super) fn decoded_feature_reference_name<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<Cow<'a, str>, CodecError> {
    match ctx.validate_utf8(bytes, "creo feature reference name UTF-8")? {
        Ok(name) => Ok(Cow::Borrowed(name)),
        Err(_) => ctx
            .copy_retained_lossy_utf8(bytes, "creo decoded feature reference name")
            .map(Cow::Owned),
    }
}

pub(in super::super) fn owned_section_feature_id(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    definition_id: u32,
) -> Result<Option<u32>, CodecError> {
    let Some(definition) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.features.definitions,
        |definition| Ok(definition.identity.id() == definition_id),
        "creo history section definition lookup",
    )?
    else {
        return Ok(None);
    };
    Ok(crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.features.rows,
        |row| {
            Ok(row.root_schema_class == Some(SchemaClass::Section)
                && definition.offset >= row.body_offset
                && row
                    .body_offset
                    .checked_add(row.body.len())
                    .is_some_and(|end| definition.offset < end))
        },
        "creo history section owner lookup",
    )?
    .map(|row| row.feature_id))
}

pub(super) fn section_definition_for_history_feature<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<&'a crate::feature::definitions::FeatureDefinition>, CodecError> {
    let Some(row) = crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.features.rows,
        |row| {
            Ok(row.feature_id == feature_id && row.root_schema_class == Some(SchemaClass::Section))
        },
        "creo history section feature lookup",
    )?
    else {
        return Ok(None);
    };
    crate::decode::uniqueness::exactly_one_by(
        ctx,
        &scan.features.definitions,
        |definition| {
            Ok(definition.offset >= row.body_offset
                && row
                    .body_offset
                    .checked_add(row.body.len())
                    .is_some_and(|end| definition.offset < end))
        },
        "creo history owned section definition lookup",
    )
}

pub(in super::super) fn feature_source_properties<'ctx>(
    ctx: &'ctx DecodeContext<'_>,
    scan: &ContainerScan,
    feature_id: u32,
) -> Result<(BTreeMap<String, String>, ScopedReservation<'ctx>), CodecError> {
    let mut node_storage = ctx.reserve_scoped(0, "creo feature source property nodes")?;
    let mut properties = BTreeMap::new();
    if let Some(recipe) = super::operations::feature_recipe(ctx, scan, feature_id)? {
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
    Ok((properties, node_storage))
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
