// SPDX-License-Identifier: Apache-2.0

const CLOSED_EDGE_RELATIVE_TOLERANCE: f64 = 1e-6;
const PARALLEL_PLANE_TOLERANCE: f64 = 1e-9;
const AXIS_UNIT_TOLERANCE: f64 = 1e-12;
const AXIS_ZERO_TOLERANCE: f64 = 1e-12;
const NEXT_FACE_CLEARANCE: f64 = 1e-3;
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::{
    archive::Bundle, coordinate, features::queries, id, optional_rows, rows, text, Mapping,
};

pub(crate) fn vector(row: &Value, factor: f64) -> Mapping<[f64; 3]> {
    if !row.is_object() {
        return Err("a geometric vector is missing".into());
    }
    let result = [
        coordinate(row, "x")? * factor,
        coordinate(row, "y")? * factor,
        coordinate(row, "z")? * factor,
    ];
    if result.iter().all(|n| n.is_finite()) {
        Ok(result)
    } else {
        Err("geometry exceeds the finite range".into())
    }
}

pub(crate) fn point([x, y, z]: [f64; 3]) -> Value {
    json!({"x": x, "y": y, "z": z})
}
pub(crate) fn sub(
    [first_x, first_y, first_z]: [f64; 3],
    [second_x, second_y, second_z]: [f64; 3],
) -> [f64; 3] {
    [first_x - second_x, first_y - second_y, first_z - second_z]
}
pub(crate) fn cross(
    [first_x, first_y, first_z]: [f64; 3],
    [second_x, second_y, second_z]: [f64; 3],
) -> [f64; 3] {
    [
        first_y * second_z - first_z * second_y,
        first_z * second_x - first_x * second_z,
        first_x * second_y - first_y * second_x,
    ]
}
pub(crate) fn unit(v: [f64; 3]) -> Mapping<[f64; 3]> {
    let length = v.iter().map(|v| v * v).sum::<f64>().sqrt();
    if length > 0.0 && length.is_finite() {
        Ok(v.map(|n| n / length))
    } else {
        Err("geometry has no finite direction".into())
    }
}

fn dot([first_x, first_y, first_z]: [f64; 3], [second_x, second_y, second_z]: [f64; 3]) -> f64 {
    first_x * second_x + first_y * second_y + first_z * second_z
}

fn along(
    [first_x, first_y, first_z]: [f64; 3],
    parameter: f64,
    [second_x, second_y, second_z]: [f64; 3],
) -> [f64; 3] {
    [
        first_x + parameter * second_x,
        first_y + parameter * second_y,
        first_z + parameter * second_z,
    ]
}
fn polyline(edge: &Value) -> Mapping<Vec<[f64; 3]>> {
    let curve = edge
        .get("curve")
        .ok_or("the input edge has no analytic curve")?;
    let geometry = edge
        .get("geometry")
        .ok_or("the input edge has no geometric data")?;
    let at = |key: &str| -> Mapping<[f64; 3]> {
        vector(
            geometry.get(key).ok_or("an input edge point is missing")?,
            1000.0,
        )
    };
    let (start, mid, end) = (at("startPoint")?, at("midPoint")?, at("endPoint")?);
    match text(curve, "type")? {
        "LINE" => Ok(vec![start, end]),
        "CIRCLE" => {
            let center = vector(
                curve.get("origin").ok_or("circle center is missing")?,
                1000.0,
            )?;
            let axis = unit(vector(
                curve
                    .get("normal")
                    .or_else(|| curve.get("direction"))
                    .ok_or("circle axis is missing")?,
                1.0,
            )?)?;
            let offset = sub(start, center);
            let radial = sub(offset, axis.map(|n| n * dot(offset, axis)));
            let radius = dot(radial, radial).sqrt();
            let x = unit(radial)?;
            let y = cross(axis, x);
            let angle = |p: [f64; 3]| {
                let q = sub(p, center);
                dot(q, y).atan2(dot(q, x)).rem_euclid(std::f64::consts::TAU)
            };
            let closed = dot(sub(end, start), sub(end, start)).sqrt()
                <= CLOSED_EDGE_RELATIVE_TOLERANCE * radius.max(1.0);
            let sweep = if closed {
                std::f64::consts::TAU
            } else {
                let to = angle(end);
                if angle(mid) < to {
                    to
                } else {
                    to - std::f64::consts::TAU
                }
            };
            Ok((0..=32)
                .map(|step| {
                    let t = sweep * f64::from(step) / 32.0;
                    along(along(center, radius * t.cos(), x), radius * t.sin(), y)
                })
                .collect())
        }
        other => Err(format!("{other} input edges are not supported yet")),
    }
}
fn within(
    point: [f64; 3],
    origin: [f64; 3],
    normal: [f64; 3],
    boundary: &[Vec<[f64; 3]>],
    on: f64,
) -> Option<bool> {
    let [nx, _, _] = normal;
    let helper = if nx.abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let axis_x = unit(cross(normal, helper)).ok()?;
    let axis_y = cross(normal, axis_x);
    let flat = |p: [f64; 3]| {
        let q = sub(p, origin);
        [dot(q, axis_x), dot(q, axis_y)]
    };
    let [px, py] = flat(point);
    let mut inside = false;
    for points in boundary {
        for pair in points.windows(2) {
            let [a, b] = pair else { continue };
            let ([ax, ay], [bx, by]) = (flat(*a), flat(*b));
            let (dx, dy) = (bx - ax, by - ay);
            let length = dx * dx + dy * dy;
            let t = if length > 0.0 {
                (((px - ax) * dx + (py - ay) * dy) / length).clamp(0.0, 1.0)
            } else {
                0.0
            };
            if (px - ax - t * dx).hypot(py - ay - t * dy) <= on {
                return Some(false);
            }
            if (ay > py) != (by > py) && px < ax + (py - ay) * dx / dy {
                inside = !inside;
            }
        }
    }
    inside.then_some(true)
}

