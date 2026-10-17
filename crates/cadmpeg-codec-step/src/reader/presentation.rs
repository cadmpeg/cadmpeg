// SPDX-License-Identifier: Apache-2.0
//! STEP presentation style and topology color decoding.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::reference::references;
use super::{named_parameter, RecordExt, ValueExt};
use cadmpeg_core::decode::DecodeContext;
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

use super::decode_text;
use super::groups::NonemptyGroup;
use super::topology::TopologyData;
use super::StageOutcome;

pub(super) fn decode(
    exchange: &Exchange,
    topology: &TopologyData,
    ir: &mut CadIr,
    product_definition_ids_by_source: &BTreeMap<u64, Vec<ProductDefinitionId>>,
    ctx: &DecodeContext<'_>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let face_indices = collect_identity_indices(ir.model.faces.iter().map(|face| face.id.as_str()));
    let body_indices =
        collect_identity_indices(ir.model.bodies.iter().map(|body| body.id.as_str()));
    let indices = PresentationIndices {
        faces: &face_indices,
        bodies: &body_indices,
    };
    let entity_ids = EntityIds {
        edges: ir
            .model
            .edges
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        vertices: ir
            .model
            .vertices
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        points: ir
            .model
            .points
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        curves: ir
            .model
            .curves
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        surfaces: ir
            .model
            .surfaces
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        products: product_definition_ids_by_source,
        occurrences: ir
            .model
            .occurrences
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        pmi: ir
            .model
            .pmi
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
        tessellations: ir
            .model
            .tessellations
            .iter()
            .map(|item| item.id.as_str())
            .collect::<std::collections::BTreeSet<_>>(),
    };
    let mut appearance_ids = BTreeMap::<(u64, u32), AppearanceId>::new();
    let mut hidden_style_ids = BTreeSet::new();
    let mut hidden_layer_ids = BTreeSet::new();
    let mut deferred_invisibility = BTreeMap::<u64, (bool, BTreeSet<u64>, BTreeSet<u64>)>::new();
    for (&id, record) in exchange.records() {
        if record.partial("INVISIBILITY").is_none() {
            continue;
        }
        let Some(items) = named_parameter(record, "INVISIBILITY", 0).and_then(ValueExt::list)
        else {
            losses.push(
                StepLossCode::DecodeWarning.note(format!("INVISIBILITY #{id} has no item set")),
            );
            continue;
        };
        let mut supported = true;
        let mut style_targets = BTreeSet::new();
        let mut layer_targets = BTreeSet::new();
        for target in items.iter().filter_map(ValueExt::reference) {
            if exchange
                .records()
                .get(&target)
                .is_some_and(|record| record.partial("PRESENTATION_LAYER_ASSIGNMENT").is_some())
            {
                hidden_layer_ids.insert(target);
                layer_targets.insert(target);
                continue;
            }
            if exchange
                .records()
                .get(&target)
                .is_some_and(|record| styled_item_parts(record).is_some())
            {
                hidden_style_ids.insert(target);
                style_targets.insert(target);
                continue;
            }
            if exchange
                .records()
                .get(&target)
                .is_some_and(super::drawing::is_supported_invisibility_target)
            {
                continue;
            }
            if exchange
                .records()
                .get(&target)
                .is_some_and(super::pmi::is_supported_invisibility_target)
            {
                continue;
            }
            let (body_ids, target_supported) =
                invisible_body_ids(target, exchange, topology, &body_indices, ctx)?;
            let mut hidden = false;
            for body_id in body_ids {
                if let Some(index) = body_indices.get(body_id.as_str()) {
                    ir.model.bodies[*index].visible = Some(false);
                    hidden = true;
                }
            }
            if !target_supported || !hidden {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "INVISIBILITY #{id} targets unsupported item #{target}"
                )));
                supported = false;
            }
        }
        if style_targets.is_empty() && layer_targets.is_empty() && supported {
            typed.insert(id);
        } else if !style_targets.is_empty() || !layer_targets.is_empty() {
            deferred_invisibility.insert(id, (supported, style_targets, layer_targets));
        }
    }
    for (&layer_id, layer) in exchange.records() {
        if layer.partial("PRESENTATION_LAYER_ASSIGNMENT").is_none() {
            continue;
        }
        let Some(assigned_items) =
            named_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 2).and_then(ValueExt::list)
        else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no assigned item set"
            )));
            continue;
        };
        if assigned_items.is_empty() {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has an empty assigned item set"
            )));
            continue;
        }
        let Some(name) =
            named_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 0).and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    layer_id,
                    "presentation layer name",
                    StepLossCode::MetadataStringInvalid,
                )
            })
        else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no name"
            )));
            continue;
        };
        let description = named_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 1)
            .and_then(|value| {
                decode_text(
                    exchange,
                    value,
                    &mut losses,
                    layer_id,
                    "presentation layer description",
                    StepLossCode::MetadataStringInvalid,
                )
            })
            .filter(|value| !value.is_empty());
        let mut items = Vec::new();
        for id in assigned_items.iter().filter_map(ValueExt::reference) {
            append_presentation_items(id, exchange, topology, &entity_ids, indices, &mut items);
        }
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId::from(ids::presentation(kind!("layer"), layer_id)),
            name,
            description,
            visible: hidden_layer_ids.contains(&layer_id).then_some(false),
            items,
        });
        typed.insert(layer_id);
    }
    let mut styles = Vec::new();
    for (&id, record) in exchange.records() {
        if styled_item_parts(record).is_some() {
            let order = style_application_order(id, exchange, ctx)?;
            styles.push((id, order));
        }
    }
    let mut overridden_styles = BTreeSet::new();
    for (id, _) in &styles {
        if let Some(overridden) = overridden_style(&exchange.records()[id]) {
            overridden_styles.insert(overridden);
        }
    }
    styles.sort_by_key(|(_, left)| *left);
    let mut scalar_color_candidates =
        HashMap::<AppearanceTarget, NonemptyGroup<(u64, Color)>>::new();
    let mut style_domains = BTreeMap::new();
    for (style_id, _) in styles {
        if overridden_styles.contains(&style_id) {
            typed.insert(style_id);
            continue;
        }
        let style = &exchange.records()[&style_id];
        let Some(parts) = styled_item_parts(style) else {
            continue;
        };
        let Some(target_step) = parts.target.reference() else {
            losses.push(
                StepLossCode::DecodeWarning
                    .note(format!("STYLED_ITEM #{style_id} has no resolved target")),
            );
            continue;
        };
        if parts.styles.list().is_some_and(<[Value]>::is_empty) {
            typed.insert(style_id);
            continue;
        }
        let domain = style_domain(target_step, exchange, &mut style_domains, ctx)?;
        let mut active = BTreeSet::new();
        let mut color_cache = ColorCache::default();
        let mut invalid_surface_sides = BTreeSet::new();
        let mut style_references = Vec::new();
        for reference in parts
            .styles
            .list()
            .into_iter()
            .flatten()
            .flat_map(|value| references(value, ctx))
        {
            let reference = reference?;
            style_references.push(reference);
        }
        let mut context_style_ids = BTreeSet::new();
        for reference in &style_references {
            if exchange
                .records()
                .get(reference)
                .is_some_and(is_presentation_style_by_context)
            {
                context_style_ids.insert(*reference);
            }
        }
        if !context_style_ids.is_empty() {
            let message = context_style_message(style_id, &context_style_ids, exchange);
            losses.push(StepLossCode::ContextDependentStyleUnresolved.note(message));
            continue;
        }
        let color = combine_color_resolutions(style_references.iter().copied().map(|reference| {
            find_color(
                reference,
                exchange,
                domain,
                ColorSearchState {
                    active: &mut active,
                    cache: &mut color_cache,
                    losses: &mut losses,
                    invalid_surface_sides: &mut invalid_surface_sides,
                },
                ctx,
            )
        }))?;
        let color = if color.is_none() && matches!(domain, StyleDomain::Curve | StyleDomain::Point)
        {
            combine_color_resolutions(style_references.iter().copied().map(|reference| {
                find_color(
                    reference,
                    exchange,
                    StyleDomain::Surface,
                    ColorSearchState {
                        active: &mut active,
                        cache: &mut color_cache,
                        losses: &mut losses,
                        invalid_surface_sides: &mut invalid_surface_sides,
                    },
                    ctx,
                )
            }))?
        } else {
            color
        };
        let color = match color {
            Some(ColorResolution::Candidate(candidate)) => candidate,
            Some(ColorResolution::Ambiguous { .. }) => {
                losses.push(StepLossCode::ConflictingScalarColors.note(format!(
                    "STYLED_ITEM #{style_id} has distinct equal-precedence colors; no scalar color is selected and the source style graph remains retained"
                )));
                continue;
            }
            None => {
                let mut visited = BTreeSet::new();
                if !contains_null_style(parts.styles, exchange, &mut visited, ctx)? {
                    losses.push(StepLossCode::DecodeWarning.note(format!(
                        "STYLED_ITEM #{style_id} has no resolved surface color"
                    )));
                }
                continue;
            }
        };
        let ColorCandidate {
            id: color_id,
            color,
            name,
            ..
        } = color;
        let appearance_key = (color_id, color.a().to_bits());
        let appearance_id = if let Some(appearance_id) = appearance_ids.get(&appearance_key) {
            appearance_id.clone()
        } else {
            let key = if color.a() == 1.0 {
                IdentityKey::from(color_id)
            } else {
                IdentityKey::from(color_id)
                    .dash(key_word!("alpha"))
                    .dash(color.a().to_bits())
            };
            let id = AppearanceId::from(ids::presentation(kind!("appearance"), key));
            ir.model.appearances.push(Appearance {
                id: id.clone(),
                name,
                asset_guid: None,
                library_id: None,
                visual_guid: None,
                physical_token: None,
                schema: Some("step_surface_style".into()),
                category: None,
                base_color: Some(color),
                textures: Vec::new(),
                properties: BTreeMap::new(),
            });
            appearance_ids.insert(appearance_key, id.clone());
            id
        };
        let target_steps =
            expand_style_targets(target_step, exchange, &mut typed, &mut BTreeSet::new(), ctx)?;
        for (ordinal, target_step) in target_steps.into_iter().enumerate() {
            let targets = appearance_targets(target_step, exchange, topology, &entity_ids, indices);
            if targets.is_empty() {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "STYLED_ITEM #{style_id} targets unsupported item #{target_step}"
                )));
                continue;
            }
            for (target_ordinal, target) in targets.into_iter().enumerate() {
                push_scalar_candidate(&mut scalar_color_candidates, &target, style_id, color)?;
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: ids::presentation(
                        kind!("binding"),
                        IdentityKey::from(style_id)
                            .colon(ordinal)
                            .dash(target_ordinal),
                    )
                    .into(),
                    target,
                    appearance: appearance_id.clone(),
                    source_entity_id: Some(format!("#{style_id}")),
                    object_type: None,
                    visible: style_is_hidden(
                        style_id,
                        &hidden_style_ids,
                        exchange,
                        &mut BTreeSet::new(),
                        ctx,
                    )?
                    .then_some(false),
                    channels: BTreeMap::new(),
                });
            }
        }
        typed.insert(style_id);
        if let Some(overridden) = overridden_style(style) {
            typed.insert(overridden);
        }
        for &(id, _) in color_cache.keys() {
            if !invalid_surface_sides.contains(&id) {
                typed.insert(id);
            }
        }
        typed.insert(color_id);
    }
    for (invisibility_id, (mut supported, style_targets, layer_targets)) in deferred_invisibility {
        for style_id in style_targets {
            let mut matched = false;
            for binding in &mut ir.model.appearance_bindings {
                let Some(binding_style_id) = binding
                    .source_entity_id
                    .as_deref()
                    .and_then(|source_id| source_id.strip_prefix('#'))
                    .and_then(|source_id| source_id.parse::<u64>().ok())
                else {
                    continue;
                };
                if style_inherits_from(
                    binding_style_id,
                    style_id,
                    exchange,
                    &mut BTreeSet::new(),
                    ctx,
                )? {
                    binding.visible = Some(false);
                    matched = true;
                }
            }
            if !matched {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "INVISIBILITY #{invisibility_id} targets unsupported item #{style_id}"
                )));
                supported = false;
            }
        }
        for layer_id in layer_targets {
            let expected_id = ids::presentation(kind!("layer"), layer_id);
            let mut matched = false;
            for layer in &mut ir.model.presentation_layers {
                if layer.id.as_str() == expected_id.as_str() {
                    layer.visible = Some(false);
                    matched = true;
                    break;
                }
            }
            if !matched {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "INVISIBILITY #{invisibility_id} targets unsupported item #{layer_id}"
                )));
                supported = false;
            }
        }
        if supported {
            typed.insert(invisibility_id);
        }
    }
    for (target, candidates) in scalar_color_candidates {
        let mut color = candidates.first().1;
        let mut conflict = false;
        for (_, candidate) in candidates.iter().skip(1) {
            if candidate.r() != color.r()
                || candidate.g() != color.g()
                || candidate.b() != color.b()
            {
                conflict = true;
            } else if candidate.a() < color.a() {
                color = *candidate;
            }
        }
        if conflict {
            let message = scalar_conflict_message(&candidates, &target);
            losses.push(StepLossCode::ConflictingScalarColors.note(message));
        } else {
            let slot = match &target {
                AppearanceTarget::Face(face) => face_indices
                    .get(face.as_str())
                    .map(|&index| &mut ir.model.faces[index].color),
                AppearanceTarget::Body(body) => body_indices
                    .get(body.as_str())
                    .map(|&index| &mut ir.model.bodies[index].color),
                _ => None,
            };
            if let Some(slot) = slot {
                *slot = Some(color);
            }
        }
    }
    Ok(StageOutcome {
        value: (),
        claims: typed,
        losses,
        notes: Vec::new(),
    })
}

