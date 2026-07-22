// SPDX-License-Identifier: Apache-2.0
//! STEP presentation style and topology color decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{
    AppearanceId, BodyId, CurveId, EdgeId, FaceId, LayerId, OccurrenceId, PmiId, PointId,
    ProductId, SurfaceId, VertexId,
};
use cadmpeg_ir::presentation::{PresentationItem, PresentationLayer};
use cadmpeg_ir::topology::Color;

use crate::parse::{Exchange, RawRecord, Value};

pub(super) struct PresentationResult {
    pub typed_records: BTreeSet<u64>,
    pub warnings: Vec<String>,
}

pub(super) fn decode(exchange: &Exchange, ir: &mut CadIr) -> PresentationResult {
    let mut typed = BTreeSet::new();
    let mut warnings = Vec::new();
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
            .products
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
        if record.simple_name() != Some("INVISIBILITY") {
            continue;
        }
        let Some(items) = record.parameter(0).and_then(ValueExt::list) else {
            warnings.push(format!("INVISIBILITY #{id} has no item set"));
            continue;
        };
        for target in items.iter().filter_map(ValueExt::reference) {
            let body_id = format!("step:data:body#{target}");
            if let Some(index) = body_indices.get(&body_id) {
                ir.model.bodies[*index].visible = Some(false);
            } else {
                warnings.push(format!(
                    "INVISIBILITY #{id} targets unsupported item #{target}"
                ));
            }
        }
        typed.insert(id);
    }
    for (&layer_id, layer) in &exchange.records {
        if layer.simple_name() != Some("PRESENTATION_LAYER_ASSIGNMENT") {
            continue;
        }
        let Some(name) = layer.parameter(0).and_then(ValueExt::text) else {
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
        let description = layer
            .parameter(1)
            .and_then(ValueExt::text)
            .filter(|value| !value.is_empty());
        let items = layer
            .parameter(2)
            .and_then(ValueExt::list)
            .into_iter()
            .flatten()
            .filter_map(ValueExt::reference)
            .map(|id| presentation_item(id, exchange, &entity_ids, &face_indices, &body_indices))
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
        .filter_map(|(&id, record)| {
            matches!(
                record.simple_name(),
                Some("STYLED_ITEM" | "OVER_RIDING_STYLED_ITEM")
            )
            .then_some(id)
        })
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
        let Some(target_step) = style.parameter(2).and_then(ValueExt::reference) else {
            warnings.push(format!("STYLED_ITEM #{style_id} has no resolved target"));
            continue;
        };
        if style
            .parameter(1)
            .and_then(ValueExt::list)
            .is_some_and(<[Value]>::is_empty)
        {
            typed.insert(style_id);
            continue;
        }
        let domain = style_domain(target_step, exchange);
        let mut active = BTreeSet::new();
        let mut color_cache = BTreeMap::new();
        let Some((color_id, color, name)) = style
            .parameter(1)
            .and_then(ValueExt::list)
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
                    0,
                )
            })
        else {
            let mut visited = BTreeSet::new();
            if !style
                .parameter(1)
                .is_some_and(|value| contains_null_style(value, exchange, &mut visited, 0))
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
                let id = AppearanceId(format!("step:presentation:appearance#{color_id}"));
                ir.model.appearances.push(Appearance {
                    id: id.clone(),
                    name,
                    asset_guid: None,
                    visual_guid: None,
                    physical_token: None,
                    schema: Some("step_surface_style".into()),
                    category: None,
                    base_color: Some(color),
                    properties: BTreeMap::new(),
                });
                id
            })
            .clone();
        let target_steps =
            expand_style_targets(target_step, exchange, &mut typed, &mut BTreeSet::new(), 0);
        for (ordinal, target_step) in target_steps.into_iter().enumerate() {
            let face_id = format!("step:data:face#{target_step}");
            let body_id = format!("step:data:body#{target_step}");
            let edge_id = format!("step:data:edge#{target_step}");
            let surface_id = format!("step:data:surface#{target_step}");
            let curve_id = format!("step:data:curve#{target_step}");
            let point_id = format!("step:data:point#{target_step}");
            let tessellation_id = format!("step:tessellation:mesh#{target_step}");
            let target = if let Some(&index) = face_indices.get(&face_id) {
                ir.model.faces[index].color = Some(color);
                AppearanceTarget::Face(FaceId(face_id))
            } else if let Some(&index) = body_indices.get(&body_id) {
                ir.model.bodies[index].color = Some(color);
                AppearanceTarget::Body(BodyId(body_id))
            } else if entity_ids.edges.contains(&edge_id) {
                AppearanceTarget::Edge(EdgeId(edge_id))
            } else if entity_ids.surfaces.contains(&surface_id) {
                AppearanceTarget::Surface(SurfaceId(surface_id))
            } else if entity_ids.curves.contains(&curve_id) {
                AppearanceTarget::Curve(CurveId(curve_id))
            } else if entity_ids.points.contains(&point_id) {
                AppearanceTarget::Point(PointId(point_id))
            } else if entity_ids.tessellations.contains(&tessellation_id) {
                AppearanceTarget::Tessellation(tessellation_id)
            } else if exchange.records.contains_key(&target_step) {
                AppearanceTarget::Source {
                    source_id: format!("#{target_step}"),
                }
            } else {
                warnings.push(format!(
                    "STYLED_ITEM #{style_id} targets unsupported item #{target_step}"
                ));
                continue;
            };
            ir.model.appearance_bindings.push(AppearanceBinding {
                id: format!("step:presentation:binding#{style_id}:{ordinal}"),
                target,
                appearance: appearance_id.clone(),
                source_entity_id: Some(format!("#{style_id}")),
                object_type: None,
                channels: BTreeMap::new(),
            });
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
    }
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
        return vec![id];
    };
    if !matches!(
        record.simple_name(),
        Some("GEOMETRIC_SET" | "GEOMETRIC_CURVE_SET")
    ) {
        active.remove(&id);
        return vec![id];
    }
    typed.insert(id);
    let targets = record
        .parameter(1)
        .and_then(ValueExt::list)
        .into_iter()
        .flatten()
        .filter_map(ValueExt::reference)
        .flat_map(|item| expand_style_targets(item, exchange, typed, active, depth + 1))
        .collect();
    active.remove(&id);
    targets
}

