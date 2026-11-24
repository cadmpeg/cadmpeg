// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::document::Model;
use serde_json::{json, Value};
use std::collections::BTreeSet;

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
pub(super) fn source_properties(feature: &Value, definition: &Value) -> Mapping<Value> {
    Ok(
        match definition.pointer("/value/kind").and_then(Value::as_str) {
            Some("fillet" | "chamfer") => json!({"tangent_propagation":
            if enabled(feature, "tangentPropagation")? { "resolved" } else { "off" }}),
            _ => json!({}),
        },
    )
}

pub(super) fn enabled(feature: &Value, key: &str) -> Mapping<bool> {
    if !rows(feature, "parameters")?
        .iter()
        .any(|p| p.get("parameterId").and_then(Value::as_str) == Some(key))
    {
        return Ok(false);
    }
    required_boolean(feature, key)
}
pub(super) fn required_boolean(feature: &Value, key: &str) -> Mapping<bool> {
    parameter(feature, key)?
        .get("value")
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("parameter {key} must be Boolean"))
}
pub(super) fn quantity(
    parameter: &Value,
    angle: bool,
    model: &Model,
    unresolved: &BTreeSet<String>,
) -> Mapping<f64> {
    fn scale(unit: &str, angle: bool) -> Option<f64> {
        match (angle, unit) {
            (true, "radian" | "rad") => Some(1.0),
            (true, "degree" | "deg") => Some(std::f64::consts::PI / 180.0),
            (false, "meter" | "m") => Some(1000.0),
            (false, "millimeter" | "mm") => Some(1.0),
            (false, "centimeter" | "cm") => Some(10.0),
            (false, "inch" | "in") => Some(25.4),
            (false, "foot" | "ft") => Some(304.8),
            _ => None,
        }
    }
    if let Some(expression) = parameter.get("expression") {
        let expression = expression
            .as_str()
            .ok_or("quantity expression must be a string")?;
        let evaluated = super::expression::evaluate(
            expression,
            if angle {
                super::expression::Dimension::Angle
            } else {
                super::expression::Dimension::Length
            },
            model,
            unresolved,
        );
        if let Ok(value) = &evaluated {
            return Ok(value.value);
        }
        if let Some((scalar, unit)) = expression.trim().rsplit_once(char::is_whitespace) {
            if let Some(factor) = scale(unit, angle) {
                if let Ok(value) = super::expression::evaluate(
                    scalar,
                    super::expression::Dimension::Scalar,
                    model,
                    unresolved,
                ) {
                    let result = value.value * factor;
                    if result.is_finite() {
                        return Ok(result);
                    }
                }
            }
        }
        if expression.contains('#') {
            return evaluated.map(|value| value.value);
        }
    }
    let factor =
        scale(text(parameter, "units")?, angle).ok_or("quantity units are not supported yet")?;
    let result = number(parameter, "value")? * factor;
    if result.is_finite() {
        Ok(result)
    } else {
        Err("quantity exceeds the finite range".into())
    }
}

