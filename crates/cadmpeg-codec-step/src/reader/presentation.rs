// SPDX-License-Identifier: Apache-2.0
//! STEP presentation style and topology color decoding.

use crate::ids::{key_word, kind};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use super::{named_parameter, RecordExt, ValueExt};
use super::reference::references;
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

use super::decode_text_charged;
use super::topology::TopologyData;
use super::StageOutcome;

pub(super) fn decode(
    exchange: &Exchange,
    topology: &TopologyData,
    ir: &mut CadIr,
    product_definition_ids_by_source: &BTreeMap<u64, Vec<ProductDefinitionId>>,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<StageOutcome<()>, CodecError> {
    let mut typed = HashSet::new();
    let mut losses = Vec::new();
    let graph_limit = super::record_graph_limit(ctx);
    let face_indices = collect_identity_indices(
        ir.model.faces.iter().map(|face| face.id.as_str()),
        ctx,
        "step_presentation_face_indices",
    )?;
    let body_indices = collect_identity_indices(
        ir.model.bodies.iter().map(|body| body.id.as_str()),
        ctx,
        "step_presentation_body_indices",
    )?;
    let entity_ids = EntityIds {
        edges: collect_borrowed_identity_set(ir.model.edges.iter().map(|item| item.id.as_str()), ctx, "step_presentation_edge_ids")?,
        vertices: collect_borrowed_identity_set(ir.model.vertices.iter().map(|item| item.id.as_str()), ctx, "step_presentation_vertex_ids")?,
        points: collect_borrowed_identity_set(ir.model.points.iter().map(|item| item.id.as_str()), ctx, "step_presentation_point_ids")?,
        curves: collect_borrowed_identity_set(ir.model.curves.iter().map(|item| item.id.as_str()), ctx, "step_presentation_curve_ids")?,
        surfaces: collect_borrowed_identity_set(ir.model.surfaces.iter().map(|item| item.id.as_str()), ctx, "step_presentation_surface_ids")?,
        products: product_definition_ids_by_source,
        occurrences: collect_borrowed_identity_set(ir.model.occurrences.iter().map(|item| item.id.as_str()), ctx, "step_presentation_occurrence_ids")?,
        pmi: collect_borrowed_identity_set(ir.model.pmi.iter().map(|item| item.id.as_str()), ctx, "step_presentation_pmi_ids")?,
        tessellations: collect_borrowed_identity_set(ir.model.tessellations.iter().map(|item| item.id.as_str()), ctx, "step_presentation_tessellation_ids")?,
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
                insert_presentation_set(&mut hidden_layer_ids, target, ctx, "step_presentation_hidden_layer_ids")?;
                insert_presentation_set(&mut layer_targets, target, ctx, "step_presentation_invisibility_layer_targets")?;
                continue;
            }
            if exchange
                .records()
                .get(&target)
                .is_some_and(|record| styled_item_parts(record).is_some())
            {
                insert_presentation_set(&mut hidden_style_ids, target, ctx, "step_presentation_hidden_style_ids")?;
                insert_presentation_set(&mut style_targets, target, ctx, "step_presentation_invisibility_style_targets")?;
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
                invisible_body_ids(target, exchange, topology, &body_indices);
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
            named_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 0).map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
                    layer_id,
                    "presentation layer name",
                    StepLossCode::MetadataStringInvalid,
                    ctx,
                )
            })
            .transpose()?
            .flatten()
        else {
            losses.push(StepLossCode::DecodeWarning.note(format!(
                "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no name"
            )));
            continue;
        };
        let description = named_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 1)
            .map(|value| {
                decode_text_charged(
                    exchange,
                    value,
                    &mut losses,
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
        for id in assigned_items.iter().filter_map(ValueExt::reference) {
            items.extend(presentation_item(
                id,
                exchange,
                topology,
                &entity_ids,
                &face_indices,
                &body_indices,
            ));
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
            push_presentation_vec(&mut styles, id, ctx, "step_presentation_style_ids")?;
        }
    }
    let mut overridden_styles = BTreeSet::new();
    for id in &styles {
        if let Some(overridden) = overridden_style(&exchange.records()[id]) {
            insert_presentation_set(&mut overridden_styles, overridden, ctx, "step_presentation_overridden_styles")?;
        }
    }
    styles.sort_by_key(|id| style_application_order(*id, exchange, graph_limit));
    let mut scalar_color_candidates = HashMap::<AppearanceTarget, Vec<(u64, Color)>>::new();
    for style_id in styles {
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
        let domain = style_domain(target_step, exchange);
        let mut active = BTreeSet::new();
        let mut color_cache = BTreeMap::new();
        let mut invalid_surface_sides = BTreeSet::new();
        let mut style_references = Vec::new();
        for reference in parts.styles.list().into_iter().flatten().flat_map(references) {
            push_presentation_vec(&mut style_references, reference, ctx, "step_presentation_style_references")?;
        }
        let mut context_style_ids = BTreeSet::new();
        for reference in &style_references {
            if exchange.records().get(reference).is_some_and(is_presentation_style_by_context) {
                insert_presentation_set(&mut context_style_ids, *reference, ctx, "step_presentation_context_style_ids")?;
            }
        }
        if !context_style_ids.is_empty() {
            let contexts = context_style_ids
                .iter()
                .map(|context_style_id| {
                    let context = exchange
                        .records()
                        .get(context_style_id)
                        .and_then(presentation_style_context)
                        .and_then(ValueExt::reference)
                        .map_or_else(|| "unresolved".to_string(), |id| format!("#{id}"));
                    format!("#{context_style_id} in {context}")
                })
                .collect::<Vec<_>>();
            losses.push(StepLossCode::ContextDependentStyleUnresolved.note(format!(
                "STYLED_ITEM #{style_id} has context-dependent style assignments {}; no presentation context is selected by the neutral model; those source branches remain opaque",
                contexts.join(", ")
            )));
            continue;
        }
        let color =
            combine_color_resolutions(style_references.iter().copied().map(|reference| {
                find_color(
                    reference,
                    exchange,
                    domain,
                    &mut active,
                    &mut color_cache,
                    &mut losses,
                    &mut invalid_surface_sides,
                    0,
                    ctx,
                )
            }))?;
        let color = if color.is_none() && matches!(domain, StyleDomain::Curve | StyleDomain::Point) {
            combine_color_resolutions(style_references.iter().copied().map(|reference| {
                find_color(
                    reference,
                    exchange,
                    StyleDomain::Surface,
                    &mut active,
                    &mut color_cache,
                    &mut losses,
                    &mut invalid_surface_sides,
                    0,
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
                if !contains_null_style(parts.styles, exchange, &mut visited, 0) {
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
        let appearance_id = appearance_ids
            .entry((color_id, color.a().to_bits()))
            .or_insert_with(|| {
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
                id
            })
            .clone();
        let target_steps = expand_style_targets(
            target_step,
            exchange,
            &mut typed,
            &mut BTreeSet::new(),
            0,
            graph_limit,
        );
        for (ordinal, target_step) in target_steps.into_iter().enumerate() {
            let targets = appearance_targets(
                target_step,
                exchange,
                topology,
                &entity_ids,
                &face_indices,
                &body_indices,
            );
            if targets.is_empty() {
                losses.push(StepLossCode::DecodeWarning.note(format!(
                    "STYLED_ITEM #{style_id} targets unsupported item #{target_step}"
                )));
                continue;
            }
            for (target_ordinal, target) in targets.into_iter().enumerate() {
                match &target {
                    AppearanceTarget::Face(_) | AppearanceTarget::Body(_) => {
                        scalar_color_candidates
                            .entry(target.clone())
                            .or_default()
                            .push((style_id, color));
                    }
                    _ => {}
                }
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
                    )
                    .then_some(false),
                    channels: BTreeMap::new(),
                });
            }
        }
        typed.insert(style_id);
        if let Some(overridden) = overridden_style(style) {
            typed.insert(overridden);
        }
        typed.extend(
            color_cache
                .keys()
                .filter(|(id, _)| !invalid_surface_sides.contains(id))
                .map(|(id, _)| *id),
        );
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
                if style_inherits_from(binding_style_id, style_id, exchange, &mut BTreeSet::new()) {
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
        let mut colors = Vec::<Color>::new();
        for (_, color) in &candidates {
            let Some(existing) = colors.iter_mut().find(|existing| {
                existing.r() == color.r() && existing.g() == color.g() && existing.b() == color.b()
            }) else {
                colors.push(*color);
                continue;
            };
            if color.a() < existing.a() {
                *existing = *color;
            }
        }
        if let [color] = colors.as_slice() {
            match target {
                AppearanceTarget::Face(face) => {
                    if let Some(&index) = face_indices.get(face.as_str()) {
                        ir.model.faces[index].color = Some(*color);
                    }
                }
                AppearanceTarget::Body(body) => {
                    if let Some(&index) = body_indices.get(body.as_str()) {
                        ir.model.bodies[index].color = Some(*color);
                    }
                }
                _ => {}
            }
        } else {
            let style_ids = candidates
                .iter()
                .map(|(style_id, _)| format!("#{style_id}"))
                .collect::<Vec<_>>();
            losses.push(StepLossCode::ConflictingScalarColors.note(format!(
                    "independent styled items {} assign conflicting scalar colors to {:?}; scalar color omitted and appearance bindings retain every assignment",
                    style_ids.join(", "),
                    target,
                )));
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
) -> (Vec<BodyId>, bool) {
    let mut body_ids = BTreeSet::new();
    let mut active = BTreeSet::new();
    let supported = collect_invisible_body_ids(
        id,
        exchange,
        topology,
        body_indices,
        &mut active,
        &mut body_ids,
    );
    (body_ids.into_iter().collect(), supported)
}

fn collect_invisible_body_ids(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    body_indices: &BTreeMap<String, usize>,
    active: &mut BTreeSet<u64>,
    body_ids: &mut BTreeSet<BodyId>,
) -> bool {
    if !active.insert(id) {
        return false;
    }
    if let Some(ids) = topology.body_by_root.get(&id) {
        body_ids.extend(ids.iter().cloned());
        active.remove(&id);
        return !ids.is_empty();
    }
    let fallback = BodyId::from(ids::data(kind!("body"), id));
    if body_indices.contains_key(fallback.as_str()) {
        body_ids.insert(fallback);
        active.remove(&id);
        return true;
    }

    let Some(record) = exchange.records().get(&id) else {
        active.remove(&id);
        return false;
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
        if let Some(reference) = styled_item_parts(record)
            .and_then(|parts| parts.target.reference())
        {
            found_reference = true;
            supported &= collect_invisible_body_ids(
                reference,
                exchange,
                topology,
                body_indices,
                active,
                body_ids,
            );
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
                    active,
                    body_ids,
                );
            }
        }
    }
    active.remove(&id);
    found_reference && supported
}

fn expand_style_targets(
    id: u64,
    exchange: &Exchange,
    typed: &mut HashSet<u64>,
    active: &mut BTreeSet<u64>,
    depth: usize,
    graph_limit: usize,
) -> Vec<u64> {
    if depth >= graph_limit || !active.insert(id) {
        return Vec::new();
    }
    let Some(record) = exchange.records().get(&id) else {
        active.remove(&id);
        return vec![id];
    };
    let Some(set_name) = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
        )
        .then_some(partial.name.as_str())
    }) else {
        active.remove(&id);
        return vec![id];
    };
    typed.insert(id);
    let targets = named_parameter(record, set_name, 1)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(ValueExt::reference)
        .flat_map(|item| {
            expand_style_targets(item, exchange, typed, active, depth + 1, graph_limit)
        })
        .collect();
    active.remove(&id);
    targets
}

fn appearance_targets(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    entity_ids: &EntityIds<'_>,
    face_indices: &BTreeMap<String, usize>,
    body_indices: &BTreeMap<String, usize>,
) -> Vec<AppearanceTarget> {
    if let Some(bodies) = topology.body_by_root.get(&id) {
        return bodies
            .iter()
            .filter(|body| body_indices.contains_key(body.as_str()))
            .cloned()
            .map(AppearanceTarget::Body)
            .collect();
    }
    if let Some(faces) = topology.faces_by_source.get(&id) {
        return faces
            .iter()
            .filter(|face| face_indices.contains_key(face.as_str()))
            .cloned()
            .map(AppearanceTarget::Face)
            .collect();
    }
    if let Some(edges) = topology.edges_by_source.get(&id) {
        return edges
            .iter()
            .filter(|edge| entity_ids.edges.contains(edge.as_str()))
            .cloned()
            .map(AppearanceTarget::Edge)
            .collect();
    }
    if let Some(vertices) = topology.vertices_by_source.get(&id) {
        return vertices
            .iter()
            .filter(|vertex| entity_ids.vertices.contains(vertex.as_str()))
            .cloned()
            .map(AppearanceTarget::Vertex)
            .collect();
    }
    let face_id = ids::data(kind!("face"), id);
    let body_id = ids::data(kind!("body"), id);
    let edge_id = ids::data(kind!("edge"), id);
    let surface_id = ids::data(kind!("surface"), id);
    let curve_id = ids::data(kind!("curve"), id);
    let point_id = ids::data(kind!("point"), id);
    let tessellation_id = ids::tessellation(kind!("mesh"), id);
    if face_indices.contains_key(face_id.as_str()) {
        return vec![AppearanceTarget::Face(FaceId::from(face_id))];
    }
    if body_indices.contains_key(body_id.as_str()) {
        return vec![AppearanceTarget::Body(BodyId::from(body_id))];
    }
    if entity_ids.edges.contains(edge_id.as_str()) {
        return vec![AppearanceTarget::Edge(EdgeId::from(edge_id))];
    }
    if entity_ids.surfaces.contains(surface_id.as_str()) {
        return vec![AppearanceTarget::Surface(SurfaceId::from(surface_id))];
    }
    if entity_ids.curves.contains(curve_id.as_str()) {
        return vec![AppearanceTarget::Curve(CurveId::from(curve_id))];
    }
    if entity_ids.points.contains(point_id.as_str()) {
        return vec![AppearanceTarget::Point(PointId::from(point_id))];
    }
    if entity_ids.tessellations.contains(tessellation_id.as_str()) {
        return vec![AppearanceTarget::Tessellation(
            tessellation_id.into_string(),
        )];
    }
    if exchange.records().contains_key(&id) {
        return vec![AppearanceTarget::Source {
            source_id: format!("#{id}"),
        }];
    }
    Vec::new()
}

fn presentation_item(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyData,
    entity_ids: &EntityIds<'_>,
    face_indices: &BTreeMap<String, usize>,
    body_indices: &BTreeMap<String, usize>,
) -> Vec<PresentationItem> {
    if let Some(bodies) = topology.body_by_root.get(&id) {
        return bodies
            .iter()
            .filter(|body| body_indices.contains_key(body.as_str()))
            .cloned()
            .map(|body| PresentationItem::Body { body })
            .collect();
    }
    if let Some(faces) = topology.faces_by_source.get(&id) {
        return faces
            .iter()
            .filter(|face| face_indices.contains_key(face.as_str()))
            .cloned()
            .map(|face| PresentationItem::Face { face })
            .collect();
    }
    if let Some(edges) = topology.edges_by_source.get(&id) {
        return edges
            .iter()
            .filter(|edge| entity_ids.edges.contains(edge.as_str()))
            .cloned()
            .map(|edge| PresentationItem::Edge { edge })
            .collect();
    }
    if let Some(vertices) = topology.vertices_by_source.get(&id) {
        return vertices
            .iter()
            .filter(|vertex| entity_ids.vertices.contains(vertex.as_str()))
            .cloned()
            .map(|vertex| PresentationItem::Vertex { vertex })
            .collect();
    }
    if let Some(products) = entity_ids.products.get(&id) {
        return products
            .iter()
            .cloned()
            .map(|product| PresentationItem::Product { product })
            .collect();
    }
    vec![presentation_item_one(
        id,
        exchange,
        entity_ids,
        face_indices,
        body_indices,
    )]
}

fn presentation_item_one(
    id: u64,
    exchange: &Exchange,
    entity_ids: &EntityIds<'_>,
    face_indices: &BTreeMap<String, usize>,
    body_indices: &BTreeMap<String, usize>,
) -> PresentationItem {
    let candidate = |kind: &crate::ids::IdentityKind| ids::data(kind, id);
    let body = candidate(kind!("body"));
    if body_indices.contains_key(body.as_str()) {
        return PresentationItem::Body {
            body: BodyId::from(body),
        };
    }
    let face = candidate(kind!("face"));
    if face_indices.contains_key(face.as_str()) {
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

fn collect_borrowed_identity_set<'a>(
    identities: impl IntoIterator<Item = &'a str>,
    ctx: Option<&DecodeContext<'_>>,
    operation: &'static str,
) -> Result<BTreeSet<&'a str>, CodecError> {
    let mut result = BTreeSet::new();
    for identity in identities {
        if !result.contains(identity) {
            if let Some(ctx) = ctx {
                ctx.charge_collection_items(1, operation)?;
            }
            result.insert(identity);
        }
    }
    Ok(result)
}

fn insert_presentation_set<T: Ord>(
    values: &mut BTreeSet<T>,
    value: T,
    ctx: Option<&DecodeContext<'_>>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if !values.contains(&value) {
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
        }
        values.insert(value);
    }
    Ok(())
}

