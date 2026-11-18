// SPDX-License-Identifier: Apache-2.0
const SPLINE_DOMAIN_TOLERANCE: f64 = 1e-9;
const POINT_U_TOLERANCE: f64 = 1e-6;
const POINT_V_TOLERANCE: f64 = 1e-6;
use std::collections::BTreeMap;

use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::Report;
use super::{
    coordinate, definition,
    features::{parameter, quantity, queries},
    id, number, optional_rows, push, query as compressed, rows, text, topology, Mapping,
};
const ORIGIN_ID: &str = "IB";
fn frame(feature: &Value, model: &Model, face: &dyn Fn(&str) -> Mapping<Value>) -> Mapping<Value> {
    let [query] = queries(feature, "sketchPlane")? else {
        return Err("multiple sketch planes are not supported yet".into());
    };
    let deterministic = query
        .get("deterministicIds")
        .and_then(Value::as_array)
        .filter(|ids| !ids.is_empty());
    let face_frame = || -> Mapping<Value> {
        let [native] = deterministic.map(Vec::as_slice).unwrap_or_default() else {
            return Err("a sketch on more than one face is not supported yet".into());
        };
        face(native.as_str().ok_or("a sketch face id is not a string")?)
    };
    let plane = if query
        .get("queryString")
        .and_then(Value::as_str)
        .is_some_and(|s| s.contains("qCompressed("))
    {
        let resolved = compressed::resolved(query)?;
        if resolved.get("queryType").and_then(Value::as_str) == Some("CAP_FACE") {
            return cap_frame(&resolved, model).or_else(|reason| {
                if deterministic.is_some() {
                    face_frame()
                } else {
                    Err(reason)
                }
            });
        }
        if resolved.get("queryType").and_then(Value::as_str) != Some("DUMMY") {
            if resolved.get("entityType").and_then(Value::as_str) != Some("FACE") {
                return Err("the sketch plane query does not select a face".into());
            }
            return face_frame();
        }
        let [operation] = rows(&resolved, "operationId")? else {
            return Err("the sketch plane has multiple source operations".into());
        };
        if resolved.get("entityType").and_then(Value::as_str) != Some("FACE")
            || resolved.get("historyType").and_then(Value::as_str) != Some("CREATION")
            || resolved.get("queryType").and_then(Value::as_str) != Some("DUMMY")
            || resolved.get("disambiguationData").is_some()
        {
            return Err("this sketch-plane query has no resolved in-plane frame".into());
        }
        operation
            .as_str()
            .and_then(|s| s.strip_suffix(".planeOp"))
            .ok_or("this sketch plane has no resolved in-plane frame")?
            .to_owned()
    } else if deterministic.is_some() {
        return face_frame();
    } else if let Some(feature_id) = query.get("featureId").and_then(Value::as_str) {
        feature_id.to_owned()
    } else {
        let source_query = text(query, "queryString")?;
        let query_string = source_query
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        ["Top", "Front", "Right"]
            .into_iter()
            .find(|name| {
                source_query.contains(&format!("\"{name}\""))
                    && query_string.trim_end_matches(';')
                        == format!("qCreatedBy(makeId(\"{name}\"),EntityType.FACE)")
            })
            .ok_or("the sketch plane query is not supported yet")?
            .to_owned()
    };
    let (normal, u_axis) = match plane.as_str() {
        "Top" => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        "Front" => ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
        "Right" => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        _ => return Err("this sketch plane has no resolved in-plane frame".into()),
    };
    Ok(
        json!({"origin": topology::point([0.0; 3]), "normal": topology::point(normal), "u_axis": topology::point(u_axis)}),
    )
}
fn cap_frame(query: &Value, model: &Model) -> Mapping<Value> {
    let reason = "this cap-face sketch attachment is not supported yet";
    let [operation] = rows(query, "operationId")? else {
        return Err(reason.into());
    };
    let native = operation
        .as_str()
        .and_then(|s| s.strip_suffix(".opExtrude"))
        .ok_or(reason)?;
    if text(query, "entityType")? != "FACE" || text(query, "historyType")? != "CREATION" {
        return Err(reason.into());
    }
    let is_start = query
        .get("isStart")
        .and_then(Value::as_bool)
        .ok_or(reason)?;
    let feature = model
        .features
        .iter()
        .find(|f| f.id.as_str() == id("feature", native))
        .ok_or(reason)?;
    let wire = serde_json::to_value(feature).map_err(|_| reason)?;
    let extrude = wire.pointer("/definition/value/value").ok_or(reason)?;
    if wire
        .pointer("/definition/value/kind")
        .and_then(Value::as_str)
        != Some("extrude")
    {
        return Err(reason.into());
    }
    let profile = extrude.pointer("/profile/value").ok_or(reason)?;
    let sketch_id = match profile.get("kind").and_then(Value::as_str) {
        Some("sketch") => profile.get("value").and_then(Value::as_str),
        Some("sketch_regions") => profile.pointer("/value/sketch").and_then(Value::as_str),
        _ => None,
    }
    .ok_or(reason)?;
    let sketch = model
        .sketches
        .iter()
        .find(|s| s.id.as_str() == sketch_id)
        .ok_or(reason)?;
    let sketch = serde_json::to_value(sketch).map_err(|_| reason)?;
    let frame = sketch.pointer("/placement/frame").ok_or(reason)?;
    let origin = topology::vector(frame.get("origin").ok_or(reason)?, 1.0)?;
    let normal = topology::unit(topology::vector(frame.get("normal").ok_or(reason)?, 1.0)?)?;
    let sign = match extrude.pointer("/direction/kind").and_then(Value::as_str) {
        Some("profile_normal") => 1.0,
        Some("reversed_profile_normal") => -1.0,
        _ => return Err(reason.into()),
    };
    if extrude.pointer("/extent/kind").and_then(Value::as_str) != Some("one_sided")
        || extrude
            .pointer("/extent/side/termination/kind")
            .and_then(Value::as_str)
            != Some("blind")
    {
        return Err(reason.into());
    }
    let depth = extrude
        .pointer("/extent/side/termination/length")
        .and_then(Value::as_f64)
        .ok_or(reason)?;
    if is_start || sign < 0.0 {
        return Err("sketches on reversed or start extrusion caps need an explicit source frame; this attachment is not supported yet".into());
    }
    let distance = depth;
    let [ox, oy, oz] = origin;
    let [nx, ny, nz] = normal;
    Ok(
        json!({"origin": topology::point([ox + nx * distance, oy + ny * distance, oz + nz * distance]),
        "normal": topology::point(normal), "u_axis": frame.get("u_axis")}),
    )
}