fn length(
    feature: &Value,
    key: &str,
    model: &Model,
    unresolved: &BTreeSet<String>,
) -> Mapping<f64> {
    let n = quantity(parameter(feature, key)?, false, model, unresolved)
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

fn present_scope(feature: &Value, key: &str) -> Mapping<bool> {
    if rows(feature, "parameters")?
        .iter()
        .any(|parameter| parameter.get("parameterId").and_then(Value::as_str) == Some(key))
    {
        Ok(!rows(parameter(feature, key)?, "queries")?.is_empty())
    } else {
        Ok(false)
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
        && !whole_sketch_region_query(query)
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
    let sketch_id = id("sketch", text(query, "featureId")?);
    if !model.sketches.iter().any(|s| s.id.as_str() == sketch_id) {
        return Err("the selected sketch has no resolved geometry and plane".into());
    }
    if model.sketch_entities.iter().any(|e| {
        e.sketch.as_str() == sketch_id
            && matches!(
                e.geometry.definition(),
                cadmpeg_ir::sketches::SketchGeometryDefinition::Native { .. }
            )
    }) {
        return Err("the selected sketch contains unresolved geometry".into());
    }
    if super::boolean_field(query, "filterInnerLoops")? {
        if query.get("btType").and_then(Value::as_str) != Some("BTMIndividualSketchRegionQuery-140")
        {
            return Err("filtered inner sketch regions are not supported yet".into());
        }
        return super::regions::filtered_profile(&sketch_id, model, inputs);
    }
    Ok(json!({"kind": "sketch", "value": sketch_id}))
}
/// A sketch-region query whose query string selects every region of its sketch. Its
/// `deterministicIds` are the regions that string resolved to at the last regeneration, so the
/// string, not the list, is the selection.
fn whole_sketch_region_query(query: &Value) -> bool {
    let (Some("BTMIndividualSketchRegionQuery-140"), Some(sketch), Some(string)) = (
        query.get("btType").and_then(Value::as_str),
        query.get("featureId").and_then(Value::as_str),
        query.get("queryString").and_then(Value::as_str),
    ) else {
        return false;
    };
    let inner = match query.get("filterInnerLoops").and_then(Value::as_bool) {
        Some(true) => "true",
        Some(false) | None => "false",
    };
    let expected = format!("query=qSketchRegion(id+\"{sketch}\",{inner});");
    let stripped: String = string.chars().filter(|c| !c.is_whitespace()).collect();
    // The literal keeps its exact text, so ids that differ only in whitespace stay distinct.
    string.contains(&format!("\"{sketch}\""))
        && stripped == expected.replace(char::is_whitespace, "")
}
fn sketch_entities(queries: &[Value], model: &Model, entity_type: &str) -> Mapping<Value> {
    sketch_entities_in_order(queries, model, entity_type, false)
}

pub(super) fn sketch_entities_in_order(
    queries: &[Value],
    model: &Model,
    entity_type: &str,
    source_order: bool,
) -> Mapping<Value> {
    let mut selected = std::collections::BTreeSet::new();
    let mut ordered = Vec::new();
    let mut selected_sketch = None;
    for source in queries {
        for query in super::query::resolved_leaves(source)? {
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
            if !selected.insert(entity_id.clone()) {
                return Err("the sketch selection repeats an entity".into());
            }
            ordered.push(entity_id);
        }
    }
    if !source_order {
        ordered = selected.into_iter().collect();
    }
    Ok(
        json!({"kind":"sketch_entities","value":{"sketch":selected_sketch.ok_or("the sketch selection is empty")?,"entities":ordered}}),
    )
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

pub(super) fn operation(feature: &Value) -> Mapping<&'static str> {
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
    unresolved: &BTreeSet<String>,
) -> Mapping<()> {
    let source = parameter(feature, key)?;
    let Some(expression) = source
        .get("expression")
        .and_then(Value::as_str)
        .filter(|e| e.contains('#'))
    else {
        return Ok(());
    };
    let evaluated = super::expression::evaluate(expression, dimension, model, unresolved)?;
    let native = text(feature, "featureId")?;
    let kind = if dimension == super::expression::Dimension::Angle {
        "angle"
    } else {
        "length"
    };
    push(
        &mut model.parameters,
        json!({"id":id("parameter",&format!("{native}/{key}")),"owner":id("feature",native),"ordinal":0,
        "name":format!("{key}{ordinal}"),"expression":evaluated.expression,"value":{"kind":kind,"value":evaluated.value},
        "dependencies":evaluated.dependencies,"properties":{"source_kind":source_kind}}),
    )
}
pub(super) fn report_evaluated_variables(
    feature: &Value,
    model: &Model,
    report: &mut Report,
) -> Mapping<()> {
    let native = text(feature, "featureId")?;
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
                value(feature, "revolveType") == Some("FULL") || enabled(feature, "fullRevolve")?
            }
            ("turns" | "angle" | "pitch", Ok("sweep")) => !enabled(feature, "hasTwist")?,
            ("scaleFactor", Ok("sweep")) => !enabled(feature, "hasScale")?,
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
    Ok(())
}

pub(super) fn dependencies(
    source: &Value,
    definition: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &Model,
) -> Mapping<Vec<String>> {
    let mut result = Vec::new();
    if definition.pointer("/value/kind").and_then(Value::as_str) == Some("native") {
        return Ok(result);
    }
    let operation = definition.pointer("/value/kind").and_then(Value::as_str);
    if operation == Some("sketch") && sketch::built_in_frame(source)
        || operation == Some("datum_offset_plane") && super::planes::built_in(source)
    {
        return Ok(result);
    }
    let reference = |native: &str| -> Mapping<String> {
        let feature = id("feature", native);
        if model.features.iter().any(|f| f.id.as_str() == feature) {
            return Ok(feature);
        }
        Err(format!(
            "feature dependency {native:?} is unresolved before this feature"
        ))
    };
    let parameters = rows(source, "parameters")?;
    let hole_profile = if parameters
        .iter()
        .any(|p| p.get("parameterId").and_then(Value::as_str) == Some("locations"))
    {
        "locations"
    } else {
        "entities"
    };
    for parameter in parameters {
        let key = text(parameter, "parameterId")?;
        if operation == Some("extrude")
            && key == "endBound"
            && value(source, "endBound") == Some("UP_TO_NEXT")
        {
            let faces = definition
                .pointer("/value/value/extent/side/termination/face/value/faces")
                .and_then(Value::as_array)
                .ok_or("UP_TO_NEXT producer selection is missing")?;
            for face in faces {
                let producer = inputs.producer(
                    ordinal,
                    face.as_str().ok_or("UP_TO_NEXT target id is missing")?,
                    topology::OperandKind::Face,
                )?;
                let feature = reference(&producer)?;
                if !result.contains(&feature) {
                    result.push(feature);
                }
            }
            continue;
        }
        let consumed = match operation {
            Some("datum_offset_plane") => key == "entities",
            Some("sketch") => key == "sketchPlane",
            Some("extrude") => {
                key == if value(source, "bodyType") == Some("THIN") {
                    "wallShape"
                } else {
                    "entities"
                } || key == "endBoundEntityFace"
                    && value(source, "endBound") == Some("UP_TO_SURFACE")
                    || key == "booleanScope" && value(source, "operationType") != Some("NEW")
            }
            Some("revolve") => {
                matches!(key, "entities" | "axis")
                    || key == "booleanScope" && value(source, "operationType") != Some("NEW")
            }
            Some("sweep") => {
                matches!(key, "profiles" | "path")
                    || key == "booleanScope" && value(source, "operationType") != Some("NEW")
            }
            Some("fillet" | "chamfer" | "shell") => key == "entities",
            Some("hole") => key == "scope" || key == hole_profile,
            _ => false,
        };
        if !consumed {
            continue;
        }
        if operation == Some("sweep") && key == "path" {
            for source in rows(parameter, "queries")? {
                for query in super::query::resolved_leaves(source)? {
                    for operation in rows(&query, "operationId")? {
                        let native = operation
                            .as_str()
                            .and_then(|operation| operation.strip_suffix(".wireOp"))
                            .ok_or("the sweep path producer is unresolved")?;
                        let feature = reference(native)?;
                        if !result.contains(&feature) {
                            result.push(feature);
                        }
                    }
                }
            }
            continue;
        }
        for source in rows(parameter, "queries")? {
            let mut query = source.clone();
            if source
                .get("queryString")
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("qCompressed("))
            {
                let resolved = super::query::resolved(source)?;
                for (key, value) in resolved
                    .as_object()
                    .ok_or("dependency query is not an object")?
                {
                    if query.get(key).is_some_and(|outer| outer != value) {
                        return Err("feature dependency query fields contradict each other".into());
                    }
                    query[key] = value.clone();
                }
            }
            let mut explicit = Vec::new();
            if query.get("featureId").is_some() {
                explicit.push(text(&query, "featureId")?.to_owned());
            }
            if query.get("operationId").is_some() {
                for operation in rows(&query, "operationId")? {
                    let (native, _) = operation
                        .as_str()
                        .and_then(|operation| operation.rsplit_once('.'))
                        .ok_or("feature dependency operation is unresolved")?;
                    explicit.push(native.to_owned());
                }
            }
            let historical = match (operation, key) {
                (Some("extrude" | "revolve" | "sweep"), "booleanScope")
                | (Some("hole"), "scope") => Some(topology::OperandKind::Body),
                (Some("datum_offset_plane"), "entities")
                | (Some("sketch"), "sketchPlane")
                | (Some("extrude"), "endBoundEntityFace") => Some(topology::OperandKind::Face),
                (Some("fillet" | "chamfer"), "entities") => Some(topology::OperandKind::Edge),
                (Some("shell"), "entities") => Some(topology::OperandKind::Face),
                (Some("revolve"), "axis")
                    if query.get("queryType").and_then(Value::as_str) != Some("SKETCH_ENTITY") =>
                {
                    Some(topology::OperandKind::Edge)
                }
                _ => None,
            };
            let mut producers = Vec::new();
            if let Some(kind) = historical.filter(|_| {
                query.get("deterministicIds").is_some()
                    && !explicit
                        .iter()
                        .any(|native| super::planes::created(model, native).is_ok())
            }) {
                let ids = rows(&query, "deterministicIds")?;
                if ids.is_empty() {
                    return Err("feature dependency selection is empty".into());
                }
                for native in ids {
                    let producer = inputs.producer(
                        ordinal,
                        native
                            .as_str()
                            .ok_or("feature dependency deterministic id must be a string")?,
                        kind,
                    )?;
                    if explicit.iter().any(|named| named != &producer) {
                        return Err(
                            "feature dependency explicit producer contradicts stored topology"
                                .into(),
                        );
                    }
                    producers.push(producer);
                }
            } else {
                producers = explicit;
            }
            if matches!(operation, Some("fillet" | "chamfer")) {
                let groups = definition
                    .pointer("/value/value/groups")
                    .and_then(Value::as_array)
                    .ok_or("feature dependency blend groups are missing")?;
                for group in groups {
                    let edges = group
                        .pointer("/edges/value/edges")
                        .and_then(Value::as_array)
                        .ok_or("feature dependency final edge selection is missing")?;
                    for edge in edges {
                        let producer = inputs.producer(
                            ordinal,
                            edge.as_str().ok_or("selected edge id is missing")?,
                            topology::OperandKind::Edge,
                        )?;
                        if !producers.contains(&producer) {
                            producers.push(producer);
                        }
                    }
                }
            }
            if producers.is_empty() {
                return Err("feature dependency has no resolved producer".into());
            }
            for producer in producers {
                let feature = reference(&producer)?;
                if !result.contains(&feature) {
                    result.push(feature);
                }
            }
        }
    }
    Ok(result)
}

