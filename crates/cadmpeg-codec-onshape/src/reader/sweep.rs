// SPDX-License-Identifier: Apache-2.0
use cadmpeg_ir::{document::Model, sketches::SketchGeometryDefinition};
use serde_json::{json, Value};
use std::collections::BTreeSet;

use super::{definition, features, regions, rows, text, topology, Mapping};

const PARAMETER_IDS: &[&str] = &[
    "bodyType",
    "operationType",
    "surfaceOperationType",
    "profiles",
    "surfaceProfiles",
    "wallShape",
    "midplane",
    "thickness1",
    "flipWall",
    "thickness2",
    "thickness",
    "path",
    "pathOppositeDirection",
    "extendToFullPath",
    "profileControl",
    "lockFaces",
    "lockDirectionQuery",
    "hasTwist",
    "twistType",
    "turns",
    "angle",
    "pitch",
    "ccw",
    "hasScale",
    "scaleFactor",
    "trimEnds",
    "defaultScope",
    "booleanScope",
    "defaultSurfaceScope",
    "booleanSurfaceScope",
];
const PATH_CONNECTION_ROUNDOFF_FACTOR: f64 = 64.0;
const PATH_CONNECTION_WORK_LIMIT: usize = 1_000_000;
pub(super) const PATH_BUDGET_EXHAUSTED: &str =
    "the sweep path connectivity work budget is exhausted";

pub(super) fn validate_record(feature: &Value) -> Mapping<()> {
    let mut seen = BTreeSet::new();
    for row in rows(feature, "parameters")? {
        let key = text(row, "parameterId")?;
        if !seen.insert(key) {
            return Err(format!("parameter {key} repeats"));
        }
        match key {
            "bodyType"
            | "operationType"
            | "surfaceOperationType"
            | "profileControl"
            | "twistType" => {
                text(row, "value")?;
            }
            "profiles"
            | "surfaceProfiles"
            | "wallShape"
            | "path"
            | "lockFaces"
            | "lockDirectionQuery"
            | "booleanScope"
            | "booleanSurfaceScope" => {
                for query in rows(row, "queries")? {
                    if !query.is_object() {
                        return Err(format!("parameter {key} has a malformed query"));
                    }
                    if query.get("queryString").is_some() {
                        let source = text(query, "queryString")?;
                        if source.contains("qCompressed(") {
                            super::query::resolved_leaves(query)?;
                        }
                    }
                    if let Some(ids) = query.get("deterministicIds") {
                        if !ids
                            .as_array()
                            .is_some_and(|ids| ids.iter().all(Value::is_string))
                        {
                            return Err(format!("parameter {key} has malformed deterministic ids"));
                        }
                    }
                }
            }
            "midplane"
            | "flipWall"
            | "pathOppositeDirection"
            | "extendToFullPath"
            | "hasTwist"
            | "ccw"
            | "hasScale"
            | "trimEnds"
            | "defaultScope"
            | "defaultSurfaceScope" => {
                features::required_boolean(feature, key)?;
            }
            _ => {}
        }
    }
    features::parameter(feature, "bodyType")?;
    features::parameter(feature, "operationType")?;
    features::parameter(feature, "profiles")?;
    features::parameter(feature, "path")?;
    Ok(())
}

pub(super) fn map(feature: &Value, model: &Model, inputs: &topology::Inputs<'_>) -> Mapping<Value> {
    for row in rows(feature, "parameters")? {
        let key = text(row, "parameterId")?;
        if !PARAMETER_IDS.contains(&key) {
            return Err(format!("sweep parameter {key} is not supported yet"));
        }
        if key == "profileControl" && text(row, "value")? != "NONE" {
            return Err("this sweep profile control is not supported yet".into());
        }
        if matches!(key, "lockFaces" | "lockDirectionQuery") && !rows(row, "queries")?.is_empty() {
            return Err(format!("sweep {key} is not supported yet"));
        }
    }
    if features::value(feature, "bodyType") != Some("SOLID") {
        return Err("this sweep body type is not supported yet".into());
    }
    if !matches!(
        features::value(feature, "operationType"),
        Some("NEW" | "ADD")
    ) {
        return Err("this sweep Boolean operation is not supported yet".into());
    }
    for key in [
        "pathOppositeDirection",
        "extendToFullPath",
        "hasTwist",
        "hasScale",
        "trimEnds",
    ] {
        if features::enabled(feature, key)? {
            return Err(format!("sweep {key} is not supported yet"));
        }
    }
    let profile = features::profile(feature, "profiles", model, inputs)?;
    let selected = features::sketch_entities_in_order(
        features::queries(feature, "path")?,
        model,
        "EDGE",
        true,
    )?;
    let selection = selected.get("value").ok_or("the sweep path is missing")?;
    let curves = order_path(rows(selection, "entities")?, model)?;
    Ok(definition(
        "sweep",
        json!({
            "shape":{"kind":"solid", "op":features::operation(feature)?,
                "section":{"kind":"profile", "value":profile}, "sections":[]},
            "path":{"kind":"sketch_curves", "value":{"sketch":selection["sketch"], "curves":curves}},
            "path_tangent":false, "linearize":false
        }),
    ))
}

