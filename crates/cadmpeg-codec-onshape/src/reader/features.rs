// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::Report;
const DRILL_TIP_ANGLE_TOLERANCE: f64 = 1e-9;
use super::{definition, id, number, push, rows, sketch, text, topology, Mapping};

pub(super) fn parameter<'a>(feature: &'a Value, key: &str) -> Mapping<&'a Value> {
    let mut found = rows(feature, "parameters")?
        .iter()
        .filter(|p| p.get("parameterId").and_then(Value::as_str) == Some(key));
    let result = found
        .next()
        .ok_or_else(|| format!("parameter {key} is missing"))?;
    if found.next().is_some() {
        return Err(format!("parameter {key} repeats"));
    }
    Ok(result)
}

pub(super) fn value<'a>(feature: &'a Value, key: &str) -> Option<&'a str> {
    parameter(feature, key).ok()?.get("value")?.as_str()
}
pub(super) fn source_properties(feature: &Value, definition: &Value) -> Value {
    match definition.pointer("/value/kind").and_then(Value::as_str) {
        Some("fillet" | "chamfer") => json!({"tangent_propagation":
            if enabled(feature, "tangentPropagation") { "resolved" } else { "off" }}),
        _ => json!({}),
    }
}

fn enabled(feature: &Value, key: &str) -> bool {
    parameter(feature, key)
        .ok()
        .and_then(|p| p.get("value"))
        .and_then(Value::as_bool)
        == Some(true)
}
pub(super) fn quantity(parameter: &Value, angle: bool, model: &Model) -> Mapping<f64> {
    fn scale(unit: &str, angle: bool) -> Option<f64> {
        match (angle, unit) {
            (true, "radian" | "rad" | "") => Some(1.0),
            (true, "degree" | "deg") => Some(std::f64::consts::PI / 180.0),
            (false, "meter" | "m" | "") => Some(1000.0),
            (false, "millimeter" | "mm") => Some(1.0),
            (false, "centimeter" | "cm") => Some(10.0),
            (false, "inch" | "in") => Some(25.4),
            (false, "foot" | "ft") => Some(304.8),
            _ => None,
        }
    }
    if let Some(expression) = parameter.get("expression").and_then(Value::as_str) {
        if expression.contains('#') {
            return super::expression::evaluate(
                expression,
                if angle {
                    super::expression::Dimension::Angle
                } else {
                    super::expression::Dimension::Length
                },
                model,
            )
            .map(|v| v.value);
        }
        if let Some((scalar, unit)) = expression.trim().rsplit_once(char::is_whitespace) {
            if let Some(factor) = scale(unit, angle) {
                if let Ok(value) =
                    super::expression::evaluate(scalar, super::expression::Dimension::Scalar, model)
                {
                    let result = value.value * factor;
                    if result.is_finite() {
                        return Ok(result);
                    }
                }
            }
        }
    }
    let factor = scale(
        parameter.get("units").and_then(Value::as_str).unwrap_or(""),
        angle,
    )
    .ok_or("quantity units are not supported yet")?;
    let result = number(parameter, "value")? * factor;
    if result.is_finite() {
        Ok(result)
    } else {
        Err("quantity exceeds the finite range".into())
    }
}

fn length(feature: &Value, key: &str, model: &Model) -> Mapping<f64> {
    let n = quantity(parameter(feature, key)?, false, model)
        .map_err(|reason| format!("{key}: {reason}"))?;
    if n > 0.0 {
        Ok(n)
    } else {
        Err(format!("{key} must have a positive resolved length"))
    }
}

pub(super) fn queries<'a>(feature: &'a Value, key: &str) -> Mapping<&'a [Value]> {
    let list = rows(parameter(feature, key)?, "queries")?;
    if list.is_empty() {
        Err(format!("{key} has no resolved queries"))
    } else {
        Ok(list)
    }
}

