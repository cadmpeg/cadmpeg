// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::super::{features, number, rows, text, Mapping};
use super::{cross, sub, unit, vector, State};

const ROUNDOFF_FACTOR: f64 = 128.0;
const WORK_LIMIT: usize = 1_000_000;

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}

fn along([x, y, z]: [f64; 3], amount: f64, [dx, dy, dz]: [f64; 3]) -> [f64; 3] {
    [x + amount * dx, y + amount * dy, z + amount * dz]
}

fn roundoff(points: &[[f64; 3]]) -> f64 {
    ROUNDOFF_FACTOR * f64::EPSILON * points.iter().flatten().map(|v| v.abs()).fold(1.0, f64::max)
}

fn convex(points: &[[f64; 3]], ends: &[[f64; 3]], normal: [f64; 3]) -> Mapping<()> {
    if points.len() < 3
        || points
            .len()
            .checked_mul(points.len())
            .ok_or("boundary count overflow")?
            > WORK_LIMIT
    {
        return Err("needs a bounded closed convex boundary".into());
    }
    let tolerance = roundoff(points);
    let mut sense = None;
    for (index, start) in points.iter().enumerate() {
        let next = points
            .get((index + 1) % points.len())
            .ok_or("boundary endpoint is missing")?;
        let end = ends.get(index).ok_or("boundary endpoint is missing")?;
        if sub(*next, *end).into_iter().any(|v| v.abs() > tolerance) {
            return Err("needs a closed boundary of stored endpoints".into());
        }
        let direction = unit(sub(*end, *start))?;
        for candidate in points {
            let side = dot(cross(direction, sub(*candidate, *start)), normal);
            if !side.is_finite() {
                return Err("boundary exceeds the finite range".into());
            }
            if side.abs() > tolerance {
                let positive = side > 0.0;
                if sense.is_some_and(|sense| sense != positive) {
                    return Err("needs a convex boundary of stored lines".into());
                }
                sense = Some(positive);
            }
        }
    }
    if sense.is_none() {
        return Err("needs a nondegenerate closed boundary".into());
    }
    Ok(())
}