fn invisible_body_ids(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    body_indices: &BTreeMap<String, usize>,
    ctx: &DecodeContext<'_>,
) -> Result<(BTreeSet<BodyId>, bool), CodecError> {
    let mut body_ids = BTreeSet::new();
    let mut walked = BTreeMap::new();
    let supported = collect_invisible_body_ids(
        id,
        exchange,
        topology,
        body_indices,
        &mut walked,
        &mut body_ids,
        ctx,
    )?;
    Ok((body_ids, supported))
}

/// Collects the bodies that `id` hides and reports whether every item on
/// the way is supported. `walked` maps each record already reached to
/// `None` while it is on the current path and to its result after. A record
/// that reaches a cycle is unsupported from any path, so a kept result holds
/// for every later caller.
fn collect_invisible_body_ids(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    body_indices: &BTreeMap<String, usize>,
    walked: &mut BTreeMap<u64, Option<bool>>,
    body_ids: &mut BTreeSet<BodyId>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if let Some(supported) = walked.get(&id) {
        return Ok(supported.unwrap_or(false));
    }
    let _nested = ctx.enter_nested("step_presentation_invisible_body_walk")?;
    walked.insert(id, None);
    let supported =
        collect_record_body_ids(id, exchange, topology, body_indices, walked, body_ids, ctx)?;
    walked.insert(id, Some(supported));
    Ok(supported)
}