pub(super) fn profile(
    feature: &Value,
    key: &str,
    model: &Model,
    inputs: &topology::Inputs<'_>,
) -> Mapping<Value> {
    inputs
        .regions
        .try_borrow()
        .map_err(|_| "the sketch-region recovery state is already in use")?
        .check()?;
    let query = queries(feature, key)?;
    if query.iter().all(|q| {
        super::query::resolved(q)
            .is_ok_and(|q| q.get("queryType").and_then(Value::as_str) == Some("SKETCH_ENTITY"))
    }) {
        return sketch_entities(
            query,
            model,
            if value(feature, "bodyType") == Some("THIN") {
                "EDGE"
            } else {
                "VERTEX"
            },
        );
    }
    if query.iter().any(|q| {
        q.get("queryString")
            .and_then(Value::as_str)
            .is_some_and(|s| s.contains("qCompressed("))
    }) {
        return super::regions::profile(query, model, inputs);
    }
    let [query] = query else {
        return Err("multiple profile queries are not supported yet".into());
    };
    if query
        .get("deterministicIds")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
    {
        return Err("deterministic sketch-region subsets are not supported yet".into());
    }
    if !matches!(
        query.get("btType").and_then(Value::as_str),
        Some(
            "BTMIndividualSketchRegionQuery-140"
                | "BTMIndividualSketchUniqueVerticesQuery-1472"
                | "BTMIndividualCreatedByQuery-137"
        )
    ) {
        return Err("the sketch query is not supported yet".into());
    }
    if query.get("filterInnerLoops").and_then(Value::as_bool) == Some(true) {
        return Err("filtered inner sketch regions are not supported yet".into());
    }
    let sketch_id = id("sketch", text(query, "featureId")?);
    if !model.sketches.iter().any(|s| s.id.as_str() == sketch_id) {
        return Err("the selected sketch has no resolved geometry and plane".into());
    }
    Ok(json!({"kind": "sketch", "value": sketch_id}))
}
fn sketch_entities(queries: &[Value], model: &Model, entity_type: &str) -> Mapping<Value> {
    let mut selected = std::collections::BTreeSet::new();
    let mut selected_sketch = None;
    for source in queries {
        let query = super::query::resolved(source)?;
        if text(&query, "queryType")? != "SKETCH_ENTITY"
            || text(&query, "historyType")? != "CREATION"
            || text(&query, "entityType")? != entity_type
        {
            return Err("this sketch entity selection is not supported yet".into());
        }
        let [operation] = rows(&query, "operationId")? else {
            return Err("a sketch entity needs one source operation".into());
        };
        let native = operation
            .as_str()
            .and_then(|s| s.strip_suffix(".wireOp"))
            .ok_or("the source sketch operation is not supported yet")?;
        let sketch = id("sketch", native);
        if selected_sketch.as_ref().is_some_and(|id| id != &sketch) {
            return Err("selections from multiple sketches are not supported yet".into());
        }
        let entity_id = id(
            "entity",
            &format!("{native}/{}", text(&query, "sketchEntityId")?),
        );
        let entity = model
            .sketch_entities
            .iter()
            .find(|e| e.id().as_str() == entity_id)
            .ok_or("the selected sketch entity has no solved geometry")?;
        if entity_type == "VERTEX" {
            let wire = serde_json::to_value(entity)
                .map_err(|_| "the selected sketch entity is unreadable")?;
            if wire.pointer("/geometry/kind").and_then(Value::as_str) != Some("point") {
                return Err(
                    "hole locations on sketch curve endpoints are not supported yet".into(),
                );
            }
        }
        selected_sketch = Some(sketch);
        if !selected.insert(entity_id) {
            return Err("the sketch selection repeats an entity".into());
        }
    }
    Ok(
        json!({"kind":"sketch_entities","value":{"sketch":selected_sketch.ok_or("the sketch selection is empty")?,"entities":selected}}),
    )
}
fn profile_samples(profile: &Value, model: &Model) -> Mapping<(Vec<[f64; 3]>, [f64; 3])> {
    let value = profile.get("value").ok_or("the profile is missing")?;
    let sketch_id = value
        .as_str()
        .or_else(|| value.get("sketch").and_then(Value::as_str))
        .ok_or("the profile sketch is missing")?;
    let mut chosen = std::collections::BTreeSet::new();
    for region in optional_array(value, "regions") {
        let holes = optional_array(region, "hole_boundaries")
            .iter()
            .flat_map(|hole| hole.as_array().map(Vec::as_slice).unwrap_or_default());
        for boundary in optional_array(region, "outer_boundary").iter().chain(holes) {
            chosen.insert(text(boundary, "entity")?.to_owned());
        }
    }
    let sketch = model
        .sketches
        .iter()
        .find(|s| s.id.as_str() == sketch_id)
        .ok_or("the profile sketch has no resolved frame")?;
    let sketch = serde_json::to_value(sketch).map_err(|_| "the profile sketch is unreadable")?;
    let frame = sketch
        .pointer("/placement/frame")
        .ok_or("the profile frame is missing")?;
    let at = |key: &str| -> Mapping<[f64; 3]> {
        topology::vector(
            frame.get(key).ok_or("the profile frame is incomplete")?,
            1.0,
        )
    };
    let origin = at("origin")?;
    let u = topology::unit(at("u_axis")?)?;
    let normal = topology::unit(at("normal")?)?;
    let v = topology::cross(normal, u);
    let ([ox, oy, oz], [ux, uy, uz], [vx, vy, vz]) = (origin, u, v);
    let world = |[a, b]: [f64; 2]| -> [f64; 3] {
        [
            ox + a * ux + b * vx,
            oy + a * uy + b * vy,
            oz + a * uz + b * vz,
        ]
    };
    let mut samples = Vec::new();
    for entity in &model.sketch_entities {
        let entity = serde_json::to_value(entity).map_err(|_| "a profile curve is unreadable")?;
        let selected = if chosen.is_empty() {
            entity.get("construction").and_then(Value::as_bool) != Some(true)
        } else {
            chosen.contains(text(&entity, "id")?)
        };
        if entity.get("sketch").and_then(Value::as_str) != Some(sketch_id) || !selected {
            continue;
        }
        let geometry = entity
            .get("geometry")
            .ok_or("a profile curve has no geometry")?;
        let uv = |key: &str| -> Mapping<[f64; 2]> {
            let point = geometry
                .get(key)
                .ok_or("a profile curve point is missing")?;
            Ok([number(point, "u")?, number(point, "v")?])
        };
        match text(geometry, "kind")? {
            "point" => {}
            "line" => {
                let ([ax, ay], [bx, by]) = (uv("start")?, uv("end")?);
                for step in 0..=4 {
                    let t = f64::from(step) / 4.0;
                    samples.push(world([ax + t * (bx - ax), ay + t * (by - ay)]));
                }
            }
            kind @ ("circle" | "arc") => {
                let [cx, cy] = uv("center")?;
                let radius = number(geometry, "radius")?;
                let (from, to) = if kind == "circle" {
                    (0.0, std::f64::consts::TAU)
                } else {
                    let from = number(geometry, "start_angle")?;
                    let to = number(geometry, "end_angle")?;
                    (
                        from,
                        if to < from {
                            to + std::f64::consts::TAU
                        } else {
                            to
                        },
                    )
                };
                for step in 0..=32 {
                    let t = from + (to - from) * f64::from(step) / 32.0;
                    samples.push(world([cx + radius * t.cos(), cy + radius * t.sin()]));
                }
            }
            _ => {
                return Err("an up-to-next extrude of a spline profile is not supported yet".into())
            }
        }
    }
    if samples.is_empty() || samples.iter().flatten().any(|n| !n.is_finite()) {
        return Err("the up-to-next profile has no resolved curves".into());
    }
    Ok((samples, normal))
}

