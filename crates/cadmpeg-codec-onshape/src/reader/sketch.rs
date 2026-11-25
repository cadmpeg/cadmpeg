// SPDX-License-Identifier: Apache-2.0
const SPLINE_DOMAIN_TOLERANCE: f64 = 1e-9;
const POINT_U_TOLERANCE: f64 = 1e-6;
const POINT_V_TOLERANCE: f64 = 1e-6;
const EPS_FRAME_X_IN_PLANE: f64 = 64.0 * f64::EPSILON;
const EPS_FRAME_ROUNDOFF: f64 = 64.0 * f64::EPSILON;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::Report;
use super::{
    coordinate, definition,
    features::{parameter, quantity, queries},
    id, number, optional_rows, push, query as compressed, rows, text, topology, Mapping,
};
const ORIGIN_ID: &str = "IB";
pub(super) fn frame(
    feature: &Value,
    face: &dyn Fn(&str) -> Mapping<Value>,
    datum: &dyn Fn(&str) -> Mapping<Value>,
) -> Mapping<Value> {
    let [query] = queries(feature, "sketchPlane")? else {
        return Err("multiple sketch planes are not supported yet".into());
    };
    if query.get("deterministicIds").is_some() {
        for native in rows(query, "deterministicIds")? {
            if !native.is_string() {
                return Err("a sketch face id is not a string".into());
            }
        }
    }
    let deterministic = query
        .get("deterministicIds")
        .and_then(Value::as_array)
        .filter(|ids| !ids.is_empty());
    let face_frame = || -> Mapping<Value> {
        let [native] = deterministic.map(Vec::as_slice).unwrap_or_default() else {
            return Err("a sketch on more than one face is not supported yet".into());
        };
        let support = face(native.as_str().ok_or("a sketch face id is not a string")?)?;
        // Without a stored plane object the face closure returns the frame admitted from
        // the sketch's `sketchMatrix` row, complete with its in-plane axis.
        if feature.get("plane").is_none() && support.get("u_axis").is_some() {
            return Ok(support);
        }
        attachment_frame(feature, &support)
    };
    let plane = if query
        .get("queryString")
        .and_then(Value::as_str)
        .is_some_and(|s| s.contains("qCompressed("))
    {
        let resolved = compressed::resolved(query)?;
        if resolved.get("queryType").and_then(Value::as_str) == Some("CAP_FACE") {
            if text(&resolved, "entityType")? != "FACE"
                || text(&resolved, "historyType")? != "CREATION"
            {
                return Err("the cap attachment query has no stored face creation".into());
            }
            super::boolean_field(&resolved, "isStart")?;
            return if deterministic.is_some() {
                face_frame()
            } else {
                stored_frame(feature)
            };
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
    } else if let Some(feature_id) = query
        .get("featureId")
        .and_then(Value::as_str)
        .filter(|native| deterministic.is_none() || datum(native).is_ok())
    {
        feature_id.to_owned()
    } else if deterministic.is_some() {
        return face_frame();
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
        _ => {
            let support = datum(&plane)?;
            return if feature.get("plane").is_some() {
                attachment_frame(feature, &support)
            } else {
                Ok(support)
            };
        }
    };
    let support = json!({"origin": topology::point([0.0; 3]), "normal": topology::point(normal), "u_axis": topology::point(u_axis)});
    if feature.get("plane").is_some() {
        attachment_frame(feature, &support)
    } else {
        Ok(support)
    }
}

pub(super) fn built_in_frame(feature: &Value) -> bool {
    let Ok([query]) = queries(feature, "sketchPlane") else {
        return false;
    };
    if query
        .get("queryString")
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("qCompressed("))
        && !compressed::resolved(query)
            .is_ok_and(|query| query.get("queryType").and_then(Value::as_str) == Some("DUMMY"))
    {
        return false;
    }
    frame(feature, &|_| Err("historical face".into()), &|_| {
        Err("created plane".into())
    })
    .is_ok()
}

fn stored_frame(feature: &Value) -> Mapping<Value> {
    let plane = feature
        .get("plane")
        .ok_or("the stored plane and x axis leave the sketch attachment translation and rotation unresolved")?;
    let origin = topology::vector(
        plane
            .get("origin")
            .ok_or("the sketch attachment origin is missing")?,
        1000.0,
    )?;
    let normal = topology::unit(topology::vector(
        plane
            .get("normal")
            .ok_or("the sketch attachment normal is missing")?,
        1.0,
    )?)?;
    let x = topology::unit(topology::vector(
        plane
            .get("x")
            .ok_or("the sketch attachment stored x axis is missing")?,
        1.0,
    )?)?;
    let dot: f64 = x.into_iter().zip(normal).map(|(a, b)| a * b).sum();
    if dot.abs() > EPS_FRAME_X_IN_PLANE {
        return Err("the stored sketch x axis is not in its plane".into());
    }
    Ok(
        json!({"origin":topology::point(origin),"normal":topology::point(normal),"u_axis":topology::point(x)}),
    )
}