fn endpoints(entity: &cadmpeg_ir::sketches::SketchEntity) -> Mapping<[[f64; 2]; 2]> {
    if entity.construction {
        return Err("the sweep path contains a construction curve".into());
    }
    if let SketchGeometryDefinition::Nurbs { curve } = entity.geometry.definition() {
        let points = curve.control_points();
        let [from, to] = cadmpeg_ir::eval::nurbs_pcurve_parameter_domain(
            curve.degree(),
            curve.knots(),
            points.len(),
        )
        .ok_or("the sweep path spline has no parameter domain")?
        .endpoints();
        let degree = cadmpeg_core::decode::index_from_u32(curve.degree());
        if curve.knots().iter().take(degree + 1).any(|k| *k != from)
            || curve
                .knots()
                .iter()
                .skip(points.len())
                .take(degree + 1)
                .any(|k| *k != to)
        {
            return Err("the sweep path spline needs resolved clamped endpoints".into());
        }
        let first = points.first().ok_or("the sweep path spline has no poles")?;
        let last = points.last().ok_or("the sweep path spline has no poles")?;
        return Ok([[first.u, first.v], [last.u, last.v]]);
    }
    let wire = cadmpeg_core::serde_erase::json::to_value(entity)
        .map_err(|_| "the sweep path curve is unreadable")?;
    regions::curve_endpoints(&wire)
}

fn order_path(entities: &[Value], model: &Model) -> Mapping<Vec<Value>> {
    if entities
        .len()
        .checked_mul(entities.len())
        .is_none_or(|n| n > PATH_CONNECTION_WORK_LIMIT)
    {
        return Err(PATH_BUDGET_EXHAUSTED.into());
    }
    let ends = entities
        .iter()
        .map(|id| {
            let entity = model
                .sketch_entities
                .iter()
                .find(|e| Some(e.id().as_str()) == id.as_str())
                .ok_or("the sweep path curve is missing")?;
            endpoints(entity)
        })
        .collect::<Mapping<Vec<_>>>()?;
    let tolerance = PATH_CONNECTION_ROUNDOFF_FACTOR
        * f64::EPSILON
        * ends
            .iter()
            .flatten()
            .flatten()
            .copied()
            .map(f64::abs)
            .fold(1.0_f64, f64::max);
    let close = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]) <= tolerance;
    let mut vertices: Vec<[f64; 2]> = Vec::new();
    let mut edges = Vec::new();
    let mut degree = Vec::new();
    for pair in ends {
        let mut edge = [0; 2];
        for (side, point) in pair.into_iter().enumerate() {
            let vertex = vertices
                .iter()
                .position(|p| close(*p, point))
                .unwrap_or_else(|| {
                    vertices.push(point);
                    degree.push(0);
                    vertices.len() - 1
                });
            edge[side] = vertex;
            degree[vertex] += 1;
        }
        edges.push(edge);
    }
    let terminals: Vec<_> = degree
        .iter()
        .enumerate()
        .filter(|(_, d)| **d == 1)
        .map(|(i, _)| i)
        .collect();
    if degree.iter().any(|d| *d > 2) || !matches!(terminals.len(), 0 | 2) {
        return Err("the sweep path curves do not form one open or closed chain".into());
    }
    let mut vertex = terminals
        .first()
        .copied()
        .or_else(|| edges.first().map(|e| e[0]))
        .ok_or("the sweep path is empty")?;
    let mut used = BTreeSet::new();
    let mut ordered = Vec::new();
    for _ in 0..edges.len() {
        let (index, edge) = edges
            .iter()
            .enumerate()
            .find(|(i, e)| !used.contains(i) && e.contains(&vertex))
            .ok_or("the sweep path curves do not form one open or closed chain")?;
        used.insert(index);
        ordered.push(entities[index].clone());
        vertex = if edge[0] == vertex { edge[1] } else { edge[0] };
    }
    Ok(ordered)
}

#[cfg(test)]
mod tests;