fn point(u: f64, v: f64) -> Value {
    json!({"u": u, "v": v})
}

pub(super) fn geometry(entity: &Value) -> Mapping<Value> {
    if entity.get("btType").and_then(Value::as_str) == Some("BTMSketchPoint-158") {
        return Ok(
            json!({"kind": "point", "position": point(coordinate(entity, "x")? * 1000.0, coordinate(entity, "y")? * 1000.0)}),
        );
    }
    let geometry = entity
        .get("geometry")
        .ok_or("a sketch entity has no solved geometry")?;
    match text(geometry, "btType")? {
        "BTCurveGeometryLine-117" => {
            if entity.get("btType").and_then(Value::as_str) != Some("BTMSketchCurveSegment-155") {
                return Err("unbounded sketch lines are not supported yet".into());
            }
            let x = coordinate(geometry, "pntX")?;
            let y = coordinate(geometry, "pntY")?;
            let dx = coordinate(geometry, "dirX")?;
            let dy = coordinate(geometry, "dirY")?;
            let start = coordinate(entity, "startParam")?;
            let end = coordinate(entity, "endParam")?;
            Ok(
                json!({"kind": "line", "start": point((x + start * dx) * 1000.0, (y + start * dy) * 1000.0),
                "end": point((x + end * dx) * 1000.0, (y + end * dy) * 1000.0)}),
            )
        }
        "BTCurveGeometryCircle-115" => {
            let center = point(
                coordinate(geometry, "xCenter")? * 1000.0,
                coordinate(geometry, "yCenter")? * 1000.0,
            );
            let radius = number(geometry, "radius")? * 1000.0;
            if entity.get("btType").and_then(Value::as_str) == Some("BTMSketchCurve-4") {
                return Ok(json!({"kind": "circle", "center": center, "radius": radius}));
            }
            let direction = coordinate(geometry, "yDir")?.atan2(coordinate(geometry, "xDir")?);
            let sign = if geometry.get("clockwise").and_then(Value::as_bool) == Some(true) {
                -1.0
            } else {
                1.0
            };
            let a = direction + sign * coordinate(entity, "startParam")?;
            let b = direction + sign * coordinate(entity, "endParam")?;
            let (a, b) = if sign < 0.0 { (b, a) } else { (a, b) };
            Ok(
                json!({"kind": "arc", "center": center, "radius": radius, "start_angle": a, "end_angle": b}),
            )
        }
        "BTCurveGeometryEllipse-1189" => {
            let x = coordinate(geometry, "xDir")?;
            let y = coordinate(geometry, "yDir")?;
            if x == 0.0 && y == 0.0 {
                return Err("the ellipse axis has no resolved direction".into());
            }
            let mut curve = json!({"kind": "ellipse",
                "center": point(coordinate(geometry, "xCenter")? * 1000.0,
                    coordinate(geometry, "yCenter")? * 1000.0),
                "major_angle": y.atan2(x),
                "major_radius": number(geometry, "radius")? * 1000.0,
                "minor_radius": number(geometry, "minorRadius")? * 1000.0});
            if entity.get("btType").and_then(Value::as_str) != Some("BTMSketchCurve-4") {
                if entity.get("btType").and_then(Value::as_str) != Some("BTMSketchCurveSegment-155")
                {
                    return Err("this elliptical sketch curve is not supported yet".into());
                }
                let start = coordinate(entity, "startParam")?;
                let end = coordinate(entity, "endParam")?;
                let (start, end) =
                    if geometry.get("clockwise").and_then(Value::as_bool) == Some(true) {
                        (-end, -start)
                    } else {
                        (start, end)
                    };
                if end <= start || end - start > std::f64::consts::TAU {
                    return Err(
                        "the elliptical arc must span a positive interval of at most one turn"
                            .into(),
                    );
                }
                curve
                    .as_object_mut()
                    .ok_or("ellipse is missing")?
                    .insert("bounds".into(), json!([start, end]));
            }
            Ok(curve)
        }
        "BTCurveGeometrySpline-118" | "BTCurveGeometryControlPointSpline-2197" => {
            let knots = rows(geometry, "knots")?;
            match entity.get("btType").and_then(Value::as_str) {
                Some("BTMSketchCurve-4") => {}
                Some("BTMSketchCurveSegment-155") => {
                    let first = knots.first().and_then(Value::as_f64);
                    let last = knots.last().and_then(Value::as_f64);
                    let (Some(first), Some(last)) = (first, last) else {
                        return Err("spline knots are missing".into());
                    };
                    let tolerance = SPLINE_DOMAIN_TOLERANCE * (last - first).abs().max(1.0);
                    if (coordinate(entity, "startParam")? - first).abs() > tolerance
                        || (coordinate(entity, "endParam")? - last).abs() > tolerance
                    {
                        return Err("trimmed sketch splines are not supported yet".into());
                    }
                }
                _ => return Err("trimmed sketch splines are not supported yet".into()),
            }
            if geometry.get("isRational").and_then(Value::as_bool) == Some(true)
                || geometry.get("isPeriodic").and_then(Value::as_bool) == Some(true)
                || geometry.get("dimension").and_then(Value::as_u64) != Some(2)
            {
                return Err(
                    "rational, periodic, or non-planar sketch splines are not supported yet".into(),
                );
            }
            let controls = rows(geometry, "controlPoints")?;
            if controls.len() % 2 != 0
                || geometry.get("controlPointCount").and_then(Value::as_u64)
                    != Some(cadmpeg_core::decode::u64_from_index(controls.len() / 2))
            {
                return Err("spline control point count does not match its coordinates".into());
            }
            let mut points = Vec::new();
            for [u, v] in controls.as_chunks::<2>().0 {
                points.push(point(
                    u.as_f64().ok_or("spline coordinate is missing")? * 1000.0,
                    v.as_f64().ok_or("spline coordinate is missing")? * 1000.0,
                ));
            }
            Ok(
                json!({"kind": "nurbs", "curve": {"degree": geometry.get("degree"), "knots": knots,
                "poles": {"kind": "polynomial", "points": points}, "periodic": false}}),
            )
        }
        other => Err(format!("{other} sketch geometry is not supported yet")),
    }
}