fn collect_record_body_ids(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    body_indices: &BTreeMap<String, usize>,
    walked: &mut BTreeMap<u64, Option<bool>>,
    body_ids: &mut BTreeSet<BodyId>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if let Some(ids) = topology.body_by_root.get(&id) {
        for body in ids {
            if !body_ids.contains(body) {
                let body = body.clone();
                body_ids.insert(body);
            }
        }
        return Ok(!ids.is_empty());
    }
    let fallback = BodyId::from(ids::data(kind!("body"), id));
    if body_indices.contains_key(fallback.as_str()) {
        body_ids.insert(fallback);
        return Ok(true);
    }
    let Some(record) = exchange.records().get(&id) else {
        return Ok(false);
    };
    let mut found_reference = false;
    let mut supported = true;
    if record
        .partials
        .iter()
        .any(|partial| partial.name == "STYLED_ITEM")
        || record
            .partials
            .iter()
            .any(|partial| partial.name == "OVER_RIDING_STYLED_ITEM")
    {
        if let Some(reference) =
            styled_item_parts(record).and_then(|parts| parts.target.reference())
        {
            found_reference = true;
            supported &= collect_invisible_body_ids(
                reference,
                exchange,
                topology,
                body_indices,
                walked,
                body_ids,
                ctx,
            )?;
        }
    } else if record
        .partials
        .iter()
        .any(|partial| super::representation::is_representation_name(&partial.name))
    {
        if let Some(references) = super::representation::items(record) {
            for reference in references {
                found_reference = true;
                supported &= collect_invisible_body_ids(
                    reference,
                    exchange,
                    topology,
                    body_indices,
                    walked,
                    body_ids,
                    ctx,
                )?;
            }
        }
    }
    Ok(found_reference && supported)
}

/// The style targets that `id` stands for, expanding geometric sets in
/// order. `visited` holds every record already reached, so each target
/// appears once, at its first occurrence.
fn expand_style_targets(
    id: u64,
    exchange: &Exchange,
    typed: &mut HashSet<u64>,
    visited: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<u64>, CodecError> {
    if visited.contains(&id) {
        return Ok(Vec::new());
    }
    let _nested = ctx.enter_nested("step_presentation_style_target_walk")?;
    visited.insert(id);
    let Some(record) = exchange.records().get(&id) else {
        return Ok(vec![id]);
    };
    let Some(set_name) = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
        )
        .then_some(partial.name.as_str())
    }) else {
        return Ok(vec![id]);
    };
    typed.insert(id);
    let mut targets = Vec::new();
    for item in named_parameter(record, set_name, 1)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(ValueExt::reference)
    {
        targets.extend(expand_style_targets(item, exchange, typed, visited, ctx)?);
    }
    Ok(targets)
}