fn optional_array<'a>(row: &'a Value, key: &str) -> &'a [Value] {
    row.get(key)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn sketch_axis(feature: &Value, model: &Model) -> Mapping<Value> {
    let profile = sketch_entities(queries(feature, "axis")?, model, "EDGE")?;
    let sketch_id = profile
        .pointer("/value/sketch")
        .and_then(Value::as_str)
        .ok_or("the axis sketch is missing")?;
    let [entity_id] = rows(
        profile
            .get("value")
            .ok_or("the axis selection is missing")?,
        "entities",
    )?
    else {
        return Err("a revolve axis needs exactly one sketch construction line".into());
    };
    let entity = model
        .sketch_entities
        .iter()
        .find(|e| Some(e.id().as_str()) == entity_id.as_str())
        .ok_or("the axis line is missing")?;
    let entity = serde_json::to_value(entity).map_err(|_| "the axis line is unreadable")?;
    if entity.pointer("/geometry/kind").and_then(Value::as_str) != Some("line")
        || entity.get("construction").and_then(Value::as_bool) != Some(true)
    {
        return Err("the selected revolve axis is not a sketch construction line; this axis is not supported yet".into());
    }
    let sketch = model
        .sketches
        .iter()
        .find(|s| s.id.as_str() == sketch_id)
        .ok_or("the axis sketch has no resolved frame")?;
    let sketch = serde_json::to_value(sketch).map_err(|_| "the axis sketch is unreadable")?;
    let frame = sketch
        .pointer("/placement/frame")
        .ok_or("the axis frame is missing")?;
    let origin = topology::vector(
        frame.get("origin").ok_or("the axis origin is missing")?,
        1.0,
    )?;
    let u = topology::vector(
        frame
            .get("u_axis")
            .ok_or("the axis in-plane direction is missing")?,
        1.0,
    )?;
    let normal = topology::vector(
        frame.get("normal").ok_or("the axis normal is missing")?,
        1.0,
    )?;
    let v = topology::cross(normal, u);
    let world = |key: &str| -> Mapping<[f64; 3]> {
        let point = entity
            .get("geometry")
            .and_then(|g| g.get(key))
            .ok_or("the axis endpoint is missing")?;
        let a = number(point, "u")?;
        let b = number(point, "v")?;
        let [ox, oy, oz] = origin;
        let [ux, uy, uz] = u;
        let [vx, vy, vz] = v;
        Ok([
            ox + a * ux + b * vx,
            oy + a * uy + b * vy,
            oz + a * uz + b * vz,
        ])
    };
    let start = world("start")?;
    let end = world("end")?;
    Ok(json!({"origin":topology::point(start),
        "direction":topology::point(topology::unit(topology::sub(end,start))?),
        "reference":{"kind":"sketch_curves","value":{"sketch":sketch_id,"curves":[entity_id]}}}))
}