fn presentation_item(
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
    match exchange.records.get(&id).and_then(RecordExt::simple_name) {
        Some("PRODUCT")
            if entity_ids
                .products
                .contains(&format!("step:product:product#{id}")) =>
        {
            PresentationItem::Product {
                product: ProductId(format!("step:product:product#{id}")),
            }
        }
        Some("NEXT_ASSEMBLY_USAGE_OCCURRENCE")
            if entity_ids
                .occurrences
                .contains(&format!("step:product:occurrence#{id}")) =>
        {
            PresentationItem::Occurrence {
                occurrence: OccurrenceId(format!("step:product:occurrence#{id}")),
            }
        }
        Some(name)
            if (name == "DATUM"
                || name == "DATUM_SYSTEM"
                || name.starts_with("DIMENSIONAL_")
                || name.ends_with("_TOLERANCE"))
                && entity_ids
                    .pmi
                    .contains(&format!("step:presentation:pmi#{id}")) =>
        {
            PresentationItem::Pmi {
                annotation: PmiId(format!("step:presentation:pmi#{id}")),
            }
        }
        Some("TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_FACE" | "TRIANGULATED_SURFACE_SET")
            if entity_ids
                .tessellations
                .contains(&format!("step:tessellation:mesh#{id}")) =>
        {
            PresentationItem::Tessellation {
                tessellation: format!("step:tessellation:mesh#{id}"),
            }
        }
        _ => PresentationItem::Source {
            source_id: format!("#{id}"),
        },
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
    (style.simple_name() == Some("OVER_RIDING_STYLED_ITEM"))
        .then(|| style.parameter(3).and_then(ValueExt::reference))?
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
    let style = exchange.records.get(&id)?;
    let depth = if let Some(base) = overridden_style(style) {
        style_depth(base, exchange, active, depth + 1)?.checked_add(1)?
    } else {
        0
    };
    active.remove(&id);
    Some(depth)
}

type CachedColor = Option<(u64, Color, Option<String>)>;

fn find_color(
    id: u64,
    exchange: &Exchange,
    domain: StyleDomain,
    active: &mut BTreeSet<u64>,
    cache: &mut BTreeMap<u64, CachedColor>,
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
    let record = exchange.records.get(&id)?;
    let name = record.simple_name()?;
    let record_domain = if name.starts_with("SURFACE_STYLE") {
        Some(StyleDomain::Surface)
    } else if name == "CURVE_STYLE" {
        Some(StyleDomain::Curve)
    } else if name == "POINT_STYLE" {
        Some(StyleDomain::Point)
    } else {
        None
    };
    let incompatible =
        record_domain.is_some_and(|candidate| domain != StyleDomain::Any && candidate != domain);
    let result = if incompatible {
        for reference in record.parameters().iter().flat_map(references) {
            let _ = find_color(reference, exchange, domain, active, cache, depth + 1);
        }
        None
    } else {
        match name {
            "COLOUR_RGB" => {
                let r = record.parameter(1)?.number()?;
                let g = record.parameter(2)?.number()?;
                let b = record.parameter(3)?.number()?;
                if ![r, g, b]
                    .iter()
                    .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
                {
                    return None;
                }
                Some((
                    id,
                    Color {
                        r: r as f32,
                        g: g as f32,
                        b: b as f32,
                        a: 1.0,
                    },
                    record.parameter(0).and_then(ValueExt::text),
                ))
            }
            "DRAUGHTING_PRE_DEFINED_COLOUR" => {
                let name = record.parameter(0)?.text()?;
                predefined(&name).map(|color| (id, color, Some(name)))
            }
            _ => record
                .parameters()
                .iter()
                .flat_map(references)
                .find_map(|reference| {
                    find_color(reference, exchange, domain, active, cache, depth + 1)
                }),
        }
    };
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
    fn text(&self) -> Option<String>;
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
    fn text(&self) -> Option<String> {
        if let Value::String(bytes) = self {
            crate::strings::decode(bytes).ok()
        } else {
            None
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
        let exchange = crate::parse::parse(
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
            0,
        )
        .expect("surface color");
        assert_eq!(color.1.r, 1.0);
        assert_eq!(color.1.b, 0.0);
    }
}