pub(crate) struct State {
    pub row: Value,
    faces: BTreeMap<String, Value>,
    edges: BTreeMap<String, Value>,
    complete_vertices: bool,
}
pub(crate) struct Inputs<'a> {
    bundle: &'a Bundle<'a>,
    prefix: &'a str,
    mid: &'a str,
    indices: BTreeSet<usize>,
    features: &'a [Value],
    pub(super) regions: std::cell::RefCell<super::regions::Recovery>,
}

impl<'a> Inputs<'a> {
    pub(crate) fn new(
        bundle: &'a Bundle<'a>,
        prefix: &'a str,
        mid: &'a str,
        features: &'a [Value],
    ) -> Mapping<Self> {
        let index = bundle
            .json(&format!("{prefix}/bodydetails/index.json"))
            .map_err(|_| "bodydetails/index.json is missing or unreadable; fetch a new bundle")?;
        let entries = index
            .as_array()
            .or_else(|| index.get("indices").and_then(Value::as_array))
            .filter(|rows| rows.len() <= super::ROWS)
            .ok_or("the bodydetails index is missing or exceeds the rollback limit")?;
        let mut indices = BTreeSet::new();
        for entry in entries {
            let ordinal = if let Some(ordinal) = entry.as_u64() {
                usize::try_from(ordinal)
                    .map_err(|_| "a rollback index exceeds this platform's range")?
            } else {
                entry
                    .as_str()
                    .and_then(|path| path.strip_suffix(".json"))
                    .and_then(|i| i.parse::<usize>().ok())
                    .ok_or("a rollback index has no integer or file name")?
            };
            if !indices.insert(ordinal) {
                return Err("the bodydetails index repeats a rollback index".into());
            }
        }
        Ok(Self {
            bundle,
            prefix,
            mid,
            indices,
            features,
            regions: std::cell::RefCell::default(),
        })
    }

    pub(crate) fn sketch_entity(&self, sketch: &str, entity: &str) -> Mapping<&Value> {
        let sketch = self
            .features
            .iter()
            .find(|f| f.get("featureId").and_then(Value::as_str) == Some(sketch))
            .ok_or("the region's source sketch is missing")?;
        rows(sketch, "entities")?
            .iter()
            .find(|e| e.get("entityId").and_then(Value::as_str) == Some(entity))
            .ok_or_else(|| "the region's source sketch entity is missing".into())
    }