fn operation(feature: &Value) -> Mapping<&'static str> {
    match value(feature, "operationType") {
        Some("NEW") => Ok("new_body"),
        Some("ADD") => Ok("join"),
        Some("REMOVE") => Ok("cut"),
        Some("INTERSECT") => Ok("intersect"),
        _ => Err("the Boolean operation is not supported yet".into()),
    }
}
fn owned_variable(
    feature: &Value,
    ordinal: usize,
    key: &str,
    source_kind: &str,
    dimension: super::expression::Dimension,
    model: &mut Model,
) -> Mapping<()> {
    let source = parameter(feature, key)?;
    let Some(expression) = source
        .get("expression")
        .and_then(Value::as_str)
        .filter(|e| e.contains('#'))
    else {
        return Ok(());
    };
    let evaluated = super::expression::evaluate(expression, dimension, model)?;
    let native = text(feature, "featureId")?;
    let kind = if dimension == super::expression::Dimension::Angle {
        "angle"
    } else {
        "length"
    };
    push(
        &mut model.parameters,
        json!({"id":id("parameter",&format!("{native}/{key}")),"owner":id("feature",native),"ordinal":0,
        "name":format!("{key}{ordinal}"),"expression":evaluated.expression,"value":{"kind":kind,"value":evaluated.value.abs()},
        "dependencies":evaluated.dependencies,"properties":{"source_kind":source_kind}}),
    )
}
pub(super) fn report_evaluated_variables(feature: &Value, model: &Model, report: &mut Report) {
    let Ok(native) = text(feature, "featureId") else {
        return;
    };
    let owner = id("feature", native);
    let name = feature
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(native);
    for row in rows(feature, "parameters").unwrap_or(&[]) {
        let (Some(key), Some(true)) = (
            row.get("parameterId").and_then(Value::as_str),
            row.get("expression")
                .and_then(Value::as_str)
                .map(|e| e.contains('#')),
        ) else {
            continue;
        };
        let unused = match (key, text(feature, "featureType")) {
            ("depth", Ok("extrude")) => value(feature, "endBound") != Some("BLIND"),
            ("angle", Ok("revolve")) => {
                value(feature, "revolveType") == Some("FULL") || enabled(feature, "fullRevolve")
            }
            _ => false,
        };
        let driven = unused
            || model.parameters.iter().any(|p| {
                p.owner.as_ref().is_some_and(|o| o.as_str() == owner)
                    && p.id.as_str() == id("parameter", &format!("{native}/{key}"))
            });
        if !driven {
            report.push_coded(
                name,
                crate::loss::OnshapeLossCode::VariableEvaluated,
                format!("{key} uses Part Studio variables; kept as its evaluated value, since driving this value from a parameter is not supported yet"),
            );
        }
    }
}

