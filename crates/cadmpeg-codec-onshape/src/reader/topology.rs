// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::json;
use serde_json::Value;

mod next;

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

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OperandKind {
    Body,
    Face,
    Edge,
}

pub(crate) struct State {
    pub row: Value,
    faces: BTreeMap<String, Value>,
    /// Faces of bodies that no composite consumes.
    live_faces: BTreeSet<String>,
    edges: BTreeMap<String, Value>,
    complete_vertices: bool,
}
pub(crate) struct Inputs<'a> {
    bundle: &'a Bundle<'a>,
    prefix: &'a str,
    mid: &'a str,
    sketch_planes: Mapping<BTreeMap<&'a str, Option<&'a Value>>>,
    indices: BTreeSet<usize>,
    features: &'a [Value],
    body_features: Vec<usize>,
    provenance: std::cell::RefCell<Provenance<'a>>,
    pub(super) regions: std::cell::RefCell<super::regions::Recovery>,
    pub(super) sampling_report: std::cell::RefCell<super::Report>,
}

fn rollback_indices(bundle: &Bundle<'_>, prefix: &str) -> Mapping<BTreeSet<usize>> {
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
    Ok(indices)
}

fn sketch_planes<'a>(
    bundle: &'a Bundle<'_>,
    prefix: &str,
) -> Mapping<BTreeMap<&'a str, Option<&'a Value>>> {
    let path = format!("{prefix}/sketches.json");
    if !bundle.contains(&path) {
        return Ok(BTreeMap::new());
    }
    let response = bundle
        .json(&path)
        .map_err(|_| "the sketch attachment response is unreadable")?;
    let mut index = BTreeMap::new();
    for row in rows(response, "sketches")? {
        let Some(native) = row.get("featureId").and_then(Value::as_str) else {
            continue;
        };
        index
            .entry(native)
            .and_modify(|row| *row = None)
            .or_insert(Some(row));
    }
    Ok(index)
}

impl<'a> Inputs<'a> {
    pub(crate) fn new(
        bundle: &'a Bundle<'a>,
        prefix: &'a str,
        mid: &'a str,
        features: &'a [Value],
        report: &mut super::Report,
    ) -> Self {
        let indices = rollback_indices(bundle, prefix).unwrap_or_else(|reason| {
            report.push_coded(
                "bodydetails index",
                crate::loss::OnshapeLossCode::RecordMalformed,
                format!(
                    "{reason}; features that need rollback topology keep the final STEP geometry"
                ),
            );
            BTreeSet::new()
        });
        Self {
            bundle,
            prefix,
            mid,
            indices,
            sketch_planes: sketch_planes(bundle, prefix),
            features,
            body_features: features
                .iter()
                .enumerate()
                .filter(|(_, feature)| {
                    !matches!(
                        feature.get("featureType").and_then(Value::as_str),
                        Some("newSketch" | "assignVariable")
                    ) && feature.get("suppressed").and_then(Value::as_bool) != Some(true)
                        && feature.get("parentSuppressed").and_then(Value::as_bool) != Some(true)
                })
                .map(|(ordinal, _)| ordinal)
                .collect(),
            provenance: std::cell::RefCell::default(),
            regions: std::cell::RefCell::default(),
            sampling_report: std::cell::RefCell::default(),
        }
    }

    pub(crate) fn sketch_plane(&self, native: &str) -> Mapping<Option<Value>> {
        let index = self.sketch_planes.as_ref().map_err(Clone::clone)?;
        match index.get(native) {
            None => Ok(None),
            Some(None) => Err("the sketch attachment identity repeats".into()),
            Some(Some(row)) => super::planes::sketch_plane(row).map(Some),
        }
    }

    pub(crate) fn sketch_entities(&self, sketch: &str) -> Mapping<&[Value]> {
        let sketch = self
            .features
            .iter()
            .find(|feature| feature.get("featureId").and_then(Value::as_str) == Some(sketch))
            .ok_or("the region's source sketch is missing")?;
        rows(sketch, "entities")
    }

    pub(crate) fn sketch_entity(&self, sketch: &str, entity: &str) -> Mapping<&Value> {
        self.sketch_entities(sketch)?
            .iter()
            .find(|source| source.get("entityId").and_then(Value::as_str) == Some(entity))
            .ok_or_else(|| "the region's source sketch entity is missing".into())
    }