fn appearance_targets(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
) -> std::vec::Vec<cadmpeg_ir::appearance::AppearanceTarget> {
    let mut targets = Vec::new();
    if let Some(bodies) = topology.body_by_root.get(&id) {
        for body in bodies {
            if indices.bodies.contains_key(body.as_str()) {
                let body = body.clone();
                targets.push(AppearanceTarget::Body(body));
            }
        }
        return targets;
    }
    if let Some(faces) = topology.faces_by_source.get(&id) {
        for face in faces {
            if indices.faces.contains_key(face.as_str()) {
                let face = face.clone();
                targets.push(AppearanceTarget::Face(face));
            }
        }
        return targets;
    }
    if let Some(edges) = topology.edges_by_source.get(&id) {
        for edge in edges {
            if entity_ids.edges.contains(edge.as_str()) {
                let edge = edge.clone();
                targets.push(AppearanceTarget::Edge(edge));
            }
        }
        return targets;
    }
    if let Some(vertices) = topology.vertices_by_source.get(&id) {
        for vertex in vertices {
            if entity_ids.vertices.contains(vertex.as_str()) {
                let vertex = vertex.clone();
                targets.push(AppearanceTarget::Vertex(vertex));
            }
        }
        return targets;
    }
    let face_id = ids::data(kind!("face"), id);
    let body_id = ids::data(kind!("body"), id);
    let edge_id = ids::data(kind!("edge"), id);
    let surface_id = ids::data(kind!("surface"), id);
    let curve_id = ids::data(kind!("curve"), id);
    let point_id = ids::data(kind!("point"), id);
    let tessellation_id = ids::tessellation(kind!("mesh"), id);
    let target = if indices.faces.contains_key(face_id.as_str()) {
        Some(AppearanceTarget::Face(FaceId::from(face_id)))
    } else if indices.bodies.contains_key(body_id.as_str()) {
        Some(AppearanceTarget::Body(BodyId::from(body_id)))
    } else if entity_ids.edges.contains(edge_id.as_str()) {
        Some(AppearanceTarget::Edge(EdgeId::from(edge_id)))
    } else if entity_ids.surfaces.contains(surface_id.as_str()) {
        Some(AppearanceTarget::Surface(SurfaceId::from(surface_id)))
    } else if entity_ids.curves.contains(curve_id.as_str()) {
        Some(AppearanceTarget::Curve(CurveId::from(curve_id)))
    } else if entity_ids.points.contains(point_id.as_str()) {
        Some(AppearanceTarget::Point(PointId::from(point_id)))
    } else if entity_ids.tessellations.contains(tessellation_id.as_str()) {
        Some(AppearanceTarget::Tessellation(
            tessellation_id.into_string(),
        ))
    } else if exchange.records().contains_key(&id) {
        Some(AppearanceTarget::Source {
            source_id: format!("#{id}"),
        })
    } else {
        None
    };
    if let Some(target) = target {
        targets.push(target);
    }
    targets
}

fn append_presentation_items(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
    items: &mut Vec<PresentationItem>,
) {
    if let Some(bodies) = topology.body_by_root.get(&id) {
        for body in bodies {
            if indices.bodies.contains_key(body.as_str()) {
                let body = body.clone();
                items.push(PresentationItem::Body { body });
            }
        }
        return;
    }
    if let Some(faces) = topology.faces_by_source.get(&id) {
        for face in faces {
            if indices.faces.contains_key(face.as_str()) {
                let face = face.clone();
                items.push(PresentationItem::Face { face });
            }
        }
        return;
    }
    if let Some(edges) = topology.edges_by_source.get(&id) {
        for edge in edges {
            if entity_ids.edges.contains(edge.as_str()) {
                let edge = edge.clone();
                items.push(PresentationItem::Edge { edge });
            }
        }
        return;
    }
    if let Some(vertices) = topology.vertices_by_source.get(&id) {
        for vertex in vertices {
            if entity_ids.vertices.contains(vertex.as_str()) {
                let vertex = vertex.clone();
                items.push(PresentationItem::Vertex { vertex });
            }
        }
        return;
    }
    if let Some(products) = entity_ids.products.get(&id) {
        for product in products {
            let product = product.clone();
            items.push(PresentationItem::Product { product });
        }
        return;
    }
    items.push(presentation_item_one(id, exchange, entity_ids, indices));
}