pub(super) fn map(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &mut Model,
    report: &mut Report,
) -> Mapping<Value> {
    if feature.get("btType").and_then(Value::as_str) != Some("BTMSketch-151") {
        return Err("only planar sketches are supported yet".into());
    }
    let native = text(feature, "featureId")?;
    let sketch_id = id("sketch", native);
    let plane = frame(feature, model, &|face| {
        inputs.load(ordinal, feature)?.face_frame(face)
    })?;
    let mut loci = BTreeMap::new();
    for entity in rows(feature, "entities")? {
        let native_entity = text(entity, "entityId")?;
        let entity_id = id("entity", &format!("{native}/{native_entity}"));
        if loci
            .insert(
                native_entity.to_owned(),
                json!({"kind": "entity", "value": entity_id}),
            )
            .is_some()
        {
            return Err("sketch entity ids repeat".into());
        }
        let clockwise = entity
            .pointer("/geometry/clockwise")
            .and_then(Value::as_bool)
            == Some(true);
        for (key, kind) in [
            ("startPointId", if clockwise { "end" } else { "start" }),
            ("endPointId", if clockwise { "start" } else { "end" }),
            ("centerId", "center"),
        ] {
            if let Some(endpoint) = entity
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                if loci
                    .insert(
                        endpoint.to_owned(),
                        json!({"kind": kind, "value": entity_id}),
                    )
                    .is_some()
                {
                    return Err("sketch endpoint ids repeat".into());
                }
            }
        }
        push(
            &mut model.sketch_entities,
            json!({"id": entity_id, "sketch": sketch_id,
            "endpoint_refs": [], "construction": entity.get("isConstruction").and_then(Value::as_bool).unwrap_or(false), "geometry": geometry(entity)?}),
        )?;
    }
    let mut sketch =
        json!({"id": sketch_id, "placement": {"kind": "resolved", "frame": plane}, "profiles": []});
    if let Some(name) = feature.get("name").filter(|name| !name.is_null()) {
        sketch
            .as_object_mut()
            .ok_or("sketch is missing")?
            .insert("name".into(), name.clone());
    }
    push(&mut model.sketches, sketch)?;
    for constraint in optional_rows(feature, "constraints")? {
        let parameter_count = model.parameters.len();
        let entity_count = model.sketch_entities.len();
        let constraint_count = model.sketch_constraints.len();
        let result = constraint_row(constraint, native, &sketch_id, &plane, &loci, model)
            .and_then(|row| push(&mut model.sketch_constraints, row));
        if let Err(reason) = result {
            model.parameters.truncate(parameter_count);
            model.sketch_entities.truncate(entity_count);
            model.sketch_constraints.truncate(constraint_count);
            let kind = constraint
                .get("constraintType")
                .and_then(Value::as_str)
                .unwrap_or("Unknown");
            report.push_coded(
                feature
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(native),
                crate::loss::OnshapeLossCode::ConstraintNotSupported,
                format!("{kind} sketch constraint is not supported yet: {reason}; kept the solved sketch geometry"),
            );
        }
    }
    Ok(definition(
        "sketch",
        json!({"sketch": {"kind": "planar", "sketch": sketch_id}}),
    ))
}