    pub(crate) fn load(&self, ordinal: usize, feature: &Value) -> Mapping<State> {
        if !self.indices.contains(&ordinal) {
            return Err(format!(
                "bodydetails for rollback index {ordinal} is missing"
            ));
        }
        let mut report = self
            .sampling_report
            .try_borrow_mut()
            .map_err(|_| "edge sampling report is already in use")?;
        load(
            self.bundle,
            self.prefix,
            ordinal,
            self.mid,
            feature,
            &mut report,
        )
    }
    pub(crate) fn producer(
        &self,
        ordinal: usize,
        native: &str,
        kind: OperandKind,
    ) -> Mapping<String> {
        let mut provenance = self
            .provenance
            .try_borrow_mut()
            .map_err(|_| "feature dependency provenance is already in use")?;
        if let Some((index, reason)) = &provenance.failed {
            if ordinal >= *index {
                return Err(reason.clone());
            }
        }
        if provenance.through.is_none_or(|through| through < ordinal) {
            let lower = provenance
                .through
                .map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded);
            for index in self
                .indices
                .range((lower, std::ops::Bound::Included(ordinal)))
            {
                let response: &'a Value = self
                    .bundle
                    .json(&format!("{}/bodydetails/{index}.json", self.prefix))
                    .map_err(|_| "feature dependency rollback is unreadable")?;
                let current = match operands(response) {
                    Ok(current) => current,
                    Err(reason) => {
                        provenance.failed = Some((*index, reason.clone()));
                        return Err(reason);
                    }
                };
                provenance.observe(*index, current)?;
            }
        }
        let history = provenance
            .history
            .get(&kind)
            .and_then(|by_id| by_id.get(native))
            .ok_or("feature dependency topology id has no rollback producer")?;
        let position = history.partition_point(|transition| transition.at <= ordinal);
        let interval = position
            .checked_sub(1)
            .and_then(|at| history.get(at))
            .and_then(|transition| transition.interval.clone())
            .ok_or("feature dependency topology producer is unresolved")?;
        let first = self
            .body_features
            .partition_point(|feature| *feature < interval.start);
        let end = self
            .body_features
            .partition_point(|feature| *feature < interval.end);
        let candidates = self
            .body_features
            .get(first..end)
            .ok_or("feature dependency rollback range is unresolved")?;
        let [producer] = candidates else {
            return Err(if candidates.is_empty() {
                "feature dependency topology producer is unresolved"
            } else {
                "feature dependency topology producer is ambiguous"
            }
            .into());
        };
        Ok(text(
            self.features
                .get(*producer)
                .ok_or("feature dependency producer is missing")?,
            "featureId",
        )?
        .to_owned())
    }

    /// The feature id names exactly one feature before `ordinal`.
    pub(crate) fn unique_feature(&self, native: &str, ordinal: usize) -> Mapping<()> {
        if self
            .features
            .iter()
            .take(ordinal)
            .filter(|feature| feature.get("featureId").and_then(Value::as_str) == Some(native))
            .count()
            != 1
        {
            return Err("the feature dependency producer id is missing or ambiguous".into());
        }
        Ok(())
    }

    /// Whole-studio references need every row accounted for. An unknown body must
    /// not disappear from the evidence and make a subset look like the whole studio.
    pub(crate) fn solid_parts(&self, ordinal: usize) -> Mapping<BTreeMap<String, Option<String>>> {
        let parts = self.parts(ordinal)?;
        let response = self
            .bundle
            .json(&format!("{}/bodydetails/{ordinal}.json", self.prefix))
            .map_err(|_| "the whole-studio rollback snapshot is unreadable")?;
        for body in rows(response, "bodies")? {
            if text(body, "type")? != "SOLID"
                || body
                    .get("consumedByComposite")
                    .is_some_and(|value| value.as_bool() != Some(false))
            {
                return Err("the whole-studio selection contains an unresolved or non-solid body; select a studio of separate solids".into());
            }
            text(body, "id")?;
        }
        Ok(parts)
    }

    /// The solid parts before and after the feature at `ordinal`. A body that is not a
    /// free solid is accounted for by its id, and the feature leaves each of them as it
    /// was; a sheet or composite body that the feature does not touch does not withhold
    /// the solids.
    pub(crate) fn solid_parts_across(&self, ordinal: usize) -> Mapping<(Parts, Parts)> {
        let others = |at: usize| -> Mapping<BTreeMap<String, String>> {
            let response = self
                .bundle
                .json(&format!("{}/bodydetails/{at}.json", self.prefix))
                .map_err(|_| "the whole-studio rollback snapshot is unreadable")?;
            let mut others = BTreeMap::new();
            for body in rows(response, "bodies")? {
                let kind = text(body, "type")?;
                let consumed = body
                    .get("consumedByComposite")
                    .is_some_and(|value| value.as_bool() != Some(false));
                if kind != "SOLID" || consumed {
                    others.insert(text(body, "id")?.to_owned(), format!("{kind}/{consumed}"));
                }
            }
            Ok(others)
        };
        if others(ordinal)? != others(ordinal + 1)? {
            return Err("the derived feature changes a body that is not a free solid".into());
        }
        Ok((self.parts(ordinal)?, self.parts(ordinal + 1)?))
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
        super::microversion(response, "microversionId", self.mid)?;
        let mut parts = BTreeMap::new();
        for body in rows(response, "bodies")? {
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

struct Operand<'a> {
    record: &'a Value,
    boundary: Vec<(&'a str, usize)>,
}

const F64_EXACT_INTEGER_LIMIT: u64 = 9_007_199_254_740_992;

fn stored_equal(first: &Value, second: &Value) -> bool {
    if std::ptr::eq(first, second) {
        return true;
    }
    match (first, second) {
        (Value::Number(first), Value::Number(second)) => {
            if first == second {
                return true;
            }
            if first.is_f64() == second.is_f64() {
                return false;
            }
            let integer = if first.is_f64() { second } else { first };
            let magnitude = integer
                .as_u64()
                .or_else(|| integer.as_i64().map(i64::unsigned_abs));
            magnitude.is_some_and(|value| value <= F64_EXACT_INTEGER_LIMIT)
                && first.as_f64() == second.as_f64()
        }
        (Value::Array(first), Value::Array(second)) => {
            first.len() == second.len()
                && first
                    .iter()
                    .zip(second)
                    .all(|(first, second)| stored_equal(first, second))
        }
        (Value::Object(first), Value::Object(second)) => {
            first.len() == second.len()
                && first.iter().all(|(key, value)| {
                    second
                        .get(key)
                        .is_some_and(|other| stored_equal(value, other))
                })
        }
        _ => first == second,
    }
}

impl PartialEq for Operand<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.boundary == other.boundary && stored_equal(self.record, other.record)
    }
}