fn presentation_item_one(
    id: u64,
    exchange: &Exchange,
    entity_ids: &EntityIds<'_>,
    indices: PresentationIndices<'_>,
) -> PresentationItem {
    let candidate = |kind: &crate::ids::IdentityKind| ids::data(kind, id);
    let body = candidate(kind!("body"));
    if indices.bodies.contains_key(body.as_str()) {
        return PresentationItem::Body {
            body: BodyId::from(body),
        };
    }
    let face = candidate(kind!("face"));
    if indices.faces.contains_key(face.as_str()) {
        return PresentationItem::Face {
            face: FaceId::from(face),
        };
    }
    let edge = candidate(kind!("edge"));
    if entity_ids.edges.contains(edge.as_str()) {
        return PresentationItem::Edge {
            edge: EdgeId::from(edge),
        };
    }
    let vertex = candidate(kind!("vertex"));
    if entity_ids.vertices.contains(vertex.as_str()) {
        return PresentationItem::Vertex {
            vertex: VertexId::from(vertex),
        };
    }
    let point = candidate(kind!("point"));
    if entity_ids.points.contains(point.as_str()) {
        return PresentationItem::Point {
            point: PointId::from(point),
        };
    }
    let curve = candidate(kind!("curve"));
    if entity_ids.curves.contains(curve.as_str()) {
        return PresentationItem::Curve {
            curve: CurveId::from(curve),
        };
    }
    let surface = candidate(kind!("surface"));
    if entity_ids.surfaces.contains(surface.as_str()) {
        return PresentationItem::Surface {
            surface: SurfaceId::from(surface),
        };
    }
    let Some(record) = exchange.records().get(&id) else {
        return PresentationItem::Source {
            source_id: super::step_source_id(id),
        };
    };
    let has = |name: &str| record.partial(name).is_some();
    if has("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        && entity_ids
            .occurrences
            .contains(ids::product(kind!("occurrence"), id).as_str())
    {
        PresentationItem::Occurrence {
            occurrence: OccurrenceId::from(ids::product(kind!("occurrence"), id)),
        }
    } else if record.partials.iter().any(|partial| {
        (partial.name == "DATUM"
            || partial.name == "DATUM_SYSTEM"
            || partial.name.starts_with("DIMENSIONAL_")
            || partial.name.ends_with("_TOLERANCE")
            || super::pmi::is_presentation_annotation(&partial.name))
            && entity_ids
                .pmi
                .contains(ids::presentation(kind!("pmi"), id).as_str())
    }) {
        PresentationItem::Pmi {
            annotation: PmiId::from(ids::presentation(kind!("pmi"), id)),
        }
    } else if (has("TRIANGULATED_FACE")
        || has("COMPLEX_TRIANGULATED_FACE")
        || has("TRIANGULATED_SURFACE_SET")
        || has("COMPLEX_TRIANGULATED_SURFACE_SET"))
        && entity_ids
            .tessellations
            .contains(ids::tessellation(kind!("mesh"), id).as_str())
    {
        PresentationItem::Tessellation {
            tessellation: ids::tessellation(kind!("mesh"), id).into_string(),
        }
    } else {
        PresentationItem::Source {
            source_id: super::step_source_id(id),
        }
    }
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

fn push_scalar_candidate(
    candidates: &mut HashMap<AppearanceTarget, NonemptyGroup<(u64, Color)>>,
    target: &AppearanceTarget,
    style_id: u64,
    color: Color,
) -> Result<(), CodecError> {
    if !matches!(
        target,
        AppearanceTarget::Face(_) | AppearanceTarget::Body(_)
    ) {
        return Ok(());
    }
    if !candidates.contains_key(target) {
        let key = match target {
            AppearanceTarget::Face(face) => AppearanceTarget::Face(face.clone()),
            AppearanceTarget::Body(body) => AppearanceTarget::Body(body.clone()),
            _ => return Ok(()),
        };

        candidates.insert(key, NonemptyGroup::new((style_id, color)));
        return Ok(());
    }
    let values = candidates
        .get_mut(target)
        .ok_or_else(|| CodecError::malformed("presentation scalar target was not indexed"))?;
    values.push((style_id, color));
    Ok(())
}

fn collect_identity_indices<'a>(
    identities: impl IntoIterator<Item = &'a str>,
) -> std::collections::BTreeMap<std::string::String, usize> {
    let mut result = BTreeMap::new();
    for (index, identity) in identities.into_iter().enumerate() {
        if let Some(existing) = result.get_mut(identity) {
            *existing = index;
            continue;
        }
        let copy = identity.to_owned();
        result.insert(copy, index);
    }
    result
}

fn overridden_style(style: &RawRecord) -> Option<u64> {
    styled_item_parts(style)
        .and_then(|parts| parts.overridden)
        .and_then(ValueExt::reference)
}

struct StyledItemParts<'a> {
    styles: &'a Value,
    target: &'a Value,
    overridden: Option<&'a Value>,
}

fn styled_item_parts(record: &RawRecord) -> Option<StyledItemParts<'_>> {
    if let Some(partial) = record
        .partials
        .iter()
        .find(|partial| partial.name == "OVER_RIDING_STYLED_ITEM")
    {
        let parameters = partial.parameters.as_slice();
        let target = parameters.get(parameters.len().checked_sub(2)?)?;
        let styles = parameters.get(parameters.len().checked_sub(3)?)?;
        let overridden = parameters.last()?;
        return Some(StyledItemParts {
            styles,
            target,
            overridden: Some(overridden),
        });
    }
    let partial = record
        .partials
        .iter()
        .find(|partial| partial.name == "STYLED_ITEM")?;
    let parameters = partial.parameters.as_slice();
    let target = parameters.last()?;
    let styles = parameters.get(parameters.len().checked_sub(2)?)?;
    Some(StyledItemParts {
        styles,
        target,
        overridden: None,
    })
}

fn is_presentation_style_by_context(record: &RawRecord) -> bool {
    record
        .partials
        .iter()
        .any(|partial| partial.name == "PRESENTATION_STYLE_BY_CONTEXT")
}

fn presentation_style_context(record: &RawRecord) -> Option<&Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "PRESENTATION_STYLE_BY_CONTEXT")
        .and_then(|partial| partial.parameters.last())
}

struct ContextStyleDetails<'a> {
    ids: &'a BTreeSet<u64>,
    exchange: &'a Exchange,
}

impl std::fmt::Display for ContextStyleDetails<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, context_style_id) in self.ids.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            let context = self
                .exchange
                .records()
                .get(context_style_id)
                .and_then(presentation_style_context)
                .and_then(ValueExt::reference);
            match context {
                Some(context) => write!(formatter, "#{context_style_id} in #{context}")?,
                None => write!(formatter, "#{context_style_id} in unresolved")?,
            }
        }
        Ok(())
    }
}

fn context_style_message(
    style_id: u64,
    ids: &BTreeSet<u64>,
    exchange: &Exchange,
) -> std::string::String {
    let details = ContextStyleDetails { ids, exchange };
    format!("STYLED_ITEM #{style_id} has context-dependent style assignments {details}; no presentation context is selected by the neutral model; those source branches remain opaque")
}

struct ScalarStyleIds<'a>(&'a NonemptyGroup<(u64, Color)>);