pub(super) fn prove(
    state: &State,
    feature: &Value,
    profile: &Value,
    model: &Model,
) -> Mapping<Value> {
    if state.faces.len() != 1 {
        return Err("needs one stored target face for an exact first intersection".into());
    }
    let (native, face) = state
        .faces
        .first_key_value()
        .ok_or("stored target face is missing")?;
    let surface = face.get("surface").ok_or("target geometry is missing")?;
    if text(surface, "type")? != "PLANE" {
        return Err("needs a stored planar target".into());
    }
    let origin = vector(
        surface
            .get("origin")
            .ok_or("stored target origin is missing")?,
        1000.0,
    )?;
    let normal = unit(vector(
        surface
            .get("normal")
            .ok_or("stored target normal is missing")?,
        1.0,
    )?)?;
    let [boundary] = rows(face, "loops")? else {
        return Err("needs one closed target boundary".into());
    };
    if boundary.get("isOuter").and_then(Value::as_bool) != Some(true) {
        return Err("stored target boundary side is missing".into());
    }
    let mut target = Vec::new();
    let mut target_ends = Vec::new();
    for coedge in rows(boundary, "coedges")? {
        let edge = state
            .edges
            .get(text(coedge, "edgeId")?)
            .ok_or("stored target edge is missing")?;
        if edge.pointer("/curve/type").and_then(Value::as_str) != Some("LINE") {
            return Err("needs stored straight target edges".into());
        }
        let geometry = edge
            .get("geometry")
            .ok_or("stored target endpoints are missing")?;
        let mut start = vector(
            geometry
                .get("startPoint")
                .ok_or("stored target start is missing")?,
            1000.0,
        )?;
        let mut end = vector(
            geometry
                .get("endPoint")
                .ok_or("stored target end is missing")?,
            1000.0,
        )?;
        if !coedge
            .get("orientation")
            .and_then(Value::as_bool)
            .ok_or("stored target edge direction is missing")?
        {
            std::mem::swap(&mut start, &mut end);
        }
        let curve = edge.get("curve").ok_or("stored target line is missing")?;
        let line_origin = vector(
            curve
                .get("origin")
                .ok_or("stored target line origin is missing")?,
            1000.0,
        )?;
        let line_direction = unit(vector(
            curve
                .get("direction")
                .ok_or("stored target line direction is missing")?,
            1.0,
        )?)?;
        let tolerance = roundoff(&[start, end, line_origin]);
        for endpoint in [start, end] {
            if cross(sub(endpoint, line_origin), line_direction)
                .into_iter()
                .any(|value| value.abs() > tolerance)
            {
                return Err("target endpoints contradict the stored line geometry".into());
            }
        }
        target.push(start);
        target_ends.push(end);
    }
    convex(&target, &target_ends, normal)?;
    let tolerance = roundoff(&target);
    if target
        .iter()
        .any(|p| dot(sub(*p, origin), normal).abs() > tolerance)
    {
        return Err("target endpoints disagree with the stored plane".into());
    }
    if text(profile, "kind")? != "sketch" {
        return Err("needs one closed source sketch boundary".into());
    }
    let sketch_id = text(profile, "value")?;
    let sketch = model
        .sketches
        .iter()
        .find(|sketch| sketch.id.as_str() == sketch_id)
        .ok_or("source sketch is missing")?;
    let wire = json::to_value(sketch).map_err(|_| "source sketch is unreadable")?;
    let frame = wire
        .pointer("/placement/frame")
        .ok_or("source frame is missing")?;
    let base = vector(frame.get("origin").ok_or("source origin is missing")?, 1.0)?;
    let source_normal = unit(vector(
        frame.get("normal").ok_or("source normal is missing")?,
        1.0,
    )?)?;
    let u = unit(vector(
        frame.get("u_axis").ok_or("source x axis is missing")?,
        1.0,
    )?)?;
    let v = cross(source_normal, u);
    let reversed = features::enabled(feature, "oppositeDirection")?;
    let travel = source_normal.map(|v| if reversed { -v } else { v });
    let across = dot(normal, travel);
    if across.abs() <= ROUNDOFF_FACTOR * f64::EPSILON {
        return Err("stored target plane is parallel to travel".into());
    }
    let mut source = Vec::new();
    let mut source_ends = Vec::new();
    for entity in model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch.as_str() == sketch_id && !entity.construction)
    {
        let wire = json::to_value(entity).map_err(|_| "source curve is unreadable")?;
        let geometry = wire.get("geometry").ok_or("source geometry is missing")?;
        if text(geometry, "kind")? != "line" {
            return Err("needs stored straight source edges".into());
        }
        let world = |key| -> Mapping<[f64; 3]> {
            let p = geometry
                .get(key)
                .ok_or("stored source endpoint is missing")?;
            let pu = number(p, "u")?;
            let pv = number(p, "v")?;
            Ok(along(along(base, pu, u), pv, v))
        };
        source.push(world("start")?);
        source_ends.push(world("end")?);
    }
    convex(&source, &source_ends, source_normal)?;
    if source
        .len()
        .checked_mul(target.len())
        .ok_or("intersection work overflow")?
        > WORK_LIMIT
    {
        return Err("intersection work exceeds the bound".into());
    }
    for (index, start) in target.iter().enumerate() {
        let end = target_ends.get(index).ok_or("target endpoint is missing")?;
        let direction = unit(sub(*end, *start))?;
        let interior = target
            .iter()
            .map(|p| dot(cross(direction, sub(*p, *start)), normal))
            .find(|v| v.abs() > tolerance)
            .ok_or("target boundary side is unresolved")?;
        for p in &source {
            let distance = dot(sub(origin, *p), normal) / across;
            if !distance.is_finite() || distance <= tolerance {
                return Err("stored target plane has no forward intersection".into());
            }
            let hit = along(*p, distance, travel);
            let side = dot(cross(direction, sub(hit, *start)), normal);
            if !side.is_finite() || side * interior.signum() < -tolerance {
                return Err("stored target boundary does not cover the complete profile".into());
            }
        }
    }
    Ok(
        json!({"kind":"historical","value":{"state":state.row.get("id"),"native":"Onshape UP_TO_NEXT","faces":[native]}}),
    )
}
