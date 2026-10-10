// SPDX-License-Identifier: Apache-2.0
//! STEP presentation style and topology color decoding.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet};

use super::reference::{references, References};
use super::{RecordExt, ValueExt};
use cadmpeg_core::decode::{DecodeContext, DepthGuard, ScopedReservation};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    AppearanceId, BodyId, CurveId, EdgeId, FaceId, IdentityKey, LayerId, OccurrenceId, PmiId,
    PointId, ProductDefinitionId, SurfaceId, VertexId,
};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::scalar::Fraction;
use cadmpeg_ir::topology::Color;

use crate::ids;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::topology::TopologyData;
use super::StageOutcome;
use super::{decode_output_text, decode_text_scoped};

pub(super) fn decode<'ctx>(
    exchange: &Exchange,
    topology: &TopologyData,
    ir: &mut CadIr,
    product_definition_ids_by_source: &BTreeMap<u64, Vec<ProductDefinitionId>>,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<
    StageOutcome<(
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        cadmpeg_core::decode::ScopedReservation<'ctx>,
    )>,
    CodecError,
> {
    let slot_storage = std::cell::RefCell::new(ctx.reserve_scoped(0, "STEP stage report buffers")?);
    let mut claim_storage = ctx.reserve_scoped(0, "STEP stage claim storage")?;
    let mut scratch_storage = ctx.reserve_scoped(0, "STEP decode scratch")?;
    let mut style_domains = StyleDomainIndex::new(ctx)?;
    let mut invisible = InvisibleIndex::new(ctx)?;
    let mut typed = BTreeSet::new();
    let mut losses = Vec::new();
    let graph_limit = super::record_graph_limit(ctx);
    let (face_indices_buffer, _face_index_storage) =
        ctx.with_scoped_storage("STEP presentation face index scratch", || {
            collect_identity_indices(
                ir.model.faces.iter().map(|face| face.id.as_str()),
                ctx,
                "step_presentation_face_indices",
            )
        })?;
    let face_indices = face_indices_buffer;
    let (body_indices_buffer, _body_index_storage) =
        ctx.with_scoped_storage("STEP presentation body index scratch", || {
            collect_identity_indices(
                ir.model.bodies.iter().map(|body| body.id.as_str()),
                ctx,
                "step_presentation_body_indices",
            )
        })?;
    let body_indices = body_indices_buffer;
    let indices = PresentationIndices {
        faces: &face_indices,
        bodies: &body_indices,
    };
    let (entity_ids_buffer, _entity_id_storage) =
        ctx.with_scoped_storage("STEP presentation entity index scratch", || {
            Ok::<_, CodecError>(EntityIds {
                edges: ctx.collect_btree_set(
                    ir.model.edges.iter().map(|item| item.id.as_str()),
                    "step_presentation_edge_ids",
                )?,
                vertices: ctx.collect_btree_set(
                    ir.model.vertices.iter().map(|item| item.id.as_str()),
                    "step_presentation_vertex_ids",
                )?,
                points: ctx.collect_btree_set(
                    ir.model.points.iter().map(|item| item.id.as_str()),
                    "step_presentation_point_ids",
                )?,
                curves: ctx.collect_btree_set(
                    ir.model.curves.iter().map(|item| item.id.as_str()),
                    "step_presentation_curve_ids",
                )?,
                surfaces: ctx.collect_btree_set(
                    ir.model.surfaces.iter().map(|item| item.id.as_str()),
                    "step_presentation_surface_ids",
                )?,
                products: product_definition_ids_by_source,
                occurrences: ctx.collect_btree_set(
                    ir.model.occurrences.iter().map(|item| item.id.as_str()),
                    "step_presentation_occurrence_ids",
                )?,
                pmi: ctx.collect_btree_set(
                    ir.model.pmi.iter().map(|item| item.id.as_str()),
                    "step_presentation_pmi_ids",
                )?,
                tessellations: ctx.collect_btree_set(
                    ir.model.tessellations.iter().map(|item| item.id.as_str()),
                    "step_presentation_tessellation_ids",
                )?,
            })
        })?;
    let entity_ids = entity_ids_buffer;
    let mut appearance_ids = BTreeMap::<(u64, u32), AppearanceId>::new();
    let mut emitted_style_ancestors = BTreeSet::new();
    let mut emitted_layer_ids = BTreeSet::new();
    let mut hidden_style_ids = BTreeSet::new();
    let mut hidden_layer_ids = BTreeSet::new();
    let mut deferred_invisibility = BTreeMap::<u64, (bool, BTreeSet<u64>, BTreeSet<u64>)>::new();
    let mut records = exchange.records().iter();
    ctx.charge_work(0, "STEP decode traversal")?;
    for _ in 0..records.len() {
        let (&id, record) = ctx
            .next_charged(&mut records, "STEP decode traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        let Some(invisibility) = record.partial(ctx, "INVISIBILITY")? else {
            continue;
        };
        let Some(items) = invisibility.parameters.first().and_then(ValueExt::list) else {
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("INVISIBILITY #{id} has no item set"),
                    "step_presentation_loss_text",
                )?),
                "step_presentation_losses",
            )?;
            continue;
        };
        let mut supported = true;
        let mut style_targets = BTreeSet::new();
        let mut layer_targets = BTreeSet::new();
        let mut items = items.iter();
        ctx.charge_work(0, "STEP invisibility item traversal")?;
        for _ in 0..items.len() {
            let value = ctx
                .next_charged(&mut items, "STEP invisibility item traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            let Some(target) = ValueExt::reference(value) else {
                continue;
            };
            let target_record =
                ctx.get_btree_map(exchange.records(), &target, "STEP presentation record get")?;
            if target_record
                .map(|record| record.partial(ctx, "PRESENTATION_LAYER_ASSIGNMENT"))
                .transpose()?
                .flatten()
                .is_some()
            {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut hidden_layer_ids,
                        target,
                        "step_presentation_hidden_layer_ids",
                    )
                })?;
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut layer_targets,
                        target,
                        "step_presentation_invisibility_layer_targets",
                    )
                })?;
                continue;
            }
            if target_record
                .map(|record| styled_item_parts(ctx, record))
                .transpose()?
                .flatten()
                .is_some()
            {
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut hidden_style_ids,
                        target,
                        "step_presentation_hidden_style_ids",
                    )
                })?;
                scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut style_targets,
                        target,
                        "step_presentation_invisibility_style_targets",
                    )
                })?;
                continue;
            }
            if let Some(record) = target_record {
                if super::drawing::is_supported_invisibility_target(ctx, record)? {
                    continue;
                }
            }
            if let Some(record) = target_record {
                if super::pmi::is_supported_invisibility_target(ctx, record)? {
                    continue;
                }
            }
            let (summary, body_ids, _body_storage) =
                invisible.resolve(target, exchange, topology, &body_indices)?;
            let mut body_ids = body_ids.into_iter();
            ctx.charge_work(0, "STEP presentation body_ids traversal")?;
            for _ in 0..body_ids.len() {
                let (_body_id, index) = ctx
                    .next_charged(&mut body_ids, "STEP presentation body_ids traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
                if let Some(index) = index {
                    ir.model.bodies[index].visible = Some(false);
                }
            }
            if !summary.is_hidden() {
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
                    &mut losses,
                    StepLossCode::DecodeWarning.note(ctx.format_retained(
                        format_args!("INVISIBILITY #{id} targets unsupported item #{target}"),
                        "step_presentation_loss_text",
                    )?),
                    "step_presentation_losses",
                )?;
                supported = false;
            }
        }
        if style_targets.is_empty() && layer_targets.is_empty() && supported {
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(&mut typed, id, "step_presentation_typed_claims")
            })?;
        } else if !style_targets.is_empty() || !layer_targets.is_empty() {
            scratch_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut deferred_invisibility,
                    id,
                    (supported, style_targets, layer_targets),
                    "step_presentation_deferred_invisibility",
                )
            })?;
        }
    }
    let mut records = exchange.records().iter();
    ctx.charge_work(0, "STEP decode traversal")?;
    for _ in 0..records.len() {
        let (&layer_id, layer) = ctx
            .next_charged(&mut records, "STEP decode traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        let Some(assignment) = layer.partial(ctx, "PRESENTATION_LAYER_ASSIGNMENT")? else {
            continue;
        };
        let Some(assigned_items) = assignment.parameters.get(2).and_then(ValueExt::list) else {
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no assigned item set"
                    ),
                    "step_presentation_loss_text",
                )?),
                "step_presentation_losses",
            )?;
            continue;
        };
        if assigned_items.is_empty() {
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!(
                        "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has an empty assigned item set"
                    ),
                    "step_presentation_loss_text",
                )?),
                "step_presentation_losses",
            )?;
            continue;
        }
        let Some(name) = assignment
            .parameters
            .first()
            .map(|value| {
                decode_output_text(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    layer_id,
                    "presentation layer name",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
        else {
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no name"),
                    "step_presentation_loss_text",
                )?),
                "step_presentation_losses",
            )?;
            continue;
        };
        let description = assignment
            .parameters
            .get(1)
            .map(|value| {
                decode_output_text(
                    exchange,
                    value,
                    (&mut losses, &slot_storage),
                    layer_id,
                    "presentation layer description",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
            .filter(|value| !value.is_empty());
        let mut items = Vec::new();
        let mut assigned_items = assigned_items.iter();
        ctx.charge_work(0, "STEP layer assigned item traversal")?;
        for _ in 0..assigned_items.len() {
            let value = ctx
                .next_charged(&mut assigned_items, "STEP layer assigned item traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            let Some(id) = ValueExt::reference(value) else {
                continue;
            };
            append_presentation_items(
                id,
                exchange,
                topology,
                &entity_ids,
                indices,
                &mut items,
                ctx,
            )?;
        }
        ctx.push_vec(
            &mut ir.model.presentation_layers,
            PresentationLayer {
                id: LayerId::from(ids::presentation(kind!("layer"), layer_id)),
                name,
                description,
                visible: ctx
                    .contains_btree_set(
                        &hidden_layer_ids,
                        &layer_id,
                        "STEP presentation hidden_layer_ids contains",
                    )?
                    .then_some(false),
                items,
            },
            "step_presentation_layer_records",
        )?;
        scratch_storage.with_storage(|| {
            ctx.insert_btree_set(&mut emitted_layer_ids, layer_id, "STEP emitted layer IDs")
        })?;
        claim_storage.with_storage(|| {
            ctx.insert_btree_set(&mut typed, layer_id, "step_presentation_typed_claims")
        })?;
    }
    let mut styles = Vec::new();
    let mut style_depths = BTreeMap::new();
    let mut style_visibility = BTreeMap::new();
    let mut records = exchange.records().iter();
    ctx.charge_work(0, "STEP decode traversal")?;
    for _ in 0..records.len() {
        let (&id, record) = ctx
            .next_charged(&mut records, "STEP decode traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        if styled_item_parts(ctx, record)?.is_some() {
            let order = style_application_order(
                id,
                exchange,
                graph_limit,
                &mut style_depths,
                &mut scratch_storage,
                ctx,
            )?;
            scratch_storage.with_storage(|| {
                ctx.push_vec(&mut styles, (id, order), "step_presentation_style_ids")
            })?;
        }
    }
    let mut overridden_styles = BTreeSet::new();
    let mut styles_to_check = styles.iter();
    ctx.charge_work(0, "STEP decode traversal")?;
    for _ in 0..styles_to_check.len() {
        let &(id, _) = ctx
            .next_charged(&mut styles_to_check, "STEP decode traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        if let Some(overridden) = overridden_style(
            ctx,
            ctx.get_btree_map(
                exchange.records(),
                &id,
                "STEP overridden style record lookup",
            )?
            .ok_or_else(|| CodecError::malformed("STEP styled item was not indexed"))?,
        )? {
            scratch_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut overridden_styles,
                    overridden,
                    "step_presentation_overridden_styles",
                )
            })?;
        }
    }
    ctx.stable_sort_by(
        &mut styles,
        |value| &value.1,
        Ord::cmp,
        "step_presentation_style_ids_sort",
    )?;
    let mut scalar_color_candidates: ScalarCandidates = std::array::from_fn(|_| BTreeMap::new());
    let mut style_colors = StyleColors {
        exchange,
        prefixes: BTreeMap::new(),
        values: BTreeMap::new(),
        queries: Vec::new(),
        frames: Vec::new(),
        frame_storage: ctx.reserve_scoped(0, "STEP shared color frames")?,
        completed: ColorCompletions::new(ctx)?,
        ctx,
        storage: ctx.reserve_scoped(0, "STEP shared color scratch")?,
    };
    let mut styles = styles.into_iter();
    ctx.charge_work(0, "STEP presentation styles traversal")?;
    for _ in 0..styles.len() {
        let (style_id, _) = ctx
            .next_charged(&mut styles, "STEP presentation styles traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        if ctx.contains_btree_set(
            &overridden_styles,
            &style_id,
            "STEP presentation overridden_styles contains",
        )? {
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(&mut typed, style_id, "step_presentation_typed_claims")
            })?;
            continue;
        }
        let style = ctx
            .get_btree_map(
                exchange.records(),
                &style_id,
                "STEP styled item record lookup",
            )?
            .ok_or_else(|| CodecError::malformed("STEP styled item was not indexed"))?;
        let Some(parts) = styled_item_parts(ctx, style)? else {
            continue;
        };
        let Some(target_step) = parts.target.reference() else {
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                &mut losses,
                StepLossCode::DecodeWarning.note(ctx.format_retained(
                    format_args!("STYLED_ITEM #{style_id} has no resolved target"),
                    "step_presentation_loss_text",
                )?),
                "step_presentation_losses",
            )?;
            continue;
        };
        if parts.styles.list().is_some_and(<[Value]>::is_empty) {
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(&mut typed, style_id, "step_presentation_typed_claims")
            })?;
            continue;
        }
        let domain = style_domains.domain(target_step, exchange)?;
        let color_storage =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "step color search storage")?);
        let mut style_storage = ctx.reserve_scoped(0, "STEP style selection scratch")?;
        let mut style_references = Vec::new();
        let mut values = parts.styles.list().unwrap_or_default().iter();
        ctx.charge_work(0, "STEP style value traversal")?;
        for _ in 0..values.len() {
            let value = ctx
                .next_charged(&mut values, "STEP style value traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            for reference in references(value, ctx) {
                let reference = reference?;
                style_storage.with_storage(|| {
                    ctx.push_vec(
                        &mut style_references,
                        reference,
                        "step_presentation_style_references",
                    )
                })?;
            }
        }
        let mut context_style_ids = BTreeSet::new();
        let mut references = style_references.iter();
        ctx.charge_work(0, "STEP decode traversal")?;
        for _ in 0..references.len() {
            let reference = ctx
                .next_charged(&mut references, "STEP decode traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx
                .get_btree_map(
                    exchange.records(),
                    reference,
                    "STEP presentation record get",
                )?
                .map(|record| is_presentation_style_by_context(ctx, record))
                .transpose()?
                .unwrap_or(false)
            {
                style_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut context_style_ids,
                        *reference,
                        "step_presentation_context_style_ids",
                    )
                })?;
            }
        }
        if !context_style_ids.is_empty() {
            let message = context_style_message(style_id, &context_style_ids, exchange, ctx)?;
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                &mut losses,
                StepLossCode::ContextDependentStyleUnresolved.note(message),
                "step_presentation_losses",
            )?;
            continue;
        }
        let (color_query, resolved) = style_colors.resolve(
            &style_references,
            domain,
            &color_storage,
            (&mut losses, &slot_storage),
        )?;
        let color = match &resolved.color {
            Some(ColorResolution::Candidate(candidate)) => candidate,
            Some(ColorResolution::Ambiguous { .. }) => {
                ctx.push_scoped_vec(&mut slot_storage.borrow_mut(), &mut losses, StepLossCode::ConflictingScalarColors.note(ctx.format_retained(format_args!(
                    "STYLED_ITEM #{style_id} has distinct equal-precedence colors; no scalar color is selected and the source style graph remains retained"
                ), "step_presentation_loss_text")?), "step_presentation_losses")?;
                continue;
            }
            None => {
                let mut visited = BTreeSet::new();
                if !style_storage.with_storage(|| {
                    contains_null_style(parts.styles, exchange, &mut visited, 0, ctx)
                })? {
                    ctx.push_scoped_vec(
                        &mut slot_storage.borrow_mut(),
                        &mut losses,
                        StepLossCode::DecodeWarning.note(ctx.format_retained(
                            format_args!("STYLED_ITEM #{style_id} has no resolved surface color"),
                            "step_presentation_loss_text",
                        )?),
                        "step_presentation_losses",
                    )?;
                }
                continue;
            }
        };
        let color_id = color.id;
        let name = color.name.as_deref();
        let color = color.color;
        let appearance_key = (color_id, color.a().to_bits());
        let appearance_entry = scratch_storage.with_storage(|| {
            ctx.entry_btree_map(
                &mut appearance_ids,
                appearance_key,
                "step_presentation_appearance_ids",
            )
        })?;
        let appearance_id = match appearance_entry {
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let key = if color.a() == 1.0 {
                    IdentityKey::from(color_id)
                } else {
                    IdentityKey::from(color_id)
                        .dash(key_word!("alpha"))
                        .dash(color.a().to_bits())
                };
                let id = AppearanceId::from(ids::presentation(kind!("appearance"), key));
                ctx.push_vec(
                    &mut ir.model.appearances,
                    Appearance {
                        id: id.try_clone_for_decode(ctx, "step_presentation_identity_copy")?,
                        name: name
                            .map(|name| ctx.copy_retained_text(name, "step_string_text"))
                            .transpose()?,
                        asset_guid: None,
                        library_id: None,
                        visual_guid: None,
                        physical_token: None,
                        schema: Some(ctx.copy_retained_text(
                            "step_surface_style",
                            "step_presentation_schema_text",
                        )?),
                        category: None,
                        base_color: Some(color),
                        textures: Vec::new(),
                        properties: BTreeMap::new(),
                    },
                    "step_presentation_appearance_records",
                )?;
                let stored = scratch_storage.with_storage(|| {
                    id.try_clone_for_decode(ctx, "step_presentation_identity_copy")
                })?;
                entry.insert(stored)
            }
        };
        let mut emitted = false;
        let mut target_storage = ctx.reserve_scoped(0, "STEP expanded style target scratch")?;
        let mut target_steps = Vec::new();
        let mut active_storage = ctx.reserve_scoped(0, "STEP style target active scratch")?;
        let mut active = BTreeSet::new();
        expand_style_targets(
            target_step,
            exchange,
            (&mut typed, &mut claim_storage),
            (&mut active, &mut active_storage),
            (0, graph_limit),
            &mut |target| {
                ctx.push_scoped_vec(
                    &mut target_storage,
                    &mut target_steps,
                    target,
                    "step_presentation_expanded_targets",
                )
            },
            ctx,
        )?;
        drop(active);
        drop(active_storage);
        let mut target_steps = target_steps.into_iter().enumerate();
        for _ in 0..target_steps.len() {
            let (target_ordinal, target_step) = ctx
                .next_charged(&mut target_steps, "STEP expanded style target traversal")?
                .ok_or_else(|| {
                    CodecError::malformed("STEP expanded style target source ended early")
                })?;
            let mut appearance_target_storage =
                ctx.reserve_scoped(0, "STEP appearance target slots")?;
            let targets = appearance_targets(
                target_step,
                exchange,
                topology,
                &entity_ids,
                indices,
                &mut appearance_target_storage,
                ctx,
            )?;
            if targets.is_empty() {
                let message = ctx.format_retained(
                    format_args!("STYLED_ITEM #{style_id} targets unsupported item #{target_step}"),
                    "step_presentation_unsupported_target_text",
                )?;
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
                    &mut losses,
                    StepLossCode::DecodeWarning.note(message),
                    "step_presentation_losses",
                )?;
                continue;
            }
            let mut targets = targets.into_iter().enumerate();
            ctx.charge_work(0, "STEP appearance target traversal")?;
            for _ in 0..targets.len() {
                let (target_index, target) = ctx
                    .next_charged(&mut targets, "STEP appearance target traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
                scratch_storage.with_storage(|| {
                    push_scalar_candidate(
                        &mut scalar_color_candidates,
                        &target,
                        style_id,
                        color,
                        ctx,
                    )
                })?;
                emitted = true;
                ctx.reserve_vec(
                    &mut ir.model.appearance_bindings,
                    1,
                    "step_presentation_appearance_bindings",
                )?;
                let binding = AppearanceBinding {
                    id: ids::presentation(
                        kind!("binding"),
                        IdentityKey::from(style_id)
                            .colon(target_ordinal)
                            .dash(target_index),
                    )
                    .into(),
                    target,
                    appearance: appearance_id
                        .try_clone_for_decode(ctx, "step_presentation_identity_copy")?,
                    source_entity_id: Some(ctx.format_retained(
                        format_args!("#{style_id}"),
                        "step_presentation_source_entity_id",
                    )?),
                    object_type: None,
                    visible: style_is_hidden(
                        style_id,
                        &hidden_style_ids,
                        exchange,
                        &mut style_visibility,
                        &mut scratch_storage,
                        ctx,
                    )?
                    .then_some(false),
                    channels: BTreeMap::new(),
                };
                ir.model.appearance_bindings.push(binding);
            }
        }
        if emitted && !deferred_invisibility.is_empty() {
            let mut ancestor = Some(style_id);
            while let Some(id) = ancestor {
                ctx.charge_work(1, "STEP emitted style ancestry step")?;
                if !scratch_storage.with_storage(|| {
                    ctx.insert_btree_set(
                        &mut emitted_style_ancestors,
                        id,
                        "STEP emitted style ancestors",
                    )
                })? {
                    break;
                }
                ancestor = ctx
                    .get_btree_map(
                        exchange.records(),
                        &id,
                        "STEP emitted style ancestor lookup",
                    )?
                    .map(|record| overridden_style(ctx, record))
                    .transpose()?
                    .flatten();
            }
        }
        claim_storage.with_storage(|| {
            ctx.insert_btree_set(&mut typed, style_id, "step_presentation_typed_claims")
        })?;
        if let Some(overridden) = overridden_style(ctx, style)? {
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(&mut typed, overridden, "step_presentation_typed_claims")
            })?;
        }
        style_colors.claim(color_query, domain, (&mut typed, &mut claim_storage))?;
        claim_storage.with_storage(|| {
            ctx.insert_btree_set(&mut typed, color_id, "step_presentation_typed_claims")
        })?;
    }
    let mut deferred_invisibility = deferred_invisibility.into_iter();
    ctx.charge_work(0, "STEP deferred invisibility traversal")?;
    for _ in 0..deferred_invisibility.len() {
        let (invisibility_id, (mut supported, style_targets, layer_targets)) = ctx
            .next_charged(
                &mut deferred_invisibility,
                "STEP deferred invisibility traversal",
            )?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        let mut style_targets = style_targets.into_iter();
        ctx.charge_work(0, "STEP deferred invisible style traversal")?;
        for _ in 0..style_targets.len() {
            let style_id = ctx
                .next_charged(
                    &mut style_targets,
                    "STEP deferred invisible style traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if !ctx.contains_btree_set(
                &emitted_style_ancestors,
                &style_id,
                "STEP emitted style ancestor lookup",
            )? {
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
                    &mut losses,
                    StepLossCode::DecodeWarning.note(ctx.format_retained(
                        format_args!(
                            "INVISIBILITY #{invisibility_id} targets unsupported item #{style_id}"
                        ),
                        "step_presentation_loss_text",
                    )?),
                    "step_presentation_losses",
                )?;
                supported = false;
            }
        }
        let mut layer_targets = layer_targets.into_iter();
        ctx.charge_work(0, "STEP deferred invisible layer traversal")?;
        for _ in 0..layer_targets.len() {
            let layer_id = ctx
                .next_charged(
                    &mut layer_targets,
                    "STEP deferred invisible layer traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if !ctx.contains_btree_set(
                &emitted_layer_ids,
                &layer_id,
                "STEP emitted layer lookup",
            )? {
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
                    &mut losses,
                    StepLossCode::DecodeWarning.note(ctx.format_retained(
                        format_args!(
                            "INVISIBILITY #{invisibility_id} targets unsupported item #{layer_id}"
                        ),
                        "step_presentation_loss_text",
                    )?),
                    "step_presentation_losses",
                )?;
                supported = false;
            }
        }
        if supported {
            claim_storage.with_storage(|| {
                ctx.insert_btree_set(
                    &mut typed,
                    invisibility_id,
                    "step_presentation_typed_claims",
                )
            })?;
        }
    }
    for lane in scalar_color_candidates {
        let mut groups = lane.into_iter();
        ctx.charge_work(0, "STEP scalar color group traversal")?;
        for _ in 0..groups.len() {
            let (_, (target, candidates)) = ctx
                .next_charged(&mut groups, "STEP scalar color group traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            let mut selected = None::<Color>;
            let mut conflicting = false;
            // The candidate vector is a complete, infallible reduction. It is
            // fully consumed before any lookup, formatting, or output admission.
            for (_, color) in
                ctx.admit_iter(&candidates, "STEP scalar color candidate traversal")?
            {
                match selected {
                    None => selected = Some(*color),
                    Some(existing)
                        if existing.r() != color.r()
                            || existing.g() != color.g()
                            || existing.b() != color.b() =>
                    {
                        conflicting = true;
                    }
                    Some(existing) if color.a() < existing.a() => selected = Some(*color),
                    Some(_) => {}
                }
            }
            if let Some(color) = selected.filter(|_| !conflicting) {
                match target {
                    AppearanceTarget::Face(face) => {
                        if let Some(&index) = ctx.get_btree_map(
                            &face_indices,
                            face.as_str(),
                            "STEP presentation face_indices get",
                        )? {
                            ir.model.faces[index].color = Some(color);
                        }
                    }
                    AppearanceTarget::Body(body) => {
                        if let Some(&index) = ctx.get_btree_map(
                            &body_indices,
                            body.as_str(),
                            "STEP presentation body_indices get",
                        )? {
                            ir.model.bodies[index].color = Some(color);
                        }
                    }
                    _ => {}
                }
            } else {
                let message = scalar_conflict_message(&candidates, &target, ctx)?;
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
                    &mut losses,
                    StepLossCode::ConflictingScalarColors.note(message),
                    "step_presentation_losses",
                )?;
            }
        }
    }
    Ok(StageOutcome {
        value: (claim_storage, slot_storage.into_inner()),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InvisibleSummary {
    Unsupported,
    Supported { hidden: bool },
}

impl InvisibleSummary {
    fn is_hidden(self) -> bool {
        matches!(self, Self::Supported { hidden: true })
    }
}

struct InvisibleIndex<'ctx, 'arena> {
    complete: BTreeMap<u64, InvisibleSummary>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'arena> InvisibleIndex<'ctx, 'arena> {
    fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            complete: BTreeMap::new(),
            ctx,
            storage: ctx.reserve_scoped(0, "STEP invisible stage completion scratch")?,
        })
    }

    fn resolve(
        &mut self,
        id: u64,
        exchange: &Exchange,
        topology: &TopologyData,
        body_indices: &BTreeMap<String, usize>,
    ) -> Result<
        (
            InvisibleSummary,
            BTreeMap<BodyId, Option<usize>>,
            ScopedReservation<'ctx>,
        ),
        CodecError,
    > {
        let ctx = self.ctx;
        let mut body_storage = ctx.reserve_scoped(0, "STEP invisible body selection scratch")?;
        let mut body_ids = BTreeMap::new();
        let mut walk = InvisibleWalk {
            active: BTreeSet::new(),
            complete: &mut self.complete,
            storage: &mut self.storage,
            active_storage: ctx.reserve_scoped(0, "STEP invisible active scratch")?,
        };
        let summary = body_storage.with_storage(|| {
            collect_invisible_body_ids(
                id,
                exchange,
                topology,
                body_indices,
                &mut walk,
                &mut body_ids,
                ctx,
            )
        })?;
        Ok((summary, body_ids, body_storage))
    }
}

