// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::json;
use cadmpeg_ir::{document::Model, CadIr};
use serde_json::Value;

use super::{features, id, number, push, rows, text, topology, Mapping};

pub(super) mod sketches;

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

pub(super) fn owner(ir: &CadIr) -> Mapping<cadmpeg_ir::ids::ProductDefinitionId> {
    let [definition] = ir.model.product_definitions.as_slice() else {
        return Err("thin construction needs one admitted Part Studio product scope".into());
    };
    if !definition.occurrences.is_empty()
        || definition.bodies.len() != ir.geometry.bodies.len()
        || ir
            .geometry
            .bodies
            .iter()
            .any(|body| !definition.bodies.contains(&body.id))
    {
        return Err("thin construction product scope does not own the Part Studio bodies".into());
    }
    Ok(definition.id.clone())
}

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
    let mut assumed = None;
    let thickness = if midplane {
        let value = length("thickness")?;
        if value == 0.0 {
            return Err("thin thickness is zero".into());
        }
        json!({"kind":"symmetric","value":literal(value)})
    } else {
        let first = length("thickness1")?;
        let second = length("thickness2")?;
        if first == 0.0 && second == 0.0 {
            return Err("thin thickness is zero".into());
        }
        let flip = flip ^ clockwise;
        if first == second {
            let total = first + second;
            if !total.is_finite() {
                return Err("thin thickness exceeds the finite range".into());
            }
            json!({"kind":"symmetric","value":literal(total)})
        } else if entities.len() != 1 {
            return Err("thin side over multiple curves is unresolved".into());
        } else if second == 0.0 || first == 0.0 {
            let (value, flip) = if first == 0.0 {
                (second, !flip)
            } else {
                (first, flip)
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
            json!({"kind":"one_side","value":{"value":literal(value),"side":{"kind":side}}})
        } else if closed {
            let (outside, inside) = if flip {
                (second, first)
            } else {
                (first, second)
            };
            json!({"kind":"offsets","value":{"outside":literal(outside),"inside":literal(inside)}})
        } else {
            return Err("two-sided open thin offsets are unresolved".into());
        }
    };
    let extent = operation.get("extent").ok_or("thin extent is missing")?;
    let termination = extent
        .pointer("/side/termination")
        .ok_or("thin termination is missing")?;
    let limit = match text(termination, "kind")? {
        "blind" => json!({"kind":"amount","value":literal(number(termination,"length")?)}),
        "through_all" => json!({"kind":"through_all"}),
        _ => return Err("thin termination is unresolved".into()),
    };
    let span = if text(extent, "kind")? == "symmetric" {
        json!({"kind":"symmetric","amount":literal(number(termination,"length")?),"measure":"total"})
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
    Ok((
        json!({"id":id("feature",text(source,"featureId")?),"suppressed":{"kind":"literal","value":false},"definition":{"operation":{"kind":"extrude","value":{
        "profile":{"at":{"kind":"input"},"picks":{"kind":"named","value":picks}},
        "direction":{"kind":"profile_normal","reversed":{"kind":"literal","value":reversed}},"origin":{"kind":"profile_plane"},"span":span,
        "output":{"body":{"kind":"thin","thickness":thickness},"boolean":boolean},"native_options":{},"source_settings":{}}}}}),
        assumed,
    ))
}

pub(super) fn install_scope(
    ir: &mut CadIr,
    report: &mut super::Report,
    bar: &super::RollbackBar,
) -> Mapping<()> {
    for source in &ir.model.features {
        if ir
            .design
            .features
            .iter()
            .any(|feature| feature.id == source.id)
        {
            continue;
        }
        let mut row = json!({"id":source.id,"definition":{"operation":{"kind":"native","value":{"kind":"onshape:timeline-member","native_options":{},"source_settings":{}}}}});
        if let Some(sketch) = ir
            .design
            .sketches
            .iter()
            .find(|sketch| sketch.owner == source.id)
        {
            row["definition"]["operation"] = json!({"kind":"sketch","value":sketch.id});
        } else {
            report.push_coded(source.id.as_str(), crate::loss::OnshapeLossCode::DesignOperationNative,
                "design timeline retains the native source operation; model history carries its decoded construction");
        }
        if let Some(name) = &source.name {
            row["name"] = json!(name);
        }
        if let Some(suppressed) = source.suppressed {
            row["suppressed"] = json!({"kind":"literal","value":suppressed});
        }
        push(&mut ir.design.features, row)?;
    }
    ir.design.features.sort_by_key(|feature| {
        ir.model
            .features
            .iter()
            .position(|source| source.id == feature.id)
    });
    if ir.design.features.is_empty() {
        return Ok(());
    }
    let definition = owner(ir)?;
    let container = id("container", "Part Studio");
    let bodies: Vec<_> = ir.geometry.bodies.iter().map(|body| &body.id).collect();
    let members: Vec<_> = ir
        .design
        .features
        .iter()
        .map(|feature| json!({"kind":"feature","value":feature.id}))
        .collect();
    // The bar stands before the first kept member at or after the stored
    // rollback feature. The current state is the state before the bar.
    let barred = bar.0.iter().find_map(|native| {
        let member = id("feature", native);
        ir.design
            .features
            .iter()
            .position(|feature| feature.id.as_str() == member)
    });
    let (rollback, at) = match barred {
        Some(0) => (
            json!({"kind":"before","value":members[0]}),
            json!({"kind":"start","value":container}),
        ),
        Some(index) => (
            json!({"kind":"before","value":members[index]}),
            json!({"kind":"after","value":members[index - 1]}),
        ),
        None => (
            json!({"kind":"end"}),
            json!({"kind":"after","value":members[members.len() - 1]}),
        ),
    };
    push(
        &mut ir.design.containers,
        json!({"kind":"ordered","value":{"id":container,"kind":"timeline","members":members,"folders":[],"rollback":rollback}}),
    )?;
    push(
        &mut ir.design.scopes,
        json!({"definition":definition,"roots":[container],"active_arrangements":[],"occurrences":{},"unproduced":[],
        "current":[{"at":at,"bodies":bodies,"poses":[],"status":"current"}]}),
    )?;
    Ok(())
}