    pub(crate) fn load(&self, ordinal: usize, feature: &Value) -> Mapping<State> {
        if !self.indices.contains(&ordinal) {
            return Err(format!(
                "bodydetails for rollback index {ordinal} is missing"
            ));
        }
        load(self.bundle, self.prefix, ordinal, self.mid, feature)
    }
    pub(crate) fn parts(&self, ordinal: usize) -> Mapping<BTreeMap<String, Option<String>>> {
        if !self.indices.contains(&ordinal) {
            return Err(format!(
                "bodydetails for rollback index {ordinal} is missing"
            ));
        }
        let response = self
            .bundle
            .json(&format!("{}/bodydetails/{ordinal}.json", self.prefix))
            .map_err(|_| {
                format!("bodydetails for rollback index {ordinal} is missing or unreadable")
            })?;
        super::microversion(&response, "microversionId", self.mid)?;
        let mut parts = BTreeMap::new();
        for body in rows(&response, "bodies")? {
            if body.get("type").and_then(Value::as_str) != Some("SOLID")
                || body.get("consumedByComposite").and_then(Value::as_bool) == Some(true)
            {
                continue;
            }
            let name = body
                .pointer("/properties/name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .map(str::to_owned);
            if parts.insert(text(body, "id")?.to_owned(), name).is_some() {
                return Err("bodydetails repeats a body id".into());
            }
        }
        Ok(parts)
    }
}

fn load(
    bundle: &Bundle,
    prefix: &str,
    ordinal: usize,
    mid: &str,
    feature: &Value,
) -> Mapping<State> {
    let response = bundle
        .json(&format!("{prefix}/bodydetails/{ordinal}.json"))
        .map_err(|_| {
            format!("bodydetails for rollback index {ordinal} is missing or unreadable")
        })?;
    super::microversion(&response, "microversionId", mid)?;
    let mut faces = BTreeMap::new();
    let mut edges = BTreeMap::new();
    let mut bodies = BTreeSet::new();
    let mut vertices = BTreeSet::new();
    let mut complete_vertices = true;
    let mut samples = Vec::new();
    for body in rows(&response, "bodies")? {
        if !bodies.insert(text(body, "id")?.to_owned()) {
            return Err("bodydetails repeats a body id".into());
        }
        for face in optional_rows(body, "faces")? {
            if faces
                .insert(text(face, "id")?.to_owned(), face.clone())
                .is_some()
            {
                return Err("bodydetails repeats a face id".into());
            }
        }
        let mut body_vertices = BTreeSet::new();
        for vertex in optional_rows(body, "vertices")? {
            let vertex_id = text(vertex, "id")?;
            if !vertices.insert(vertex_id.to_owned()) || !body_vertices.insert(vertex_id.to_owned())
            {
                return Err("bodydetails repeats an input vertex id".into());
            }
        }
        if vertices.len() > super::ROWS {
            return Err("bodydetails exceeds the vertex limit".into());
        }
        for edge in optional_rows(body, "edges")? {
            complete_vertices &=
                edge.get("vertices")
                    .and_then(Value::as_array)
                    .is_some_and(|ids| {
                        ids.len() <= 2
                            && ids
                                .iter()
                                .all(|id| id.as_str().is_some_and(|id| body_vertices.contains(id)))
                    });
            if edges
                .insert(text(edge, "id")?.to_owned(), edge.clone())
                .is_some()
            {
                return Err("bodydetails repeats an edge id".into());
            }
        }
        if faces.len() > super::ROWS || edges.len() > super::ROWS {
            return Err("bodydetails exceeds the topology limit".into());
        }
    }
    let mut adjacency: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (face_id, face) in &faces {
        for lp in rows(face, "loops")? {
            for coedge in rows(lp, "coedges")? {
                let edge = text(coedge, "edgeId")?;
                if !edges.contains_key(edge) {
                    return Err("a face loop refers to a missing input edge".into());
                }
                let uses = adjacency.entry(edge.to_owned()).or_default();
                if !uses.iter().any(|f| f == face_id) {
                    uses.push(face_id.clone());
                }
                if uses.len() > super::ROWS {
                    return Err("bodydetails exceeds the adjacency limit".into());
                }
            }
        }
    }
    for (edge_id, edge) in &edges {
        if let Ok(mut sample) = sample(edge) {
            sample
                .as_object_mut()
                .ok_or("edge sample is missing")?
                .insert(
                    "faces".into(),
                    json!(adjacency.get(edge_id).cloned().unwrap_or_default()),
                );
            samples.push(sample);
        }
    }
    let row = json!({"id": id("input", text(feature, "featureId")?), "input_of": id("feature", text(feature, "featureId")?),
        "bodies": bodies, "faces": faces.keys().collect::<Vec<_>>(), "edges": edges.keys().collect::<Vec<_>>(),
        "vertices": [], "edge_geometry": samples, "native_ref": format!("{prefix}/bodydetails/{ordinal}.json")});
    Ok(State {
        row,
        faces,
        edges,
        complete_vertices,
    })
}

fn sample(edge: &Value) -> Mapping<Value> {
    let curve = edge
        .get("curve")
        .ok_or("the input edge has no analytic curve")?;
    let geometry = edge
        .get("geometry")
        .ok_or("the input edge has no geometric data")?;
    let mid = vector(
        geometry.get("midPoint").ok_or("edge midpoint is missing")?,
        1000.0,
    )?;
    let start = vector(
        geometry.get("startPoint").ok_or("edge start is missing")?,
        1000.0,
    )?;
    let end = vector(
        geometry.get("endPoint").ok_or("edge end is missing")?,
        1000.0,
    )?;
    let tangent = match text(curve, "type")? {
        "LINE" => unit(vector(
            curve.get("direction").ok_or("line direction is missing")?,
            1.0,
        )?)?,
        "CIRCLE" => {
            let center = vector(
                curve.get("origin").ok_or("circle center is missing")?,
                1000.0,
            )?;
            let axis = vector(
                curve
                    .get("normal")
                    .or_else(|| curve.get("direction"))
                    .ok_or("circle axis is missing")?,
                1.0,
            )?;
            unit(cross(axis, sub(mid, center)))?
        }
        other => return Err(format!("{other} input edges are not supported yet")),
    };
    Ok(
        json!({"edge": text(edge, "id")?, "interior": point(mid), "start": point(start), "end": point(end), "tangent": point(tangent)}),
    )
}

impl State {
    pub(crate) fn whole_body_scope(&self, feature: &Value) -> Mapping<()> {
        self.whole_body_scope_key(feature, "booleanScope")
    }

