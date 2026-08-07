// SPDX-License-Identifier: Apache-2.0
//! STEP presentation style and topology color decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    AppearanceId, BodyId, CurveId, EdgeId, FaceId, LayerId, OccurrenceId, PmiId, PointId,
    ProductDefinitionId, SurfaceId, VertexId,
};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::report::{LossKind, LossNote};
use cadmpeg_ir::topology::Color;

use crate::parse::{Exchange, RawRecord, Value};

use super::decode_text;
use super::topology::TopologyResult;

pub(super) struct PresentationResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
    pub losses: Vec<LossNote>,
}

pub(super) fn decode(
    exchange: &Exchange,
    topology: &TopologyResult,
    ir: &mut CadIr,
) -> PresentationResult {
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
    let mut losses = Vec::new();
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
        products: ir
            .model
            .product_definitions
            .iter()
            .map(|item| item.id.0.clone())
            .collect(),
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
        for target in items.iter().filter_map(ValueExt::reference) {
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
        if supported {
            typed.insert(id);
        }
    }
    for (&layer_id, layer) in &exchange.records {
        if !has_partial(layer, "PRESENTATION_LAYER_ASSIGNMENT") {
            continue;
        }
        let Some(name) =
            partial_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 0).and_then(|value| {
                decode_text(
                    value,
                    &mut losses,
                    layer_id,
                    "presentation layer name",
                    LossKind::MetadataNotTransferred,
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
                    value,
                    &mut losses,
                    layer_id,
                    "presentation layer description",
                    LossKind::MetadataNotTransferred,
                )
            })
            .filter(|value| !value.is_empty());
        let items = partial_parameter(layer, "PRESENTATION_LAYER_ASSIGNMENT", 2)
            .and_then(ValueExt::list)
            .into_iter()
            .flatten()
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
            id: LayerId(format!("step:presentation:layer#{layer_id}")),
            name,
            description,
            items,
        });
        typed.insert(layer_id);
    }
    let mut styles = exchange
        .records
        .iter()
        .filter_map(|(&id, record)| styled_item_parts(record).map(|_| id))
        .collect::<Vec<_>>();
    let overridden_styles = styles
        .iter()
        .filter_map(|id| overridden_style(&exchange.records[id]))
        .collect::<BTreeSet<_>>();
    styles
        .sort_by_key(|id| style_depth(*id, exchange, &mut BTreeSet::new(), 0).unwrap_or(u32::MAX));
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
        let Some((color_id, color, name)) = parts
            .styles
            .list()
            .into_iter()
            .flatten()
            .flat_map(references)
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
            })
        else {
            let mut visited = BTreeSet::new();
            if !contains_null_style(parts.styles, exchange, &mut visited, 0) {
                warnings.push(format!(
                    "STYLED_ITEM #{style_id} has no resolved surface color"
                ));
            }
            continue;
        };
        let appearance_id = appearance_ids
            .entry(color_id)
            .or_insert_with(|| {
                let id = AppearanceId(format!("step:presentation:appearance#{color_id}"));
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
        let target_steps =
            expand_style_targets(target_step, exchange, &mut typed, &mut BTreeSet::new(), 0);
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
                    AppearanceTarget::Face(face) => {
                        if let Some(&index) = face_indices.get(&face.0) {
                            ir.model.faces[index].color = Some(color);
                        }
                    }
                    AppearanceTarget::Body(body) => {
                        if let Some(&index) = body_indices.get(&body.0) {
                            ir.model.bodies[index].color = Some(color);
                        }
                    }
                    _ => {}
                }
                ir.model.appearance_bindings.push(AppearanceBinding {
                    id: format!("step:presentation:binding#{style_id}:{ordinal}-{target_ordinal}"),
                    target,
                    appearance: appearance_id.clone(),
                    source_entity_id: Some(format!("#{style_id}")),
                    object_type: None,
                    channels: BTreeMap::new(),
                });
            }
        }
        typed.insert(style_id);
        if let Some(overridden) = overridden_style(style) {
            typed.insert(overridden);
        }
        typed.extend(color_cache.keys().copied());
        typed.insert(color_id);
    }
    PresentationResult {
        typed_records: typed,
        warnings,
        losses,
    }
}