pub(super) fn map(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &mut Model,
    design: &mut cadmpeg_ir::document::Design,
    report: &mut Report,
) -> Mapping<Value> {
    if feature
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
    {
        return Err("custom FeatureScript features are not supported yet".into());
    }
    for key in [
        "hasSecondDirection",
        "hasDraft",
        "hasSecondDirectionDraft",
        "hasOffset",
        "isVariable",
        "isPartial",
        "allowEdgeOverflow",
    ] {
        if enabled(feature, key) {
            return Err(format!("{key} is not supported yet"));
        }
    }
    if enabled(feature, "symmetric") && text(feature, "featureType")? != "extrude" {
        return Err("symmetric extents for this feature are not supported yet".into());
    }
    let scoped = if parameter(feature, "booleanScope")
        .ok()
        .and_then(|p| p.get("queries"))
        .and_then(Value::as_array)
        .is_some_and(|queries| !queries.is_empty())
    {
        let state = inputs.load(ordinal, feature)?;
        state.whole_body_scope(feature)?;
        Some(state)
    } else {
        None
    };
    match text(feature, "featureType")? {
        "newSketch" => sketch::map(feature, ordinal, inputs, model, report),
        "assignVariable" => {
            if value(feature, "mode").is_some_and(|m| m != "ASSIGNED") {
                return Err("measured variables are not supported yet".into());
            }
            let name = value(feature, "name").ok_or("the variable name is missing")?;
            if model
                .parameters
                .iter()
                .any(|p| p.owner.is_none() && p.name == name)
            {
                return Err("variable reassignment is not supported yet".into());
            }
            let (key, kind, dimension) = match value(feature, "variableType") {
                Some("LENGTH") => (
                    "lengthValue",
                    "length",
                    super::expression::Dimension::Length,
                ),
                Some("ANGLE") => ("angleValue", "angle", super::expression::Dimension::Angle),
                Some("NUMBER") => ("numberValue", "real", super::expression::Dimension::Scalar),
                _ => return Err("this variable type is not supported yet".into()),
            };
            let source = parameter(feature, key)?;
            let expression = text(source, "expression")?;
            let evaluated = super::expression::evaluate(expression, dimension, model)?;
            let ordinal = model
                .parameters
                .iter()
                .filter(|p| p.owner.is_none())
                .count();
            let properties = evaluated.display_unit.as_ref().map_or_else(
                || json!({"retain_without_sketches":"true"}),
                |unit| json!({"display_unit": unit,"retain_without_sketches":"true"}),
            );
            push(
                &mut model.parameters,
                json!({"id":id("parameter",text(feature,"featureId")?),"ordinal":ordinal,"name":name,
                "expression":evaluated.expression,"value":{"kind":kind,"value":evaluated.value},"dependencies":evaluated.dependencies,"properties":properties}),
            )?;
            Ok(definition(
                "tree_node",
                json!({"role":"equations", "children":{"children":[]}}),
            ))
        }
        "extrude" => {
            if !matches!(value(feature, "bodyType"), Some("SOLID" | "THIN")) {
                return Err("non-solid extrudes are not supported yet".into());
            }
            let key = if value(feature, "bodyType") == Some("THIN")
                && rows(feature, "parameters")?
                    .iter()
                    .any(|p| p.get("parameterId").and_then(Value::as_str) == Some("wallShape"))
            {
                "wallShape"
            } else {
                "entities"
            };
            let profile = profile(feature, key, model, inputs)?;
            let profile = if value(feature, "bodyType") == Some("THIN") {
                super::thin::profile(profile, model)?
            } else {
                profile
            };
            if enabled(feature, "symmetric") && value(feature, "endBound") != Some("BLIND") {
                return Err("symmetric extrudes with this bound are not supported yet".into());
            }
            let termination = match value(feature, "endBound") {
                Some("BLIND") => {
                    json!({"kind": "blind", "length": length(feature, "depth", model)?})
                }
                Some("THROUGH_ALL") => json!({"kind": "through_all"}),
                Some("UP_TO_SURFACE") => {
                    let state = scoped.map_or_else(|| inputs.load(ordinal, feature), Ok)?;
                    let face = state.selection(feature, "endBoundEntityFace", "faces")?;
                    state.require_planes(&face)?;
                    push(&mut model.feature_input_topologies, state.row.clone())?;
                    json!({"kind": "to_face", "face": face})
                }
                Some("UP_TO_NEXT") => {
                    let state = scoped.map_or_else(|| inputs.load(ordinal, feature), Ok)?;
                    let (samples, normal) = profile_samples(&profile, model)?;
                    let travel = if enabled(feature, "oppositeDirection") {
                        normal.map(|n| -n)
                    } else {
                        normal
                    };
                    let face = state.next_face(&samples, travel, operation(feature)? == "cut")?;
                    state.require_planes(&face)?;
                    push(&mut model.feature_input_topologies, state.row.clone())?;
                    json!({"kind": "to_face", "face": face})
                }
                _ => return Err("this extrude bound is not supported yet".into()),
            };
            if value(feature, "endBound") == Some("BLIND") {
                owned_variable(
                    feature,
                    ordinal,
                    "depth",
                    if enabled(feature, "symmetric") {
                        "TotalDistance"
                    } else {
                        "AlongDistance"
                    },
                    super::expression::Dimension::Length,
                    model,
                )?;
            }
            let direction = if enabled(feature, "oppositeDirection") {
                json!({"kind": "reversed_profile_normal"})
            } else {
                json!({"kind": "profile_normal"})
            };
            let definition = definition(
                "extrude",
                json!({"profile": {"kind": "planar", "value": profile}, "direction": direction,
                "start": {"kind": "profile_plane"}, "extent": {"kind": if enabled(feature,"symmetric") {"symmetric"} else {"one_sided"}, "side": {"termination": termination}}, "op": operation(feature)?, "solid": true}),
            );
            if value(feature, "bodyType") == Some("THIN") {
                push(
                    &mut design.features,
                    super::thin::feature(feature, &definition, model)?,
                )?;
            }
            Ok(definition)
        }
        "revolve" => {
            if value(feature, "bodyType") != Some("SOLID") {
                return Err("non-solid revolves are not supported yet".into());
            }
            let profile = profile(feature, "entities", model, inputs)?;
            let axis_queries = queries(feature, "axis")?;
            let axis = if axis_queries.iter().any(|q| {
                super::query::resolved(q).is_ok_and(|q| {
                    q.get("queryType").and_then(Value::as_str) == Some("SKETCH_ENTITY")
                })
            }) {
                sketch_axis(feature, model)?
            } else {
                scoped
                    .map_or_else(|| inputs.load(ordinal, feature), Ok)?
                    .axis(feature, "axis")?
            };
            let angle = match value(feature, "revolveType") {
                Some("FULL") => std::f64::consts::TAU,
                Some("ONE_DIRECTION") => quantity(parameter(feature, "angle")?, true, model)?,
                None if enabled(feature, "fullRevolve") => std::f64::consts::TAU,
                None if parameter(feature, "fullRevolve")
                    .ok()
                    .and_then(|p| p.get("value"))
                    .and_then(Value::as_bool)
                    == Some(false) =>
                {
                    quantity(parameter(feature, "angle")?, true, model)?
                }
                _ => return Err("this revolve extent is not supported yet".into()),
            };
            if angle < std::f64::consts::TAU {
                owned_variable(
                    feature,
                    ordinal,
                    "angle",
                    "AlongAngle",
                    super::expression::Dimension::Angle,
                    model,
                )?;
            }
            let angle = if enabled(feature, "oppositeDirection") {
                -angle
            } else {
                angle
            };
            Ok(definition(
                "revolve",
                json!({"construction": {"kind": "resolved", "value": {"profile": profile, "axis": axis,
                "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": angle}}}}, "op": operation(feature)?}),
            ))
        }
        kind @ ("fillet" | "chamfer") => {
            if kind == "fillet"
                && (value(feature, "crossSection") != Some("CIRCULAR")
                    || value(feature, "filletType").is_some_and(|s| s != "EDGE"))
            {
                return Err("only circular constant-radius edge fillets are supported yet".into());
            }
            let state = scoped.map_or_else(|| inputs.load(ordinal, feature), Ok)?;
            let mut edges = state.selection(feature, "entities", "edges")?;
            if enabled(feature, "tangentPropagation") {
                edges = state.tangent_chain(&edges)?;
            }
            state.require_edges(&edges)?;
            let mut group = json!({"edges": edges});
            let (key, spec) = if kind == "fillet" {
                (
                    "radius",
                    json!({"kind": "constant", "radius": length(feature, "radius", model)?}),
                )
            } else {
                (
                    "spec",
                    match value(feature, "chamferType") {
                        Some("EQUAL_OFFSETS") => {
                            json!({"kind": "distance", "distance": length(feature, "width", model)?})
                        }
                        Some("TWO_OFFSETS") => {
                            json!({"kind": "two_distances", "first": length(feature, "width1", model)?, "second": length(feature, "width2", model)?})
                        }
                        Some("DISTANCE_ANGLE") => {
                            json!({"kind": "distance_angle", "distance": length(feature, "width", model)?, "angle": quantity(parameter(feature, "angle")?, true, model)?})
                        }
                        _ => return Err("this chamfer type is not supported yet".into()),
                    },
                )
            };
            group
                .as_object_mut()
                .ok_or("blend group is missing")?
                .insert(key.into(), spec);
            push(&mut model.feature_input_topologies, state.row)?;
            let operation = if kind == "chamfer" {
                json!({"groups": [group], "flip_direction": enabled(feature, "oppositeDirection")})
            } else {
                json!({"groups": [group]})
            };
            Ok(definition(kind, operation))
        }
        "hole" => hole(feature, ordinal, inputs, model),
        other => Err(format!("{other} features are not supported yet")),
    }
}
const DEFAULT_TIP_ANGLE_DEGREES: f64 = 118.0;