pub(super) fn map(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &mut Model,
    report: &mut Report,
    unresolved: &BTreeSet<String>,
) -> Mapping<Value> {
    if feature.get("featureType").and_then(Value::as_str) == Some("sweep") {
        super::sweep::validate_record(feature)?;
    } else {
        let mut keys = std::collections::BTreeSet::new();
        for parameter in rows(feature, "parameters")? {
            if !keys.insert(text(parameter, "parameterId")?) {
                return Err("feature parameter keys repeat".into());
            }
        }
    }
    for key in [
        "hasSecondDirection",
        "hasDraft",
        "hasSecondDirectionDraft",
        "hasOffset",
        "isVariable",
        "isPartial",
        "allowEdgeOverflow",
        "symmetric",
        "oppositeDirection",
        "fullRevolve",
        "tangentPropagation",
        "midplane",
        "flipWall",
        "isTapped",
        "isTappedThrough",
        "depthToTip",
    ] {
        enabled(feature, key)?;
    }
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
        if enabled(feature, key)? {
            return Err(format!("{key} is not supported yet"));
        }
    }
    if enabled(feature, "symmetric")? && text(feature, "featureType")? != "extrude" {
        return Err("symmetric extents for this feature are not supported yet".into());
    }
    let scoped = if present_scope(feature, "booleanScope")? {
        let state = inputs.load(ordinal, feature)?;
        state.whole_body_scope(feature)?;
        Some(state)
    } else {
        None
    };
    match text(feature, "featureType")? {
        "cPlane" => super::planes::map(feature, ordinal, inputs, model, unresolved),
        "newSketch" => sketch::map(feature, ordinal, inputs, model, report, unresolved),
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
            let evaluated = super::expression::evaluate(expression, dimension, model, unresolved)?;
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
            let thin = value(feature, "bodyType") == Some("THIN");
            if !matches!(value(feature, "bodyType"), Some("SOLID" | "THIN")) {
                return Err("non-solid extrudes are not supported yet".into());
            }
            let profile = profile(
                feature,
                if thin { "wallShape" } else { "entities" },
                model,
                inputs,
            )?;
            let profile = if thin {
                super::thin::profile(profile, model)?
            } else {
                profile
            };
            if enabled(feature, "symmetric")? && value(feature, "endBound") != Some("BLIND") {
                return Err("symmetric extrudes with this bound are not supported yet".into());
            }
            let termination = match value(feature, "endBound") {
                Some("BLIND") => {
                    json!({"kind": "blind", "length": length(feature, "depth", model, unresolved)?})
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
                    let face = state.next_face(feature, &profile, model)?;
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
                    if enabled(feature, "symmetric")? {
                        "TotalDistance"
                    } else {
                        "AlongDistance"
                    },
                    super::expression::Dimension::Length,
                    model,
                    unresolved,
                )?;
            }
            let direction = if enabled(feature, "oppositeDirection")? {
                json!({"kind": "reversed_profile_normal"})
            } else {
                json!({"kind": "profile_normal"})
            };
            let definition = definition(
                "extrude",
                json!({"profile": {"kind": "planar", "value": profile}, "direction": direction,
                "start": {"kind": "profile_plane"}, "extent": {"kind": if enabled(feature,"symmetric")? {"symmetric"} else {"one_sided"}, "side": {"termination": termination}}, "op": operation(feature)?, "solid": true}),
            );
            Ok(definition)
        }
        "sweep" => super::sweep::map(feature, model, inputs),
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
                Some("ONE_DIRECTION") => {
                    quantity(parameter(feature, "angle")?, true, model, unresolved)?
                }
                None if enabled(feature, "fullRevolve")? => std::f64::consts::TAU,
                None if parameter(feature, "fullRevolve")
                    .ok()
                    .and_then(|p| p.get("value"))
                    .and_then(Value::as_bool)
                    == Some(false) =>
                {
                    quantity(parameter(feature, "angle")?, true, model, unresolved)?
                }
                _ => return Err("this revolve extent is not supported yet".into()),
            };
            if angle < 0.0 {
                return Err(
                    "the direction convention for a negative revolve angle is unresolved".into(),
                );
            }
            if angle < std::f64::consts::TAU {
                owned_variable(
                    feature,
                    ordinal,
                    "angle",
                    "AlongAngle",
                    super::expression::Dimension::Angle,
                    model,
                    unresolved,
                )?;
            }
            let angle = if enabled(feature, "oppositeDirection")? {
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
            if enabled(feature, "tangentPropagation")? {
                edges = state.tangent_chain(&edges)?;
            }
            state.require_edges(&edges)?;
            let mut group = json!({"edges": edges});
            let (key, spec) = if kind == "fillet" {
                (
                    "radius",
                    json!({"kind": "constant", "radius": length(feature, "radius", model, unresolved)?}),
                )
            } else {
                (
                    "spec",
                    match value(feature, "chamferType") {
                        Some("EQUAL_OFFSETS") => {
                            json!({"kind": "distance", "distance": length(feature, "width", model, unresolved)?})
                        }
                        Some("TWO_OFFSETS") => {
                            json!({"kind": "two_distances", "first": length(feature, "width1", model, unresolved)?, "second": length(feature, "width2", model, unresolved)?})
                        }
                        Some("DISTANCE_ANGLE") => {
                            json!({"kind": "distance_angle", "distance": length(feature, "width", model, unresolved)?, "angle": quantity(parameter(feature, "angle")?, true, model, unresolved)?})
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
                json!({"groups": [group], "flip_direction": enabled(feature, "oppositeDirection")?})
            } else {
                json!({"groups": [group]})
            };
            Ok(definition(kind, operation))
        }
        "hole" => hole(feature, ordinal, inputs, model, report, unresolved),
        "shell" => {
            let state = scoped.map_or_else(|| inputs.load(ordinal, feature), Ok)?;
            shell(feature, ordinal, &state, model, unresolved)
        }
        other => Err(format!("{other} features are not supported yet")),
    }
}

/// Refusal prefix for a shell control outside the stored and specified forms.
pub(super) const SHELL_REFUSAL: &str = "shell: ";

fn shell_refusal(reason: &str) -> String {
    format!("{SHELL_REFUSAL}{reason}")
}

/// Shell parameters named by the Onshape `shell` feature specification.
const SHELL_PARAMETERS: [&str; 5] = [
    "isHollow",
    "entities",
    "parts",
    "thickness",
    "oppositeDirection",
];

/// Whether every parameter key of a shell record is a specified key.
pub(super) fn shell_record_valid(feature: &Value) -> bool {
    rows(feature, "parameters").is_ok_and(|rows| {
        rows.iter().all(|row| {
            row.get("parameterId")
                .and_then(Value::as_str)
                .is_some_and(|key| SHELL_PARAMETERS.contains(&key))
        })
    })
}

/// Maps a face-removal shell. A stored `oppositeDirection` of `false` is the
/// inward wall; the opposite sense and the hollow form carry no stored case.
fn shell(
    feature: &Value,
    ordinal: usize,
    state: &topology::State,
    model: &mut Model,
    unresolved: &BTreeSet<String>,
) -> Mapping<Value> {
    if !shell_record_valid(feature) {
        return Err(shell_refusal(
            "a parameter is outside the shell specification",
        ));
    }
    if enabled(feature, "isHollow")? {
        return Err(shell_refusal("hollow shells are not supported yet"));
    }
    // Any `parts` row refuses, a repeated one too (`parameter` reports a repeat as an error).
    if rows(feature, "parameters")?
        .iter()
        .any(|p| p.get("parameterId").and_then(Value::as_str) == Some("parts"))
    {
        return Err(shell_refusal(
            "a hollow part selection on a face-removal shell is not supported yet",
        ));
    }
    if enabled(feature, "oppositeDirection")? {
        return Err(shell_refusal(
            "outward walls are not supported yet (ONSHAPE-030)",
        ));
    }
    let faces = state.selection(feature, "entities", "faces")?;
    if faces
        .pointer("/value/faces")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(shell_refusal("no faces to remove are selected"));
    }
    let thickness = length(feature, "thickness", model, unresolved)?;
    owned_variable(
        feature,
        ordinal,
        "thickness",
        "Thickness",
        super::expression::Dimension::Length,
        model,
        unresolved,
    )?;
    push(&mut model.feature_input_topologies, state.row.clone())?;
    Ok(definition(
        "shell",
        json!({"removed_faces": faces, "thickness": thickness, "outward": false}),
    ))
}