    pub(crate) fn whole_body_scope_key(&self, feature: &Value, key: &str) -> Mapping<()> {
        let bodies = rows(&self.row, "bodies")?;
        if bodies.len() != 1 {
            return Err("Boolean scopes with multiple input bodies are not supported yet".into());
        }
        let mut selected = BTreeSet::new();
        for query in queries(feature, key)? {
            for native in rows(query, "deterministicIds")? {
                let native = native
                    .as_str()
                    .ok_or("a Boolean scope id is not a string")?;
                if !selected.insert(native) {
                    return Err("a Boolean scope repeats a deterministic id".into());
                }
            }
        }
        let body = bodies
            .first()
            .and_then(Value::as_str)
            .ok_or("the input body id is missing")?;
        if selected.len() != 1 || !selected.contains(body) {
            return Err(
                "the Boolean scope does not select the sole body at this feature's rollback index"
                    .into(),
            );
        }
        Ok(())
    }

    pub(crate) fn selection(&self, feature: &Value, key: &str, kind: &str) -> Mapping<Value> {
        let mut selected = BTreeSet::new();
        for query in queries(feature, key)? {
            let ids = rows(query, "deterministicIds")?;
            if ids.is_empty() {
                return Err("query strings without deterministic ids are not supported yet".into());
            }
            for entry in ids {
                let native = entry.as_str().ok_or("a deterministic id is not a string")?;
                let exists = if kind == "edges" {
                    self.edges.contains_key(native)
                } else {
                    self.faces.contains_key(native)
                };
                if !exists {
                    return Err(format!("deterministic {kind} id {native} is absent at this feature's rollback index"));
                }
                if !selected.insert(native) {
                    return Err("a deterministic selection repeats an id".into());
                }
            }
        }
        let mut value = json!({"state": self.row.get("id"), "native": format!("Onshape {key}")});
        value
            .as_object_mut()
            .ok_or("selection is missing")?
            .insert(kind.into(), json!(selected));
        Ok(json!({"kind": "historical", "value": value}))
    }
    pub(crate) fn tangent_chain(&self, selection: &Value) -> Mapping<Value> {
        let reason = "tangent propagation needs complete input vertices and endpoint tangents; this edge set is not supported yet";
        if !self.complete_vertices {
            return Err(reason.into());
        }
        let mut incident: BTreeMap<&str, Vec<(&str, [f64; 3])>> = BTreeMap::new();
        for (edge_id, edge) in &self.edges {
            let vertices = rows(edge, "vertices").map_err(|_| reason)?;
            if vertices.is_empty()
                && edge.pointer("/curve/type").and_then(Value::as_str) == Some("CIRCLE")
            {
                continue;
            }
            let [start, end] = vertices else {
                return Err(reason.into());
            };
            for (vertex, key) in [(start, "startVector"), (end, "endVector")] {
                let vertex = vertex.as_str().ok_or(reason)?;
                let tangent = edge
                    .get("geometry")
                    .and_then(|g| g.get(key))
                    .ok_or(reason)?;
                let tangent =
                    unit(vector(tangent, 1.0).map_err(|_| reason)?).map_err(|_| reason)?;
                incident
                    .entry(vertex)
                    .or_default()
                    .push((edge_id.as_str(), tangent));
            }
        }
        let mut selected: BTreeSet<String> = rows(selection.get("value").ok_or(reason)?, "edges")?
            .iter()
            .map(|id| id.as_str().map(str::to_owned).ok_or(reason))
            .collect::<Result<_, _>>()?;
        let mut pending: Vec<String> = selected.iter().cloned().collect();
        while let Some(edge_id) = pending.pop() {
            let edge = self.edges.get(&edge_id).ok_or(reason)?;
            for vertex in rows(edge, "vertices").map_err(|_| reason)? {
                let neighbours = incident.get(vertex.as_str().ok_or(reason)?).ok_or(reason)?;
                let tangent = neighbours
                    .iter()
                    .find(|(id, _)| *id == edge_id)
                    .map(|(_, t)| *t)
                    .ok_or(reason)?;
                for (other, other_tangent) in neighbours {
                    let dot = tangent
                        .iter()
                        .zip(other_tangent)
                        .map(|(a, b)| a * b)
                        .sum::<f64>()
                        .abs();
                    if dot >= 1.0 - 32.0 * f64::EPSILON && selected.insert((*other).to_owned()) {
                        pending.push((*other).to_owned());
                    }
                }
            }
        }
        let mut result = selection.clone();
        result
            .get_mut("value")
            .and_then(Value::as_object_mut)
            .ok_or(reason)?
            .insert("edges".into(), json!(selected));
        Ok(result)
    }

