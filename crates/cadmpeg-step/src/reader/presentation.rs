// SPDX-License-Identifier: Apache-2.0
//! STEP presentation style and topology color decoding.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{AppearanceId, BodyId, FaceId};
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
    for (&style_id, style) in &exchange.records {
        if !matches!(
            style.simple_name(),
            Some("STYLED_ITEM") | Some("OVER_RIDING_STYLED_ITEM")
        ) {
            continue;
        }
        let Some(target_step) = final_target(style, exchange) else {
            warnings.push(format!("STYLED_ITEM #{style_id} has no resolved target"));
            continue;
        };
        let mut visited = BTreeSet::new();
        let Some((color_id, color, name)) = style
            .parameters()
            .iter()
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
        let face_id = format!("step:data:face#{target_step}");
        let body_id = format!("step:data:body#{target_step}");
        let target = if let Some(&index) = face_indices.get(&face_id) {
            ir.model.faces[index].color = Some(color);
            AppearanceTarget::Face(FaceId(face_id))
        } else if let Some(&index) = body_indices.get(&body_id) {
            ir.model.bodies[index].color = Some(color);
            AppearanceTarget::Body(BodyId(body_id))
        } else {
            warnings.push(format!(
                "STYLED_ITEM #{style_id} targets unsupported item #{target_step}"
            ));
            continue;
        };
        ir.model.appearance_bindings.push(AppearanceBinding {
            id: format!("step:presentation:binding#{style_id}"),
            target,
            appearance: appearance_id,
            source_entity_id: Some(format!("#{style_id}")),
            object_type: None,
            channels: BTreeMap::new(),
        });
        typed.insert(style_id);
        typed.extend(visited);
        typed.insert(color_id);
    }
    PresentationResult {
        typed_records: typed,
        warnings,
    }
}

fn final_target(style: &RawRecord, exchange: &Exchange) -> Option<u64> {
    let target = style
        .parameters()
        .iter()
        .rev()
        .find_map(ValueExt::reference)?;
    let record = exchange.records.get(&target)?;
    if matches!(
        record.simple_name(),
        Some("STYLED_ITEM") | Some("OVER_RIDING_STYLED_ITEM")
    ) {
        final_target(record, exchange)
    } else {
        Some(target)
    }
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
}