struct InvisibleWalk<'stage, 'ctx> {
    active: BTreeSet<u64>,
    complete: &'stage mut BTreeMap<u64, InvisibleSummary>,
    storage: &'stage mut ScopedReservation<'ctx>,
    active_storage: ScopedReservation<'ctx>,
}

fn collect_invisible_body_ids(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    body_indices: &BTreeMap<String, usize>,
    walk: &mut InvisibleWalk<'_, '_>,
    body_ids: &mut BTreeMap<BodyId, Option<usize>>,
    ctx: &DecodeContext<'_>,
) -> Result<InvisibleSummary, CodecError> {
    if ctx.contains_btree_set(&walk.active, &id, "STEP presentation active contains")? {
        return Ok(InvisibleSummary::Unsupported);
    }
    if let Some(supported) =
        ctx.get_btree_map(&walk.complete, &id, "STEP invisible completion lookup")?
    {
        return Ok(*supported);
    }
    let supported = collect_invisible_body_ids_uncached(
        id,
        exchange,
        topology,
        body_indices,
        walk,
        body_ids,
        ctx,
    )?;
    walk.storage.with_storage(|| {
        ctx.insert_btree_map(walk.complete, id, supported, "step_invisible_complete")
    })?;
    Ok(supported)
}

fn collect_invisible_body_ids_uncached(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    body_indices: &BTreeMap<String, usize>,
    walk: &mut InvisibleWalk<'_, '_>,
    body_ids: &mut BTreeMap<BodyId, Option<usize>>,
    ctx: &DecodeContext<'_>,
) -> Result<InvisibleSummary, CodecError> {
    let _nested = ctx.enter_nested("step_presentation_invisible_body_walk")?;
    walk.active_storage.with_storage(|| {
        ctx.insert_btree_set(
            &mut walk.active,
            id,
            "step_presentation_invisible_body_active",
        )
    })?;
    if let Some(ids) = ctx.get_btree_map(
        &topology.body_by_root,
        &id,
        "STEP presentation topology.body_by_root get",
    )? {
        let mut hidden = false;
        let mut source_body_ids = ids.iter();
        ctx.charge_work(0, "STEP presentation ids traversal")?;
        for _ in 0..source_body_ids.len() {
            let body = ctx
                .next_charged(&mut source_body_ids, "STEP presentation ids traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            let index = if let Some(index) =
                ctx.get_btree_map(body_ids, body, "STEP body ids membership")?
            {
                *index
            } else {
                let body =
                    body.try_clone_for_decode(ctx, "step_presentation_invisible_body_identity")?;
                let index = ctx
                    .get_btree_map(
                        body_indices,
                        body.as_str(),
                        "STEP presentation body_indices get",
                    )?
                    .copied();
                ctx.insert_btree_map(
                    body_ids,
                    body,
                    index,
                    "step_presentation_invisible_body_ids",
                )?;
                index
            };
            hidden |= index.is_some();
        }
        ctx.remove_btree_set(&mut walk.active, &id, "STEP presentation active remove")?;
        return Ok(if ids.is_empty() {
            InvisibleSummary::Unsupported
        } else {
            InvisibleSummary::Supported { hidden }
        });
    }
    let fallback = BodyId::from(ids::data(kind!("body"), id));
    if let Some(index) = ctx
        .get_btree_map(
            body_indices,
            fallback.as_str(),
            "STEP presentation body_indices contains_key",
        )?
        .copied()
    {
        ctx.insert_btree_map(
            body_ids,
            fallback,
            Some(index),
            "step_presentation_invisible_body_ids",
        )?;
        ctx.remove_btree_set(&mut walk.active, &id, "STEP presentation active remove")?;
        return Ok(InvisibleSummary::Supported { hidden: true });
    }

    let Some(record) =
        ctx.get_btree_map(exchange.records(), &id, "STEP presentation record get")?
    else {
        ctx.remove_btree_set(&mut walk.active, &id, "STEP presentation active remove")?;
        return Ok(InvisibleSummary::Unsupported);
    };
    let mut found_reference = false;
    let mut supported = true;
    let mut hidden = false;
    if record.partial(ctx, "STYLED_ITEM")?.is_some()
        || record.partial(ctx, "OVER_RIDING_STYLED_ITEM")?.is_some()
    {
        if let Some(reference) =
            styled_item_parts(ctx, record)?.and_then(|parts| parts.target.reference())
        {
            found_reference = true;
            let child = collect_invisible_body_ids(
                reference,
                exchange,
                topology,
                body_indices,
                walk,
                body_ids,
                ctx,
            )?;
            match child {
                InvisibleSummary::Unsupported => supported = false,
                InvisibleSummary::Supported {
                    hidden: child_hidden,
                } => hidden |= child_hidden,
            }
        }
    } else if ctx.any_by(
        &(record.partials)[..],
        |partial| Ok(super::representation::is_representation_name(&partial.name)),
        "STEP collect invisible body ids traversal",
    )? {
        if let Some(values) = super::representation::item_values(ctx, record)? {
            let mut values = values.iter();
            ctx.charge_work(0, "STEP representation item traversal")?;
            for _ in 0..values.len() {
                let value = ctx
                    .next_charged(&mut values, "STEP representation item traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
                for reference in references(value, ctx) {
                    found_reference = true;
                    let child = collect_invisible_body_ids(
                        reference?,
                        exchange,
                        topology,
                        body_indices,
                        walk,
                        body_ids,
                        ctx,
                    )?;
                    match child {
                        InvisibleSummary::Unsupported => supported = false,
                        InvisibleSummary::Supported {
                            hidden: child_hidden,
                        } => hidden |= child_hidden,
                    }
                }
            }
        }
    }
    ctx.remove_btree_set(&mut walk.active, &id, "STEP presentation active remove")?;
    Ok(if found_reference && supported {
        InvisibleSummary::Supported { hidden }
    } else {
        InvisibleSummary::Unsupported
    })
}

fn expand_style_targets<F: FnMut(u64) -> Result<(), CodecError>>(
    id: u64,
    exchange: &Exchange,
    (typed, claim_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    (active, active_storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    (depth, graph_limit): (usize, usize),
    visit: &mut F,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if depth >= graph_limit
        || ctx.contains_btree_set(active, &id, "STEP presentation active contains")?
    {
        return Ok(());
    }
    let _nested = ctx.enter_nested("step_presentation_style_target_walk")?;
    active_storage.with_storage(|| {
        ctx.insert_btree_set(active, id, "step_presentation_style_target_active")
    })?;
    let Some(record) =
        ctx.get_btree_map(exchange.records(), &id, "STEP presentation record get")?
    else {
        ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
        visit(id)?;
        return Ok(());
    };
    let Some(set_name) = ctx.find_map(
        &(record.partials)[..],
        |partial| {
            Ok(match partial.name.as_str() {
                "GEOMETRIC_SET" => Some("GEOMETRIC_SET"),
                "GEOMETRIC_CURVE_SET" => Some("GEOMETRIC_CURVE_SET"),
                _ => None,
            })
        },
        "STEP expand style targets traversal",
    )?
    else {
        ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
        visit(id)?;
        return Ok(());
    };
    claim_storage
        .with_storage(|| ctx.insert_btree_set(typed, id, "step_presentation_typed_claims"))?;
    let values = record
        .partial(ctx, set_name)?
        .and_then(|partial| partial.parameters.get(1))
        .and_then(ValueExt::list)
        .unwrap_or_default();
    let mut values = values.iter();
    ctx.charge_work(0, "STEP geometric style set traversal")?;
    for _ in 0..values.len() {
        let value = ctx
            .next_charged(&mut values, "STEP geometric style set traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        if let Some(item) = ValueExt::reference(value) {
            ctx.charge_work(1, "STEP style target edge")?;
            expand_style_targets(
                item,
                exchange,
                (typed, claim_storage),
                (active, active_storage),
                (depth + 1, graph_limit),
                visit,
                ctx,
            )?;
        }
    }
    ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
    Ok(())
}

fn appearance_targets(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
    storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<AppearanceTarget>, CodecError> {
    let mut targets = Vec::new();
    if let Some(bodies) = ctx.get_btree_map(
        &topology.body_by_root,
        &id,
        "STEP presentation topology.body_by_root get",
    )? {
        let mut bodies = bodies.iter();
        ctx.charge_work(0, "STEP presentation bodies traversal")?;
        for _ in 0..bodies.len() {
            let body = ctx
                .next_charged(&mut bodies, "STEP presentation bodies traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx.contains_key_btree_map(
                indices.bodies,
                body.as_str(),
                "STEP presentation indices.bodies contains_key",
            )? {
                let body =
                    body.try_clone_for_decode(ctx, "step_presentation_appearance_body_identity")?;
                ctx.push_scoped_vec(
                    storage,
                    &mut targets,
                    AppearanceTarget::Body(body),
                    "step_presentation_appearance_targets",
                )?;
            }
        }
        return Ok(targets);
    }
    if let Some(faces) = ctx.get_btree_map(
        &topology.faces_by_source,
        &id,
        "STEP presentation topology.faces_by_source get",
    )? {
        let mut faces = faces.iter();
        ctx.charge_work(0, "STEP presentation faces traversal")?;
        for _ in 0..faces.len() {
            let face = ctx
                .next_charged(&mut faces, "STEP presentation faces traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx.contains_key_btree_map(
                indices.faces,
                face.as_str(),
                "STEP presentation indices.faces contains_key",
            )? {
                let face =
                    face.try_clone_for_decode(ctx, "step_presentation_appearance_face_identity")?;
                ctx.push_scoped_vec(
                    storage,
                    &mut targets,
                    AppearanceTarget::Face(face),
                    "step_presentation_appearance_targets",
                )?;
            }
        }
        return Ok(targets);
    }
    if let Some(edges) = ctx.get_btree_map(
        &topology.edges_by_source,
        &id,
        "STEP presentation topology.edges_by_source get",
    )? {
        let mut edges = edges.iter();
        ctx.charge_work(0, "STEP presentation edges traversal")?;
        for _ in 0..edges.len() {
            let edge = ctx
                .next_charged(&mut edges, "STEP presentation edges traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx.contains_btree_set(&entity_ids.edges, edge.as_str(), "STEP edges membership")? {
                let edge =
                    edge.try_clone_for_decode(ctx, "step_presentation_appearance_edge_identity")?;
                ctx.push_scoped_vec(
                    storage,
                    &mut targets,
                    AppearanceTarget::Edge(edge),
                    "step_presentation_appearance_targets",
                )?;
            }
        }
        return Ok(targets);
    }
    if let Some(vertices) = ctx.get_btree_map(
        &topology.vertices_by_source,
        &id,
        "STEP presentation topology.vertices_by_source get",
    )? {
        let mut vertices = vertices.iter();
        ctx.charge_work(0, "STEP presentation vertices traversal")?;
        for _ in 0..vertices.len() {
            let vertex = ctx
                .next_charged(&mut vertices, "STEP presentation vertices traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx.contains_btree_set(
                &entity_ids.vertices,
                vertex.as_str(),
                "STEP vertices membership",
            )? {
                let vertex = vertex
                    .try_clone_for_decode(ctx, "step_presentation_appearance_vertex_identity")?;
                ctx.push_scoped_vec(
                    storage,
                    &mut targets,
                    AppearanceTarget::Vertex(vertex),
                    "step_presentation_appearance_targets",
                )?;
            }
        }
        return Ok(targets);
    }
    let face_id = ids::data(kind!("face"), id);
    let body_id = ids::data(kind!("body"), id);
    let edge_id = ids::data(kind!("edge"), id);
    let surface_id = ids::data(kind!("surface"), id);
    let curve_id = ids::data(kind!("curve"), id);
    let point_id = ids::data(kind!("point"), id);
    let tessellation_id = ids::tessellation(kind!("mesh"), id);
    let target = if ctx.contains_key_btree_map(
        indices.faces,
        face_id.as_str(),
        "STEP presentation indices.faces contains_key",
    )? {
        Some(AppearanceTarget::Face(FaceId::from(face_id)))
    } else if ctx.contains_key_btree_map(
        indices.bodies,
        body_id.as_str(),
        "STEP presentation indices.bodies contains_key",
    )? {
        Some(AppearanceTarget::Body(BodyId::from(body_id)))
    } else if ctx.contains_btree_set(
        &entity_ids.edges,
        edge_id.as_str(),
        "STEP edges membership",
    )? {
        Some(AppearanceTarget::Edge(EdgeId::from(edge_id)))
    } else if ctx.contains_btree_set(
        &entity_ids.surfaces,
        surface_id.as_str(),
        "STEP surfaces membership",
    )? {
        Some(AppearanceTarget::Surface(SurfaceId::from(surface_id)))
    } else if ctx.contains_btree_set(
        &entity_ids.curves,
        curve_id.as_str(),
        "STEP curves membership",
    )? {
        Some(AppearanceTarget::Curve(CurveId::from(curve_id)))
    } else if ctx.contains_btree_set(
        &entity_ids.points,
        point_id.as_str(),
        "STEP points membership",
    )? {
        Some(AppearanceTarget::Point(PointId::from(point_id)))
    } else if ctx.contains_btree_set(
        &entity_ids.tessellations,
        tessellation_id.as_str(),
        "STEP tessellations membership",
    )? {
        Some(AppearanceTarget::Tessellation(
            tessellation_id.into_string(),
        ))
    } else if ctx.contains_key_btree_map(
        exchange.records(),
        &id,
        "STEP presentation record contains_key",
    )? {
        Some(AppearanceTarget::Source {
            source_id: ctx.format_retained(
                format_args!("#{id}"),
                "step_presentation_source_target_text",
            )?,
        })
    } else {
        None
    };
    if let Some(target) = target {
        ctx.push_scoped_vec(
            storage,
            &mut targets,
            target,
            "step_presentation_appearance_targets",
        )?;
    }
    Ok(targets)
}

fn append_presentation_items(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
    items: &mut Vec<PresentationItem>,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    if let Some(bodies) = ctx.get_btree_map(
        &topology.body_by_root,
        &id,
        "STEP presentation topology.body_by_root get",
    )? {
        let mut bodies = bodies.iter();
        ctx.charge_work(0, "STEP presentation bodies traversal")?;
        for _ in 0..bodies.len() {
            let body = ctx
                .next_charged(&mut bodies, "STEP presentation bodies traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx.contains_key_btree_map(
                indices.bodies,
                body.as_str(),
                "STEP presentation indices.bodies contains_key",
            )? {
                let body =
                    body.try_clone_for_decode(ctx, "step_presentation_layer_body_identity")?;
                ctx.push_vec(
                    items,
                    PresentationItem::Body { body },
                    "step_presentation_layer_items",
                )?;
            }
        }
        return Ok(());
    }
    if let Some(faces) = ctx.get_btree_map(
        &topology.faces_by_source,
        &id,
        "STEP presentation topology.faces_by_source get",
    )? {
        let mut faces = faces.iter();
        ctx.charge_work(0, "STEP presentation faces traversal")?;
        for _ in 0..faces.len() {
            let face = ctx
                .next_charged(&mut faces, "STEP presentation faces traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx.contains_key_btree_map(
                indices.faces,
                face.as_str(),
                "STEP presentation indices.faces contains_key",
            )? {
                let face =
                    face.try_clone_for_decode(ctx, "step_presentation_layer_face_identity")?;
                ctx.push_vec(
                    items,
                    PresentationItem::Face { face },
                    "step_presentation_layer_items",
                )?;
            }
        }
        return Ok(());
    }
    if let Some(edges) = ctx.get_btree_map(
        &topology.edges_by_source,
        &id,
        "STEP presentation topology.edges_by_source get",
    )? {
        let mut edges = edges.iter();
        ctx.charge_work(0, "STEP presentation edges traversal")?;
        for _ in 0..edges.len() {
            let edge = ctx
                .next_charged(&mut edges, "STEP presentation edges traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx.contains_btree_set(&entity_ids.edges, edge.as_str(), "STEP edges membership")? {
                let edge =
                    edge.try_clone_for_decode(ctx, "step_presentation_layer_edge_identity")?;
                ctx.push_vec(
                    items,
                    PresentationItem::Edge { edge },
                    "step_presentation_layer_items",
                )?;
            }
        }
        return Ok(());
    }
    if let Some(vertices) = ctx.get_btree_map(
        &topology.vertices_by_source,
        &id,
        "STEP presentation topology.vertices_by_source get",
    )? {
        let mut vertices = vertices.iter();
        ctx.charge_work(0, "STEP presentation vertices traversal")?;
        for _ in 0..vertices.len() {
            let vertex = ctx
                .next_charged(&mut vertices, "STEP presentation vertices traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            if ctx.contains_btree_set(
                &entity_ids.vertices,
                vertex.as_str(),
                "STEP vertices membership",
            )? {
                let vertex =
                    vertex.try_clone_for_decode(ctx, "step_presentation_layer_vertex_identity")?;
                ctx.push_vec(
                    items,
                    PresentationItem::Vertex { vertex },
                    "step_presentation_layer_items",
                )?;
            }
        }
        return Ok(());
    }
    if let Some(products) = ctx.get_btree_map(
        entity_ids.products,
        &id,
        "STEP presentation entity_ids.products get",
    )? {
        let mut products = products.iter();
        ctx.charge_work(0, "STEP presentation products traversal")?;
        for _ in 0..products.len() {
            let product = ctx
                .next_charged(&mut products, "STEP presentation products traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            let product =
                product.try_clone_for_decode(ctx, "step_presentation_layer_product_identity")?;
            ctx.push_vec(
                items,
                PresentationItem::Product { product },
                "step_presentation_layer_items",
            )?;
        }
        return Ok(());
    }
    ctx.push_vec(
        items,
        presentation_item_one(id, exchange, entity_ids, indices, ctx)?,
        "step_presentation_layer_items",
    )
}

fn presentation_item_one(
    id: u64,
    exchange: &Exchange,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<PresentationItem, CodecError> {
    let candidate = |kind: &crate::ids::IdentityKind| ids::data(kind, id);
    let body = candidate(kind!("body"));
    if ctx.contains_key_btree_map(
        indices.bodies,
        body.as_str(),
        "STEP presentation indices.bodies contains_key",
    )? {
        return Ok(PresentationItem::Body {
            body: BodyId::from(body),
        });
    }
    let face = candidate(kind!("face"));
    if ctx.contains_key_btree_map(
        indices.faces,
        face.as_str(),
        "STEP presentation indices.faces contains_key",
    )? {
        return Ok(PresentationItem::Face {
            face: FaceId::from(face),
        });
    }
    let edge = candidate(kind!("edge"));
    if ctx.contains_btree_set(&entity_ids.edges, edge.as_str(), "STEP edges membership")? {
        return Ok(PresentationItem::Edge {
            edge: EdgeId::from(edge),
        });
    }
    let vertex = candidate(kind!("vertex"));
    if ctx.contains_btree_set(
        &entity_ids.vertices,
        vertex.as_str(),
        "STEP vertices membership",
    )? {
        return Ok(PresentationItem::Vertex {
            vertex: VertexId::from(vertex),
        });
    }
    let point = candidate(kind!("point"));
    if ctx.contains_btree_set(&entity_ids.points, point.as_str(), "STEP points membership")? {
        return Ok(PresentationItem::Point {
            point: PointId::from(point),
        });
    }
    let curve = candidate(kind!("curve"));
    if ctx.contains_btree_set(&entity_ids.curves, curve.as_str(), "STEP curves membership")? {
        return Ok(PresentationItem::Curve {
            curve: CurveId::from(curve),
        });
    }
    let surface = candidate(kind!("surface"));
    if ctx.contains_btree_set(
        &entity_ids.surfaces,
        surface.as_str(),
        "STEP surfaces membership",
    )? {
        return Ok(PresentationItem::Surface {
            surface: SurfaceId::from(surface),
        });
    }
    let Some(record) =
        ctx.get_btree_map(exchange.records(), &id, "STEP presentation record get")?
    else {
        return Ok(PresentationItem::Source {
            source_id: super::step_source_id(ctx, id)?,
        });
    };
    let has = |name: &'static str| -> Result<bool, CodecError> {
        Ok(record.partial(ctx, name)?.is_some())
    };
    Ok(
        if has("NEXT_ASSEMBLY_USAGE_OCCURRENCE")?
            && ctx.contains_btree_set(
                &entity_ids.occurrences,
                ids::product(kind!("occurrence"), id).as_str(),
                "STEP occurrences membership",
            )?
        {
            PresentationItem::Occurrence {
                occurrence: OccurrenceId::from(ids::product(kind!("occurrence"), id)),
            }
        } else if ctx.any_by(
            &record.partials[..],
            |partial| {
                Ok((partial.name == "DATUM"
                    || partial.name == "DATUM_SYSTEM"
                    || partial.name.starts_with("DIMENSIONAL_")
                    || partial.name.ends_with("_TOLERANCE")
                    || super::pmi::is_presentation_annotation(&partial.name))
                    && ctx.contains_btree_set(
                        &entity_ids.pmi,
                        ids::presentation(kind!("pmi"), id).as_str(),
                        "STEP pmi membership",
                    )?)
            },
            "STEP presentation item one traversal",
        )? {
            PresentationItem::Pmi {
                annotation: PmiId::from(ids::presentation(kind!("pmi"), id)),
            }
        } else if (has("TRIANGULATED_FACE")?
            || has("COMPLEX_TRIANGULATED_FACE")?
            || has("TRIANGULATED_SURFACE_SET")?
            || has("COMPLEX_TRIANGULATED_SURFACE_SET")?)
            && ctx.contains_btree_set(
                &entity_ids.tessellations,
                ids::tessellation(kind!("mesh"), id).as_str(),
                "STEP tessellations membership",
            )?
        {
            PresentationItem::Tessellation {
                tessellation: ids::tessellation(kind!("mesh"), id).into_string(),
            }
        } else {
            PresentationItem::Source {
                source_id: super::step_source_id(ctx, id)?,
            }
        },
    )
}

struct EntityIds<'a> {
    edges: BTreeSet<&'a str>,
    vertices: BTreeSet<&'a str>,
    points: BTreeSet<&'a str>,
    curves: BTreeSet<&'a str>,
    surfaces: BTreeSet<&'a str>,
    products: &'a BTreeMap<u64, Vec<ProductDefinitionId>>,
    occurrences: BTreeSet<&'a str>,
    pmi: BTreeSet<&'a str>,
    tessellations: BTreeSet<&'a str>,
}

#[derive(Clone, Copy)]
struct PresentationIndices<'a> {
    faces: &'a BTreeMap<String, usize>,
    bodies: &'a BTreeMap<String, usize>,
}

type ScalarCandidates = [BTreeMap<String, (AppearanceTarget, Vec<(u64, Color)>)>; 2];

fn push_scalar_candidate(
    candidates: &mut ScalarCandidates,
    target: &AppearanceTarget,
    style_id: u64,
    color: Color,
    ctx: &DecodeContext<'_>,
) -> Result<(), CodecError> {
    let (lane, identity) = match target {
        AppearanceTarget::Face(face) => (0, face.as_str()),
        AppearanceTarget::Body(body) => (1, body.as_str()),
        _ => return Ok(()),
    };
    let candidates = &mut candidates[lane];
    if let Some((_, values)) =
        ctx.get_mut_btree_map(candidates, identity, "STEP scalar color group lookup")?
    {
        return ctx.push_vec(
            values,
            (style_id, color),
            "step_presentation_scalar_color_members",
        );
    }
    let key = ctx.copy_retained_text(identity, "step_presentation_scalar_target_identity")?;
    let target = match target {
        AppearanceTarget::Face(face) => AppearanceTarget::Face(
            face.try_clone_for_decode(ctx, "step_presentation_scalar_target_identity")?,
        ),
        AppearanceTarget::Body(body) => AppearanceTarget::Body(
            body.try_clone_for_decode(ctx, "step_presentation_scalar_target_identity")?,
        ),
        _ => return Ok(()),
    };
    let group = ctx
        .entry_btree_map(candidates, key, "step_presentation_scalar_color_groups")?
        .or_insert((target, Vec::new()));
    ctx.push_vec(
        &mut group.1,
        (style_id, color),
        "step_presentation_scalar_color_members",
    )
}

fn collect_identity_indices<'a>(
    identities: impl IntoIterator<Item = &'a str, IntoIter: ExactSizeIterator>,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<BTreeMap<String, usize>, CodecError> {
    let mut result = BTreeMap::new();
    let mut identities = identities.into_iter().enumerate();
    ctx.charge_work(0, operation)?;
    for _ in 0..identities.len() {
        let (index, identity) = ctx
            .next_charged(&mut identities, operation)?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        if let Some(existing) =
            ctx.get_mut_btree_map(&mut result, identity, "STEP presentation result get_mut")?
        {
            *existing = index;
            continue;
        }
        let copy = ctx.copy_retained_text(identity, operation)?;
        ctx.insert_btree_map(&mut result, copy, index, operation)?;
    }
    Ok(result)
}

fn overridden_style(ctx: &DecodeContext<'_>, style: &RawRecord) -> Result<Option<u64>, CodecError> {
    Ok(styled_item_parts(ctx, style)?
        .and_then(|parts| parts.overridden)
        .and_then(ValueExt::reference))
}

struct StyledItemParts<'a> {
    styles: &'a Value,
    target: &'a Value,
    overridden: Option<&'a Value>,
}

fn styled_item_parts<'a>(
    ctx: &DecodeContext<'_>,
    record: &'a RawRecord,
) -> Result<Option<StyledItemParts<'a>>, CodecError> {
    if let Some(partial) = record.partial(ctx, "OVER_RIDING_STYLED_ITEM")? {
        let parameters = partial.parameters.as_slice();
        return Ok((|| {
            let target = parameters.get(parameters.len().checked_sub(2)?)?;
            let styles = parameters.get(parameters.len().checked_sub(3)?)?;
            let overridden = parameters.last()?;
            Some(StyledItemParts {
                styles,
                target,
                overridden: Some(overridden),
            })
        })());
    }
    let Some(partial) = record.partial(ctx, "STYLED_ITEM")? else {
        return Ok(None);
    };
    let parameters = partial.parameters.as_slice();
    Ok((|| {
        let target = parameters.last()?;
        let styles = parameters.get(parameters.len().checked_sub(2)?)?;
        Some(StyledItemParts {
            styles,
            target,
            overridden: None,
        })
    })())
}

