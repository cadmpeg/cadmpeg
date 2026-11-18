// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::{features, id, rows, text, Mapping};

pub(super) fn profile(profile: Value, model: &Model) -> Mapping<Value> {
    if profile.get("kind").and_then(Value::as_str) != Some("sketch") {
        return Ok(profile);
    }
    let sketch = text(&profile, "value")?;
    let mut entities = Vec::new();
    for entity in &model.sketch_entities {
        if entity.sketch.as_str() == sketch && !entity.construction {
            let value =
                serde_json::to_value(entity).map_err(|_| "the thin extrude curve is unreadable")?;
            if value.pointer("/geometry/kind").and_then(Value::as_str) != Some("point") {
                entities.push(entity.id().as_str());
            }
        }
    }
    Ok(json!({"kind":"sketch_entities","value":{"sketch":sketch,"entities":entities}}))
}

pub(super) fn feature(source: &Value, definition: &Value, model: &Model) -> Mapping<Value> {
    let extrude = definition
        .pointer("/value/value")
        .ok_or("the thin extrude definition is missing")?;
    let profile = extrude
        .pointer("/profile/value/value")
        .ok_or("the thin extrude needs explicit sketch curves")?;
    let entities = rows(profile, "entities")?;
    let mut picks = Vec::new();
    for entity in entities {
        picks.push(json!({"target":{"kind":"sketch_entity","value":entity}}));
    }
    if picks.is_empty() {
        return Err("the thin extrude has no selected sketch curves".into());
    }
    let length = |key| -> Mapping<f64> {
        features::quantity(features::parameter(source, key)?, false, model)
    };
    let literal = |value| json!({"kind":"literal","value":value});
    let thickness = if features::parameter(source, "midplane")
        .ok()
        .and_then(|p| p.get("value"))
        .and_then(Value::as_bool)
        == Some(true)
    {
        json!({"kind":"symmetric","value":literal(length("thickness")?)})
    } else {
        let first = length("thickness1")?;
        let second = length("thickness2")?;
        let flipped = features::parameter(source, "flipWall")
            .ok()
            .and_then(|p| p.get("value"))
            .and_then(Value::as_bool)
            == Some(true);
        if second == 0.0 {
            json!({"kind":"one_side","value":{"value":literal(first),"side":{"kind":if flipped {"inside"} else {"outside"}}}})
        } else {
            let (outside, inside) = if flipped {
                (second, first)
            } else {
                (first, second)
            };
            json!({"kind":"offsets","value":{"outside":literal(outside),"inside":literal(inside)}})
        }
    };
    let extent = extrude
        .get("extent")
        .ok_or("the thin extrude extent is missing")?;
    let termination = extent
        .pointer("/side/termination")
        .ok_or("the thin extrude termination is missing")?;
    let limit = match text(termination, "kind")? {
        "blind" => json!({"kind":"amount","value":literal(super::number(termination,"length")?)}),
        "through_all" => json!({"kind":"through_all"}),
        _ => return Err("thin extrudes with this bound are not supported yet".into()),
    };
    let span = if text(extent, "kind")? == "symmetric" {
        json!({"kind":"symmetric","amount":literal(super::number(termination,"length")?),"measure":"total"})
    } else {
        json!({"kind":"between","start":{"kind":"at_origin"},"end":limit})
    };
    let op = text(extrude, "op")?;
    let boolean = if op == "new_body" {
        json!({"kind":"new_body"})
    } else {
        json!({"kind":"apply","op":{"kind":op},"targets":{"kind":"all"}})
    };
    Ok(
        json!({"id":id("feature",text(source,"featureId")?),"definition":{"operation":{"kind":"extrude","value":{
        "profile":{"at":{"kind":"input"},"picks":{"kind":"named","value":picks}},
        "direction":{"kind":"profile_normal","reversed":{"kind":"literal","value":extrude.pointer("/direction/kind").and_then(Value::as_str)==Some("reversed_profile_normal")}},
        "origin":{"kind":"profile_plane"},"span":span,
        "output":{"body":{"kind":"thin","thickness":thickness},"boolean":boolean},"native_options":{},"source_settings":{}}}}}),
    )
}