fn locus_position(locus: &Value, model: &Model) -> Mapping<Value> {
    let entity_id = text(locus, "value")?;
    let entity = model
        .sketch_entities
        .iter()
        .find(|e| e.id().as_str() == entity_id)
        .ok_or("the sketch point's entity is missing")?;
    let entity =
        serde_json::to_value(entity).map_err(|_| "the sketch point's entity is unreadable")?;
    let geometry = entity
        .get("geometry")
        .ok_or("the sketch point's geometry is missing")?;
    let kind = text(locus, "kind")?;
    if text(geometry, "kind")? == "arc" && matches!(kind, "start" | "end") {
        let angle = number(
            geometry,
            if kind == "start" {
                "start_angle"
            } else {
                "end_angle"
            },
        )?;
        let center = geometry.get("center").ok_or("the arc center is missing")?;
        let radius = number(geometry, "radius")?;
        return Ok(point(
            number(center, "u")? + radius * angle.cos(),
            number(center, "v")? + radius * angle.sin(),
        ));
    }
    let key = match kind {
        "entity" if text(geometry, "kind")? == "point" => "position",
        "start" if text(geometry, "kind")? == "line" => "start",
        "end" if text(geometry, "kind")? == "line" => "end",
        "center" => "center",
        _ => return Err("this sketch point locus is not supported yet".into()),
    };
    geometry
        .get(key)
        .cloned()
        .ok_or("the sketch point's position is missing".into())
}
fn names_origin(query: &Value) -> bool {
    let source = query
        .get("queryString")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let by_id = source.is_empty()
        && query
            .get("deterministicIds")
            .and_then(Value::as_array)
            .is_some_and(|ids| ids.len() == 1 && ids.iter().all(|id| id == ORIGIN_ID));
    by_id
        || matches!(
            source.trim_end_matches(';'),
            "qOrigin(EntityType.VERTEX)" | "query=qOrigin(EntityType.VERTEX)"
        )
        || (query.get("featureId").and_then(Value::as_str) == Some("Origin")
            && query.get("btType").and_then(Value::as_str)
                == Some("BTMIndividualCreatedByQuery-137"))
}
fn projected_origin(plane: &Value) -> Mapping<[f64; 2]> {
    let origin = topology::vector(
        plane.get("origin").ok_or("the sketch frame is missing")?,
        1.0,
    )?;
    let normal = topology::unit(topology::vector(
        plane.get("normal").ok_or("the sketch frame is missing")?,
        1.0,
    )?)?;
    let u = topology::unit(topology::vector(
        plane.get("u_axis").ok_or("the sketch frame is missing")?,
        1.0,
    )?)?;
    let v = topology::cross(normal, u);
    let dot = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    Ok([0.0 - dot(origin, u), 0.0 - dot(origin, v)])
}