fn is_presentation_style_by_context(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<bool, CodecError> {
    Ok(record
        .partial(ctx, "PRESENTATION_STYLE_BY_CONTEXT")?
        .is_some())
}

fn context_style_message(
    style_id: u64,
    ids: &BTreeSet<u64>,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    let (parts_buffer, mut storage) = ctx.temporary_vec(0, "STEP context style fragments")?;
    let mut parts = parts_buffer;
    let mut ids = ids.iter();
    ctx.charge_work(0, "STEP context style detail traversal")?;
    for _ in 0..ids.len() {
        let context_style_id = ctx
            .next_charged(&mut ids, "STEP context style detail traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        let context = ctx
            .get_btree_map(
                exchange.records(),
                context_style_id,
                "STEP context style record lookup",
            )?
            .map(|record| record.partial(ctx, "PRESENTATION_STYLE_BY_CONTEXT"))
            .transpose()?
            .flatten()
            .and_then(|partial| partial.parameters.last())
            .and_then(ValueExt::reference);
        let part = storage.with_storage(|| match context {
            Some(context) => ctx.format_retained(
                format_args!("#{context_style_id} in #{context}"),
                "STEP context style details",
            ),
            None => ctx.format_retained(
                format_args!("#{context_style_id} in unresolved"),
                "STEP context style details",
            ),
        })?;
        ctx.push_scoped_vec(
            &mut storage,
            &mut parts,
            part,
            "STEP context style fragments",
        )?;
    }
    let details =
        storage.with_storage(|| ctx.join_retained(&parts, ", ", "STEP context style details"))?;
    ctx.format_retained(format_args!("STYLED_ITEM #{style_id} has context-dependent style assignments {details}; no presentation context is selected by the neutral model; those source branches remain opaque"), "step_presentation_context_style_text")
}

fn scalar_conflict_message(
    candidates: &[(u64, Color)],
    target: &AppearanceTarget,
    ctx: &DecodeContext<'_>,
) -> Result<String, CodecError> {
    let (parts_buffer, mut storage) = ctx.temporary_vec(0, "STEP scalar style fragments")?;
    let mut parts = parts_buffer;
    let mut candidates = candidates.iter();
    ctx.charge_work(0, "STEP scalar style detail traversal")?;
    for _ in 0..candidates.len() {
        let (style_id, _) = ctx
            .next_charged(&mut candidates, "STEP scalar style detail traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        let part = storage.with_storage(|| {
            ctx.format_retained(format_args!("#{style_id}"), "STEP scalar style details")
        })?;
        ctx.push_scoped_vec(
            &mut storage,
            &mut parts,
            part,
            "STEP scalar style fragments",
        )?;
    }
    let style_ids =
        storage.with_storage(|| ctx.join_retained(&parts, ", ", "STEP scalar style details"))?;
    ctx.format_retained(format_args!("independent styled items {style_ids} assign conflicting scalar colors to {target:?}; scalar color omitted and appearance bindings retain every assignment"), "step_presentation_scalar_conflict_text")
}

pub(super) fn styled_item_target(
    ctx: &DecodeContext<'_>,
    record: &RawRecord,
) -> Result<Option<u64>, CodecError> {
    Ok(styled_item_parts(ctx, record)?.and_then(|parts| parts.target.reference()))
}

/// The position of one styled item in override-depth order.
///
/// A style whose override walk revisits a style or passes the graph limit
/// states no depth. Absence takes a position of its own, after every stated
/// depth; it is not read as the deepest style.
fn style_application_order(
    id: u64,
    exchange: &Exchange,
    graph_limit: usize,
    cache: &mut BTreeMap<u64, Option<u32>>,
    storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(bool, Option<u32>), CodecError> {
    if graph_limit == 0 {
        return Ok((true, None));
    }
    let (path_buffer, mut path_storage) = ctx.temporary_vec(0, "STEP style depth path")?;
    let mut path = path_buffer;
    let mut visited_storage = ctx.reserve_scoped(0, "STEP style depth visited")?;
    let mut visited = BTreeSet::new();
    let mut current = id;
    let (mut depth, cached_base) = loop {
        // A truncated path states no depth for its suffixes; do not cache it.
        if path.len() >= graph_limit {
            return Ok((true, None));
        }
        ctx.charge_work(1, "STEP style depth step")?;
        if let Some(depth) = ctx.get_btree_map(cache, &current, "STEP style depth cache lookup")? {
            break (*depth, true);
        }
        if !visited_storage.with_storage(|| {
            ctx.insert_btree_set(&mut visited, current, "STEP style depth visited")
        })? {
            break (None, false);
        }
        ctx.push_scoped_vec(
            &mut path_storage,
            &mut path,
            current,
            "STEP style depth path",
        )?;
        let Some(record) = ctx.get_btree_map(
            exchange.records(),
            &current,
            "STEP style depth record lookup",
        )?
        else {
            break (None, false);
        };
        let Some(base) = overridden_style(ctx, record)? else {
            break (Some(0_u32), false);
        };
        current = base;
    };
    let mut first = true;
    // A cached base is one edge beyond the last uncached style.
    if cached_base && !path.is_empty() {
        depth = depth.and_then(|depth| depth.checked_add(1));
    }
    let mut path = path.iter().rev();
    ctx.charge_work(0, "STEP style depth path result traversal")?;
    for _ in 0..path.len() {
        let style = ctx
            .next_charged(&mut path, "STEP style depth path result traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        if !first {
            depth = depth.and_then(|depth| depth.checked_add(1));
        }
        first = false;
        storage.with_storage(|| {
            ctx.insert_btree_map(cache, *style, depth, "STEP style depth cache entries")
        })?;
    }
    let depth =
        depth.filter(|depth| u64::from(*depth) < cadmpeg_core::decode::u64_from_index(graph_limit));
    Ok((depth.is_none(), depth))
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SurfaceSideRank {
    NoUsage,
    Negative,
    Positive,
    Both,
}

struct ColorCandidate {
    rank: SurfaceSideRank,
    id: u64,
    color: Color,
    name: Option<String>,
}

enum ColorResolution {
    Candidate(ColorCandidate),
    Ambiguous { rank: SurfaceSideRank },
}

type CachedColor = Option<ColorResolution>;

fn clone_color_resolution(
    resolution: &CachedColor,
    ctx: &DecodeContext<'_>,
    operation: &'static str,
) -> Result<CachedColor, CodecError> {
    Ok(match resolution {
        Some(ColorResolution::Candidate(candidate)) => {
            Some(ColorResolution::Candidate(ColorCandidate {
                rank: candidate.rank,
                id: candidate.id,
                color: candidate.color,
                name: candidate
                    .name
                    .as_deref()
                    .map(|name| ctx.copy_retained_text(name, operation))
                    .transpose()?,
            }))
        }
        Some(ColorResolution::Ambiguous { rank }) => {
            Some(ColorResolution::Ambiguous { rank: *rank })
        }
        None => None,
    })
}

impl ColorResolution {
    fn priority(&self) -> SurfaceSideRank {
        match self {
            Self::Candidate(candidate) => candidate.rank,
            Self::Ambiguous { rank } => *rank,
        }
    }

    fn with_min_rank(self, rank: SurfaceSideRank) -> Self {
        match self {
            Self::Candidate(mut candidate) => {
                candidate.rank = candidate.rank.max(rank);
                Self::Candidate(candidate)
            }
            Self::Ambiguous {
                rank: candidate_rank,
            } => Self::Ambiguous {
                rank: candidate_rank.max(rank),
            },
        }
    }
}

fn combine_color_resolutions(
    resolutions: impl IntoIterator<Item = Result<CachedColor, CodecError>>,
) -> Result<CachedColor, CodecError> {
    let mut best_priority = None;
    let mut best = None;
    let mut ambiguous = false;
    for resolution in resolutions {
        let Some(resolution) = resolution? else {
            continue;
        };
        let priority = resolution.priority();
        let replace = best_priority.is_none_or(|current| priority > current);
        if replace {
            let is_ambiguous = matches!(&resolution, ColorResolution::Ambiguous { .. });
            best_priority = Some(priority);
            best = match resolution {
                ColorResolution::Candidate(candidate) => Some(candidate),
                ColorResolution::Ambiguous { .. } => None,
            };
            ambiguous = is_ambiguous;
            continue;
        }
        if best_priority == Some(priority) {
            match resolution {
                ColorResolution::Candidate(candidate) => {
                    let Some(current) = best.as_mut() else {
                        ambiguous = true;
                        continue;
                    };
                    let same_rgb = current.color.r() == candidate.color.r()
                        && current.color.g() == candidate.color.g()
                        && current.color.b() == candidate.color.b();
                    if !same_rgb {
                        ambiguous = true;
                    } else if candidate.color.a() < current.color.a()
                        || (candidate.color.a() == current.color.a() && candidate.id < current.id)
                    {
                        *current = candidate;
                    }
                }
                ColorResolution::Ambiguous { .. } => ambiguous = true,
            }
        }
    }
    let Some(rank) = best_priority else {
        return Ok(None);
    };
    if ambiguous {
        Ok(Some(ColorResolution::Ambiguous { rank }))
    } else {
        Ok(best.map(ColorResolution::Candidate))
    }
}

struct ColorResult {
    color: CachedColor,
    // None prevents sharing after a cycle, cutoff, or warning.
    height: Option<u8>,
}

// Explicitly visited completions keep local claims. Shared hits also
// claim their completed descendants.
#[derive(Clone, Copy)]
enum ColorCacheSource {
    Local,
    Completed,
    Shared,
}

struct ColorCacheEntry {
    color: usize,
    source: ColorCacheSource,
    height: Option<u8>,
}

#[derive(Default)]
struct ColorCache {
    values: BTreeMap<(u64, StyleDomain), ColorCacheEntry>,
    colors: Vec<CachedColor>,
}

struct CompletedColor {
    color: usize,
    height: u8,
}

struct ColorCompletions<'ctx> {
    values: BTreeMap<(u64, StyleDomain), CompletedColor>,
    colors: Vec<CachedColor>,
    claimed: BTreeSet<(u64, StyleDomain)>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx> ColorCompletions<'ctx> {
    fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            values: BTreeMap::new(),
            colors: Vec::new(),
            claimed: BTreeSet::new(),
            storage: ctx.reserve_scoped(0, "STEP completed color graph scratch")?,
        })
    }

    fn claim(
        &mut self,
        root: (u64, StyleDomain),
        (claims, storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
        exchange: &Exchange,
        ctx: &DecodeContext<'_>,
    ) -> Result<(), CodecError> {
        let mut walk_storage = ctx.reserve_scoped(0, "STEP shared color claim walk")?;
        let mut pending = Vec::new();
        ctx.push_scoped_vec(
            &mut walk_storage,
            &mut pending,
            root,
            "STEP shared color claim pending",
        )?;
        while !pending.is_empty() {
            ctx.charge_work(1, "STEP shared color claim step")?;
            let key = pending
                .pop()
                .ok_or_else(|| CodecError::malformed("STEP shared color claim node missing"))?;
            if ctx.contains_btree_set(&self.claimed, &key, "STEP shared color claimed lookup")? {
                continue;
            }
            // Missing records and context styles have no completion or claim.
            let Some(node) =
                ctx.get_btree_map(&self.values, &key, "STEP shared color claim lookup")?
            else {
                continue;
            };
            self.storage.with_storage(|| {
                ctx.insert_btree_set(&mut self.claimed, key, "STEP shared color claimed")
            })?;
            storage.with_storage(|| {
                ctx.insert_btree_set(claims, key.0, "step_presentation_typed_claims")
            })?;
            if node.height == 0 {
                continue;
            }
            let record = ctx
                .get_btree_map(
                    exchange.records(),
                    &key.0,
                    "STEP shared color claim record lookup",
                )?
                .ok_or_else(|| CodecError::malformed("STEP shared color claim record missing"))?;
            let mut partials = record.partials.iter();
            for _ in 0..partials.len() {
                let partial = ctx
                    .next_charged(&mut partials, "STEP shared color claim partials")?
                    .ok_or_else(|| {
                        CodecError::malformed("STEP shared color claim partial missing")
                    })?;
                let mut parameters = partial.parameters.iter();
                for _ in 0..parameters.len() {
                    let value = ctx
                        .next_charged(&mut parameters, "STEP shared color claim parameters")?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP shared color claim parameter missing")
                        })?;
                    for child in references(value, ctx) {
                        ctx.push_scoped_vec(
                            &mut walk_storage,
                            &mut pending,
                            (child?, key.1),
                            "STEP shared color claim pending",
                        )?;
                    }
                }
            }
        }
        Ok(())
    }
}