fn hole(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &mut Model,
) -> Mapping<Value> {
    if enabled(feature, "oppositeDirection") {
        return Err("reversed hole drilling is not supported yet".into());
    }
    if rows(feature, "parameters")?.iter().any(|p| {
        p.get("parameterId")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with("standard") && id != "standardTappedOrClearance")
    }) {
        return Err("this hole standard table is not supported yet".into());
    }
    let standard = parameter(feature, "standardTappedOrClearance")
        .ok()
        .and_then(|p| p.get("value"))
        .filter(|v| !v.is_null());
    let tapped = match standard.map(|s| s.get("type").and_then(Value::as_str)) {
        None => false,
        Some(Some("Tapped")) => true,
        Some(Some("Clearance")) => false,
        Some(_) => return Err("this hole standard type is not supported yet".into()),
    };
    if enabled(feature, "isTapped") && !tapped {
        return Err("a tapped hole without a thread standard is not supported yet".into());
    }
    let profile = profile(
        feature,
        if rows(feature, "parameters")?
            .iter()
            .any(|p| p.get("parameterId").and_then(Value::as_str) == Some("locations"))
        {
            "locations"
        } else {
            "entities"
        },
        model,
        inputs,
    )?;
    if parameter(feature, "scope")
        .ok()
        .and_then(|p| p.get("queries"))
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
    {
        inputs
            .load(ordinal, feature)?
            .whole_body_scope_key(feature, "scope")?;
    }
    let form = match value(feature, "style") {
        Some("SIMPLE") => json!({"kind":"simple"}),
        Some("C_BORE") => {
            json!({"kind":"counterbore","diameter":length(feature, "cBoreDiameter", model)?,"depth":length(feature, "cBoreDepth", model)?})
        }
        Some("C_SINK") => {
            json!({"kind":"countersink","diameter":length(feature, "cSinkDiameter", model)?,"angle":quantity(parameter(feature, "cSinkAngle")?, true, model)?})
        }
        _ => return Err("this hole style is not supported yet".into()),
    };
    let through = match value(feature, "endStyle") {
        Some("THROUGH" | "THROUGH_ALL") => true,
        Some("BLIND") => false,
        _ => return Err("this hole end is not supported yet".into()),
    };
    let diameter = if tapped && parameter(feature, "tapDrillDiameter").is_ok() {
        length(feature, "tapDrillDiameter", model)?
    } else {
        length(feature, "holeDiameter", model)?
    };
    let mut construction = json!({"kind": form});
    if let (Some(standard), Some(fields)) = (standard, construction.as_object_mut()) {
        fields.insert(
            "specification".into(),
            specification(feature, standard, tapped, through, model)?,
        );
    }
    let mut operation = json!({"profile": profile, "profile_filter": "points",
        "shape": {"diameter": diameter, "construction": {"kind": "form", "value": construction}}});
    let fields = operation
        .as_object_mut()
        .ok_or("the hole definition is missing")?;
    if through {
        fields.insert("extent".into(), json!({"kind": "through_all"}));
    } else {
        let tip = match parameter(feature, "tipAngle") {
            Ok(tip) => quantity(tip, true, model)?,
            Err(_) => DEFAULT_TIP_ANGLE_DEGREES.to_radians(),
        };
        let bottom = if (tip - std::f64::consts::PI).abs() < DRILL_TIP_ANGLE_TOLERANCE {
            json!({"kind": "flat"})
        } else if tip > 0.0 && tip < std::f64::consts::PI {
            json!({"kind": "angled", "included_angle": tip, "depth_to_tip": false})
        } else {
            return Err("this drill point angle is not supported yet".into());
        };
        fields.insert(
            "extent".into(),
            json!({"kind": "blind", "length": length(feature, "holeDepth", model)?}),
        );
        fields.insert("bottom".into(), bottom);
    }
    Ok(definition("hole", operation))
}
fn specification(
    feature: &Value,
    standard: &Value,
    tapped: bool,
    through: bool,
    model: &Model,
) -> Mapping<Value> {
    let name = standard
        .get("standard")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("the hole standard is missing")?;
    let size = standard
        .get("size")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if !tapped {
        let mut clearance = json!({"kind": "clearance", "standard": name, "modeled": false,
            "cosmetic": false, "hand": "right", "depth": {"kind": "hole_depth"}});
        let fields = clearance
            .as_object_mut()
            .ok_or("the hole clearance is missing")?;
        if let Some(size) = size {
            fields.insert("designation".into(), json!(size));
        }
        if let Some(fit) = standard
            .get("fit")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            fields.insert("fit".into(), json!(fit));
        }
        return Ok(clearance);
    }
    let depth = if through || enabled(feature, "isTappedThrough") {
        json!({"kind": "hole_depth"})
    } else {
        json!({"kind": "blind", "depth": length(feature, "tappedDepth", model)?})
    };
    let mut threaded = json!({"kind": "threaded", "standard": name, "modeled": false,
        "cosmetic": true, "hand": "right", "depth": depth});
    let fields = threaded
        .as_object_mut()
        .ok_or("the hole thread is missing")?;
    if let Some(size) = size {
        fields.insert("designation".into(), json!(size));
    }
    if let Some(pitch) = standard.get("pitch").and_then(Value::as_str) {
        let mut parts = pitch.split_whitespace();
        if let (Some(Ok(n)), Some(unit), None) = (
            parts.next().map(str::parse::<f64>),
            parts.next(),
            parts.next(),
        ) {
            let pitch = match unit {
                "mm" => Some(n),
                "tpi" if n > 0.0 => Some(25.4 / n),
                _ => None,
            };
            if let Some(pitch) = pitch.filter(|p| p.is_finite() && *p > 0.0) {
                fields.insert("pitch".into(), json!(pitch));
            }
        }
    }
    if parameter(feature, "majorDiameter").is_ok() {
        fields.insert(
            "major_diameter".into(),
            json!(length(feature, "majorDiameter", model)?),
        );
    }
    Ok(threaded)
}