/// The solid parts of one rollback state, by body id, with the part names.
type Parts = BTreeMap<String, Option<String>>;

type Operands<'a> = BTreeMap<OperandKind, BTreeMap<&'a str, Operand<'a>>>;

struct Transition {
    at: usize,
    interval: Option<std::ops::Range<usize>>,
}

#[derive(Default)]
struct Provenance<'a> {
    through: Option<usize>,
    previous: Operands<'a>,
    history: BTreeMap<OperandKind, BTreeMap<&'a str, Vec<Transition>>>,
    failed: Option<(usize, String)>,
}

impl<'a> Provenance<'a> {
    fn observe(&mut self, at: usize, mut current: Operands<'a>) -> Mapping<()> {
        for (kind, previous) in &self.previous {
            for native in previous.keys() {
                if current
                    .get(kind)
                    .and_then(|values| values.get(*native))
                    .is_none()
                {
                    self.history
                        .entry(*kind)
                        .or_default()
                        .entry(*native)
                        .or_default()
                        .push(Transition { at, interval: None });
                }
            }
        }
        for kind in [OperandKind::Edge, OperandKind::Face, OperandKind::Body] {
            let Some(values) = current.get_mut(&kind) else {
                continue;
            };
            for (native, value) in values {
                for (edge, revision) in &mut value.boundary {
                    *revision = self
                        .history
                        .get(&OperandKind::Edge)
                        .and_then(|edges| edges.get(*edge))
                        .and_then(|changes| changes.last())
                        .ok_or("feature dependency boundary has no stored edge revision")?
                        .at;
                }
                if self
                    .previous
                    .get(&kind)
                    .and_then(|values| values.get(*native))
                    != Some(value)
                {
                    self.history
                        .entry(kind)
                        .or_default()
                        .entry(*native)
                        .or_default()
                        .push(Transition {
                            at,
                            interval: Some(self.through.unwrap_or(0)..at),
                        });
                }
            }
        }
        self.previous = current;
        self.through = Some(at);
        Ok(())
    }
}