fn hole(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &mut Model,
    report: &mut Report,
    unresolved: &BTreeSet<String>,
) -> Mapping<Value> {
    if enabled(feature, "oppositeDirection")? {
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
    if enabled(feature, "isTapped")? && !tapped {
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
    if present_scope(feature, "scope")? {
        inputs
            .load(ordinal, feature)?
            .whole_body_scope_key(feature, "scope")?;
    }
    let form = match value(feature, "style") {
        Some("SIMPLE") => json!({"kind":"simple"}),
        Some("C_BORE") => {
            json!({"kind":"counterbore","diameter":length(feature, "cBoreDiameter", model, unresolved)?,"depth":length(feature, "cBoreDepth", model, unresolved)?})
        }
        Some("C_SINK") => {
            json!({"kind":"countersink","diameter":length(feature, "cSinkDiameter", model, unresolved)?,"angle":quantity(parameter(feature, "cSinkAngle")?, true, model, unresolved)?})
        }
        _ => return Err("this hole style is not supported yet".into()),
    };
    let through = match value(feature, "endStyle") {
        Some("THROUGH" | "THROUGH_ALL") => true,
        Some("BLIND") => false,
        _ => return Err("this hole end is not supported yet".into()),
    };
    let diameter = if tapped && parameter(feature, "tapDrillDiameter").is_ok() {
        length(feature, "tapDrillDiameter", model, unresolved)?
    } else {
        length(feature, "holeDiameter", model, unresolved)?
    };
    let mut construction = json!({"kind": form});
    if let (Some(standard), Some(fields)) = (standard, construction.as_object_mut()) {
        fields.insert(
            "specification".into(),
            specification(feature, standard, tapped, model, report, unresolved)?,
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
        let tip = quantity(parameter(feature, "tipAngle")?, true, model, unresolved)?;
        let bottom = if (tip - std::f64::consts::PI).abs() < DRILL_TIP_ANGLE_TOLERANCE {
            json!({"kind": "flat"})
        } else if tip > 0.0 && tip < std::f64::consts::PI {
            json!({"kind": "angled", "included_angle": tip, "depth_to_tip": required_boolean(feature, "depthToTip")?})
        } else {
            return Err("this drill point angle is not supported yet".into());
        };
        fields.insert(
            "extent".into(),
            json!({"kind": "blind", "length": length(feature, "holeDepth", model, unresolved)?}),
        );
        fields.insert("bottom".into(), bottom);
    }
    Ok(definition("hole", operation))
}
fn specification(
    feature: &Value,
    standard: &Value,
    tapped: bool,
    model: &Model,
    report: &mut Report,
    unresolved: &BTreeSet<String>,
) -> Mapping<Value> {
    let name = standard
        .get("standard")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or("the hole standard is missing")?;
    let hand = text(standard, "hand")?;
    if !matches!(hand, "left" | "right") {
        return Err("thread hand is unresolved".into());
    }
    let modeled = super::boolean_field(standard, "modeled")?;
    let cosmetic = super::boolean_field(standard, "cosmetic")?;
    if standard.get("modeled").is_none() || standard.get("cosmetic").is_none() {
        return Err("hole standard modeled and cosmetic facts are unresolved".into());
    }
    let size = standard
        .get("size")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if !tapped {
        let mut clearance = json!({"kind": "clearance", "standard": name, "hand": hand, "modeled": modeled, "cosmetic": cosmetic, "depth": {"kind": "hole_depth"}});
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
    let depth = if enabled(feature, "isTappedThrough")? {
        json!({"kind": "hole_depth"})
    } else {
        json!({"kind": "blind", "depth": length(feature, "tappedDepth", model, unresolved)?})
    };
    let mut threaded = json!({"kind": "threaded", "standard": name, "hand": hand, "modeled": modeled, "cosmetic": cosmetic, "depth": depth});
    let fields = threaded
        .as_object_mut()
        .ok_or("the hole thread is missing")?;
    if let Some(size) = size {
        fields.insert("designation".into(), json!(size));
    }
    if let Some(source_pitch) = standard.get("pitch") {
        let pitch = source_pitch.as_str().and_then(|pitch| {
            let mut parts = pitch.split_whitespace();
            let (Some(Ok(n)), Some(unit), None) = (
                parts.next().map(str::parse::<f64>),
                parts.next(),
                parts.next(),
            ) else {
                return None;
            };
            match unit {
                "mm" => Some(n),
                "in" => Some(n * 25.4),
                "tpi" if n > 0.0 => Some(25.4 / n),
                _ => None,
            }
        });
        if let Some(pitch) = pitch.filter(|p| p.is_finite() && *p > 0.0) {
            fields.insert("pitch".into(), json!(pitch));
        } else {
            report.push_coded(
                text(feature, "featureId")?,
                crate::loss::OnshapeLossCode::RecordMalformed,
                format!("thread pitch {source_pitch} is unresolved; kept the thread without pitch"),
            );
        }
    }
    if parameter(feature, "majorDiameter").is_ok() {
        fields.insert(
            "major_diameter".into(),
            json!(length(feature, "majorDiameter", model, unresolved)?),
        );
    }
    Ok(threaded)
}

#[cfg(test)]
mod tests;