struct ColorSearchState<'a, 'ctx> {
    storage: &'a std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'ctx>>,
    active: &'a mut BTreeSet<u64>,
    cache: &'a mut ColorCache,
    completed: Option<&'a mut ColorCompletions<'ctx>>,
    losses: (
        &'a mut Vec<LossNote>,
        &'a std::cell::RefCell<ScopedReservation<'ctx>>,
    ),
    invalid_surface_sides: &'a mut BTreeSet<u64>,
}

struct CachedStyleColors {
    color: CachedColor,
    losses: Vec<LossNote>,
    claims: Vec<((u64, StyleDomain), bool)>,
    claims_applied: bool,
}

struct StyleColors<'source, 'ctx, 'arena> {
    exchange: &'source Exchange,
    // Fixed-size prefix keys identify an ordered reference list without
    // variable-length map-key comparisons or repeated graph evaluation.
    prefixes: BTreeMap<(usize, u64), usize>,
    values: BTreeMap<(usize, StyleDomain), usize>,
    queries: Vec<CachedStyleColors>,
    // Successful walks drain frames; their backing remains available.
    frames: Vec<ColorFrame<'source, 'ctx, 'arena>>,
    frame_storage: ScopedReservation<'ctx>,
    completed: ColorCompletions<'ctx>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'source, 'ctx, 'arena> StyleColors<'source, 'ctx, 'arena> {
    fn resolve(
        &mut self,
        references: &[u64],
        domain: StyleDomain,
        storage: &std::cell::RefCell<ScopedReservation<'ctx>>,
        (losses, slot_storage): (
            &mut Vec<LossNote>,
            &std::cell::RefCell<ScopedReservation<'ctx>>,
        ),
    ) -> Result<(usize, &CachedStyleColors), CodecError> {
        let ctx = self.ctx;
        let exchange = self.exchange;
        let mut prefix = 0;
        let mut reference_prefix = references.iter();
        ctx.charge_work(0, "STEP style color query references")?;
        for _ in 0..reference_prefix.len() {
            let &reference = ctx
                .next_charged(&mut reference_prefix, "STEP style color query references")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            let next = self.prefixes.len() + 1;
            prefix = match self.storage.with_storage(|| {
                ctx.entry_btree_map(
                    &mut self.prefixes,
                    (prefix, reference),
                    "step_style_color_query_prefixes",
                )
            })? {
                std::collections::btree_map::Entry::Occupied(entry) => *entry.get(),
                std::collections::btree_map::Entry::Vacant(entry) => *entry.insert(next),
            };
        }
        let cached_before = ctx.contains_key_btree_map(
            &self.values,
            &(prefix, domain),
            "STEP style color query lookup",
        )?;
        if !cached_before {
            // Local cache history precedes shared lookup. Shared completions
            // are acyclic, warning-free, and fit wholly below the cutoff.
            let loss_start = losses.len();
            let mut active = BTreeSet::new();
            let mut cache = ColorCache::default();
            let mut invalid_surface_sides = BTreeSet::new();
            let mut resolve = |domain| {
                let mut references = references.iter().copied();
                ctx.charge_work(0, "STEP color reference traversal")?;
                let resolutions = (0..references.len()).map(|_| {
                    let reference = ctx
                        .next_charged(&mut references, "STEP color reference traversal")?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP color reference source ended early")
                        })?;
                    find_color(
                        reference,
                        exchange,
                        domain,
                        ColorSearchState {
                            storage,
                            active: &mut active,
                            cache: &mut cache,
                            completed: Some(&mut self.completed),
                            losses: (&mut *losses, slot_storage),
                            invalid_surface_sides: &mut invalid_surface_sides,
                        },
                        0,
                        Some((&mut self.frames, &mut self.frame_storage)),
                        ctx,
                    )
                });
                combine_color_resolutions(resolutions)
            };
            let color = resolve(domain)?;
            let color =
                if color.is_none() && matches!(domain, StyleDomain::Curve | StyleDomain::Point) {
                    resolve(StyleDomain::Surface)?
                } else {
                    color
                };
            self.storage.with_storage(|| {
                let mut claims = Vec::new();
                let mut cache_entries = cache.values.iter();
                ctx.charge_work(0, "STEP color claim cache traversal")?;
                for _ in 0..cache_entries.len() {
                    let (&key, entry) = ctx
                        .next_charged(&mut cache_entries, "STEP color claim cache traversal")?
                        .ok_or_else(|| {
                            CodecError::malformed("STEP presentation source ended early")
                        })?;
                    if !ctx.contains_btree_set(
                        &invalid_surface_sides,
                        &key.0,
                        "STEP presentation invalid_surface_sides contains",
                    )? {
                        ctx.push_vec(
                            &mut claims,
                            (key, matches!(entry.source, ColorCacheSource::Shared)),
                            "step_style_color_claims",
                        )?;
                    }
                }
                let cached = CachedStyleColors {
                    claims,
                    claims_applied: false,
                    color: clone_color_resolution(&color, ctx, "step_style_color_cached_value")?,
                    losses: ctx.try_collect_vec(
                        losses[loss_start..].iter().map(|loss| {
                            loss.try_clone_for_decode(ctx, "step_style_color_cached_loss")
                        }),
                        "step_style_color_cached_losses",
                    )?,
                };
                let index = self.queries.len();
                ctx.push_vec(&mut self.queries, cached, "step_style_color_query_values")?;
                ctx.insert_btree_map(
                    &mut self.values,
                    (prefix, domain),
                    index,
                    "step_style_color_query_entries",
                )
            })?;
        }
        let cached = ctx
            .get_btree_map(
                &self.values,
                &(prefix, domain),
                "STEP style color query result",
            )?
            .and_then(|index| self.queries.get(*index))
            .ok_or_else(|| CodecError::malformed("STEP style color query is missing"))?;
        if cached_before {
            let mut cached_losses = cached.losses.iter();
            ctx.charge_work(0, "STEP cached color loss traversal")?;
            for _ in 0..cached_losses.len() {
                let loss = ctx
                    .next_charged(&mut cached_losses, "STEP cached color loss traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
                let loss = loss.try_clone_for_decode(ctx, "step_style_color_loss_copy")?;
                ctx.push_scoped_vec(
                    &mut slot_storage.borrow_mut(),
                    losses,
                    loss,
                    "step_presentation_losses",
                )?;
            }
        }
        Ok((prefix, cached))
    }

    fn claim(
        &mut self,
        prefix: usize,
        domain: StyleDomain,
        (claims, storage): (&mut BTreeSet<u64>, &mut ScopedReservation<'_>),
    ) -> Result<(), CodecError> {
        let ctx = self.ctx;
        let cached = ctx
            .get_btree_map(
                &self.values,
                &(prefix, domain),
                "STEP style color claims lookup",
            )?
            .and_then(|index| self.queries.get_mut(*index))
            .ok_or_else(|| CodecError::malformed("STEP style color query is missing"))?;
        if !cached.claims_applied {
            let mut cached_claims = cached.claims.iter();
            ctx.charge_work(0, "STEP style color claim traversal")?;
            for _ in 0..cached_claims.len() {
                let &(key, shared) = ctx
                    .next_charged(&mut cached_claims, "STEP style color claim traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
                if shared {
                    self.completed
                        .claim(key, (claims, storage), self.exchange, ctx)?;
                } else {
                    storage.with_storage(|| {
                        ctx.insert_btree_set(claims, key.0, "step_presentation_typed_claims")
                    })?;
                }
            }
            cached.claims_applied = true;
        }
        Ok(())
    }
}

struct ColorFrame<'exchange, 'ctx, 'arena> {
    id: u64,
    depth: usize,
    loss_start: usize,
    partials: std::slice::Iter<'exchange, crate::parse::PartialRecord>,
    partial_operation: &'static str,
    transparency: Option<Fraction>,
    side_rank: SurfaceSideRank,
    combine_children: bool,
    parameter_operation: &'static str,
    parameters: Option<std::slice::Iter<'exchange, Value>>,
    references: Option<References<'exchange, 'ctx, 'arena>>,
    result: ColorResult,
    _depth: DepthGuard<'ctx>,
}