    pub(crate) fn require_edges(&self, selection: &Value) -> Mapping<()> {
        let ids = selection
            .pointer("/value/edges")
            .and_then(Value::as_array)
            .ok_or("input edge selection is missing")?;
        for edge_id in ids {
            let edge = edge_id
                .as_str()
                .and_then(|id| self.edges.get(id))
                .ok_or("input edge is missing")?;
            sample(edge)?;
        }
        Ok(())
    }
    pub(crate) fn next_face(
        &self,
        samples: &[[f64; 3]],
        travel: [f64; 3],
        cut: bool,
    ) -> Mapping<Value> {
        const ON: f64 = 1e-4;
        let travel = unit(travel)?;
        let mut segments = 0usize;
        let mut planes = Vec::new();
        for (face_id, face) in &self.faces {
            if face.pointer("/surface/type").and_then(Value::as_str) != Some("PLANE") {
                continue;
            }
            let surface = face.get("surface").ok_or("input face surface is missing")?;
            let origin = vector(
                surface.get("origin").ok_or("plane origin is missing")?,
                1000.0,
            )?;
            let normal = unit(vector(
                surface.get("normal").ok_or("plane normal is missing")?,
                1.0,
            )?)?;
            let across = dot(normal, travel);
            if across.abs() < PARALLEL_PLANE_TOLERANCE {
                continue;
            }
            let mut boundary = Vec::new();
            for lp in rows(face, "loops")? {
                for coedge in rows(lp, "coedges")? {
                    let edge = self
                        .edges
                        .get(text(coedge, "edgeId")?)
                        .ok_or("face boundary edge is missing")?;
                    let points = polyline(edge)?;
                    segments = segments
                        .checked_add(points.len())
                        .ok_or("next-face segment count overflows")?;
                    boundary.push(points);
                }
            }
            planes.push((face_id.as_str(), origin, normal, across, boundary));
        }
        if samples
            .len()
            .checked_mul(segments)
            .ok_or("next-face work count overflows")?
            > 50_000_000
        {
            return Err(
                "an up-to-next extrude over this many input faces is not supported yet".into(),
            );
        }
        let mut first = Vec::new();
        for p in samples {
            let mut best: Option<(f64, bool, &str)> = None;
            for (face_id, origin, normal, across, boundary) in &planes {
                let t = dot(sub(*origin, *p), *normal) / across;
                if !t.is_finite() || t <= ON {
                    continue;
                }
                let hit = along(*p, t, travel);
                let Some(inside) = within(hit, *origin, *normal, boundary, ON) else {
                    continue;
                };
                let better = match best {
                    None => true,
                    Some((d, strict, _)) => t < d - ON || (t <= d + ON && inside && !strict),
                };
                if better {
                    best = Some((t, inside, *face_id));
                }
            }
            first.push(best.ok_or(
                "an up-to-next extrude whose profile does not meet one input face everywhere is not supported yet",
            )?);
        }
        let face_id = first
            .first()
            .map(|hit| hit.2)
            .ok_or("the up-to-next profile is empty")?;
        if first.iter().any(|hit| hit.2 != face_id) {
            return Err(
                "an up-to-next extrude that ends on several input faces is not supported yet"
                    .into(),
            );
        }
        let face = self.faces.get(face_id).ok_or("input face is missing")?;
        let normal = unit(vector(
            face.pointer("/surface/normal")
                .ok_or("plane normal is missing")?,
            1.0,
        )?)?;
        let outward = match face.get("orientation").and_then(Value::as_bool) {
            Some(true) => dot(normal, travel),
            Some(false) => -dot(normal, travel),
            None => return Err("the up-to-next face has no resolved orientation".into()),
        };
        if cut != (outward > 0.0) {
            return Err(if cut {
                "an up-to-next cut that starts outside the material is not supported yet"
            } else {
                "an up-to-next extrude that starts inside the material is not supported yet"
            }
            .into());
        }
        let mut low = [f64::INFINITY; 3];
        let mut high = [f64::NEG_INFINITY; 3];
        for (p, (t, _, _)) in samples.iter().zip(&first) {
            for point in [*p, along(*p, *t, travel)] {
                for ((l, h), n) in low.iter_mut().zip(high.iter_mut()).zip(point) {
                    *l = l.min(n);
                    *h = h.max(n);
                }
            }
        }
        for (other_id, other) in &self.faces {
            if other_id.as_str() == face_id {
                continue;
            }
            let corner = |key: &str| -> Mapping<[f64; 3]> {
                vector(
                    other
                        .get("box")
                        .and_then(|b| b.get(key))
                        .ok_or("an input face has no bounding box")?,
                    1000.0,
                )
            };
            let (min, max) = (corner("minCorner")?, corner("maxCorner")?);
            let overlaps = min
                .iter()
                .zip(max)
                .zip(low.iter().zip(high))
                .all(|((a, b), (l, h))| {
                    *a < h - NEXT_FACE_CLEARANCE && b > l + NEXT_FACE_CLEARANCE
                });
            if overlaps {
                return Err("an up-to-next extrude with other input geometry in its path is not supported yet".into());
            }
        }
        let mut value = json!({"state": self.row.get("id"), "native": "Onshape up-to-next face"});
        value
            .as_object_mut()
            .ok_or("selection is missing")?
            .insert("faces".into(), json!([face_id]));
        Ok(json!({"kind": "historical", "value": value}))
    }

