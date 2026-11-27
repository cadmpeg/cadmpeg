// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::json;
use cadmpeg_ir::document::Model;
use serde_json::Value;

use super::{features, id, number, rows, text, topology, Mapping};

pub(super) fn profile(profile: Value, model: &Model) -> Mapping<Value> {
    if text(&profile, "kind")? != "sketch" {
        return Ok(profile);
    }
    let sketch = text(&profile, "value")?;
    let entities: Vec<_> = model
        .sketch_entities
        .iter()
        .filter(|entity| {
            entity.sketch.as_str() == sketch
                && !entity.construction
                && !matches!(
                    entity.geometry.definition(),
                    cadmpeg_ir::sketches::SketchGeometryDefinition::Point { .. }
                )
        })
        .map(|entity| entity.id().as_str())
        .collect();
    Ok(json!({"kind":"sketch_entities","value":{"sketch":sketch,"entities":entities}}))
}

/// The path of a one-sided thin wall's side in its design feature.
pub(super) const SIDE: &str = "definition.operation.value.output.body.thickness.value.side";

pub(super) fn feature(
    source: &Value,
    definition: &Value,
    model: &Model,
    inputs: &topology::Inputs<'_>,
    ordinal: usize,
    unresolved: &BTreeSet<String>,
) -> Mapping<(Value, Option<String>)> {
    if features::value(source, "domain") != Some("MODEL") {
        return Err("thin construction has no stored MODEL domain".into());
    }
    let midplane = features::required_boolean(source, "midplane")?;
    let flip = features::required_boolean(source, "flipWall")?;
    let reversed = features::required_boolean(source, "oppositeDirection")?;
    for key in ["hasExtrudeDirection", "startOffset"] {
        if features::enabled(source, key)? {
            return Err(format!("thin {key} construction is unresolved"));
        }
    }
    if features::value(source, "operationType") != Some("NEW") {
        inputs.load(ordinal, source)?.whole_body_scope(source)?;
    }
    let operation = definition
        .pointer("/value/value")
        .ok_or("thin operation is missing")?;
    let profile = operation
        .pointer("/profile/value/value")
        .ok_or("thin construction needs explicit source curves")?;
    let entities = rows(profile, "entities")?;
    if entities.is_empty() {
        return Err("thin construction has no source curves".into());
    }
    let mut closed = false;
    let mut clockwise = false;
    let mut reversed_interval = false;
    let mut directed = BTreeMap::new();
    for source_query in features::queries(source, "wallShape")? {
        if source_query
            .get("queryString")
            .and_then(Value::as_str)
            .is_some_and(|query| query.contains("qCompressed("))
        {
            let query = super::query::resolved(source_query)?;
            let [operation] = rows(&query, "operationId")? else {
                return Err("thin curve producer is unresolved".into());
            };
            let native = operation
                .as_str()
                .and_then(|operation| operation.strip_suffix(".wireOp"))
                .ok_or("thin sketch producer is missing")?;
            let source_id = text(&query, "sketchEntityId")?;
            let key = id("entity", &format!("{native}/{source_id}"));
            let entity = inputs.sketch_entity(native, source_id)?;
            if directed.insert(key, entity).is_some() {
                return Err("thin directed selections repeat".into());
            }
        } else {
            let native = text(source_query, "featureId")?;
            for entity in inputs.sketch_entities(native)? {
                let key = id("entity", &format!("{native}/{}", text(entity, "entityId")?));
                if directed.insert(key, entity).is_some() {
                    return Err("thin directed selections repeat".into());
                }
            }
        }
    }

    for selected in entities {
        let selected = selected.as_str().ok_or("thin curve id is missing")?;
        let entity = directed
            .get(selected)
            .ok_or("thin curve has no stored directed selection")?;
        if super::boolean_field(entity, "isConstruction")? {
            return Err("thin profile selects construction geometry".into());
        }
        let geometry = entity
            .get("geometry")
            .ok_or("stored thin curve direction is missing")?;
        match text(geometry, "btType")? {
            "BTCurveGeometryLine-117" => {
                let dx = super::coordinate(geometry, "dirX")?;
                let dy = super::coordinate(geometry, "dirY")?;
                let start = super::coordinate(entity, "startParam")?;
                let end = super::coordinate(entity, "endParam")?;
                if dx.hypot(dy) == 0.0 || start == end {
                    return Err("stored thin line direction is degenerate".into());
                }
                reversed_interval |= end < start;
            }
            "BTCurveGeometryCircle-115" | "BTCurveGeometryEllipse-1189"
                if entity.get("btType").and_then(Value::as_str) == Some("BTMSketchCurve-4") =>
            {
                closed = true;
                clockwise = geometry
                    .get("clockwise")
                    .and_then(Value::as_bool)
                    .ok_or("stored thin curve sense is missing")?;
            }
            _ => return Err("stored thin curve direction is unresolved".into()),
        }
    }
    let length = |key| -> Mapping<f64> {
        let value =
            features::quantity(features::parameter(source, key)?, false, model, unresolved)?;
        if value < 0.0 {
            return Err("thin thickness is negative".into());
        }
        Ok(value)
    };
    let literal = |value| json!({"kind":"literal","value":value});
    let formula = |key| {
        features::parameter(source, key)
            .ok()
            .and_then(|row| row.get("expression"))
            .and_then(Value::as_str)
            .is_some_and(|expression| !super::expression::literal(expression))
    };
    let native = text(source, "featureId")?;
    let dimension = |key, value| {
        if formula(key) {
            json!({"kind":"parameter","value":id("parameter", &format!("{native}/{key}"))})
        } else {
            literal(value)
        }
    };
    let mut assumed = None;
    let thickness = if midplane {
        let value = length("thickness")?;
        if value == 0.0 {
            return Err("thin thickness is zero".into());
        }
        json!({"kind":"symmetric","value":dimension("thickness", value)})
    } else {
        let first = length("thickness1")?;
        let second = length("thickness2")?;
        if first == 0.0 && second == 0.0 {
            return Err("thin thickness is zero".into());
        }
        let flip = flip ^ clockwise;
        if first == second && !formula("thickness1") && !formula("thickness2") {
            let total = first + second;
            if !total.is_finite() {
                return Err("thin thickness exceeds the finite range".into());
            }
            json!({"kind":"symmetric","value":literal(total)})
        } else if entities.len() != 1 {
            return Err("thin side over multiple curves is unresolved".into());
        } else if second == 0.0 || first == 0.0 {
            let (value, flip) = if first == 0.0 {
                (dimension("thickness2", second), !flip)
            } else {
                (dimension("thickness1", first), flip)
            };
            let side = match (closed, flip) {
                (true, false) => "outside",
                (true, true) => "inside",
                (false, false) => "right",
                (false, true) => "left",
            };
            if !closed && reversed_interval {
                assumed = Some(format!(
                    "the thin wall's {side} side over a line with endParam < startParam follows the stored line direction; the source convention for a reversed interval is unresolved (ONSHAPE-007)"
                ));
            }
            json!({"kind":"one_side","value":{"value":value,"side":{"kind":side}}})
        } else if closed {
            let first = dimension("thickness1", first);
            let second = dimension("thickness2", second);
            let (outside, inside) = if flip {
                (second, first)
            } else {
                (first, second)
            };
            json!({"kind":"offsets","value":{"outside":outside,"inside":inside}})
        } else {
            return Err("two-sided open thin offsets are unresolved".into());
        }
    };
    let extent = operation.get("extent").ok_or("thin extent is missing")?;
    let termination = extent
        .pointer("/side/termination")
        .ok_or("thin termination is missing")?;
    let limit = match text(termination, "kind")? {
        "blind" => {
            json!({"kind":"amount","value":dimension("depth", number(termination,"length")?)})
        }
        "through_all" => json!({"kind":"through_all"}),
        _ => return Err("thin termination is unresolved".into()),
    };
    let span = if text(extent, "kind")? == "symmetric" {
        json!({"kind":"symmetric","amount":dimension("depth", number(termination,"length")?),"measure":"total"})
    } else {
        json!({"kind":"between","start":{"kind":"at_origin"},"end":limit})
    };
    let op = text(operation, "op")?;
    let boolean = if op == "new_body" {
        json!({"kind":"new_body"})
    } else {
        json!({"kind":"apply","op":{"kind":op},"targets":{"kind":"all"}})
    };
    let picks: Vec<_> = entities
        .iter()
        .map(|entity| json!({"target":{"kind":"sketch_entity","value":entity}}))
        .collect();
    let operation = json!({"kind":"extrude","value":{
        "profile":{"at":{"kind":"input"},"picks":{"kind":"named","value":picks}},
        "direction":{"kind":"profile_normal","reversed":{"kind":"literal","value":reversed}},"origin":{"kind":"profile_plane"},"span":span,
        "output":{"body":{"kind":"thin","thickness":thickness},"boolean":boolean},"native_options":{},"source_settings":{}}});
    Ok((operation, assumed))
}
