// SPDX-License-Identifier: Apache-2.0
//! STEP presentation style and topology color decoding.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    AppearanceId, BodyId, CurveId, EdgeId, FaceId, LayerId, OccurrenceId, PmiId, PointId,
    ProductDefinitionId, SurfaceId, VertexId,
};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::Color;

use crate::ids::StepIdentity;
use crate::loss::StepLossCode;
use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text;
use super::topology::TopologyData;
use super::StageOutcome;

pub(super) fn decode(
    exchange: &Exchange,
    topology: &TopologyData,
    ir: &mut CadIr,
    product_definition_ids_by_source: &BTreeMap<u64, Vec<ProductDefinitionId>>,
    ctx: Option<&DecodeContext<'_>>,
) -> StageOutcome<()> {
    let mut typed = HashSet::new();
    let mut warnings = Vec::new();
    let mut losses = Vec::new();
    let graph_limit = super::record_graph_limit(ctx);
    let face_indices = ir
        .model
        .faces
        .iter()
        .enumerate()
        .map(|(index, face)| (face.id.0.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let body_indices = ir
        .model
        .bodies
        .iter()
        .enumerate()
        .map(|(index, body)| (body.id.0.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let entity_ids = EntityIds {
        edges: ir
            .model
            .edges
            .iter()
            .map(|item| item.id.0.clone())
            .collect(),
        vertices: ir
            .model
            .vertices
            .iter()
            .map(|item| item.id.0.clone())
            .collect(),
        points: ir
            .model
            .points
            .iter()
            .map(|item| item.id.0.clone())
            .collect(),
        curves: ir
            .model
            .curves
            .iter()
            .map(|item| item.id.0.clone())
            .collect(),
        surfaces: ir
            .model
            .surfaces
            .iter()
            .map(|item| item.id.0.clone())
            .collect(),
        products: product_definition_ids_by_source.clone(),
        occurrences: ir
            .model
            .occurrences
            .iter()
            .map(|item| item.id.0.clone())
            .collect(),
        pmi: ir.model.pmi.iter().map(|item| item.id.0.clone()).collect(),
        tessellations: ir
            .model
            .tessellations
            .iter()
            .map(|item| item.id.clone())
            .collect(),
    };
    let mut appearance_ids = BTreeMap::<u64, AppearanceId>::new();
    let mut hidden_style_ids = BTreeSet::new();
    let mut hidden_layer_ids = BTreeSet::new();
    let mut deferred_invisibility = BTreeMap::<u64, (bool, BTreeSet<u64>, BTreeSet<u64>)>::new();
    for (&id, record) in &exchange.records {
        if !has_partial(record, "INVISIBILITY") {
            continue;
        }
        let Some(items) = partial_parameter(record, "INVISIBILITY", 0).and_then(ValueExt::list)
        else {
            warnings.push(format!("INVISIBILITY #{id} has no item set"));
            continue;
        };
        let mut supported = true;
        let mut style_targets = BTreeSet::new();
        let mut layer_targets = BTreeSet::new();
        for target in items.iter().filter_map(ValueExt::reference) {
            if exchange
                .records
                .get(&target)
                .is_some_and(|record| has_partial(record, "PRESENTATION_LAYER_ASSIGNMENT"))
            {
                hidden_layer_ids.insert(target);
                layer_targets.insert(target);
                continue;
            }
            if exchange
                .records
                .get(&target)
                .is_some_and(|record| styled_item_parts(record).is_some())
            {
                hidden_style_ids.insert(target);
                style_targets.insert(target);
                continue;
            }
            if exchange
                .records
                .get(&target)
                .is_some_and(super::drawing::is_supported_invisibility_target)
            {
                continue;
            }
            let (body_ids, target_supported) =
                invisible_body_ids(target, exchange, topology, &body_indices);
            let mut hidden = false;
            for body_id in body_ids {
                if let Some(index) = body_indices.get(&body_id.0) {
                    ir.model.bodies[*index].visible = Some(false);
                    hidden = true;
                }
            }
            if !target_supported || !hidden {
                warnings.push(format!(
                    "INVISIBILITY #{id} targets unsupported item #{target}"
                ));
                supported = false;
            }
        }
        if style_targets.is_empty() && layer_targets.is_empty() && supported {
            typed.insert(id);
        } else if !style_targets.is_empty() || !layer_targets.is_empty() {
            deferred_invisibility.insert(id, (supported, style_targets, layer_targets));
        }
    }
    for (&layer_id, layer) in &exchange.records {
        if !has_partial(layer, "PRESENTATION_LAYER_ASSIGNMENT") {
            continue;
        }
        let Some(assigned_items) =
            partial_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 2).and_then(ValueExt::list)
        else {
            warnings.push(format!(
                "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no assigned item set"
            ));
            continue;
        };
        if assigned_items.is_empty() {
            warnings.push(format!(
                "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has an empty assigned item set"
            ));
            continue;
        }
        let Some(name) =
            partial_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 0).and_then(|value| {
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
            warnings.push(format!(
                "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has no name"
            ));
            continue;
        };
        if name.is_empty() {
            warnings.push(format!(
                "PRESENTATION_LAYER_ASSIGNMENT #{layer_id} has an empty name"
            ));
            continue;
        }
        let description = partial_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 1)
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
        let items = assigned_items
            .iter()
            .filter_map(ValueExt::reference)
            .flat_map(|id| {
                presentation_item(
                    id,
                    exchange,
                    topology,
                    &entity_ids,
                    &face_indices,
                    &body_indices,
                )
            })
            .collect();
        ir.model.presentation_layers.push(PresentationLayer {
            id: LayerId(StepIdentity::presentation("layer", layer_id)),
            name,
            description,
            visible: hidden_layer_ids.contains(&layer_id).then_some(false),
            items,
        });
        typed.insert(layer_id);
    }
    let mut styles = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| styled_item_parts(record).map(|_| id))
        .collect::<Vec<_>>();
    let context_items = presentation_context_items(exchange);
    let overridden_styles = styles
        .iter()
        .filter_map(|id| overridden_style(&exchange.records[id]))
        .collect::<BTreeSet<_>>();
    styles.sort_by_key(|id| {
        style_depth(*id, exchange, &mut BTreeSet::new(), 0, graph_limit).unwrap_or(u32::MAX)
    });
    let mut scalar_color_candidates = HashMap::<AppearanceTarget, Vec<(u64, Color)>>::new();
    for style_id in styles {
        if overridden_styles.contains(&style_id) {
            typed.insert(style_id);
            continue;
        }
        let style = &exchange.records[&style_id];
        let Some(parts) = styled_item_parts(style) else {
            continue;
        };
        let Some(target_step) = parts.target.reference() else {
            warnings.push(format!("STYLED_ITEM #{style_id} has no resolved target"));
            continue;
        };
        if parts.styles.list().is_some_and(<[Value]>::is_empty) {
            typed.insert(style_id);
            continue;
        }
        let domain = style_domain(target_step, exchange);
        let mut active = BTreeSet::new();
        let mut color_cache = BTreeMap::new();
        let style_references = parts
            .styles
            .list()
            .into_iter()
            .flatten()
            .flat_map(references)
            .collect::<Vec<_>>();
        let context_style_ids = style_references
            .iter()
            .copied()
            .filter(|reference| {
                exchange
                    .records
                    .get(reference)
                    .is_some_and(is_presentation_style_by_context)
            })
            .collect::<BTreeSet<_>>();
        let mut effective_style_references = Vec::new();
        let mut unresolved_context_style_ids = BTreeSet::new();
        for reference in style_references.iter().copied() {
            let Some(record) = exchange.records.get(&reference) else {
                effective_style_references.push(reference);
                continue;
            };
            if !context_style_ids.contains(&reference) {
                effective_style_references.push(reference);
                continue;
            }
            let Some(context) = presentation_style_context(record).and_then(ValueExt::reference)
            else {
                unresolved_context_style_ids.insert(reference);
                continue;
            };
            if context_items
                .get(&context)
                .is_some_and(|items| items.contains(&target_step))
            {
                let assignments = presentation_style_assignments(record);
                if assignments.is_empty() {
                    unresolved_context_style_ids.insert(reference);
                } else {
                    effective_style_references.extend(assignments);
                    typed.insert(reference);
                }
            } else {
                unresolved_context_style_ids.insert(reference);
            }
        }
        if !unresolved_context_style_ids.is_empty() {
            let contexts = unresolved_context_style_ids
                .iter()
                .map(|context_style_id| {
                    let context = exchange
                        .records
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
        }
        let color = effective_style_references
            .iter()
            .copied()
            .find_map(|reference| {
                find_color(
                    reference,
                    exchange,
                    domain,
                    &mut active,
                    &mut color_cache,
                    &mut losses,
                    0,
                )
            });
        let color = color.or_else(|| {
            matches!(domain, StyleDomain::Curve | StyleDomain::Point).then(|| {
                effective_style_references
                    .iter()
                    .copied()
                    .find_map(|reference| {
                        find_color(
                            reference,
                            exchange,
                            StyleDomain::Surface,
                            &mut active,
                            &mut color_cache,
                            &mut losses,
                            0,
                        )
                    })
            })?
        });
        let Some((_, color_id, color, name)) = color else {
            let mut visited = BTreeSet::new();
            if unresolved_context_style_ids.is_empty()
                && !contains_null_style(parts.styles, exchange, &mut visited, 0)
            {
                warnings.push(format!(
                    "STYLED_ITEM #{style_id} has no resolved surface color"
                ));
            }
            continue;
        };
        let appearance_id = appearance_ids
            .entry(color_id)
            .or_insert_with(|| {
                let id = AppearanceId(StepIdentity::presentation("appearance", color_id));
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
                warnings.push(format!(
                    "STYLED_ITEM #{style_id} targets unsupported item #{target_step}"
                ));
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
                    id: StepIdentity::presentation(
                        "binding",
                        format!("{style_id}:{ordinal}-{target_ordinal}"),
                    ),
                    target,
                    appearance: appearance_id.clone(),
                    source_entity_id: Some(format!("#{style_id}")),
                    object_type: None,
                    visible: hidden_style_ids.contains(&style_id).then_some(false),
                    channels: BTreeMap::new(),
                });
            }
        }
        typed.insert(style_id);
        if let Some(overridden) = overridden_style(style) {
            typed.insert(overridden);
        }
        typed.extend(color_cache.keys().map(|(id, _)| *id));
        typed.insert(color_id);
    }
    for (invisibility_id, (mut supported, style_targets, layer_targets)) in deferred_invisibility {
        for style_id in style_targets {
            let source_id = format!("#{style_id}");
            let mut matched = false;
            for binding in &mut ir.model.appearance_bindings {
                if binding.source_entity_id.as_deref() == Some(source_id.as_str()) {
                    binding.visible = Some(false);
                    matched = true;
                }
            }
            if !matched {
                warnings.push(format!(
                    "INVISIBILITY #{invisibility_id} targets unsupported item #{style_id}"
                ));
                supported = false;
            }
        }
        for layer_id in layer_targets {
            let expected_id = StepIdentity::presentation("layer", layer_id);
            let mut matched = false;
            for layer in &mut ir.model.presentation_layers {
                if layer.id.0 == expected_id {
                    layer.visible = Some(false);
                    matched = true;
                    break;
                }
            }
            if !matched {
                warnings.push(format!(
                    "INVISIBILITY #{invisibility_id} targets unsupported item #{layer_id}"
                ));
                supported = false;
            }
        }
        if supported {
            typed.insert(invisibility_id);
        }
    }
    for (target, candidates) in scalar_color_candidates {
        let mut colors = Vec::new();
        for (_, color) in &candidates {
            if !colors.contains(color) {
                colors.push(*color);
            }
        }
        if let [color] = colors.as_slice() {
            match target {
                AppearanceTarget::Face(face) => {
                    if let Some(&index) = face_indices.get(&face.0) {
                        ir.model.faces[index].color = Some(*color);
                    }
                }
                AppearanceTarget::Body(body) => {
                    if let Some(&index) = body_indices.get(&body.0) {
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
    StageOutcome {
        value: (),
        claims: typed,
        warnings,
        losses,
        notes: Vec::new(),
    }
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
    let fallback = BodyId(StepIdentity::data("body", id));
    if body_indices.contains_key(&fallback.0) {
        body_ids.insert(fallback);
        active.remove(&id);
        return true;
    }

    let Some(record) = exchange.records.get(&id) else {
        active.remove(&id);
        return false;
    };
    let references = if record
        .partials
        .iter()
        .any(|partial| partial.name == "STYLED_ITEM")
        || record
            .partials
            .iter()
            .any(|partial| partial.name == "OVER_RIDING_STYLED_ITEM")
    {
        styled_item_parts(record)
            .and_then(|parts| parts.target.reference())
            .into_iter()
            .collect::<Vec<_>>()
    } else if record
        .partials
        .iter()
        .any(|partial| super::representation::is_representation_name(&partial.name))
    {
        super::representation::items(record).unwrap_or_default()
    } else {
        Vec::new()
    };
    if references.is_empty() {
        active.remove(&id);
        return false;
    }
    let mut supported = true;
    for reference in references {
        supported &= collect_invisible_body_ids(
            reference,
            exchange,
            topology,
            body_indices,
            active,
            body_ids,
        );
    }
    active.remove(&id);
    supported
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
    let Some(record) = exchange.records.get(&id) else {
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
    let targets = partial_parameter(record, set_name, 1)
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
    entity_ids: &EntityIds,
    face_indices: &BTreeMap<String, usize>,
    body_indices: &BTreeMap<String, usize>,
) -> Vec<AppearanceTarget> {
    if let Some(bodies) = topology.body_by_root.get(&id) {
        return bodies
            .iter()
            .filter(|body| body_indices.contains_key(body.0.as_str()))
            .cloned()
            .map(AppearanceTarget::Body)
            .collect();
    }
    if let Some(faces) = topology.faces_by_source.get(&id) {
        return faces
            .iter()
            .filter(|face| face_indices.contains_key(face.0.as_str()))
            .cloned()
            .map(AppearanceTarget::Face)
            .collect();
    }
    if let Some(edges) = topology.edges_by_source.get(&id) {
        return edges
            .iter()
            .filter(|edge| entity_ids.edges.contains(edge.0.as_str()))
            .cloned()
            .map(AppearanceTarget::Edge)
            .collect();
    }
    if let Some(vertices) = topology.vertices_by_source.get(&id) {
        return vertices
            .iter()
            .filter(|vertex| entity_ids.vertices.contains(vertex.0.as_str()))
            .cloned()
            .map(AppearanceTarget::Vertex)
            .collect();
    }
    let face_id = StepIdentity::data("face", id);
    let body_id = StepIdentity::data("body", id);
    let edge_id = StepIdentity::data("edge", id);
    let surface_id = StepIdentity::data("surface", id);
    let curve_id = StepIdentity::data("curve", id);
    let point_id = StepIdentity::data("point", id);
    let tessellation_id = StepIdentity::tessellation("mesh", id);
    if face_indices.contains_key(&face_id) {
        return vec![AppearanceTarget::Face(FaceId(face_id))];
    }
    if body_indices.contains_key(&body_id) {
        return vec![AppearanceTarget::Body(BodyId(body_id))];
    }
    if entity_ids.edges.contains(&edge_id) {
        return vec![AppearanceTarget::Edge(EdgeId(edge_id))];
    }
    if entity_ids.surfaces.contains(&surface_id) {
        return vec![AppearanceTarget::Surface(SurfaceId(surface_id))];
    }
    if entity_ids.curves.contains(&curve_id) {
        return vec![AppearanceTarget::Curve(CurveId(curve_id))];
    }
    if entity_ids.points.contains(&point_id) {
        return vec![AppearanceTarget::Point(PointId(point_id))];
    }
    if entity_ids.tessellations.contains(&tessellation_id) {
        return vec![AppearanceTarget::Tessellation(tessellation_id)];
    }
    if exchange.records.contains_key(&id) {
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
    entity_ids: &EntityIds,
    face_indices: &BTreeMap<String, usize>,
    body_indices: &BTreeMap<String, usize>,
) -> Vec<PresentationItem> {
    if let Some(bodies) = topology.body_by_root.get(&id) {
        return bodies
            .iter()
            .filter(|body| body_indices.contains_key(body.0.as_str()))
            .cloned()
            .map(|body| PresentationItem::Body { body })
            .collect();
    }
    if let Some(faces) = topology.faces_by_source.get(&id) {
        return faces
            .iter()
            .filter(|face| face_indices.contains_key(face.0.as_str()))
            .cloned()
            .map(|face| PresentationItem::Face { face })
            .collect();
    }
    if let Some(edges) = topology.edges_by_source.get(&id) {
        return edges
            .iter()
            .filter(|edge| entity_ids.edges.contains(edge.0.as_str()))
            .cloned()
            .map(|edge| PresentationItem::Edge { edge })
            .collect();
    }
    if let Some(vertices) = topology.vertices_by_source.get(&id) {
        return vertices
            .iter()
            .filter(|vertex| entity_ids.vertices.contains(vertex.0.as_str()))
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
    entity_ids: &EntityIds,
    face_indices: &BTreeMap<String, usize>,
    body_indices: &BTreeMap<String, usize>,
) -> PresentationItem {
    let candidate = |kind: &str| StepIdentity::data(kind, id);
    let body = candidate("body");
    if body_indices.contains_key(&body) {
        return PresentationItem::Body { body: BodyId(body) };
    }
    let face = candidate("face");
    if face_indices.contains_key(&face) {
        return PresentationItem::Face { face: FaceId(face) };
    }
    let edge = candidate("edge");
    if entity_ids.edges.contains(&edge) {
        return PresentationItem::Edge { edge: EdgeId(edge) };
    }
    let vertex = candidate("vertex");
    if entity_ids.vertices.contains(&vertex) {
        return PresentationItem::Vertex {
            vertex: VertexId(vertex),
        };
    }
    let point = candidate("point");
    if entity_ids.points.contains(&point) {
        return PresentationItem::Point {
            point: PointId(point),
        };
    }
    let curve = candidate("curve");
    if entity_ids.curves.contains(&curve) {
        return PresentationItem::Curve {
            curve: CurveId(curve),
        };
    }
    let surface = candidate("surface");
    if entity_ids.surfaces.contains(&surface) {
        return PresentationItem::Surface {
            surface: SurfaceId(surface),
        };
    }
    let Some(record) = exchange.records.get(&id) else {
        return PresentationItem::Source {
            source_id: format!("#{id}"),
        };
    };
    let has = |name: &str| has_partial(record, name);
    if has("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        && entity_ids
            .occurrences
            .contains(&StepIdentity::product("occurrence", id))
    {
        PresentationItem::Occurrence {
            occurrence: OccurrenceId(StepIdentity::product("occurrence", id)),
        }
    } else if record.partials.iter().any(|partial| {
        (partial.name == "DATUM"
            || partial.name == "DATUM_SYSTEM"
            || partial.name.starts_with("DIMENSIONAL_")
            || partial.name.ends_with("_TOLERANCE")
            || super::pmi::is_presentation_annotation(&partial.name))
            && entity_ids
                .pmi
                .contains(&StepIdentity::presentation("pmi", id))
    }) {
        PresentationItem::Pmi {
            annotation: PmiId(StepIdentity::presentation("pmi", id)),
        }
    } else if (has("TRIANGULATED_FACE")
        || has("COMPLEX_TRIANGULATED_FACE")
        || has("TRIANGULATED_SURFACE_SET")
        || has("COMPLEX_TRIANGULATED_SURFACE_SET"))
        && entity_ids
            .tessellations
            .contains(&StepIdentity::tessellation("mesh", id))
    {
        PresentationItem::Tessellation {
            tessellation: StepIdentity::tessellation("mesh", id),
        }
    } else {
        PresentationItem::Source {
            source_id: format!("#{id}"),
        }
    }
}

struct EntityIds {
    edges: BTreeSet<String>,
    vertices: BTreeSet<String>,
    points: BTreeSet<String>,
    curves: BTreeSet<String>,
    surfaces: BTreeSet<String>,
    products: BTreeMap<u64, Vec<ProductDefinitionId>>,
    occurrences: BTreeSet<String>,
    pmi: BTreeSet<String>,
    tessellations: BTreeSet<String>,
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

fn presentation_style_assignments(record: &RawRecord) -> Vec<u64> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "PRESENTATION_STYLE_BY_CONTEXT")
        .and_then(|partial| partial.parameters.first())
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(ValueExt::reference)
        .collect()
}

fn presentation_context_items(exchange: &Exchange) -> BTreeMap<u64, BTreeSet<u64>> {
    let mut context_items = BTreeMap::<u64, BTreeSet<u64>>::new();
    for (&representation_id, record) in &exchange.records {
        let Some(items) = super::representation::items(record) else {
            continue;
        };
        context_items
            .entry(representation_id)
            .or_default()
            .extend(items.iter().copied());
        if let Some(context_id) = representation_context(record) {
            context_items.entry(context_id).or_default().extend(items);
        }
    }
    context_items
}

fn representation_context(record: &RawRecord) -> Option<u64> {
    super::representation::context(record)
}

pub(super) fn styled_item_target(record: &RawRecord) -> Option<u64> {
    styled_item_parts(record).and_then(|parts| parts.target.reference())
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
        let style = exchange.records.get(&id)?;
        if let Some(base) = overridden_style(style) {
            style_depth(base, exchange, active, depth + 1, graph_limit)?.checked_add(1)
        } else {
            Some(0)
        }
    })();
    active.remove(&id);
    result
}

type CachedColor = Option<(u8, u64, Color, Option<String>)>;

fn find_color(
    id: u64,
    exchange: &Exchange,
    domain: StyleDomain,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<(u64, StyleDomain), CachedColor>,
    losses: &mut Vec<LossNote>,
    depth: usize,
) -> CachedColor {
    if depth >= 256 {
        return None;
    }
    if let Some(result) = cache.get(&(id, domain)) {
        return result.clone();
    }
    let record = exchange.records.get(&id)?;
    if is_presentation_style_by_context(record) {
        return None;
    }
    if !active.insert(id) {
        return None;
    }
    let result = (|| {
        let side_rank = if domain == StyleDomain::Surface {
            surface_side_rank(record)
        } else {
            0
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
                let _ = find_color(
                    reference,
                    exchange,
                    domain,
                    active,
                    cache,
                    losses,
                    depth + 1,
                );
            }
            return None;
        }
        match name {
            Some("COLOUR_RGB") => {
                let rgb = record
                    .partials
                    .iter()
                    .find(|partial| partial.name == "COLOUR_RGB")?;
                let offset = usize::from(record.partials.len() == 1);
                let r = rgb.parameters.get(offset)?.number()?;
                let g = rgb.parameters.get(offset + 1)?.number()?;
                let b = rgb.parameters.get(offset + 2)?.number()?;
                if ![r, g, b]
                    .iter()
                    .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
                {
                    return None;
                }
                let name_value = if record.partials.len() == 1 {
                    rgb.parameters.first()
                } else {
                    record
                        .partials
                        .iter()
                        .find(|partial| partial.name == "COLOUR_SPECIFICATION")
                        .and_then(|partial| partial.parameters.first())
                };
                Some((
                    side_rank,
                    id,
                    Color {
                        r: r as f32,
                        g: g as f32,
                        b: b as f32,
                        a: 1.0,
                    },
                    name_value.and_then(|value| {
                        decode_text(
                            exchange,
                            value,
                            losses,
                            id,
                            "colour name",
                            StepLossCode::AttributeStringInvalid,
                        )
                    }),
                ))
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
                }?;
                let name = decode_text(
                    exchange,
                    name_value,
                    losses,
                    id,
                    "predefined colour name",
                    StepLossCode::AttributeStringInvalid,
                )?;
                predefined(&name).map(|color| (side_rank, id, color, Some(name)))
            }
            _ => {
                let mut best = None;
                for reference in record
                    .partials
                    .iter()
                    .flat_map(|partial| partial.parameters.iter())
                    .flat_map(references)
                {
                    let Some(mut candidate) = find_color(
                        reference,
                        exchange,
                        domain,
                        active,
                        cache,
                        losses,
                        depth + 1,
                    ) else {
                        continue;
                    };
                    candidate.0 = candidate.0.max(side_rank);
                    if best
                        .as_ref()
                        .is_none_or(|current: &(u8, u64, Color, Option<String>)| {
                            candidate.0 > current.0
                        })
                    {
                        best = Some(candidate);
                    }
                }
                best
            }
        }
    })();
    active.remove(&id);
    cache.insert((id, domain), result.clone());
    result
}

fn surface_side_rank(record: &RawRecord) -> u8 {
    record
        .partials
        .iter()
        .find(|partial| partial.name == "SURFACE_STYLE_USAGE")
        .and_then(|partial| partial.parameters.first())
        .and_then(ValueExt::enumeration)
        .map_or(0, |side| match side {
            "BOTH" => 3,
            "POSITIVE" => 2,
            "NEGATIVE" => 1,
            _ => 0,
        })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StyleDomain {
    Any,
    Surface,
    Curve,
    Point,
}

fn style_domain(id: u64, exchange: &Exchange) -> StyleDomain {
    let Some(record) = exchange.records.get(&id) else {
        return StyleDomain::Any;
    };
    let has_point = record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.contains("POINT") || name.contains("VERTEX")
    });
    if has_point {
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
        return StyleDomain::Curve;
    }
    if record.partials.iter().any(|partial| {
        let name = partial.name.as_str();
        name.contains("FACE")
            || name.contains("SURFACE")
            || name.contains("SOLID")
            || name.contains("SHELL")
    }) {
        StyleDomain::Surface
    } else {
        StyleDomain::Any
    }
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
        Value::Reference(id) if visited.insert(*id) => exchange.records.get(id).is_some_and(|r| {
            r.partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .any(|value| contains_null_style(value, exchange, visited, depth + 1))
        }),
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
    Some(Color { r, g, b, a: 1.0 })
}
fn references(value: &Value) -> Vec<u64> {
    match value {
        Value::Reference(id) => vec![*id],
        Value::List(values) => values.iter().flat_map(references).collect(),
        Value::Typed(_, value) => references(value),
        _ => Vec::new(),
    }
}

fn has_partial(record: &RawRecord, name: &str) -> bool {
    record.partials.iter().any(|partial| partial.name == name)
}

fn partial_parameter<'a>(record: &'a RawRecord, name: &str, index: usize) -> Option<&'a Value> {
    record
        .partials
        .iter()
        .find(|partial| partial.name == name)
        .and_then(|partial| partial.parameters.get(index))
}

trait RecordExt {
    fn simple_name(&self) -> Option<&str>;
    fn parameters(&self) -> &[Value];
    fn parameter(&self, index: usize) -> Option<&Value>;
}
impl RecordExt for RawRecord {
    fn simple_name(&self) -> Option<&str> {
        (self.partials.len() == 1).then(|| self.partials[0].name.as_str())
    }
    fn parameters(&self) -> &[Value] {
        self.partials
            .first()
            .map(|partial| partial.parameters.as_slice())
            .unwrap_or_default()
    }
    fn parameter(&self, index: usize) -> Option<&Value> {
        self.parameters().get(index)
    }
}
trait ValueExt {
    fn reference(&self) -> Option<u64>;
    fn number(&self) -> Option<f64>;
    fn list(&self) -> Option<&[Value]>;
    fn enumeration(&self) -> Option<&str>;
}
impl ValueExt for Value {
    fn reference(&self) -> Option<u64> {
        if let Value::Reference(id) = self {
            Some(*id)
        } else {
            None
        }
    }
    fn number(&self) -> Option<f64> {
        match self {
            Value::Real(value) => Some(*value),
            Value::Integer(value) => Some(*value as f64),
            _ => None,
        }
    }
    fn list(&self) -> Option<&[Value]> {
        if let Value::List(values) = self {
            Some(values)
        } else {
            None
        }
    }
    fn enumeration(&self) -> Option<&str> {
        if let Value::Enumeration(value) = self {
            Some(value)
        } else {
            None
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