fn attachment_frame(feature: &Value, support: &Value) -> Mapping<Value> {
    let frame = stored_frame(feature)?;
    let read = |row: &Value, key| {
        topology::vector(row.get(key).ok_or("attachment datum is missing")?, 1.0)
    };
    let origin = read(&frame, "origin")?;
    let normal = read(&frame, "normal")?;
    let support_origin = read(support, "origin")?;
    let support_normal = read(support, "normal")?;
    let dot = |a: [f64; 3], b: [f64; 3]| a.into_iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    let scale = origin
        .into_iter()
        .chain(support_origin)
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
    let roundoff = EPS_FRAME_ROUNDOFF;
    if (dot(normal, support_normal).abs() - 1.0).abs() > roundoff
        || dot(topology::sub(origin, support_origin), support_normal).abs() > roundoff * scale
    {
        return Err("the stored sketch attachment frame contradicts its planar face".into());
    }
    Ok(frame)
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
            let sign = if super::boolean_field(geometry, "clockwise")? {
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
                let (start, end) = if super::boolean_field(geometry, "clockwise")? {
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
            if super::boolean_field(geometry, "isRational")?
                || super::boolean_field(geometry, "isPeriodic")?
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
    unresolved: &BTreeSet<String>,
) -> Mapping<Value> {
    if feature.get("btType").and_then(Value::as_str) != Some("BTMSketch-151") {
        return Err("only planar sketches are supported yet".into());
    }
    let native = text(feature, "featureId")?;
    let sketch_id = id("sketch", native);
    let created_plane = std::cell::Cell::new(false);
    let mut plane = frame(
        feature,
        &|face| {
            let support = inputs.load(ordinal, feature)?.face_frame(face)?;
            if feature.get("plane").is_some() {
                return Ok(support);
            }
            let Some(stored) = inputs.sketch_plane(native)? else {
                return Ok(support);
            };
            attachment_frame(&json!({"plane":stored}), &support)
        },
        &|native| {
            let frame = super::planes::created(model, native)?;
            created_plane.set(true);
            Ok(frame)
        },
    )?;
    if feature.get("plane").is_none() && created_plane.get() {
        let attachment = inputs.sketch_plane(native).and_then(|stored| {
            let stored = stored.ok_or("the sketch attachment matrix is missing")?;
            attachment_frame(&json!({"plane":stored}), &plane)
        });
        match attachment {
            Ok(stored) => plane = stored,
            Err(reason) => report.push_coded(
                native,
                crate::loss::OnshapeLossCode::SketchPlaneAssumed,
                format!("{reason}; assumed the support plane's in-plane frame"),
            ),
        }
    }
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
        let clockwise =
            super::boolean_field(entity.get("geometry").unwrap_or(&Value::Null), "clockwise")?;
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
            "endpoint_refs": [], "construction": super::boolean_field(entity,"isConstruction")?, "geometry": match geometry(entity).and_then(|geometry| {
                serde_json::from_value::<cadmpeg_ir::sketches::SketchGeometry>(geometry.clone()).map_err(|e| format!("sketch geometry admission failed ({e})"))?;
                Ok(geometry)
            }) {
                Ok(geometry) => geometry,
                Err(reason) => {
                    report.push_coded(native, crate::loss::OnshapeLossCode::SketchGeometryNotSupported, reason.clone());
                    json!({"kind":"native", "native_kind":format!("Onshape {reason}")})
                }
            }}),
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
        let result = constraint_row(
            constraint, native, &sketch_id, &plane, &loci, model, unresolved,
        )
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

pub(super) struct ConstraintOutput {
    pub entities: usize,
    pub constraints: usize,
    pub parameters: usize,
}

pub(super) fn constraint_output(constraint: &Value) -> Mapping<ConstraintOutput> {
    let (entities, constraints, parameters) = match text(constraint, "constraintType")? {
        "MIDPOINT" => (1, 3, 0),
        "LENGTH" | "DISTANCE" | "RADIUS" | "DIAMETER" | "ANGLE" => (0, 1, 1),
        "HORIZONTAL" | "VERTICAL" | "FIX" | "PARALLEL" | "PERPENDICULAR" | "TANGENT" | "EQUAL"
        | "CONCENTRIC" | "COINCIDENT" => (0, 1, 0),
        kind => return Err(format!("{kind} sketch constraints are not supported yet")),
    };
    Ok(ConstraintOutput {
        entities,
        constraints,
        parameters,
    })
}

fn constraint_row(
    constraint: &Value,
    native: &str,
    sketch_id: &str,
    plane: &Value,
    loci: &BTreeMap<String, Value>,
    model: &mut Model,
    unresolved: &BTreeSet<String>,
) -> Mapping<Value> {
    constraint_output(constraint)?;
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
            if super::boolean_field(constraint, "drivenDimension")? {
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
            let value = quantity(dimension, kind == "ANGLE", model, unresolved)?;
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
                        unresolved,
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

#[cfg(test)]
mod tests;