impl std::fmt::Display for ScalarStyleIds<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (style_id, _)) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "#{style_id}")?;
        }
        Ok(())
    }
}

fn scalar_conflict_message(
    candidates: &NonemptyGroup<(u64, Color)>,
    target: &AppearanceTarget,
) -> std::string::String {
    let style_ids = ScalarStyleIds(candidates);
    format!("independent styled items {style_ids} assign conflicting scalar colors to {target:?}; scalar color omitted and appearance bindings retain every assignment")
}

pub(super) fn styled_item_target(record: &RawRecord) -> Option<u64> {
    styled_item_parts(record).and_then(|parts| parts.target.reference())
}

/// The position of one styled item in override-depth order.
///
/// A style whose override walk revisits a style states no depth. Absence
/// takes a position of its own, after every stated depth; it is not read as
/// the deepest style.
fn style_application_order(
    id: u64,
    exchange: &Exchange,
    ctx: &DecodeContext<'_>,
) -> Result<(bool, Option<u32>), CodecError> {
    let depth = style_depth(id, exchange, &mut BTreeSet::new(), ctx)?;
    Ok((depth.is_none(), depth))
}

fn style_depth(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    if active.contains(&id) {
        return Ok(None);
    }
    let _nested = ctx.enter_nested("step_presentation_style_depth_walk")?;
    active.insert(id);
    let result = if let Some(style) = exchange.records().get(&id) {
        if let Some(base) = overridden_style(style) {
            style_depth(base, exchange, active, ctx)?.and_then(|depth| depth.checked_add(1))
        } else {
            Some(0)
        }
    } else {
        None
    };
    active.remove(&id);
    Ok(result)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SurfaceSideRank {
    NoUsage,
    Negative,
    Positive,
    Both,
}

#[derive(Clone)]
struct ColorCandidate {
    rank: SurfaceSideRank,
    id: u64,
    color: Color,
    name: Option<String>,
}

#[derive(Clone)]
enum ColorResolution {
    Candidate(ColorCandidate),
    Ambiguous { rank: SurfaceSideRank },
}

type CachedColor = Option<ColorResolution>;

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

/// Small style searches keep their memo entries in the local frame. Wider
/// graphs allocate checked ordered-map entries after the inline slots fill.
#[derive(Default)]
struct ColorCache {
    inline: [Option<((u64, StyleDomain), CachedColor)>; 8],
    overflow: BTreeMap<(u64, StyleDomain), CachedColor>,
}

impl ColorCache {
    fn get(&self, key: &(u64, StyleDomain)) -> Option<&Option<ColorResolution>> {
        for slot in &self.inline {
            let Some((stored, value)) = slot else {
                break;
            };
            if stored == key {
                return Some(value);
            }
        }

        self.overflow.get(key)
    }

    fn insert(&mut self, key: (u64, StyleDomain), value: CachedColor) {
        for slot in &mut self.inline {
            match slot {
                Some((stored, cached)) if *stored == key => {
                    *cached = value;
                    return;
                }
                None => {
                    *slot = Some((key, value));
                    return;
                }
                Some(_) => {}
            }
        }
        self.overflow.insert(key, value);
    }

    fn keys(&self) -> impl Iterator<Item = &(u64, StyleDomain)> {
        self.inline
            .iter()
            .filter_map(|slot| slot.as_ref().map(|(key, _)| key))
            .chain(self.overflow.keys())
    }
}

struct ColorSearchState<'a> {
    active: &'a mut BTreeSet<u64>,
    cache: &'a mut ColorCache,
    losses: &'a mut Vec<LossNote>,
    invalid_surface_sides: &'a mut BTreeSet<u64>,
}