    pub(crate) fn require_planes(&self, selection: &Value) -> Mapping<()> {
        let ids = selection
            .pointer("/value/faces")
            .and_then(Value::as_array)
            .ok_or("input face selection is missing")?;
        if ids.len() != 1 {
            return Err("more than one terminating face is not supported yet".into());
        }
        for face_id in ids {
            let face = face_id
                .as_str()
                .and_then(|id| self.faces.get(id))
                .ok_or("input face is missing")?;
            if face.pointer("/surface/type").and_then(Value::as_str) != Some("PLANE") {
                return Err("non-planar terminating faces are not supported yet".into());
            }
            for lp in rows(face, "loops")? {
                for coedge in rows(lp, "coedges")? {
                    sample(
                        self.edges
                            .get(text(coedge, "edgeId")?)
                            .ok_or("face boundary edge is missing")?,
                    )?;
                }
            }
        }
        Ok(())
    }
    pub(crate) fn face_frame(&self, face_id: &str) -> Mapping<Value> {
        let face = self
            .faces
            .get(face_id)
            .ok_or("the sketch face is absent at this feature's rollback index")?;
        let surface = face
            .get("surface")
            .ok_or("the sketch face has no surface")?;
        if surface.get("type").and_then(Value::as_str) != Some("PLANE") {
            return Err("sketches on non-planar faces are not supported yet".into());
        }
        let sense = match face.get("orientation").and_then(Value::as_bool) {
            Some(true) => 1.0,
            Some(false) => -1.0,
            None => return Err("the sketch face has no orientation".into()),
        };
        let normal = unit(vector(
            surface
                .get("normal")
                .ok_or("the sketch face normal is missing")?,
            sense,
        )?)?;
        let origin = vector(
            surface
                .get("origin")
                .ok_or("the sketch face origin is missing")?,
            1000.0,
        )?;
        let ones = normal
            .iter()
            .filter(|n| n.abs() > 1.0 - AXIS_UNIT_TOLERANCE)
            .count();
        let zeros = normal
            .iter()
            .filter(|n| n.abs() < AXIS_ZERO_TOLERANCE)
            .count();
        if ones != 1 || zeros != 2 {
            return Err(
                "sketches on faces that are not parallel to a default plane are not supported yet"
                    .into(),
            );
        }
        let normal = normal.map(f64::round);
        let [_, _, nz] = normal;
        let u_axis = if nz == 0.0 {
            cross([0.0, 0.0, 1.0], normal)
        } else {
            [1.0, 0.0, 0.0]
        };
        let offset = origin.iter().zip(normal).map(|(o, n)| o * n).sum::<f64>();
        Ok(json!({"origin": point(normal.map(|n| n * offset + 0.0)),
            "normal": point(normal.map(|n| n + 0.0)), "u_axis": point(u_axis.map(|n| n + 0.0))}))
    }