enum ColorVisit<'exchange, 'ctx, 'arena> {
    Complete(ColorResult),
    Frame(ColorFrame<'exchange, 'ctx, 'arena>),
}

enum ColorBody<'exchange> {
    Ready {
        transparency: Option<Fraction>,
        result: CachedColor,
    },
    Children {
        record: &'exchange RawRecord,
        transparency: Option<Fraction>,
        side_rank: SurfaceSideRank,
        combine_children: bool,
        partial_operation: &'static str,
        parameter_operation: &'static str,
    },
}

fn find_color<'exchange, 'ctx, 'arena>(
    id: u64,
    exchange: &'exchange Exchange,
    domain: StyleDomain,
    state: ColorSearchState<'_, '_>,
    depth: usize,
    walk: Option<(
        &mut Vec<ColorFrame<'exchange, 'ctx, 'arena>>,
        &mut ScopedReservation<'ctx>,
    )>,
    ctx: &'ctx DecodeContext<'arena>,
) -> Result<CachedColor, CodecError> {
    let ColorSearchState {
        storage,
        active,
        cache,
        mut completed,
        losses: (losses, slot_storage),
        invalid_surface_sides,
    } = state;
    let (mut local_frames, mut local_storage) =
        ctx.temporary_vec(0, "step_presentation_color_walk_frames")?;
    let (frames, frame_storage) = walk.unwrap_or((&mut local_frames, &mut local_storage));
    let first = begin_color_visit(
        id,
        exchange,
        domain,
        ColorSearchState {
            storage,
            active: &mut *active,
            cache: &mut *cache,
            completed: completed.as_deref_mut(),
            losses: (&mut *losses, slot_storage),
            invalid_surface_sides: &mut *invalid_surface_sides,
        },
        depth,
        ctx,
    )?;
    match first {
        ColorVisit::Complete(result) => return Ok(result.color),
        ColorVisit::Frame(frame) => ctx.push_scoped_vec(
            frame_storage,
            frames,
            frame,
            "step_presentation_color_walk_frames",
        )?,
    }

    loop {
        let child_id = next_color_reference(
            frames
                .last_mut()
                .ok_or_else(|| CodecError::malformed("STEP color traversal has no active frame"))?,
            ctx,
        )?;
        if let Some(child_id) = child_id {
            let child_depth = frames.last().map(|frame| frame.depth + 1).ok_or_else(|| {
                CodecError::malformed("STEP color traversal lost its parent frame")
            })?;
            let child = begin_color_visit(
                child_id,
                exchange,
                domain,
                ColorSearchState {
                    storage,
                    active: &mut *active,
                    cache: &mut *cache,
                    completed: completed.as_deref_mut(),
                    losses: (&mut *losses, slot_storage),
                    invalid_surface_sides: &mut *invalid_surface_sides,
                },
                child_depth,
                ctx,
            )?;
            match child {
                ColorVisit::Complete(result) => {
                    let parent = frames.last_mut().ok_or_else(|| {
                        CodecError::malformed("STEP color traversal lost its parent frame")
                    })?;
                    parent.result.height =
                        parent
                            .result
                            .height
                            .zip(result.height)
                            .and_then(|(height, child)| {
                                child.checked_add(1).map(|child| height.max(child))
                            });
                    if parent.combine_children {
                        let result = result
                            .color
                            .map(|candidate| candidate.with_min_rank(parent.side_rank));
                        parent.result.color = combine_color_resolutions([
                            Ok(std::mem::take(&mut parent.result.color)),
                            Ok(result),
                        ])?;
                    }
                }
                ColorVisit::Frame(frame) => ctx.push_scoped_vec(
                    frame_storage,
                    frames,
                    frame,
                    "step_presentation_color_walk_frames",
                )?,
            }
            continue;
        }

        let frame = frames
            .pop()
            .ok_or_else(|| CodecError::malformed("STEP color traversal ended without a frame"))?;
        let ColorFrame {
            id,
            transparency,
            result,
            depth,
            loss_start,
            _depth,
            ..
        } = frame;
        let result = finish_color(
            id,
            domain,
            transparency,
            result,
            ColorSearchState {
                storage,
                active: &mut *active,
                cache: &mut *cache,
                completed: completed.as_deref_mut(),
                losses: (&mut *losses, slot_storage),
                invalid_surface_sides: &mut *invalid_surface_sides,
            },
            (depth, loss_start),
            ctx,
        )?;
        drop(_depth);
        let Some(parent) = frames.last_mut() else {
            return Ok(result.color);
        };
        parent.result.height = parent
            .result
            .height
            .zip(result.height)
            .and_then(|(height, child)| child.checked_add(1).map(|child| height.max(child)));
        if parent.combine_children {
            let result = result
                .color
                .map(|candidate| candidate.with_min_rank(parent.side_rank));
            parent.result.color = combine_color_resolutions([
                Ok(std::mem::take(&mut parent.result.color)),
                Ok(result),
            ])?;
        }
    }
}

