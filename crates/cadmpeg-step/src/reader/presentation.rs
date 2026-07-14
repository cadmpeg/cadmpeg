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
    let mut appearance_ids = BTreeMap::<u64, AppearanceId>::new();
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
            .map(|id| presentation_item(id, exchange, ir))
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
    styles.sort_by_key(|id| style_depth(*id, exchange, &mut BTreeSet::new()).unwrap_or(u32::MAX));
    for style_id in styles {
        let style = &exchange.records[&style_id];
        let Some(target_step) = style.parameter(2).and_then(ValueExt::reference) else {
            warnings.push(format!("STYLED_ITEM #{style_id} has no resolved target"));
            continue;
        };
        let mut null_style_records = BTreeSet::new();
        if style
            .parameter(1)
            .and_then(ValueExt::list)
            .is_some_and(<[Value]>::is_empty)
        {
            typed.insert(style_id);
            continue;
        }
        if style
            .parameter(1)
            .is_some_and(|value| has_null_style(value, exchange, &mut null_style_records))
        {
            typed.insert(style_id);
            typed.extend(null_style_records);
            continue;
        }
        let mut visited = BTreeSet::new();
        let Some((color_id, color, name)) = style
            .parameter(1)
            .and_then(ValueExt::list)
            .into_iter()
            .flatten()
            .flat_map(references)
            .find_map(|reference| find_color(reference, exchange, &mut visited))
        else {
            warnings.push(format!(
                "STYLED_ITEM #{style_id} has no resolved surface color"
            ));
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
            expand_style_targets(target_step, exchange, &mut typed, &mut BTreeSet::new());
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
            } else if ir
                .model
                .edges
                .iter()
                .any(|edge| edge.id.as_str() == edge_id)
            {
                AppearanceTarget::Edge(EdgeId(edge_id))
            } else if ir
                .model
                .surfaces
                .iter()
                .any(|surface| surface.id.as_str() == surface_id)
            {
                AppearanceTarget::Surface(SurfaceId(surface_id))
            } else if ir
                .model
                .curves
                .iter()
                .any(|curve| curve.id.as_str() == curve_id)
            {
                AppearanceTarget::Curve(CurveId(curve_id))
            } else if ir
                .model
                .points
                .iter()
                .any(|point| point.id.as_str() == point_id)
            {
                AppearanceTarget::Point(PointId(point_id))
            } else if ir
                .model
                .tessellations
                .iter()
                .any(|mesh| mesh.id == tessellation_id)
            {
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
        typed.extend(visited);
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
) -> Vec<u64> {
    if !active.insert(id) {
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
        .flat_map(|item| expand_style_targets(item, exchange, typed, active))
        .collect();
    active.remove(&id);
    targets
}

fn presentation_item(id: u64, exchange: &Exchange, ir: &CadIr) -> PresentationItem {
    let candidate = |kind: &str| format!("step:data:{kind}#{id}");
    let body = candidate("body");
    if ir.model.bodies.iter().any(|item| item.id.as_str() == body) {
        return PresentationItem::Body { body: BodyId(body) };
    }
    let face = candidate("face");
    if ir.model.faces.iter().any(|item| item.id.as_str() == face) {
        return PresentationItem::Face { face: FaceId(face) };
    }
    let edge = candidate("edge");
    if ir.model.edges.iter().any(|item| item.id.as_str() == edge) {
        return PresentationItem::Edge { edge: EdgeId(edge) };
    }
    let vertex = candidate("vertex");
    if ir
        .model
        .vertices
        .iter()
        .any(|item| item.id.as_str() == vertex)
    {
        return PresentationItem::Vertex {
            vertex: VertexId(vertex),
        };
    }
    let point = candidate("point");
    if ir.model.points.iter().any(|item| item.id.as_str() == point) {
        return PresentationItem::Point {
            point: PointId(point),
        };
    }
    let curve = candidate("curve");
    if ir.model.curves.iter().any(|item| item.id.as_str() == curve) {
        return PresentationItem::Curve {
            curve: CurveId(curve),
        };
    }
    let surface = candidate("surface");
    if ir
        .model
        .surfaces
        .iter()
        .any(|item| item.id.as_str() == surface)
    {
        return PresentationItem::Surface {
            surface: SurfaceId(surface),
        };
    }
    match exchange.records.get(&id).and_then(RecordExt::simple_name) {
        Some("PRODUCT") => PresentationItem::Product {
            product: ProductId(format!("step:product:product#{id}")),
        },
        Some("NEXT_ASSEMBLY_USAGE_OCCURRENCE") => PresentationItem::Occurrence {
            occurrence: OccurrenceId(format!("step:product:occurrence#{id}")),
        },
        Some(name)
            if name == "DATUM"
                || name == "DATUM_SYSTEM"
                || name.starts_with("DIMENSIONAL_")
                || name.ends_with("_TOLERANCE") =>
        {
            PresentationItem::Pmi {
                annotation: PmiId(format!("step:presentation:pmi#{id}")),
            }
        }
        Some("TRIANGULATED_FACE" | "COMPLEX_TRIANGULATED_FACE" | "TRIANGULATED_SURFACE_SET") => {
            PresentationItem::Tessellation {
                tessellation: format!("step:tessellation:mesh#{id}"),
            }
        }
        _ => PresentationItem::Source {
            source_id: format!("#{id}"),
        },
    }
}

fn overridden_style(style: &RawRecord) -> Option<u64> {
    (style.simple_name() == Some("OVER_RIDING_STYLED_ITEM"))
        .then(|| style.parameter(3).and_then(ValueExt::reference))?
}

fn style_depth(id: u64, exchange: &Exchange, active: &mut BTreeSet<u64>) -> Option<u32> {
    if !active.insert(id) {
        return None;
    }
    let style = exchange.records.get(&id)?;
    let depth = if let Some(base) = overridden_style(style) {
        style_depth(base, exchange, active)?.checked_add(1)?
    } else {
        0
    };
    active.remove(&id);
    Some(depth)
}

fn find_color(
    id: u64,
    exchange: &Exchange,
    visited: &mut BTreeSet<u64>,
) -> Option<(u64, Color, Option<String>)> {
    if !visited.insert(id) {
        return None;
    }
    let record = exchange.records.get(&id)?;
    match record.simple_name()? {
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
            .find_map(|reference| find_color(reference, exchange, visited)),
    }
}

fn has_null_style(value: &Value, exchange: &Exchange, visited: &mut BTreeSet<u64>) -> bool {
    match value {
        Value::Typed(name, _) if name == "NULL_STYLE" => true,
        Value::Typed(_, value) => has_null_style(value, exchange, visited),
        Value::List(values) => values
            .iter()
            .any(|value| has_null_style(value, exchange, visited)),
        Value::Reference(id) if visited.insert(*id) => {
            exchange.records.get(id).is_some_and(|record| {
                record
                    .parameters()
                    .iter()
                    .any(|value| has_null_style(value, exchange, visited))
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
        &self.partials[0].parameters
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