fn find_color(
    id: u64,
    exchange: &Exchange,
    domain: StyleDomain,
    state: ColorSearchState<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<CachedColor, CodecError> {
    let ColorSearchState {
        active,
        cache,
        losses,
        invalid_surface_sides,
    } = state;
    if let Some(result) = cache.get(&(id, domain)) {
        return Ok(result.clone());
    }
    let Some(record) = exchange.records().get(&id) else {
        return Ok(None);
    };
    if is_presentation_style_by_context(record) {
        return Ok(None);
    }
    if active.contains(&id) {
        return Ok(None);
    }
    let _nested = ctx.enter_nested("step_presentation_color_walk")?;
    active.insert(id);
    let transparency = if domain == StyleDomain::Surface {
        surface_transparency(id, record, exchange, losses, ctx)?
    } else {
        None
    };
    let result = (|| -> Result<CachedColor, CodecError> {
        let side_rank = if domain == StyleDomain::Surface {
            let Some(rank) = surface_side_rank(id, record, losses, invalid_surface_sides) else {
                return Ok(None);
            };
            rank
        } else {
            SurfaceSideRank::NoUsage
        };
        let name = record.simple_name().or_else(|| {
            record.partials.iter().find_map(|partial| {
                matches!(
                    partial.name.as_str(),
                    "COLOUR_RGB" | "DRAUGHTING_PRE_DEFINED_COLOUR"
                )
                .then_some(partial.name.as_str())
            })
        });
        let record_domain = record.partials.iter().find_map(|partial| {
            if partial.name.starts_with("SURFACE_STYLE") {
                Some(StyleDomain::Surface)
            } else if partial.name == "CURVE_STYLE" {
                Some(StyleDomain::Curve)
            } else if partial.name == "POINT_STYLE" {
                Some(StyleDomain::Point)
            } else {
                None
            }
        });
        let incompatible = record_domain
            .is_some_and(|candidate| domain != StyleDomain::Any && candidate != domain);
        if incompatible {
            for reference in record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .flat_map(|value| references(value, ctx))
            {
                let reference = reference?;
                // The recursive search caches the colour and records its losses.
                find_color(
                    reference,
                    exchange,
                    domain,
                    ColorSearchState {
                        active: &mut *active,
                        cache: &mut *cache,
                        losses: &mut *losses,
                        invalid_surface_sides: &mut *invalid_surface_sides,
                    },
                    ctx,
                )?;
            }
            return Ok(None);
        }
        match name {
            Some("COLOUR_RGB") => {
                let Some(rgb) = record
                    .partials
                    .iter()
                    .find(|partial| partial.name == "COLOUR_RGB")
                else {
                    return Ok(None);
                };
                let offset = usize::from(record.partials.len() == 1);
                let Some(r) = rgb.parameters.get(offset).and_then(ValueExt::number) else {
                    return Ok(None);
                };
                let Some(g) = rgb.parameters.get(offset + 1).and_then(ValueExt::number) else {
                    return Ok(None);
                };
                let Some(b) = rgb.parameters.get(offset + 2).and_then(ValueExt::number) else {
                    return Ok(None);
                };
                let name_value = if record.partials.len() == 1 {
                    rgb.parameters.first()
                } else {
                    record
                        .partials
                        .iter()
                        .find(|partial| partial.name == "COLOUR_SPECIFICATION")
                        .and_then(|partial| partial.parameters.first())
                };
                let Some((r, g, b)) = cadmpeg_core::convert::f32_from_f64(r)
                    .zip(cadmpeg_core::convert::f32_from_f64(g))
                    .zip(cadmpeg_core::convert::f32_from_f64(b))
                    .map(|((r, g), b)| (r, g, b))
                else {
                    return Ok(None);
                };
                let Some(color) = Color::new(r, g, b, 1.0) else {
                    return Ok(None);
                };
                let name = name_value.and_then(|value| {
                    decode_text(
                        exchange,
                        value,
                        losses,
                        id,
                        "colour name",
                        StepLossCode::AttributeStringInvalid,
                    )
                });
                Ok(Some(ColorResolution::Candidate(ColorCandidate {
                    rank: side_rank,
                    id,
                    color,
                    name,
                })))
            }
            Some("DRAUGHTING_PRE_DEFINED_COLOUR") => {
                let name_value = if record.partials.len() == 1 {
                    record.parameter(0)
                } else {
                    record
                        .partials
                        .iter()
                        .find(|partial| partial.name == "PRE_DEFINED_ITEM")
                        .and_then(|partial| partial.parameters.first())
                };
                let Some(name_value) = name_value else {
                    return Ok(None);
                };
                let Some(name) = decode_text(
                    exchange,
                    name_value,
                    losses,
                    id,
                    "predefined colour name",
                    StepLossCode::AttributeStringInvalid,
                ) else {
                    return Ok(None);
                };
                Ok(predefined(&name).map(|color| {
                    ColorResolution::Candidate(ColorCandidate {
                        rank: side_rank,
                        id,
                        color,
                        name: Some(name),
                    })
                }))
            }
            _ => combine_color_resolutions(
                record
                    .partials
                    .iter()
                    .flat_map(|partial| partial.parameters.iter())
                    .flat_map(|value| references(value, ctx))
                    .map(|reference| {
                        let reference = reference?;
                        find_color(
                            reference,
                            exchange,
                            domain,
                            ColorSearchState {
                                active: &mut *active,
                                cache: &mut *cache,
                                losses: &mut *losses,
                                invalid_surface_sides: &mut *invalid_surface_sides,
                            },
                            ctx,
                        )
                    })
                    .map(|result| {
                        result.map(|candidate| {
                            candidate.map(|candidate| candidate.with_min_rank(side_rank))
                        })
                    }),
            ),
        }
    })();
    active.remove(&id);
    let mut result = result?;
    if let Some(transparency) = transparency {
        match result.as_mut() {
            Some(ColorResolution::Candidate(candidate)) => {
                if let Some(alpha) = cadmpeg_core::convert::f32_from_f64(1.0 - transparency.get()) {
                    candidate.color = candidate.color.with_alpha(alpha).unwrap_or(candidate.color);
                }
            }
            Some(ColorResolution::Ambiguous { .. }) => {}
            None => {}
        }
    }
    cache.insert((id, domain), result.clone());
    Ok(result)
}

struct TransparencyDetails<'a>(&'a [(u64, Fraction)]);

impl std::fmt::Display for TransparencyDetails<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (index, (property_id, transparency)) in self.0.iter().enumerate() {
            if index > 0 {
                formatter.write_str(", ")?;
            }
            write!(formatter, "#{property_id}={}", transparency.get())?;
        }
        Ok(())
    }
}