fn begin_color_visit<'exchange, 'ctx, 'arena>(
    id: u64,
    exchange: &'exchange Exchange,
    domain: StyleDomain,
    state: ColorSearchState<'_, '_>,
    depth: usize,
    ctx: &'ctx DecodeContext<'arena>,
) -> Result<ColorVisit<'exchange, 'ctx, 'arena>, CodecError> {
    let ColorSearchState {
        storage,
        active,
        cache,
        mut completed,
        losses: (losses, slot_storage),
        invalid_surface_sides,
    } = state;
    if depth >= 256 {
        return Ok(ColorVisit::Complete(ColorResult {
            color: None,
            height: None,
        }));
    }
    if let Some(result) =
        ctx.get_btree_map(&cache.values, &(id, domain), "STEP presentation cache get")?
    {
        let color = match result.source {
            ColorCacheSource::Local => cache.colors.get(result.color),
            ColorCacheSource::Completed | ColorCacheSource::Shared => completed
                .as_ref()
                .and_then(|completed| completed.colors.get(result.color)),
        }
        .ok_or_else(|| CodecError::malformed("STEP color cache value missing"))?;
        let result = storage.borrow_mut().with_storage(|| {
            Ok::<_, CodecError>(ColorResult {
                color: clone_color_resolution(color, ctx, "step_presentation_color_cache_copy")?,
                height: result.height,
            })
        })?;
        return Ok(ColorVisit::Complete(result));
    }
    if let Some(completed) = completed.as_deref_mut() {
        if let Some(result) = ctx.get_btree_map(
            &completed.values,
            &(id, domain),
            "STEP completed color lookup",
        )? {
            if depth + usize::from(result.height) < 256 {
                let result = storage.borrow_mut().with_storage(|| {
                    let color = completed.colors.get(result.color).ok_or_else(|| {
                        CodecError::malformed("STEP completed color value missing")
                    })?;
                    let color =
                        clone_color_resolution(color, ctx, "step_presentation_color_cache_copy")?;
                    let height = Some(result.height);
                    ctx.insert_btree_map(
                        &mut cache.values,
                        (id, domain),
                        ColorCacheEntry {
                            color: result.color,
                            source: ColorCacheSource::Shared,
                            height,
                        },
                        "step_presentation_color_cache_entries",
                    )?;
                    Ok::<_, CodecError>(ColorResult { color, height })
                })?;
                return Ok(ColorVisit::Complete(result));
            }
        }
    }
    let Some(record) =
        ctx.get_btree_map(exchange.records(), &id, "STEP presentation record get")?
    else {
        return Ok(ColorVisit::Complete(ColorResult {
            color: None,
            height: Some(0),
        }));
    };
    if is_presentation_style_by_context(ctx, record)? {
        return Ok(ColorVisit::Complete(ColorResult {
            color: None,
            height: Some(0),
        }));
    }
    if ctx.contains_btree_set(active, &id, "STEP presentation active contains")? {
        return Ok(ColorVisit::Complete(ColorResult {
            color: None,
            height: None,
        }));
    }
    let depth_guard = ctx.enter_nested("step_presentation_color_walk")?;
    storage
        .borrow_mut()
        .with_storage(|| ctx.insert_btree_set(active, id, "step_presentation_color_active"))?;
    let loss_start = losses.len();
    // Surface fallback can exclude an invalid usage from claims even when
    // another partial selects this record's non-surface domain.
    let height = if matches!(domain, StyleDomain::Curve | StyleDomain::Point) {
        record
            .partial(ctx, "SURFACE_STYLE_USAGE")?
            .is_none_or(|usage| {
                matches!(
                    usage.parameters.first().and_then(ValueExt::enumeration),
                    Some("BOTH" | "POSITIVE" | "NEGATIVE")
                )
            })
            .then_some(0)
    } else {
        Some(0)
    };
    let body = (|| -> Result<ColorBody<'exchange>, CodecError> {
        let transparency = if domain == StyleDomain::Surface {
            surface_transparency(id, record, exchange, (losses, slot_storage), ctx)?
        } else {
            None
        };
        let side_rank = if domain == StyleDomain::Surface {
            let Some(rank) = surface_side_rank(
                id,
                record,
                (losses, slot_storage),
                invalid_surface_sides,
                storage,
                ctx,
            )?
            else {
                return Ok(ColorBody::Ready {
                    transparency,
                    result: None,
                });
            };
            rank
        } else {
            SurfaceSideRank::NoUsage
        };
        let name = if let Some(name) = record.simple_name() {
            Some(name)
        } else {
            ctx.find_map(
                &record.partials[..],
                |partial| {
                    Ok(matches!(
                        partial.name.as_str(),
                        "COLOUR_RGB" | "DRAUGHTING_PRE_DEFINED_COLOUR"
                    )
                    .then_some(partial.name.as_str()))
                },
                "STEP color classification partial traversal",
            )?
        };
        let record_domain = ctx.find_map(
            &record.partials[..],
            |partial| {
                Ok(if partial.name.starts_with("SURFACE_STYLE") {
                    Some(StyleDomain::Surface)
                } else if partial.name == "CURVE_STYLE" {
                    Some(StyleDomain::Curve)
                } else if partial.name == "POINT_STYLE" {
                    Some(StyleDomain::Point)
                } else {
                    None
                })
            },
            "STEP find color traversal",
        )?;
        let incompatible = record_domain
            .is_some_and(|candidate| domain != StyleDomain::Any && candidate != domain);
        if incompatible {
            return Ok(ColorBody::Children {
                record,
                transparency,
                side_rank,
                combine_children: false,
                partial_operation: "STEP find color traversal",
                parameter_operation: "STEP record reference parameter traversal",
            });
        }
        let result = match name {
            Some("COLOUR_RGB") => {
                let Some(rgb) = record.partial(ctx, "COLOUR_RGB")? else {
                    return Ok(ColorBody::Ready {
                        transparency,
                        result: None,
                    });
                };
                let offset = usize::from(record.partials.len() == 1);
                let Some(r) = rgb.parameters.get(offset).and_then(ValueExt::number) else {
                    return Ok(ColorBody::Ready {
                        transparency,
                        result: None,
                    });
                };
                let Some(g) = rgb.parameters.get(offset + 1).and_then(ValueExt::number) else {
                    return Ok(ColorBody::Ready {
                        transparency,
                        result: None,
                    });
                };
                let Some(b) = rgb.parameters.get(offset + 2).and_then(ValueExt::number) else {
                    return Ok(ColorBody::Ready {
                        transparency,
                        result: None,
                    });
                };
                let name_value = if record.partials.len() == 1 {
                    rgb.parameters.first()
                } else {
                    record
                        .partial(ctx, "COLOUR_SPECIFICATION")?
                        .and_then(|partial| partial.parameters.first())
                };
                let Some((r, g, b)) = cadmpeg_core::convert::f32_from_f64(r)
                    .zip(cadmpeg_core::convert::f32_from_f64(g))
                    .zip(cadmpeg_core::convert::f32_from_f64(b))
                    .map(|((r, g), b)| (r, g, b))
                else {
                    return Ok(ColorBody::Ready {
                        transparency,
                        result: None,
                    });
                };
                let Some(color) = Color::new(r, g, b, 1.0) else {
                    return Ok(ColorBody::Ready {
                        transparency,
                        result: None,
                    });
                };
                let name = name_value
                    .map(|value| {
                        decode_text_scoped(
                            exchange,
                            value,
                            (losses, slot_storage),
                            id,
                            ("colour name", StepLossCode::AttributeStringInvalid),
                            ctx,
                            &mut storage.borrow_mut(),
                        )
                    })
                    .transpose()?
                    .flatten();
                Some(ColorResolution::Candidate(ColorCandidate {
                    rank: side_rank,
                    id,
                    color,
                    name,
                }))
            }
            Some("DRAUGHTING_PRE_DEFINED_COLOUR") => {
                let name_value = if record.partials.len() == 1 {
                    record.parameter(0)
                } else {
                    record
                        .partial(ctx, "PRE_DEFINED_ITEM")?
                        .and_then(|partial| partial.parameters.first())
                };
                let Some(name_value) = name_value else {
                    return Ok(ColorBody::Ready {
                        transparency,
                        result: None,
                    });
                };
                let Some(name) = decode_text_scoped(
                    exchange,
                    name_value,
                    (losses, slot_storage),
                    id,
                    (
                        "predefined colour name",
                        StepLossCode::AttributeStringInvalid,
                    ),
                    ctx,
                    &mut storage.borrow_mut(),
                )?
                else {
                    return Ok(ColorBody::Ready {
                        transparency,
                        result: None,
                    });
                };
                predefined(&name).map(|color| {
                    ColorResolution::Candidate(ColorCandidate {
                        rank: side_rank,
                        id,
                        color,
                        name: Some(name),
                    })
                })
            }
            _ => {
                return Ok(ColorBody::Children {
                    record,
                    transparency,
                    side_rank,
                    combine_children: true,
                    partial_operation: "STEP color partial traversal",
                    parameter_operation: "STEP color parameter traversal",
                });
            }
        };
        Ok(ColorBody::Ready {
            transparency,
            result,
        })
    })()?;
    match body {
        ColorBody::Ready {
            transparency,
            result,
        } => {
            let result = finish_color(
                id,
                domain,
                transparency,
                ColorResult {
                    color: result,
                    height,
                },
                ColorSearchState {
                    storage,
                    active,
                    cache,
                    completed,
                    losses: (losses, slot_storage),
                    invalid_surface_sides,
                },
                (depth, loss_start),
                ctx,
            )?;
            drop(depth_guard);
            Ok(ColorVisit::Complete(result))
        }
        ColorBody::Children {
            record,
            transparency,
            side_rank,
            combine_children,
            partial_operation,
            parameter_operation,
        } => Ok(ColorVisit::Frame(ColorFrame {
            id,
            depth,
            loss_start,
            partials: record.partials.iter(),
            partial_operation,
            transparency,
            side_rank,
            combine_children,
            parameter_operation,
            parameters: None,
            references: None,
            result: ColorResult {
                color: None,
                height,
            },
            _depth: depth_guard,
        })),
    }
}

