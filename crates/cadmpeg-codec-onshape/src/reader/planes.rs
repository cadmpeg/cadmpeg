// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;

use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::{definition, features, id, sketch, topology, Mapping};

const MATRIX_ROTATION_TOLERANCE: f64 = 64.0 * f64::EPSILON;

pub(super) fn created(model: &Model, native: &str) -> Mapping<Value> {
    let feature_id = id("feature", native);
    let feature = model
        .features
        .iter()
        .find(|feature| feature.id.as_str() == feature_id)
        .ok_or("the selected datum plane is unresolved")?;
    let wire = serde_json::to_value(feature.evaluation.definition())
        .map_err(|_| "the selected datum plane is unreadable")?;
    match wire.pointer("/value/kind").and_then(Value::as_str) {
        Some("datum_plane") => wire
            .pointer("/value/value/frame")
            .cloned()
            .ok_or_else(|| "the selected datum plane frame is missing".into()),
        Some("datum_offset_plane") => {
            let frame = wire
                .pointer("/value/value/reference/frame")
                .cloned()
                .ok_or("the offset support frame is missing")?;
            let distance = wire
                .pointer("/value/value/distance")
                .and_then(Value::as_f64)
                .ok_or("the plane offset is missing")?;
            offset(frame, distance)
        }
        _ => Err("the selected feature has no resolved datum plane frame".into()),
    }
}

pub(super) fn map(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &Model,
    unresolved: &BTreeSet<String>,
) -> Mapping<Value> {
    if features::value(feature, "cplaneType") != Some("OFFSET") {
        return Err("this cplaneType is not supported yet".into());
    }
    let source = json!({"parameters":[{
        "parameterId":"sketchPlane", "queries":features::queries(feature,"entities")?
    }]});
    let frame = sketch::frame(
        &source,
        &|face| {
            let mut support = inputs.load(ordinal, feature)?.oriented_face_frame(face)?;
            let normal = topology::vector(
                support
                    .get("normal")
                    .ok_or("the support normal is missing")?,
                1.0,
            )?;
            let [nx, ny, _] = normal;
            let axis = if nx.abs() < ny.abs() {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            support["u_axis"] = topology::point(topology::unit(topology::cross(axis, normal))?);
            Ok(support)
        },
        &|native| created(model, native),
    )?;
    let distance = features::quantity(
        features::parameter(feature, "offset")?,
        false,
        model,
        unresolved,
    )?;
    if features::enabled(feature, "oppositeDirection")? {
        return Err("a flipped offset plane is not supported yet".into());
    }
    Ok(definition(
        "datum_offset_plane",
        json!({
            "reference":{"kind":"resolved_plane","frame":frame},
            "distance":distance
        }),
    ))
}

fn offset(mut frame: Value, distance: f64) -> Mapping<Value> {
    let origin = topology::vector(frame.get("origin").ok_or("plane origin is missing")?, 1.0)?;
    let normal = topology::vector(frame.get("normal").ok_or("plane normal is missing")?, 1.0)?;
    let [ox, oy, oz] = origin;
    let [nx, ny, nz] = normal;
    frame["origin"] = topology::point([ox + distance * nx, oy + distance * ny, oz + distance * nz]);
    Ok(frame)
}

pub(super) fn built_in(feature: &Value) -> bool {
    let Ok(queries) = features::queries(feature, "entities") else {
        return false;
    };
    sketch::built_in_frame(&json!({"parameters":[{"parameterId":"sketchPlane","queries":queries}]}))
}

pub(super) fn sketch_plane(row: &Value) -> Mapping<Value> {
    let [xx, yx, nx, ox, xy, yy, ny, oy, xz, yz, nz, oz, zx, zy, zz, one] =
        super::rows(row, "sketchMatrix")?
    else {
        return Err("the sketch attachment matrix needs sixteen entries".into());
    };
    let scalar = |value: &Value| {
        value
            .as_f64()
            .filter(|n| n.is_finite())
            .ok_or("the sketch attachment matrix needs finite numbers")
    };
    if [scalar(zx)?, scalar(zy)?, scalar(zz)?, scalar(one)?] != [0.0, 0.0, 0.0, 1.0] {
        return Err("the sketch attachment matrix has an invalid homogeneous row".into());
    }
    let x = [scalar(xx)?, scalar(xy)?, scalar(xz)?];
    let y = [scalar(yx)?, scalar(yy)?, scalar(yz)?];
    let normal = [scalar(nx)?, scalar(ny)?, scalar(nz)?];
    let expected_y = topology::cross(normal, x);
    if y.into_iter()
        .zip(expected_y)
        .any(|(a, b)| (a - b).abs() > MATRIX_ROTATION_TOLERANCE)
        || [x, y, normal].into_iter().any(|axis| {
            (axis.into_iter().map(|n| n * n).sum::<f64>() - 1.0).abs() > MATRIX_ROTATION_TOLERANCE
        })
    {
        return Err("the sketch attachment matrix needs an orthonormal rotation".into());
    }
    Ok(
        json!({"origin":topology::point([scalar(ox)?,scalar(oy)?,scalar(oz)?]),
        "normal":topology::point(normal),"x":topology::point(x)}),
    )
}

#[cfg(test)]
mod tests;