    pub(crate) fn axis(&self, feature: &Value, key: &str) -> Mapping<Value> {
        let selection = self.selection(feature, key, "edges")?;
        let ids = selection
            .pointer("/value/edges")
            .and_then(Value::as_array)
            .ok_or("axis selection is missing")?;
        let [edge_id] = ids.as_slice() else {
            return Err("a revolve axis needs exactly one straight input edge".into());
        };
        let edge = edge_id
            .as_str()
            .and_then(|id| self.edges.get(id))
            .ok_or("axis edge is missing")?;
        let curve = edge.get("curve").ok_or("axis curve is missing")?;
        if text(curve, "type")? != "LINE" {
            return Err("a curved revolve axis is not supported yet".into());
        }
        Ok(
            json!({"origin": point(vector(curve.get("origin").ok_or("axis origin is missing")?, 1000.0)?),
            "direction": point(unit(vector(curve.get("direction").ok_or("axis direction is missing")?, 1.0)?)?)}),
        )
    }
}

#[cfg(test)]
impl State {
    pub(crate) fn testing(edges: Vec<Value>) -> Self {
        Self {
            row: Value::Null,
            faces: BTreeMap::new(),
            complete_vertices: true,
            edges: edges
                .into_iter()
                .filter_map(|edge| Some((edge.get("id")?.as_str()?.to_owned(), edge)))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests;