fn next_color_reference<'exchange, 'ctx, 'arena>(
    frame: &mut ColorFrame<'exchange, 'ctx, 'arena>,
    ctx: &'ctx DecodeContext<'arena>,
) -> Result<Option<u64>, CodecError> {
    loop {
        if let Some(references) = &mut frame.references {
            if let Some(reference) = references.next() {
                return reference.map(Some);
            }
            frame.references = None;
        }
        ctx.charge_work(0, frame.parameter_operation)?;
        let value = match &mut frame.parameters {
            Some(parameters) if !parameters.as_slice().is_empty() => {
                ctx.next_charged(parameters, frame.parameter_operation)?
            }
            Some(_) | None => None,
        };
        if let Some(value) = value {
            frame.references = Some(references(value, ctx));
            continue;
        }
        if frame.parameters.is_some() {
            frame.parameters = None;
        }
        ctx.charge_work(0, frame.partial_operation)?;
        if frame.partials.as_slice().is_empty() {
            return Ok(None);
        }
        let partial = ctx
            .next_charged(&mut frame.partials, frame.partial_operation)?
            .ok_or_else(|| CodecError::malformed("STEP color partial source ended early"))?;
        frame.parameters = Some(partial.parameters.iter());
    }
}

fn finish_color(
    id: u64,
    domain: StyleDomain,
    transparency: Option<Fraction>,
    mut result: ColorResult,
    state: ColorSearchState<'_, '_>,
    (depth, loss_start): (usize, usize),
    ctx: &DecodeContext<'_>,
) -> Result<ColorResult, CodecError> {
    let ColorSearchState {
        storage,
        active,
        cache,
        completed,
        losses: (losses, _),
        ..
    } = state;
    ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
    if let Some(transparency) = transparency {
        match result.color.as_mut() {
            Some(ColorResolution::Candidate(candidate)) => {
                if let Some(alpha) = cadmpeg_core::convert::f32_from_f64(1.0 - transparency.get()) {
                    candidate.color = candidate.color.with_alpha(alpha).unwrap_or(candidate.color);
                }
            }
            Some(ColorResolution::Ambiguous { .. }) => {}
            None => {}
        }
    }
    if losses.len() != loss_start {
        result.height = None;
    }
    let mut shared = None;
    if let (Some(completed), Some(height)) = (completed, result.height) {
        if depth + usize::from(height) < 256 {
            if let Some(cached) = ctx.get_btree_map(
                &completed.values,
                &(id, domain),
                "STEP completed color lookup",
            )? {
                shared = Some(cached.color);
            } else {
                let index = completed.colors.len();
                completed.storage.with_storage(|| {
                    let color =
                        clone_color_resolution(&result.color, ctx, "STEP completed color value")?;
                    ctx.push_vec(&mut completed.colors, color, "STEP completed color values")?;
                    ctx.insert_btree_map(
                        &mut completed.values,
                        (id, domain),
                        CompletedColor {
                            color: index,
                            height,
                        },
                        "STEP completed color entries",
                    )
                })?;
                shared = Some(index);
            }
        }
    }
    let (color, source) = if let Some(index) = shared {
        (index, ColorCacheSource::Completed)
    } else {
        let index = cache.colors.len();
        storage.borrow_mut().with_storage(|| {
            let color =
                clone_color_resolution(&result.color, ctx, "step_presentation_color_cache_value")?;
            ctx.push_vec(&mut cache.colors, color, "STEP local color values")
        })?;
        (index, ColorCacheSource::Local)
    };
    storage.borrow_mut().with_storage(|| {
        ctx.insert_btree_map(
            &mut cache.values,
            (id, domain),
            ColorCacheEntry {
                color,
                source,
                height: result.height,
            },
            "step_presentation_color_cache_entries",
        )
    })?;
    Ok(result)
}

fn surface_transparency(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    ctx: &DecodeContext<'_>,
) -> Result<Option<Fraction>, CodecError> {
    let (candidates_buffer, mut candidate_storage) =
        ctx.temporary_vec(0, "step_presentation_transparency_candidates")?;
    let mut candidates = candidates_buffer;
    if let Some(partial) = record.partial(ctx, "SURFACE_STYLE_RENDERING_WITH_PROPERTIES")? {
        let mut parameters = partial.parameters.iter();
        ctx.charge_work(0, "STEP surface transparency parameter traversal")?;
        for _ in 0..parameters.len() {
            let value = ctx
                .next_charged(
                    &mut parameters,
                    "STEP surface transparency parameter traversal",
                )?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            for property_id in references(value, ctx) {
                let property_id = property_id?;
                let Some(property) = ctx.get_btree_map(
                    exchange.records(),
                    &property_id,
                    "STEP presentation record get",
                )?
                else {
                    continue;
                };
                let Some(transparency) = property
                    .partial(ctx, "SURFACE_STYLE_TRANSPARENT")?
                    .and_then(|partial| partial.parameters.first())
                    .and_then(ValueExt::number)
                    .and_then(Fraction::new)
                else {
                    continue;
                };
                ctx.push_scoped_vec(
                    &mut candidate_storage,
                    &mut candidates,
                    (property_id, transparency),
                    "step_presentation_transparency_candidates",
                )?;
            }
        }
    }
    match candidates.as_slice() {
        [] => Ok(None),
        [(_, transparency)] => Ok(Some(*transparency)),
        _ => {
            let (parts_buffer, mut detail_storage) =
                ctx.temporary_vec(0, "STEP transparency detail fragments")?;
            let mut parts = parts_buffer;
            let mut candidates = candidates.iter();
            ctx.charge_work(0, "STEP transparency detail traversal")?;
            for _ in 0..candidates.len() {
                let &(property_id, transparency) = ctx
                    .next_charged(&mut candidates, "STEP transparency detail traversal")?
                    .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
                let part = detail_storage.with_storage(|| {
                    ctx.format_retained(
                        format_args!("#{property_id}={}", transparency.get()),
                        "STEP transparency details",
                    )
                })?;
                ctx.push_scoped_vec(
                    &mut detail_storage,
                    &mut parts,
                    part,
                    "STEP transparency detail fragments",
                )?;
            }
            let details = detail_storage
                .with_storage(|| ctx.join_retained(&parts, ", ", "STEP transparency details"))?;
            let message = ctx.format_retained(format_args!("surface style rendering #{id} has conflicting transparency properties ({details}); transparency omitted"), "step_presentation_transparency_conflict_text")?;
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                losses,
                StepLossCode::SurfaceTransparencyConflict.note(message),
                "step_presentation_losses",
            )?;
            Ok(None)
        }
    }
}