fn push_presentation_vec<T>(
    values: &mut Vec<T>,
    value: T,
    ctx: Option<&DecodeContext<'_>>,
    operation: &'static str,
) -> Result<(), CodecError> {
    if let Some(ctx) = ctx {
        ctx.charge_collection_items(1, operation)?;
    }
    values.try_reserve(1).map_err(|_| match ctx {
        Some(ctx) => ctx.refuse_codec_limit(operation, 0, 1),
        None => cadmpeg_core::decode::refuse_local_limit(operation, 0, 1),
    })?;
    values.push(value);
    Ok(())
}

fn collect_identity_indices<'a>(
    identities: impl IntoIterator<Item = &'a str>,
    ctx: Option<&DecodeContext<'_>>,
    operation: &'static str,
) -> Result<BTreeMap<String, usize>, CodecError> {
    let mut result = BTreeMap::new();
    for (index, identity) in identities.into_iter().enumerate() {
        if let Some(existing) = result.get_mut(identity) {
            *existing = index;
            continue;
        }
        if let Some(ctx) = ctx {
            ctx.charge_collection_items(1, operation)?;
            ctx.charge_retained(cadmpeg_core::decode::u64_from_index(identity.len()), operation)?;
        }
        let mut copy = String::new();
        copy.try_reserve_exact(identity.len()).map_err(|_| match ctx {
            Some(ctx) => ctx.refuse_codec_limit(operation, 0, 1),
            None => cadmpeg_core::decode::refuse_local_limit(operation, 0, 1),
        })?;
        copy.push_str(identity);
        result.insert(copy, index);
    }
    Ok(result)
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

pub(super) fn styled_item_target(record: &RawRecord) -> Option<u64> {
    styled_item_parts(record).and_then(|parts| parts.target.reference())
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
) -> (bool, Option<u32>) {
    let depth = style_depth(id, exchange, &mut BTreeSet::new(), 0, graph_limit);
    (depth.is_none(), depth)
}

fn style_depth(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
    graph_limit: usize,
) -> Option<u32> {
    if depth >= graph_limit || !active.insert(id) {
        return None;
    }
    let result = (|| {
        let style = exchange.records().get(&id)?;
        if let Some(base) = overridden_style(style) {
            style_depth(base, exchange, active, depth + 1, graph_limit)?.checked_add(1)
        } else {
            Some(0)
        }
    })();
    active.remove(&id);
    result
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

#[allow(clippy::too_many_arguments)] // Recursive search keeps cache, loss, and invalid-source tracking separate.
fn find_color(
    id: u64,
    exchange: &Exchange,
    domain: StyleDomain,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<(u64, StyleDomain), CachedColor>,
    losses: &mut Vec<LossNote>,
    invalid_surface_sides: &mut BTreeSet<u64>,
    depth: usize,
    ctx: Option<&DecodeContext<'_>>,
) -> Result<CachedColor, CodecError> {
    if depth >= 256 {
        return Ok(None);
    }
    if let Some(result) = cache.get(&(id, domain)) {
        return Ok(result.clone());
    }
    let Some(record) = exchange.records().get(&id) else {
        return Ok(None);
    };
    if is_presentation_style_by_context(record) {
        return Ok(None);
    }
    if !active.insert(id) {
        return Ok(None);
    }
    let transparency = (domain == StyleDomain::Surface)
        .then(|| surface_transparency(id, record, exchange, losses))
        .flatten();
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
                .flat_map(references)
            {
                // The recursive search caches the colour and records its losses.
                find_color(
                    reference,
                    exchange,
                    domain,
                    active,
                    cache,
                    losses,
                    invalid_surface_sides,
                    depth + 1,
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
                    .find(|partial| partial.name == "COLOUR_RGB") else {
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
                let Some(color) = Color::new(r as f32, g as f32, b as f32, 1.0) else {
                    return Ok(None);
                };
                let name = name_value
                    .map(|value| {
                        decode_text_charged(
                            exchange,
                            value,
                            losses,
                            id,
                            "colour name",
                            StepLossCode::AttributeStringInvalid,
                            ctx,
                        )
                    })
                    .transpose()?
                    .flatten();
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
                let Some(name) = decode_text_charged(
                    exchange,
                    name_value,
                    losses,
                    id,
                    "predefined colour name",
                    StepLossCode::AttributeStringInvalid,
                    ctx,
                )? else {
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
                    .flat_map(references)
                    .map(|reference| {
                        find_color(
                            reference,
                            exchange,
                            domain,
                            active,
                            cache,
                            losses,
                            invalid_surface_sides,
                            depth + 1,
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
                candidate.color = candidate
                    .color
                    .with_alpha((1.0 - transparency.get()) as f32)
                    .unwrap_or(candidate.color);
            }
            Some(ColorResolution::Ambiguous { .. }) => {}
            None => {}
        }
    }
    cache.insert((id, domain), result.clone());
    Ok(result)
}

fn surface_transparency(
    id: u64,
    record: &RawRecord,
    exchange: &Exchange,
    losses: &mut Vec<LossNote>,
) -> Option<Fraction> {
    let candidates = record
        .partials
        .iter()
        .filter(|partial| partial.name == "SURFACE_STYLE_RENDERING_WITH_PROPERTIES")
        .flat_map(|partial| partial.parameters.iter().flat_map(references))
        .filter_map(|property_id| {
            let property = exchange.records().get(&property_id)?;
            let transparency = property
                .partials
                .iter()
                .find(|partial| partial.name == "SURFACE_STYLE_TRANSPARENT")
                .and_then(|partial| partial.parameters.first())
                .and_then(ValueExt::number)?;
            Fraction::new(transparency).map(|transparency| (property_id, transparency))
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [] => None,
        [(_, transparency)] => Some(*transparency),
        _ => {
            let details = candidates
                .iter()
                .map(|(property_id, transparency)| format!("#{property_id}={}", transparency.get()))
                .collect::<Vec<_>>()
                .join(", ");
            losses.push(StepLossCode::SurfaceTransparencyConflict.note(format!(
                "surface style rendering #{id} has conflicting transparency properties ({details}); transparency omitted"
            )));
            None
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
            losses.push(StepLossCode::SurfaceSideInvalid.note(format!(
                "SURFACE_STYLE_USAGE #{id} has invalid surface_side .{side}.; style omitted"
            )));
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

fn style_domain(id: u64, exchange: &Exchange) -> StyleDomain {
    style_domain_at(id, exchange, &mut BTreeSet::new())
}

fn style_domain_at(id: u64, exchange: &Exchange, active: &mut BTreeSet<u64>) -> StyleDomain {
    if !active.insert(id) {
        return StyleDomain::Any;
    }
    let Some(record) = exchange.records().get(&id) else {
        active.remove(&id);
        return StyleDomain::Any;
    };
    let set_name = record.partials.iter().find_map(|partial| {
        matches!(
            partial.name.as_str(),
            "GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET"
        )
        .then_some(partial.name.as_str())
    });
    if let Some(set_name) = set_name {
        let member_domains = named_parameter(record, set_name, 1)
            .and_then(ValueExt::list)
            .into_iter()
            .flatten()
            .filter_map(ValueExt::reference)
            .map(|member| style_domain_at(member, exchange, active))
            .collect::<Vec<_>>();
        if !member_domains.is_empty() {
            let first = member_domains[0];
            if member_domains.iter().all(|domain| *domain == first) {
                active.remove(&id);
                return first;
            }
            if member_domains.iter().any(|domain| {
                matches!(
                    domain,
                    StyleDomain::Surface | StyleDomain::Curve | StyleDomain::Point
                )
            }) {
                active.remove(&id);
                return StyleDomain::Any;
            }
        }
    }
    let has_point = record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.contains("POINT") || name.contains("VERTEX")
    });
    if has_point {
        active.remove(&id);
        return StyleDomain::Point;
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
        active.remove(&id);
        return StyleDomain::Curve;
    }
    let result = if record.partials.iter().any(|partial| {
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
    };
    active.remove(&id);
    result
}

fn style_is_hidden(
    id: u64,
    hidden_style_ids: &BTreeSet<u64>,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
) -> bool {
    if hidden_style_ids.contains(&id) || !active.insert(id) {
        return hidden_style_ids.contains(&id);
    }
    let hidden = exchange
        .records()
        .get(&id)
        .and_then(overridden_style)
        .is_some_and(|base| style_is_hidden(base, hidden_style_ids, exchange, active));
    active.remove(&id);
    hidden
}

fn style_inherits_from(
    id: u64,
    ancestor: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
) -> bool {
    if id == ancestor || !active.insert(id) {
        return id == ancestor;
    }
    let inherits = exchange
        .records()
        .get(&id)
        .and_then(overridden_style)
        .is_some_and(|base| style_inherits_from(base, ancestor, exchange, active));
    active.remove(&id);
    inherits
}

fn contains_null_style(
    value: &Value,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
    depth: usize,
) -> bool {
    if depth >= 256 {
        return false;
    }
    match value {
        Value::Typed(name, _) if name == "NULL_STYLE" => true,
        Value::Typed(_, value) => contains_null_style(value, exchange, visited, depth + 1),
        Value::List(values) => values
            .iter()
            .any(|value| contains_null_style(value, exchange, visited, depth + 1)),
        Value::Reference(id) if visited.insert(*id) => {
            exchange.records().get(id).is_some_and(|r| {
                r.partials
                    .iter()
                    .flat_map(|partial| partial.parameters.iter())
                    .any(|value| contains_null_style(value, exchange, visited, depth + 1))
            })
        }
        _ => false,
    }
}

fn predefined(name: &str) -> Option<Color> {
    let (r, g, b) = match name.to_ascii_lowercase().as_str() {
        "black" => (0.0, 0.0, 0.0),
        "white" => (1.0, 1.0, 1.0),
        "red" => (1.0, 0.0, 0.0),
        "green" => (0.0, 1.0, 0.0),
        "blue" => (0.0, 0.0, 1.0),
        "yellow" => (1.0, 1.0, 0.0),
        "magenta" => (1.0, 0.0, 1.0),
        "cyan" => (0.0, 1.0, 1.0),
        _ => return None,
    };
    Color::new(r, g, b, 1.0)
}
#[cfg(test)]
pub(crate) mod tests;