fn constraint_row(
    constraint: &Value,
    native: &str,
    sketch_id: &str,
    plane: &Value,
    loci: &BTreeMap<String, Value>,
    model: &mut Model,
) -> Mapping<Value> {
    let kind = text(constraint, "constraintType")?;
    let cid = id(
        "constraint",
        &format!("{native}/{}", text(constraint, "entityId")?),
    );
    let locus = |key: &str| -> Mapping<Value> {
        let operand = parameter(constraint, key)?
            .get("value")
            .and_then(Value::as_str)
            .ok_or("a constraint operand is missing")?;
        loci.get(operand)
            .cloned()
            .ok_or_else(|| format!("constraint operand {operand} has no local geometry"))
    };
    let entity = |key: &str| -> Mapping<Value> {
        let locus = locus(key)?;
        if locus.get("kind").and_then(Value::as_str) != Some("entity") {
            return Err("an endpoint constraint needs explicit loci".into());
        }
        locus
            .get("value")
            .cloned()
            .ok_or("constraint entity is missing".into())
    };
    let definition = match kind {
        "HORIZONTAL" | "VERTICAL" | "FIX" => {
            json!({"kind": if kind == "FIX" { "fixed" } else if kind == "HORIZONTAL" { "horizontal" } else { "vertical" }, "entity": entity("localFirst")?})
        }
        "PARALLEL" | "PERPENDICULAR" | "TANGENT" | "EQUAL" | "CONCENTRIC" => {
            json!({"kind": kind.to_ascii_lowercase(), "first": entity("localFirst")?, "second": entity("localSecond")?})
        }
        "COINCIDENT" if parameter(constraint, "externalSecond").is_ok() => {
            let [query] = queries(constraint, "externalSecond")? else {
                return Err("multiple external constraint references are not supported yet".into());
            };
            if !names_origin(query) {
                return Err("the external coincident reference does not identify the sketch origin; this constraint is not supported yet".into());
            }
            let point = locus("localFirst")?;
            let [u, v] = projected_origin(plane)?;
            let at = locus_position(&point, model)?;
            if (number(&at, "u")? - u).abs() > POINT_U_TOLERANCE
                || (number(&at, "v")? - v).abs() > POINT_V_TOLERANCE
            {
                return Err("the point coincident with the origin is not at the origin in the solved sketch; this constraint is not supported yet".into());
            }
            json!({"kind":"point_coordinate_values", "point":point,"values":[u, v]})
        }
        "COINCIDENT" => {
            json!({"kind": "coincident_loci", "loci": [locus("localFirst")?, locus("localSecond")?]})
        }
        "MIDPOINT" => {
            let midpoint = locus("localMidpoint")?;
            let first = locus("localEntity1")?;
            let second = locus("localEntity2")?;
            let a = locus_position(&first, model)?;
            let b = locus_position(&second, model)?;
            if a == b {
                return Err("a midpoint needs two distinct source points".into());
            }
            let carrier = id(
                "entity",
                &format!("{native}/midpoint/{}", text(constraint, "entityId")?),
            );
            push(
                &mut model.sketch_entities,
                json!({"id":carrier,"sketch":sketch_id,"endpoint_refs":[],"construction":true,
                "geometry":{"kind":"line","start":a,"end":b}}),
            )?;
            for (suffix, endpoint, source) in [("a", "start", first), ("b", "end", second)] {
                push(
                    &mut model.sketch_constraints,
                    json!({"id":format!("{cid}-{suffix}"),"sketch":sketch_id,
                    "definition":{"kind":"coincident_loci","loci":[{"kind":endpoint,"value":carrier},source]}}),
                )?;
            }
            json!({"kind":"midpoint","point":midpoint,"entity":carrier})
        }
        "LENGTH" | "DISTANCE" | "RADIUS" | "DIAMETER" | "ANGLE" => {
            if constraint.get("drivenDimension").and_then(Value::as_bool) == Some(true) {
                return Err("driven sketch dimensions are not supported yet".into());
            }
            let key = match kind {
                "LENGTH" => "length",
                "DISTANCE" => "distance",
                "RADIUS" => "radius",
                "DIAMETER" => "length",
                _ => "angle",
            };
            let dimension = parameter(constraint, key).or_else(|reason| {
                if kind == "DIAMETER"
                    && rows(constraint, "parameters").is_ok_and(|rows| {
                        rows.iter()
                            .all(|p| p.get("parameterId").and_then(Value::as_str) != Some("length"))
                    })
                {
                    parameter(constraint, "diameter")
                } else {
                    Err(reason)
                }
            })?;
            let value = quantity(dimension, kind == "ANGLE", model)?;
            let pid = id(
                "parameter",
                &format!("{native}/{}", text(constraint, "entityId")?),
            );
            let parameter_ordinal = model.parameters.len();
            let owner = id("feature", native);
            let ordinal_in_owner = model
                .parameters
                .iter()
                .filter(|parameter| {
                    parameter
                        .owner
                        .as_ref()
                        .is_some_and(|scope| scope.as_str() == owner)
                })
                .count();
            let evaluated = dimension
                .get("expression")
                .and_then(Value::as_str)
                .filter(|s| s.contains('#'))
                .map(|source| {
                    super::expression::evaluate(
                        source,
                        if kind == "ANGLE" {
                            super::expression::Dimension::Angle
                        } else {
                            super::expression::Dimension::Length
                        },
                        model,
                    )
                })
                .transpose()?;
            let expression = evaluated.as_ref().map_or_else(
                || format!("{value} {}", if kind == "ANGLE" { "rad" } else { "mm" }),
                |v| v.expression.clone(),
            );
            let dependencies = evaluated.map(|v| v.dependencies).unwrap_or_default();
            push(
                &mut model.parameters,
                json!({"id": pid, "owner": owner, "ordinal": ordinal_in_owner, "name": format!("dimension{parameter_ordinal}"),
                "dependencies": dependencies, "properties": {}, "expression": expression,
                "value": {"kind": if kind == "ANGLE" { "angle" } else { "length" }, "value": value}}),
            )?;
            match kind {
                "LENGTH" => {
                    json!({"kind": "distance", "entities": [entity("localFirst")?], "parameter": pid})
                }
                "DISTANCE" => {
                    json!({"kind": "distance_loci", "first": locus("localFirst")?, "second": locus("localSecond")?, "parameter": pid})
                }
                "ANGLE" => {
                    json!({"kind": "angle", "first": entity("localFirst")?, "second": entity("localSecond")?, "parameter": pid})
                }
                _ => {
                    json!({"kind": kind.to_ascii_lowercase(), "entity": entity("localFirst")?, "parameter": pid})
                }
            }
        }
        _ => return Err(format!("{kind} sketch constraints are not supported yet")),
    };
    Ok(json!({"id": cid, "sketch": sketch_id, "definition": definition}))
}