fn surface_side_rank(
    id: u64,
    record: &RawRecord,
    (losses, slot_storage): (
        &mut Vec<LossNote>,
        &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ),
    invalid_surface_sides: &mut BTreeSet<u64>,
    storage: &std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'_>>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<SurfaceSideRank>, CodecError> {
    let Some(partial) = record.partial(ctx, "SURFACE_STYLE_USAGE")? else {
        return Ok(Some(SurfaceSideRank::NoUsage));
    };
    let Some(side) = partial.parameters.first().and_then(ValueExt::enumeration) else {
        storage.borrow_mut().with_storage(|| {
            ctx.insert_btree_set(
                invalid_surface_sides,
                id,
                "step_presentation_invalid_surface_sides",
            )
        })?;
        ctx.push_scoped_vec(
            &mut slot_storage.borrow_mut(),
            losses,
            StepLossCode::SurfaceSideInvalid.note(ctx.format_retained(
                format_args!("SURFACE_STYLE_USAGE #{id} has no valid surface_side; style omitted"),
                "step_presentation_loss_text",
            )?),
            "step_presentation_losses",
        )?;
        return Ok(None);
    };
    match side {
        "BOTH" => Ok(Some(SurfaceSideRank::Both)),
        "POSITIVE" => Ok(Some(SurfaceSideRank::Positive)),
        "NEGATIVE" => Ok(Some(SurfaceSideRank::Negative)),
        _ => {
            storage.borrow_mut().with_storage(|| {
                ctx.insert_btree_set(
                    invalid_surface_sides,
                    id,
                    "step_presentation_invalid_surface_sides",
                )
            })?;
            let message = ctx.format_retained(
                format_args!(
                    "SURFACE_STYLE_USAGE #{id} has invalid surface_side .{side}.; style omitted"
                ),
                "step_presentation_invalid_surface_side_text",
            )?;
            ctx.push_scoped_vec(
                &mut slot_storage.borrow_mut(),
                losses,
                StepLossCode::SurfaceSideInvalid.note(message),
                "step_presentation_losses",
            )?;
            Ok(None)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StyleDomain {
    Any,
    Surface,
    Curve,
    Point,
}

impl cadmpeg_core::decode::cost::DecodeCost for StyleDomain {
    const FIXED_BYTES: Option<u64> =
        Some(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()));
    fn decode_cost(
        &self,
        _ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        _operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        Ok(cadmpeg_core::decode::u64_from_index(std::mem::size_of::<
            Self,
        >()))
    }
}

struct StyleDomainIndex<'ctx, 'arena> {
    complete: BTreeMap<u64, StyleDomain>,
    ctx: &'ctx DecodeContext<'arena>,
    storage: ScopedReservation<'ctx>,
}

impl<'ctx, 'arena> StyleDomainIndex<'ctx, 'arena> {
    fn new(ctx: &'ctx DecodeContext<'arena>) -> Result<Self, CodecError> {
        Ok(Self {
            complete: BTreeMap::new(),
            ctx,
            storage: ctx.reserve_scoped(0, "STEP style domain cache scratch")?,
        })
    }

    fn domain(&mut self, id: u64, exchange: &Exchange) -> Result<StyleDomain, CodecError> {
        let mut active_storage = self
            .ctx
            .reserve_scoped(0, "STEP style domain active scratch")?;
        let mut active = BTreeSet::new();
        self.storage.with_storage(|| {
            style_domain_at(
                id,
                exchange,
                &mut active,
                &mut self.complete,
                &mut active_storage,
                self.ctx,
            )
        })
    }
}

fn style_domain_at(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<u64, StyleDomain>,
    active_storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<StyleDomain, CodecError> {
    if ctx.contains_btree_set(active, &id, "STEP presentation active contains")? {
        return Ok(StyleDomain::Any);
    }
    if let Some(domain) = ctx.get_btree_map(cache, &id, "STEP style domain cache lookup")? {
        return Ok(*domain);
    }
    let domain = style_domain_uncached(id, exchange, active, cache, active_storage, ctx)?;
    // Any is absorbing in a geometric set. A reachable cycle thus has Any
    // for every entry point, independent of the active path.
    ctx.insert_btree_map(cache, id, domain, "step_style_domain_cache")?;
    Ok(domain)
}

fn style_domain_uncached(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<u64, StyleDomain>,
    active_storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<StyleDomain, CodecError> {
    let _nested = ctx.enter_nested("step_presentation_style_domain_walk")?;
    active_storage.with_storage(|| {
        ctx.insert_btree_set(active, id, "step_presentation_style_domain_active")
    })?;
    let Some(record) =
        ctx.get_btree_map(exchange.records(), &id, "STEP presentation record get")?
    else {
        ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
        return Ok(StyleDomain::Any);
    };
    let set_name = ctx.find_map(
        &record.partials[..],
        |partial| {
            Ok(match partial.name.as_str() {
                "GEOMETRIC_SET" => Some("GEOMETRIC_SET"),
                "GEOMETRIC_CURVE_SET" => Some("GEOMETRIC_CURVE_SET"),
                _ => None,
            })
        },
        "STEP style domain at traversal",
    )?;
    if let Some(set_name) = set_name {
        let mut first = None;
        let mut same = true;
        let members = record
            .partial(ctx, set_name)?
            .and_then(|partial| partial.parameters.get(1))
            .and_then(ValueExt::list)
            .unwrap_or_default();
        let mut members = members.iter();
        ctx.charge_work(0, "STEP style domain member traversal")?;
        for _ in 0..members.len() {
            let value = ctx
                .next_charged(&mut members, "STEP style domain member traversal")?
                .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
            let Some(member) = ValueExt::reference(value) else {
                continue;
            };
            let domain = style_domain_at(member, exchange, active, cache, active_storage, ctx)?;
            if first.is_some_and(|first| first != domain) {
                same = false;
            }
            if first.is_none() {
                first = Some(domain);
            }
        }
        if let Some(first) = first {
            ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
            return Ok(if same { first } else { StyleDomain::Any });
        }
    }
    let has_point = ctx
        .find_map(
            &record.partials[..],
            |partial| -> Result<Option<()>, CodecError> {
                let name = partial.name.as_str();
                Ok((ctx
                    .position_by(
                        name.as_bytes().windows(b"POINT".len()),
                        |window| Ok(window == &b"POINT"[..]),
                        "STEP style domain point containment",
                    )?
                    .is_some()
                    || ctx
                        .position_by(
                            name.as_bytes().windows(b"VERTEX".len()),
                            |window| Ok(window == &b"VERTEX"[..]),
                            "STEP style domain vertex containment",
                        )?
                        .is_some())
                .then_some(()))
            },
            "STEP style domain at traversal",
        )?
        .is_some();
    if has_point {
        ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
        return Ok(StyleDomain::Point);
    }
    let has_curve = ctx
        .find_map(
            &record.partials[..],
            |partial| -> Result<Option<()>, CodecError> {
                let name = partial.name.as_str();
                Ok((ctx
                    .position_by(
                        name.as_bytes().windows(b"CURVE".len()),
                        |window| Ok(window == &b"CURVE"[..]),
                        "STEP style domain curve containment",
                    )?
                    .is_some()
                    || ctx
                        .position_by(
                            name.as_bytes().windows(b"EDGE".len()),
                            |window| Ok(window == &b"EDGE"[..]),
                            "STEP style domain edge containment",
                        )?
                        .is_some()
                    || ctx
                        .position_by(
                            name.as_bytes().windows(b"_LINE".len()),
                            |window| Ok(window == &b"_LINE"[..]),
                            "STEP style domain line containment",
                        )?
                        .is_some()
                    || matches!(
                        name,
                        "LINE" | "POLYLINE" | "CIRCLE" | "ELLIPSE" | "HYPERBOLA" | "PARABOLA"
                    ))
                .then_some(()))
            },
            "STEP style domain at traversal",
        )?
        .is_some();
    if has_curve {
        ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
        return Ok(StyleDomain::Curve);
    }
    let result = if ctx
        .find_map(
            &record.partials[..],
            |partial| -> Result<Option<()>, CodecError> {
                let name = partial.name.as_str();
                Ok((ctx
                    .position_by(
                        name.as_bytes().windows(b"FACE".len()),
                        |window| Ok(window == &b"FACE"[..]),
                        "STEP style domain face containment",
                    )?
                    .is_some()
                    || ctx
                        .position_by(
                            name.as_bytes().windows(b"SURFACE".len()),
                            |window| Ok(window == &b"SURFACE"[..]),
                            "STEP style domain surface containment",
                        )?
                        .is_some()
                    || ctx
                        .position_by(
                            name.as_bytes().windows(b"SOLID".len()),
                            |window| Ok(window == &b"SOLID"[..]),
                            "STEP style domain solid containment",
                        )?
                        .is_some()
                    || ctx
                        .position_by(
                            name.as_bytes().windows(b"SHELL".len()),
                            |window| Ok(window == &b"SHELL"[..]),
                            "STEP style domain shell containment",
                        )?
                        .is_some()
                    || matches!(
                        name,
                        "PLANE" | "CYLINDER" | "CONE" | "SPHERE" | "TORUS" | "DEGENERATE_TORUS"
                    ))
                .then_some(()))
            },
            "STEP style domain at traversal",
        )?
        .is_some()
    {
        StyleDomain::Surface
    } else {
        StyleDomain::Any
    };
    ctx.remove_btree_set(active, &id, "STEP presentation active remove")?;
    Ok(result)
}

fn style_is_hidden(
    id: u64,
    hidden_style_ids: &BTreeSet<u64>,
    exchange: &Exchange,
    cache: &mut BTreeMap<u64, bool>,
    storage: &mut ScopedReservation<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let (path_buffer, mut path_storage) = ctx.temporary_vec(0, "STEP hidden style path")?;
    let mut path = path_buffer;
    let (depth_guards_buffer, mut depth_storage) =
        ctx.temporary_vec(0, "STEP hidden style depth guards")?;
    let mut depth_guards = depth_guards_buffer;
    let mut visited_storage = ctx.reserve_scoped(0, "STEP hidden style visited")?;
    let mut visited = BTreeSet::new();
    let mut current = id;
    let hidden = loop {
        ctx.charge_work(1, "STEP hidden style step")?;
        if let Some(hidden) =
            ctx.get_btree_map(cache, &current, "STEP hidden style cache lookup")?
        {
            break *hidden;
        }
        if ctx.contains_btree_set(
            hidden_style_ids,
            &current,
            "STEP presentation hidden_style_ids contains",
        )? {
            break true;
        }
        if !visited_storage.with_storage(|| {
            ctx.insert_btree_set(&mut visited, current, "STEP hidden style visited")
        })? {
            break false;
        }
        let depth_guard = ctx.enter_nested("step_presentation_hidden_style_walk")?;
        ctx.push_scoped_vec(
            &mut depth_storage,
            &mut depth_guards,
            depth_guard,
            "STEP hidden style depth guards",
        )?;
        ctx.push_scoped_vec(
            &mut path_storage,
            &mut path,
            current,
            "STEP hidden style path",
        )?;
        let Some(base) = ctx
            .get_btree_map(
                exchange.records(),
                &current,
                "STEP hidden style record lookup",
            )?
            .map(|record| overridden_style(ctx, record))
            .transpose()?
            .flatten()
        else {
            break false;
        };
        current = base;
    };
    let mut path = path.into_iter();
    ctx.charge_work(0, "STEP hidden style path result traversal")?;
    for _ in 0..path.len() {
        let style = ctx
            .next_charged(&mut path, "STEP hidden style path result traversal")?
            .ok_or_else(|| CodecError::malformed("STEP presentation source ended early"))?;
        storage.with_storage(|| {
            ctx.insert_btree_map(cache, style, hidden, "STEP hidden style cache entries")
        })?;
    }
    Ok(hidden)
}

fn contains_null_style(
    value: &Value,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    depth: usize,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if depth >= 256 {
        return Ok(false);
    }
    let _nested = ctx.enter_nested("step_presentation_null_style_walk")?;
    match value {
        Value::Typed(name, _) if name == "NULL_STYLE" => Ok(true),
        Value::Typed(_, value) => contains_null_style(value, exchange, visited, depth + 1, ctx),
        Value::List(values) => ctx.any_by(
            values,
            |value| contains_null_style(value, exchange, visited, depth + 1, ctx),
            "STEP contains null style value traversal",
        ),
        Value::Reference(id)
            if !ctx.contains_btree_set(visited, id, "STEP presentation visited contains")? =>
        {
            ctx.insert_btree_set(visited, *id, "step_presentation_null_style_visited")?;
            if let Some(record) =
                ctx.get_btree_map(exchange.records(), id, "STEP presentation record get")?
            {
                return ctx.any_by(
                    &record.partials[..],
                    |partial| {
                        ctx.any_by(
                            &partial.parameters,
                            |value| contains_null_style(value, exchange, visited, depth + 1, ctx),
                            "STEP record parameter traversal",
                        )
                    },
                    "STEP contains null style traversal",
                );
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

fn predefined(name: &str) -> Option<Color> {
    let (r, g, b) = if name.eq_ignore_ascii_case("black") {
        (0.0, 0.0, 0.0)
    } else if name.eq_ignore_ascii_case("white") {
        (1.0, 1.0, 1.0)
    } else if name.eq_ignore_ascii_case("red") {
        (1.0, 0.0, 0.0)
    } else if name.eq_ignore_ascii_case("green") {
        (0.0, 1.0, 0.0)
    } else if name.eq_ignore_ascii_case("blue") {
        (0.0, 0.0, 1.0)
    } else if name.eq_ignore_ascii_case("yellow") {
        (1.0, 1.0, 0.0)
    } else if name.eq_ignore_ascii_case("magenta") {
        (1.0, 0.0, 1.0)
    } else if name.eq_ignore_ascii_case("cyan") {
        (0.0, 1.0, 1.0)
    } else {
        return None;
    };
    Color::new(r, g, b, 1.0)
}
#[cfg(test)]
pub(crate) mod tests;