fn invisible_body_ids(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyResult,
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
    topology: &TopologyResult,
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
    let fallback = BodyId(format!("step:data:body#{id}"));
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
        .any(|partial| partial.name == "PRESENTATION_LAYER_ASSIGNMENT")
    {
        partial_parameter(record, "PRESENTATION_LAYER_ASSIGNMENT", 2)
            .and_then(ValueExt::list)
            .into_iter()
            .flatten()
            .filter_map(ValueExt::reference)
            .collect::<Vec<_>>()
    } else if record.partials.iter().any(|partial| {
        partial.name == "REPRESENTATION" || partial.name == "PRESENTATION_REPRESENTATION"
    }) {
        record
            .partials
            .iter()
            .find(|partial| {
                partial.name == "REPRESENTATION" || partial.name == "PRESENTATION_REPRESENTATION"
            })
            .and_then(|partial| partial.parameters.get(1))
            .and_then(ValueExt::list)
            .into_iter()
            .flatten()
            .filter_map(ValueExt::reference)
            .collect::<Vec<_>>()
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
    typed: &mut BTreeSet<u64>,
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Vec<u64> {
    if depth >= super::MAX_RECORD_GRAPH_DEPTH || !active.insert(id) {
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
        .flat_map(|item| expand_style_targets(item, exchange, typed, active, depth + 1))
        .collect();
    active.remove(&id);
    targets
}

fn appearance_targets(
    id: u64,
    exchange: &Exchange,
    topology: &TopologyResult,
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
    let face_id = format!("step:data:face#{id}");
    let body_id = format!("step:data:body#{id}");
    let edge_id = format!("step:data:edge#{id}");
    let surface_id = format!("step:data:surface#{id}");
    let curve_id = format!("step:data:curve#{id}");
    let point_id = format!("step:data:point#{id}");
    let tessellation_id = format!("step:tessellation:mesh#{id}");
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
    topology: &TopologyResult,
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
    let candidate = |kind: &str| format!("step:data:{kind}#{id}");
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
    if has("PRODUCT")
        && entity_ids
            .products
            .contains(&format!("step:product:product#{id}"))
    {
        PresentationItem::Product {
            product: ProductDefinitionId(format!("step:product:product#{id}")),
        }
    } else if has("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
        && entity_ids
            .occurrences
            .contains(&format!("step:product:occurrence#{id}"))
    {
        PresentationItem::Occurrence {
            occurrence: OccurrenceId(format!("step:product:occurrence#{id}")),
        }
    } else if record.partials.iter().any(|partial| {
        (partial.name == "DATUM"
            || partial.name == "DATUM_SYSTEM"
            || partial.name.starts_with("DIMENSIONAL_")
            || partial.name.ends_with("_TOLERANCE"))
            && entity_ids
                .pmi
                .contains(&format!("step:presentation:pmi#{id}"))
    }) {
        PresentationItem::Pmi {
            annotation: PmiId(format!("step:presentation:pmi#{id}")),
        }
    } else if (has("TRIANGULATED_FACE")
        || has("COMPLEX_TRIANGULATED_FACE")
        || has("TRIANGULATED_SURFACE_SET")
        || has("COMPLEX_TRIANGULATED_SURFACE_SET"))
        && entity_ids
            .tessellations
            .contains(&format!("step:tessellation:mesh#{id}"))
    {
        PresentationItem::Tessellation {
            tessellation: format!("step:tessellation:mesh#{id}"),
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
    products: BTreeSet<String>,
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

pub(super) fn styled_item_target(record: &RawRecord) -> Option<u64> {
    styled_item_parts(record).and_then(|parts| parts.target.reference())
}

fn style_depth(
    id: u64,
    exchange: &Exchange,
    active: &mut BTreeSet<u64>,
    depth: usize,
) -> Option<u32> {
    if depth >= super::MAX_RECORD_GRAPH_DEPTH || !active.insert(id) {
        return None;
    }
    let result = (|| {
        let style = exchange.records.get(&id)?;
        if let Some(base) = overridden_style(style) {
            style_depth(base, exchange, active, depth + 1)?.checked_add(1)
        } else {
            Some(0)
        }
    })();
    active.remove(&id);
    result
}

type CachedColor = Option<(u64, Color, Option<String>)>;

fn find_color(
    id: u64,
    exchange: &Exchange,
    domain: StyleDomain,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<u64, CachedColor>,
    losses: &mut Vec<LossNote>,
    depth: usize,
) -> Option<(u64, Color, Option<String>)> {
    if depth >= 256 {
        return None;
    }
    if let Some(result) = cache.get(&id) {
        return result.clone();
    }
    if !active.insert(id) {
        return None;
    }
    let result = (|| {
        let record = exchange.records.get(&id)?;
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
                    id,
                    Color {
                        r: r as f32,
                        g: g as f32,
                        b: b as f32,
                        a: 1.0,
                    },
                    name_value.and_then(|value| {
                        decode_text(
                            value,
                            losses,
                            id,
                            "colour name",
                            LossKind::AttributesNotTransferred,
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
                    name_value,
                    losses,
                    id,
                    "predefined colour name",
                    LossKind::AttributesNotTransferred,
                )?;
                predefined(&name).map(|color| (id, color, Some(name)))
            }
            _ => record
                .partials
                .iter()
                .flat_map(|partial| partial.parameters.iter())
                .flat_map(references)
                .find_map(|reference| {
                    find_color(
                        reference,
                        exchange,
                        domain,
                        active,
                        cache,
                        losses,
                        depth + 1,
                    )
                }),
        }
    })();
    active.remove(&id);
    cache.insert(id, result.clone());
    result
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StyleDomain {
    Any,
    Surface,
    Curve,
    Point,
}

fn style_domain(id: u64, exchange: &Exchange) -> StyleDomain {
    match exchange.records.get(&id).and_then(RecordExt::simple_name) {
        Some(name)
            if name.contains("FACE")
                || name.contains("SURFACE")
                || name.contains("SOLID")
                || name.contains("SHELL") =>
        {
            StyleDomain::Surface
        }
        _ => StyleDomain::Any,
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
            r.parameters()
                .iter()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_color_search_ignores_curve_style_colors() {
        let (exchange, _) = crate::parse::parse(
            b"ISO-10303-21;HEADER;ENDSEC;DATA;\
#1=COLOUR_RGB('curve',0.,0.,1.);\
#2=CURVE_STYLE('',#1);\
#3=COLOUR_RGB('surface',1.,0.,0.);\
#4=SURFACE_STYLE_FILL_AREA(#3);\
#5=PRESENTATION_STYLE_ASSIGNMENT((#2,#4));\
ENDSEC;END-ISO-10303-21;",
        )
        .expect("parse style graph");
        let color = find_color(
            5,
            &exchange,
            StyleDomain::Surface,
            &mut BTreeSet::new(),
            &mut BTreeMap::new(),
            &mut Vec::new(),
            0,
        )
        .expect("surface color");
        assert_eq!(color.1.r, 1.0);
        assert_eq!(color.1.b, 0.0);
    }
}