fn operands(response: &Value) -> Mapping<Operands<'_>> {
    let mut result = Operands::new();
    let mut insert = |kind, native, record, boundary| -> Mapping<()> {
        if result
            .entry(kind)
            .or_default()
            .insert(native, Operand { record, boundary })
            .is_some()
        {
            return Err("feature dependency topology id repeats within its kind".into());
        }
        Ok(())
    };
    for body in rows(response, "bodies")? {
        insert(OperandKind::Body, text(body, "id")?, body, Vec::new())?;
        let mut edges = BTreeMap::new();
        for edge in optional_rows(body, "edges")? {
            let native = text(edge, "id")?;
            if edges.insert(native, edge).is_some() {
                return Err("feature dependency topology repeats an edge".into());
            }
            insert(OperandKind::Edge, native, edge, Vec::new())?;
        }
        for face in optional_rows(body, "faces")? {
            let mut boundary = BTreeMap::new();
            for row in optional_rows(face, "loops")? {
                for coedge in optional_rows(row, "coedges")? {
                    let native = text(coedge, "edgeId")?;
                    if !edges.contains_key(native) {
                        return Err("feature dependency boundary edge is unresolved".into());
                    }
                    boundary.insert(native, 0usize);
                }
            }
            insert(
                OperandKind::Face,
                text(face, "id")?,
                face,
                boundary.into_iter().collect(),
            )?;
        }
    }
    Ok(result)
}

fn load(
    bundle: &Bundle,
    prefix: &str,
    ordinal: usize,
    mid: &str,
    feature: &Value,
    report: &mut super::Report,
) -> Mapping<State> {
    let response = bundle
        .json(&format!("{prefix}/bodydetails/{ordinal}.json"))
        .map_err(|_| {
            format!("bodydetails for rollback index {ordinal} is missing or unreadable")
        })?;
    super::microversion(response, "microversionId", mid)?;
    let mut faces = BTreeMap::new();
    let mut live_faces = BTreeSet::new();
    let mut edges = BTreeMap::new();
    let mut bodies = BTreeSet::new();
    let mut vertices = BTreeSet::new();
    let mut complete_vertices = true;
    let mut samples = Vec::new();
    for body in rows(response, "bodies")? {
        if !bodies.insert(text(body, "id")?.to_owned()) {
            return Err("bodydetails repeats a body id".into());
        }
        let consumed = body.get("consumedByComposite").and_then(Value::as_bool) == Some(true);
        for face in optional_rows(body, "faces")? {
            let face_id = text(face, "id")?;
            if faces.insert(face_id.to_owned(), face.clone()).is_some() {
                return Err("bodydetails repeats a face id".into());
            }
            if !consumed {
                live_faces.insert(face_id.to_owned());
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
        match sample(edge) {
            Ok(mut sample) => {
                let row = sample.as_object_mut().ok_or("edge sample is missing")?;
                row.insert(
                    "faces".into(),
                    json!(adjacency.get(edge_id).cloned().unwrap_or_default()),
                );
                row.insert("supports".into(), json!([]));
                samples.push(sample);
            }
            Err(reason) => report.push_coded(
                edge_id,
                crate::loss::OnshapeLossCode::EdgeGeometryUnsampled,
                format!("rollback index {ordinal}: {reason}; omitted the edge geometry sample"),
            ),
        }
    }
    let row = json!({"id": id("input", text(feature, "featureId")?), "input_of": id("feature", text(feature, "featureId")?),
        "bodies": bodies, "faces": faces.keys().collect::<Vec<_>>(), "edges": edges.keys().collect::<Vec<_>>(),
        "vertices": [], "edge_geometry": samples, "face_owners": [], "face_geometry": [], "body_writers": [], "native_ref": format!("{prefix}/bodydetails/{ordinal}.json")});
    Ok(State {
        row,
        faces,
        live_faces,
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
        feature: &Value,
        profile: &Value,
        model: &cadmpeg_ir::document::Model,
    ) -> Mapping<Value> {
        next::prove(self, feature, profile, model).map_err(|reason| {
            format!(
                "UP_TO_NEXT needs exact coverage and first intersection over the complete profile: {reason}"
            )
        })
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
            .ok_or("the sketch attachment surface is missing")?;
        if text(surface, "type")? != "PLANE" {
            return Err("the sketch attachment surface is not planar".into());
        }
        Ok(
            json!({"origin":point(vector(surface.get("origin").ok_or("the sketch attachment support origin is missing")?,1000.0)?),
            "normal":point(unit(vector(surface.get("normal").ok_or("the sketch attachment support normal is missing")?,1.0)?)?)}),
        )
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
            live_faces: BTreeSet::new(),
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