fn surface_transparency(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    losses: &mut Vec<LossNote>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Fraction>, CodecError> {
    let mut candidates = Vec::new();
    for property_id in record
        .partials
        .iter()
        .filter(|partial| partial.name == "SURFACE_STYLE_RENDERING_WITH_PROPERTIES")
        .flat_map(|partial| {
            partial
                .parameters
                .iter()
                .flat_map(|value| references(value, ctx))
        })
    {
        let property_id = property_id?;
        let Some(property) = exchange.records().get(&property_id) else {
            continue;
        };
        let Some(transparency) = property
            .partials
            .iter()
            .find(|partial| partial.name == "SURFACE_STYLE_TRANSPARENT")
            .and_then(|partial| partial.parameters.first())
            .and_then(ValueExt::number)
            .and_then(Fraction::new)
        else {
            continue;
        };
        candidates.push((property_id, transparency));
    }
    match candidates.as_slice() {
        [] => Ok(None),
        [(_, transparency)] => Ok(Some(*transparency)),
        _ => {
            let details = TransparencyDetails(&candidates);
            let message = format!("surface style rendering #{id} has conflicting transparency properties ({details}); transparency omitted");
            losses.push(StepLossCode::SurfaceTransparencyConflict.note(message));
            Ok(None)
        }
    }
}

fn surface_side_rank(
    id: u64,
    record: &RawRecord,
    losses: &mut Vec<LossNote>,
    invalid_surface_sides: &mut BTreeSet<u64>,
) -> Option<SurfaceSideRank> {
    let Some(partial) = record
        .partials
        .iter()
        .find(|partial| partial.name == "SURFACE_STYLE_USAGE")
    else {
        return Some(SurfaceSideRank::NoUsage);
    };
    let Some(side) = partial.parameters.first().and_then(ValueExt::enumeration) else {
        invalid_surface_sides.insert(id);
        losses.push(StepLossCode::SurfaceSideInvalid.note(format!(
            "SURFACE_STYLE_USAGE #{id} has no valid surface_side; style omitted"
        )));
        return None;
    };
    match side {
        "BOTH" => Some(SurfaceSideRank::Both),
        "POSITIVE" => Some(SurfaceSideRank::Positive),
        "NEGATIVE" => Some(SurfaceSideRank::Negative),
        _ => {
            invalid_surface_sides.insert(id);
            let message = format!(
                "SURFACE_STYLE_USAGE #{id} has invalid surface_side .{side}.; style omitted"
            );
            losses.push(StepLossCode::SurfaceSideInvalid.note(message));
            None
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

/// The style domain of `id`. `domains` keeps the domain of every record
/// already resolved; a record that reaches a cycle resolves to
/// [`StyleDomain::Any`] from any path, so a kept domain holds for every
/// later caller.
fn style_domain(
    id: u64,
    exchange: &Exchange,
    domains: &mut BTreeMap<u64, StyleDomain>,
    ctx: &DecodeContext<'_>,
) -> Result<StyleDomain, CodecError> {
    style_domain_at(id, exchange, &mut BTreeSet::new(), domains, ctx)
}

fn style_domain_at(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    domains: &mut BTreeMap<u64, StyleDomain>,
    ctx: &DecodeContext<'_>,
) -> Result<StyleDomain, CodecError> {
    if let Some(domain) = domains.get(&id) {
        return Ok(*domain);
    }
    if active.contains(&id) {
        return Ok(StyleDomain::Any);
    }
    let _nested = ctx.enter_nested("step_presentation_style_domain_walk")?;
    let domain = record_style_domain(id, exchange, active, domains, ctx)?;
    domains.insert(id, domain);
    Ok(domain)
}

fn record_style_domain(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    domains: &mut BTreeMap<u64, StyleDomain>,
    ctx: &DecodeContext<'_>,
) -> Result<StyleDomain, CodecError> {
    let Some(record) = exchange.records().get(&id) else {
        return Ok(StyleDomain::Any);
    };
    let set_name = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
        )
        .then_some(partial.name.as_str())
    });
    if let Some(set_name) = set_name {
        active.insert(id);
        let mut first = None;
        let mut same = true;
        for member in named_parameter(record, set_name, 1)
            .and_then(ValueExt::list)
            .into_iter()
            .flatten()
            .filter_map(ValueExt::reference)
        {
            let domain = style_domain_at(member, exchange, active, domains, ctx)?;
            if first.is_some_and(|first| first != domain) {
                same = false;
            }
            if first.is_none() {
                first = Some(domain);
            }
        }
        active.remove(&id);
        if let Some(first) = first {
            return Ok(if same { first } else { StyleDomain::Any });
        }
    }
    let has_point = record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.contains("POINT") || name.contains("VERTEX")
    });
    if has_point {
        return Ok(StyleDomain::Point);
    }
    let has_curve = record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.contains("CURVE")
            || name.contains("EDGE")
            || name.contains("_LINE")
            || matches!(
                name,
                "LINE" | "POLYLINE" | "CIRCLE" | "ELLIPSE" | "HYPERBOLA" | "PARABOLA"
            )
    });
    if has_curve {
        return Ok(StyleDomain::Curve);
    }
    Ok(
        if record.partials.iter().any(|partial| {
            let name = partial.name.as_str();
            name.contains("FACE")
                || name.contains("SURFACE")
                || name.contains("SOLID")
                || name.contains("SHELL")
                || matches!(
                    name,
                    "PLANE" | "CYLINDER" | "CONE" | "SPHERE" | "TORUS" | "DEGENERATE_TORUS"
                )
        }) {
            StyleDomain::Surface
        } else {
            StyleDomain::Any
        },
    )
}

fn style_is_hidden(
    id: u64,
    hidden_style_ids: &BTreeSet<u64>,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if hidden_style_ids.contains(&id) || active.contains(&id) {
        return Ok(hidden_style_ids.contains(&id));
    }
    let _nested = ctx.enter_nested("step_presentation_hidden_style_walk")?;
    active.insert(id);
    let hidden = if let Some(base) = exchange.records().get(&id).and_then(overridden_style) {
        style_is_hidden(base, hidden_style_ids, exchange, active, ctx)?
    } else {
        false
    };
    active.remove(&id);
    Ok(hidden)
}

fn style_inherits_from(
    id: u64,
    ancestor: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if id == ancestor || active.contains(&id) {
        return Ok(id == ancestor);
    }
    let _nested = ctx.enter_nested("step_presentation_style_inheritance_walk")?;
    active.insert(id);
    let inherits = if let Some(base) = exchange.records().get(&id).and_then(overridden_style) {
        style_inherits_from(base, ancestor, exchange, active, ctx)?
    } else {
        false
    };
    active.remove(&id);
    Ok(inherits)
}

fn contains_null_style(
    value: &Value,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let _nested = ctx.enter_nested("step_presentation_null_style_walk")?;
    match value {
        Value::Typed(name, _) if name == "NULL_STYLE" => Ok(true),
        Value::Typed(_, value) => contains_null_style(value, exchange, visited, ctx),
        Value::List(values) => {
            for value in values {
                if contains_null_style(value, exchange, visited, ctx)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Value::Reference(id) if !visited.contains(id) => {
            visited.insert(*id);
            if let Some(record) = exchange.records().get(id) {
                for value in record
                    .partials
                    .iter()
                    .flat_map(|partial| partial.parameters.iter())
                {
                    if contains_null_style(value, exchange, visited, ctx)? {
                        return Ok(true);
                    }
                }
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
