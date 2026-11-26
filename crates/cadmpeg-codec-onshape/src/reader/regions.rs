// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::serde_erase::json;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::json;
use cadmpeg_ir::document::Model;
use serde_json::Value;

use super::intersection::ROUNDOFF_FACTOR;
use super::{id, query, rows, text, topology, Mapping};
const WORK_LIMIT: usize = 1_000_000;
const OUTPUT_LIMIT: usize = 20_000;
const OUTPUT_BYTES: usize = 8 << 20;
const BUDGET_LOSS: &str = "the sketch region recovery work or boundary output budget is exhausted; simplify the sketch or split the bundle";

struct Budget {
    work: usize,
    output: usize,
    bytes: usize,
    exhausted: bool,
}

impl Budget {
    fn work(&mut self, count: usize) -> Mapping<()> {
        if self.exhausted {
            return Err(BUDGET_LOSS.into());
        }
        self.work = self.work.checked_sub(count).ok_or_else(|| {
            self.work = 0;
            self.exhausted = true;
            BUDGET_LOSS
        })?;
        Ok(())
    }

    fn output(&mut self, count: usize) -> Mapping<()> {
        if self.exhausted {
            return Err(BUDGET_LOSS.into());
        }
        self.output = self.output.checked_sub(count).ok_or_else(|| {
            self.output = 0;
            self.exhausted = true;
            BUDGET_LOSS
        })?;
        Ok(())
    }

    fn boundary(&mut self, edge: &Edge) -> Mapping<()> {
        self.output(1)?;
        let boundary = Boundary {
            entity: &edge.id,
            parameter_range: edge.range,
            reversed: false,
        };
        serde_json::to_writer(ByteBudget(&mut self.bytes), &boundary).map_err(|_| {
            self.exhausted = true;
            BUDGET_LOSS.to_owned()
        })
    }
}

#[derive(serde::Serialize)]
struct Boundary<'a> {
    entity: &'a str,
    parameter_range: [f64; 2],
    reversed: bool,
}

struct ByteBudget<'a>(&'a mut usize);

impl std::io::Write for ByteBudget<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        let Some(remaining) = self.0.checked_sub(bytes.len()) else {
            *self.0 = 0;
            return Err(std::io::Error::other(BUDGET_LOSS));
        };
        *self.0 = remaining;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) struct Recovery {
    budget: Budget,
    loops: BTreeMap<String, Mapping<Loops>>,
}

impl Recovery {
    pub(super) fn exhausted(&self) -> bool {
        self.budget.exhausted
    }

    pub(super) fn check(&self) -> Mapping<()> {
        if self.exhausted() {
            Err(BUDGET_LOSS.into())
        } else {
            Ok(())
        }
    }
}

impl Default for Recovery {
    fn default() -> Self {
        Self {
            budget: Budget {
                work: WORK_LIMIT,
                output: OUTPUT_LIMIT,
                bytes: OUTPUT_BYTES,
                exhausted: false,
            },
            loops: BTreeMap::new(),
        }
    }
}

struct Loops {
    rings: Vec<Ring>,
    open: Vec<Edge>,
}

fn operation<'a>(query: &'a Value, suffix: &str) -> Mapping<&'a str> {
    let [operation] = rows(query, "operationId")? else {
        return Err("the sketch-region query has multiple source operations".into());
    };
    operation
        .as_str()
        .and_then(|s| s.strip_suffix(suffix))
        .ok_or_else(|| "the sketch-region query has no resolved sketch operation".into())
}

fn kind(query: &Value, entity: &str, query_type: &str) -> Mapping<()> {
    if query.get("entityType").and_then(Value::as_str) != Some(entity)
        || query.get("historyType").and_then(Value::as_str) != Some("CREATION")
        || query.get("queryType").and_then(Value::as_str) != Some(query_type)
    {
        return Err("this sketch-region boundary query is not supported yet".into());
    }
    Ok(())
}

pub(super) fn profile(
    queries: &[Value],
    model: &Model,
    inputs: &topology::Inputs<'_>,
) -> Mapping<Value> {
    let mut recovery = inputs
        .regions
        .try_borrow_mut()
        .map_err(|_| "the sketch-region recovery state is already in use")?;
    let mut sketch = None;
    let mut regions = Vec::new();
    let mut picks = BTreeSet::new();
    for source in queries {
        recovery.budget.work(1)?;
        let [pick] = rows(source, "deterministicIds")? else {
            return Err(
                "a sketch-region query needs one boundary description per deterministic region"
                    .into(),
            );
        };
        let pick = pick
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or("a sketch-region id is missing")?;
        if !picks.insert(pick) {
            return Err("the sketch-region selection repeats a deterministic id".into());
        }
        let resolved = query::resolved(source)?;
        kind(&resolved, "FACE", "IMPRINT")?;
        let native = operation(&resolved, ".imprint")?;
        let sketch_id = id("sketch", native);
        recovery.budget.work(model.sketches.len())?;
        if !model.sketches.iter().any(|s| s.id.as_str() == sketch_id) {
            return Err("the selected sketch has no resolved geometry and plane".into());
        }
        if sketch.as_ref().is_some_and(|s| s != &sketch_id) {
            return Err("regions from multiple sketches are not supported yet".into());
        }
        sketch = Some(sketch_id.clone());
        let [disambiguation] = rows(&resolved, "disambiguationData")? else {
            return Err("the sketch region has no unique boundary disambiguation".into());
        };
        if text(disambiguation, "disambiguationType")? != "TOPOLOGY" {
            return Err("the sketch region has no topology boundary disambiguation".into());
        }
        let mut selected = BTreeSet::new();
        let mut edges = Vec::new();
        let mut outside = true;
        for boundary in rows(disambiguation, "entities")? {
            let [edge, side] = boundary
                .as_array()
                .map(Vec::as_slice)
                .ok_or("a sketch-region boundary is missing")?
            else {
                return Err("a sketch-region boundary has no directed edge".into());
            };
            if !matches!(side.as_i64(), Some(-1 | 1)) {
                return Err("a sketch-region boundary has no resolved side".into());
            }
            kind(edge, "EDGE", "IMPRINT")?;
            if operation(edge, ".imprint")? != native {
                return Err("a sketch-region boundary belongs to another sketch".into());
            }
            let derived = edge
                .get("derivedFrom")
                .ok_or("a sketch-region edge has no source sketch entity")?;
            kind(derived, "EDGE", "SKETCH_ENTITY")?;
            if operation(derived, ".wireOp")? != native {
                return Err("a sketch-region edge belongs to another sketch".into());
            }
            let source_id = text(derived, "sketchEntityId")?;
            let entity_id = id("entity", &format!("{native}/{source_id}"));
            recovery.budget.work(model.sketch_entities.len())?;
            let entity = model
                .sketch_entities
                .iter()
                .find(|e| e.id().as_str() == entity_id)
                .ok_or("a sketch-region boundary has no solved source curve")?;
            let mut entity =
                json::to_value(entity).map_err(|_| "a sketch-region source curve is unreadable")?;
            if entity.get("construction").and_then(Value::as_bool) == Some(true) {
                return Err("a sketch-region boundary selects construction geometry".into());
            }
            outside &= match entity.pointer("/geometry/kind").and_then(Value::as_str) {
                Some("circle") => true,
                Some("ellipse") => entity
                    .pointer("/geometry/bounds")
                    .is_none_or(Value::is_null),
                _ => false,
            };
            if edge.get("disambiguationData").is_some() {
                let range = ellipse_interval(edge, native, source_id, inputs)?;
                entity
                    .get_mut("geometry")
                    .and_then(Value::as_object_mut)
                    .ok_or("the sketch-region ellipse geometry is missing")?
                    .insert("bounds".to_owned(), json!(range));
            }
            let mut resolved = Edge::new(entity_id.clone(), &entity)?;
            let clockwise = super::boolean_field(
                inputs
                    .sketch_entity(native, source_id)?
                    .get("geometry")
                    .unwrap_or(&Value::Null),
                "clockwise",
            )?;
            resolved.side =
                side.as_i64().ok_or("boundary side is missing")? * if clockwise { -1 } else { 1 };
            outside &= resolved.side == -1;
            let key = (entity_id, resolved.range.map(f64::to_bits));
            if !selected.insert(key) {
                return Err("a sketch-region boundary repeats a source curve; its trimmed intervals are ambiguous".into());
            }
            edges.push(resolved);
        }
        if edges.is_empty() {
            return Err("the sketch region has no resolved boundary edges".into());
        }
        attach_tangencies(&sketch_id, &mut edges, model, &mut recovery.budget)?;
        let region = if outside {
            exterior_region(&sketch_id, &edges, model, &mut recovery)?
        } else {
            recovery
                .budget
                .work(edges.len().checked_mul(edges.len()).ok_or(BUDGET_LOSS)?)?;
            for edge in &edges {
                recovery.budget.boundary(edge)?;
            }
            region(edges)?
        };
        if regions.contains(&region) {
            return Err("different sketch-region ids describe the same boundary".into());
        }
        regions.push(region);
    }
    Ok(
        json!({"kind": "sketch_regions", "value": {"sketch": sketch.ok_or("the sketch-region selection is empty")?, "regions": regions}}),
    )
}

/// The regions `qSketchRegion(sketch, true)` selects: every region not contained in another
/// region, each with the loops directly inside it as holes. The whole sketch when no loop is
/// contained. Two crossing loops keep the whole sketch; crossing loops among three or more, and
/// loops nested more than one level, are refused.
pub(super) fn filtered_profile(
    sketch: &str,
    model: &Model,
    inputs: &topology::Inputs<'_>,
) -> Mapping<Value> {
    let mut recovery = inputs
        .regions
        .try_borrow_mut()
        .map_err(|_| "the sketch-region recovery state is already in use")?;
    let Recovery { budget, loops } = &mut *recovery;
    let topology = loops
        .entry(sketch.to_owned())
        .or_insert_with(|| source_loops(sketch, model, budget))
        .as_ref()
        .map_err(Clone::clone)?;
    if topology.open.is_empty() {
        if let [first, second] = topology.rings.as_slice() {
            if has_exterior_vertex(first, second, budget)?
                && has_exterior_vertex(second, first, budget)?
            {
                return Ok(json!({"kind":"sketch", "value":sketch}));
            }
        }
    }
    let mut containers: Vec<Vec<usize>> = topology.rings.iter().map(|_| Vec::new()).collect();
    for (i, ring) in topology.rings.iter().enumerate() {
        let point = ring.edges.first().ok_or("the sketch region is empty")?.0.a;
        for (j, other) in topology.rings.iter().enumerate() {
            if i == j {
                continue;
            }
            budget.work(
                ring.edges
                    .len()
                    .checked_mul(other.edges.len())
                    .ok_or(BUDGET_LOSS)?,
            )?;
            admit_separate_rings(ring, other)?;
            budget.work(other.edges.len())?;
            if other.contains(point)? {
                containers.get_mut(i).ok_or(BUDGET_LOSS)?.push(j);
            }
        }
    }
    if containers.iter().any(|c| c.len() > 1) {
        return Err(
            "filtered sketch regions nested more than one level are not supported yet".into(),
        );
    }
    if containers.iter().all(Vec::is_empty) {
        return Ok(json!({"kind":"sketch", "value":sketch}));
    }
    for edge in &topology.open {
        if !isolated(edge, topology, budget)? {
            return Err("the filtered sketch regions have an unresolved open divider".into());
        }
    }
    let mut regions = Vec::new();
    for (i, outer) in topology.rings.iter().enumerate() {
        if !containers.get(i).ok_or(BUDGET_LOSS)?.is_empty() {
            continue;
        }
        let mut holes = Vec::new();
        for (j, hole) in topology.rings.iter().enumerate() {
            budget.work(1)?;
            if containers.get(j).ok_or(BUDGET_LOSS)?.as_slice() == [i] {
                for (edge, _) in &hole.edges {
                    budget.boundary(edge)?;
                }
                holes.push(hole.oriented(false)?);
            }
        }
        for (edge, _) in &outer.edges {
            budget.boundary(edge)?;
        }
        regions.push(
            json!({"kind":"trimmed", "outer_boundary":outer.oriented(true)?, "hole_boundaries":holes}),
        );
    }
    Ok(json!({"kind":"sketch_regions", "value":{"sketch":sketch, "regions":regions}}))
}

fn has_exterior_vertex(inner: &Ring, outer: &Ring, budget: &mut Budget) -> Mapping<bool> {
    for axis in [[1.0, 0.0], [0.0, 1.0]] {
        let mut low = f64::INFINITY;
        let mut high = f64::NEG_INFINITY;
        for (edge, _) in &outer.edges {
            budget.work(1)?;
            let [from, to] = edge.projection(axis, edge.conic()?);
            low = low.min(from);
            high = high.max(to);
        }
        if !low.is_finite() || !high.is_finite() {
            return Ok(false);
        }
        for (edge, _) in &inner.edges {
            budget.work(1)?;
            for point in [edge.a, edge.b] {
                let value = dot(point, axis);
                let scale = low.abs().max(high.abs()).max(value.abs()).max(1.0);
                let roundoff = ROUNDOFF_FACTOR * f64::EPSILON * scale;
                if value < low - roundoff || value > high + roundoff {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn ellipse_interval(
    edge: &Value,
    native: &str,
    source: &str,
    inputs: &topology::Inputs<'_>,
) -> Mapping<[f64; 2]> {
    let refused = "the sketch-region intersection trimming is not supported yet";
    let entity = inputs.sketch_entity(native, source)?;
    if text(entity, "btType")? != "BTMSketchCurve-4" {
        return Err(refused.into());
    }
    let geometry = entity.get("geometry").ok_or(refused)?;
    if text(geometry, "btType")? != "BTCurveGeometryEllipse-1189" {
        return Err(refused.into());
    }
    let cx = super::coordinate(geometry, "xCenter")? * 1000.0;
    let cy = super::coordinate(geometry, "yCenter")? * 1000.0;
    let angle = super::coordinate(geometry, "yDir")?.atan2(super::coordinate(geometry, "xDir")?);
    let major = super::number(geometry, "radius")? * 1000.0;
    let minor = super::number(geometry, "minorRadius")? * 1000.0;
    let clockwise = geometry.get("clockwise").and_then(Value::as_bool) == Some(true);
    let sign = if clockwise { -1.0 } else { 1.0 };
    let [data] = rows(edge, "disambiguationData")? else {
        return Err(refused.into());
    };
    if text(data, "disambiguationType")? != "TOPOLOGY" {
        return Err(refused.into());
    }
    let mut start = None;
    let mut end = None;
    for vertex in rows(data, "entities")? {
        let [vertex, side] = vertex.as_array().map(Vec::as_slice).ok_or(refused)? else {
            return Err(refused.into());
        };
        kind(vertex, "VERTEX", "INTERSECT")?;
        if operation(vertex, ".imprint")? != native {
            return Err(refused.into());
        }
        let [first, second] = rows(vertex, "derivedFrom")? else {
            return Err(refused.into());
        };
        for curve in [first, second] {
            kind(curve, "EDGE", "SKETCH_ENTITY")?;
            if operation(curve, ".wireOp")? != native {
                return Err(refused.into());
            }
        }
        if text(first, "sketchEntityId")? != source {
            return Err(refused.into());
        }
        let chord = super::sketch::geometry(
            inputs.sketch_entity(native, text(second, "sketchEntityId")?)?,
        )?;
        if text(&chord, "kind")? != "line" {
            return Err(refused.into());
        }
        let mut candidates = Vec::new();
        for key in ["start", "end"] {
            let point = chord.get(key).ok_or(refused)?;
            let x = super::number(point, "u")? - cx;
            let y = super::number(point, "v")? - cy;
            let u = (angle.cos() * x + angle.sin() * y) / major;
            let v = (-angle.sin() * x + angle.cos() * y) / minor;
            let residual = (u * u + v * v - 1.0).abs();
            if !residual.is_finite() || residual > 32.0 * f64::EPSILON {
                return Err(
                    "the sketch-region intersection is not a solved ellipse endpoint".into(),
                );
            }
            candidates.push((sign * v.atan2(u)).rem_euclid(std::f64::consts::TAU));
        }
        candidates.sort_by(f64::total_cmp);
        let [a, b] = candidates.as_slice() else {
            return Err(refused.into());
        };
        if a == b {
            return Err("the sketch-region intersection order is ambiguous".into());
        }
        let [order] = rows(vertex, "disambiguationData")? else {
            return Err(refused.into());
        };
        if text(order, "disambiguationType")? != "ORDER" {
            return Err(refused.into());
        }
        let parameter = match order.get("order").and_then(Value::as_u64) {
            Some(0) => *a,
            Some(1) => *b,
            _ => return Err(refused.into()),
        };
        let target = match side.as_i64() {
            Some(1) => &mut start,
            Some(-1) => &mut end,
            _ => return Err(refused.into()),
        };
        if target.replace(parameter).is_some() {
            return Err(refused.into());
        }
    }
    let (start, end) = (start.ok_or(refused)?, end.ok_or(refused)?);
    let (start, mut end) = if clockwise {
        (-end, -start)
    } else {
        (start, end)
    };
    if end < start {
        end += std::f64::consts::TAU;
    }
    if end == start {
        return Err("the sketch-region intersection interval is ambiguous".into());
    }
    Ok([start, end])
}

pub(super) fn curve_endpoints(entity: &Value) -> Mapping<[[f64; 2]; 2]> {
    let edge = Edge::new(String::new(), entity)?;
    Ok([edge.a, edge.b])
}

struct Edge {
    id: String,
    a: [f64; 2],
    b: [f64; 2],
    range: [f64; 2],
    closed: bool,
    geometry: Value,
    side: i64,
    tangent_peers: BTreeSet<String>,
}

impl Edge {
    fn new(id: String, entity: &Value) -> Mapping<Self> {
        let geometry = entity
            .get("geometry")
            .ok_or("a sketch-region curve has no geometry")?;
        let point = |key| -> Mapping<[f64; 2]> {
            let point = geometry
                .get(key)
                .ok_or("a sketch-region endpoint is missing")?;
            Ok([super::number(point, "u")?, super::number(point, "v")?])
        };
        let (a, b, range, closed) = match text(geometry, "kind")? {
            "line" => {
                let a = point("start")?;
                let b = point("end")?;
                (a, b, [0.0, 1.0], false)
            }
            kind @ ("circle" | "arc") => {
                let [cx, cy] = point("center")?;
                let radius = super::number(geometry, "radius")?;
                let from = if kind == "circle" {
                    0.0
                } else {
                    super::number(geometry, "start_angle")?
                };
                let mut to = if kind == "circle" {
                    std::f64::consts::TAU
                } else {
                    super::number(geometry, "end_angle")?
                };
                if to < from {
                    to += std::f64::consts::TAU;
                }
                let at = |t: f64| [cx + radius * t.cos(), cy + radius * t.sin()];
                let a = at(from);
                let b = if kind == "circle" { a } else { at(to) };
                (a, b, [from, to], kind == "circle")
            }
            "ellipse" => {
                let [cx, cy] = point("center")?;
                let major = super::number(geometry, "major_radius")?;
                let minor = super::number(geometry, "minor_radius")?;
                let angle = super::number(geometry, "major_angle")?;
                let bounds = geometry.get("bounds").filter(|v| !v.is_null());
                let (from, to) = if let Some(bounds) = bounds {
                    let bounds = bounds.as_array().ok_or("ellipse bounds are missing")?;
                    let from = bounds
                        .first()
                        .and_then(Value::as_f64)
                        .ok_or("ellipse start is missing")?;
                    let to = bounds
                        .get(1)
                        .and_then(Value::as_f64)
                        .ok_or("ellipse end is missing")?;
                    (from, to)
                } else {
                    (0.0, std::f64::consts::TAU)
                };
                let at = |t: f64| {
                    [
                        cx + major * angle.cos() * t.cos() - minor * angle.sin() * t.sin(),
                        cy + major * angle.sin() * t.cos() + minor * angle.cos() * t.sin(),
                    ]
                };
                let a = at(from);
                let b = if bounds.is_none() { a } else { at(to) };
                (a, b, [from, to], bounds.is_none())
            }
            _ => {
                return Err(
                    "the sketch-region source curve has no resolved boundary parameter range"
                        .into(),
                )
            }
        };
        if a.iter().chain(&b).chain(&range).any(|n| !n.is_finite()) {
            return Err("the sketch-region boundary exceeds the finite range".into());
        }
        if text(geometry, "kind")? != "line"
            && !(range[1] > range[0] && range[1] - range[0] <= std::f64::consts::TAU)
        {
            return Err("the sketch-region curve interval is missing or exceeds one turn".into());
        }
        Ok(Self {
            id,
            a,
            b,
            range,
            closed,
            geometry: geometry.clone(),
            side: 1,
            tangent_peers: BTreeSet::new(),
        })
    }

    fn boundary(&self, reverse: bool) -> Value {
        json!({"entity": self.id, "parameter_range": self.range, "reversed": reverse})
    }
}
fn isolated(edge: &Edge, loops: &Loops, budget: &mut Budget) -> Mapping<bool> {
    let [from, to] = edge.range;
    if !(matches!(text(&edge.geometry, "kind")?, "line")
        || to > from && to - from < std::f64::consts::TAU)
    {
        return Ok(false);
    }
    for other in loops
        .rings
        .iter()
        .flat_map(|ring| ring.edges.iter().map(|(e, _)| e))
        .chain(&loops.open)
    {
        if std::ptr::eq(edge, other) {
            continue;
        }
        budget.work(1)?;
        if !separated(edge, other)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn attach_tangencies(
    sketch: &str,
    edges: &mut [Edge],
    model: &Model,
    budget: &mut Budget,
) -> Mapping<()> {
    budget.work(
        model
            .sketch_constraints
            .len()
            .checked_add(edges.len())
            .ok_or(BUDGET_LOSS)?,
    )?;
    let mut peers: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for constraint in &model.sketch_constraints {
        // Only an applied constraint of the region's own sketch states a contact.
        if constraint.sketch.as_str() != sketch
            || constraint.active == Some(false)
            || constraint.driving == Some(false)
        {
            continue;
        }
        let wire =
            json::to_value(constraint).map_err(|_| "a stored tangent constraint is unreadable")?;
        let Some(definition) = wire.get("definition") else {
            continue;
        };
        if definition.get("kind").and_then(Value::as_str) != Some("tangent") {
            continue;
        }
        let first = text(definition, "first")?;
        let second = text(definition, "second")?;
        peers.entry(first.into()).or_default().insert(second.into());
        peers.entry(second.into()).or_default().insert(first.into());
    }
    for edge in edges {
        if let Some(peers) = peers.get(&edge.id) {
            budget.work(peers.len())?;
            edge.tangent_peers = peers.clone();
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct Conic {
    center: [f64; 2],
    radii: [f64; 2],
    axis: [f64; 2],
}

impl Conic {
    fn vectors(self) -> ([f64; 2], [f64; 2]) {
        let [a, b] = self.radii;
        let [c, s] = self.axis;
        ([a * c, a * s], [-b * s, b * c])
    }

    fn normalize(self, [point_x, point_y]: [f64; 2]) -> [f64; 2] {
        let [a, b] = self.radii;
        let [c, s] = self.axis;
        [
            (c * point_x + s * point_y) / a,
            (-s * point_x + c * point_y) / b,
        ]
    }
}

impl Edge {
    fn conic(&self) -> Mapping<Option<Conic>> {
        let g = &self.geometry;
        if text(g, "kind")? == "line" {
            return Ok(None);
        }
        let center = g.get("center").ok_or("the region carrier has no center")?;
        let center = [super::number(center, "u")?, super::number(center, "v")?];
        let (a, b, angle) = if text(g, "kind")? == "ellipse" {
            (
                super::number(g, "major_radius")?,
                super::number(g, "minor_radius")?,
                super::number(g, "major_angle")?,
            )
        } else {
            let radius = super::number(g, "radius")?;
            (radius, radius, 0.0)
        };
        if a <= 0.0 || b <= 0.0 {
            return Err("conic radii that are not positive are not supported yet".into());
        }
        Ok(Some(Conic {
            center,
            radii: [a, b],
            axis: [angle.cos(), angle.sin()],
        }))
    }

    fn projection(&self, axis: [f64; 2], conic: Option<Conic>) -> [f64; 2] {
        let Some(conic) = conic else {
            let a = dot(self.a, axis);
            let b = dot(self.b, axis);
            return if a.is_finite() && b.is_finite() {
                [a.min(b), a.max(b)]
            } else {
                [f64::NEG_INFINITY, f64::INFINITY]
            };
        };
        let (cosine, sine) = conic.vectors();
        let [lo, hi] = trig_bounds(dot(cosine, axis), dot(sine, axis), self.chart());
        let center = dot(conic.center, axis);
        [center + lo, center + hi]
    }

    fn chart(&self) -> [f64; 2] {
        let [from, to] = self.range;
        let start = from.sin().atan2(from.cos());
        [start, start + (to - from)]
    }

    fn separation_axis(&self, conic: Option<Conic>) -> Option<[f64; 2]> {
        let Some(conic) = conic else {
            let [ax, ay] = self.a;
            let [bx, by] = self.b;
            return Some([ay - by, bx - ax]);
        };
        if self.closed {
            return None;
        }
        let [from, to] = self.chart();
        let middle = (from + to) * 0.5;
        let [a, b] = conic.radii;
        let [c, s] = conic.axis;
        let normal_x = middle.cos() / a;
        let normal_y = middle.sin() / b;
        Some([c * normal_x - s * normal_y, s * normal_x + c * normal_y])
    }
}

fn dot([x, y]: [f64; 2], [u, v]: [f64; 2]) -> f64 {
    x * u + y * v
}
fn trig_bounds(c: f64, s: f64, [from, to]: [f64; 2]) -> [f64; 2] {
    let amplitude = c.hypot(s);
    if ![c, s, from, to, amplitude].iter().all(|v| v.is_finite()) {
        return [f64::NEG_INFINITY, f64::INFINITY];
    }
    let span = to - from;
    if !(span > 0.0 && span < std::f64::consts::TAU) {
        return [-amplitude, amplitude];
    }
    let a = c * from.cos() + s * from.sin();
    let b = c * to.cos() + s * to.sin();
    if !a.is_finite() || !b.is_finite() {
        return [f64::NEG_INFINITY, f64::INFINITY];
    }
    let mut low = a.min(b);
    let mut high = a.max(b);
    let tolerance =
        ROUNDOFF_FACTOR * f64::EPSILON * from.abs().max(to.abs()).max(std::f64::consts::TAU);
    let phase = s.atan2(c);
    for (angle, maximum) in [(phase, true), (phase + std::f64::consts::PI, false)] {
        let offset = (angle - from).rem_euclid(std::f64::consts::TAU);
        if offset <= span + tolerance || std::f64::consts::TAU - offset <= tolerance {
            if maximum {
                high = amplitude;
            } else {
                low = -amplitude;
            }
        }
    }
    [low, high]
}

fn separated(a: &Edge, b: &Edge) -> Mapping<bool> {
    let ca = a.conic()?;
    let cb = b.conic()?;
    let scale =
        a.a.into_iter()
            .chain(a.b)
            .chain(b.a)
            .chain(b.b)
            .chain(
                ca.into_iter()
                    .chain(cb)
                    .flat_map(|c| c.center.into_iter().chain(c.radii)),
            )
            .map(f64::abs)
            .fold(1.0_f64, f64::max);
    let roundoff = ROUNDOFF_FACTOR * f64::EPSILON * scale;
    for axis in [
        Some([1.0, 0.0]),
        Some([0.0, 1.0]),
        a.separation_axis(ca),
        b.separation_axis(cb),
    ]
    .into_iter()
    .flatten()
    {
        let [x, y] = axis;
        let length = x.hypot(y);
        if !length.is_finite() || length == 0.0 {
            continue;
        }
        let axis = [x / length, y / length];
        let [al, ah] = a.projection(axis, ca);
        let [bl, bh] = b.projection(axis, cb);
        if [al, ah, bl, bh].iter().all(|v| v.is_finite())
            && (al > bh + roundoff || bl > ah + roundoff)
        {
            return Ok(true);
        }
    }
    if let (Some(conic), None) | (None, Some(conic)) = (ca, cb) {
        let line = if ca.is_none() { a } else { b };
        let offset = conic.normalize([line.a[0] - conic.center[0], line.a[1] - conic.center[1]]);
        let delta = conic.normalize([line.b[0] - line.a[0], line.b[1] - line.a[1]]);
        let qa = dot(delta, delta);
        let qb = 2.0 * dot(offset, delta);
        let qc = dot(offset, offset) - 1.0;
        let uncertainty = ROUNDOFF_FACTOR
            * f64::EPSILON
            * (qb * qb + 4.0 * qa * qc.abs()).max(1.0)
            * (1.0 + scale / conic.radii[0].min(conic.radii[1]));
        if (qb * qb - 4.0 * qa * qc).abs() <= uncertainty {
            return Ok(false);
        }
    }
    if let (Some(ca), Some(cb)) = (ca, cb) {
        for (edge, source, target) in [(a, ca, cb), (b, cb, ca)] {
            if conic_separated(edge, source, target, scale) {
                return Ok(true);
            }
        }
    }
    if let (Some(ca), Some(cb)) = (ca, cb) {
        if ca.center == cb.center && ca.radii == cb.radii && ca.axis == cb.axis {
            return Ok(false);
        }
    }
    if ca.is_none()
        && cb.is_none()
        && cross(
            [a.b[0] - a.a[0], a.b[1] - a.a[1]],
            [b.b[0] - b.a[0], b.b[1] - b.a[1]],
        ) == 0.0
    {
        return Ok(false);
    }
    Ok(intersections(a, b)?.is_empty())
}
fn conic_separated(edge: &Edge, source: Conic, target: Conic, scale: f64) -> bool {
    let [x, y] = source.center;
    let [cx, cy] = target.center;
    let [u0, v0] = target.normalize([x - cx, y - cy]);
    let (cosine, sine) = source.vectors();
    let [uc, vc] = target.normalize(cosine);
    let [us, vs] = target.normalize(sine);
    let constant = u0 * u0 + v0 * v0 + (uc * uc + vc * vc + us * us + vs * vs) * 0.5;
    let c1 = 2.0 * (u0 * uc + v0 * vc);
    let s1 = 2.0 * (u0 * us + v0 * vs);
    let c2 = (uc * uc + vc * vc - us * us - vs * vs) * 0.5;
    let s2 = uc * us + vc * vs;
    let [from, to] = edge.chart();
    let [l1, h1] = trig_bounds(c1, s1, [from, to]);
    let [l2, h2] = trig_bounds(c2, s2, [2.0 * from, 2.0 * to]);
    let [major, minor] = target.radii;
    let magnitude = (constant.abs() + c1.hypot(s1) + c2.hypot(s2)).max(1.0);
    let tolerance = ROUNDOFF_FACTOR * f64::EPSILON * magnitude * (1.0 + scale / major.min(minor));
    let low = constant + l1 + l2;
    let high = constant + h1 + h2;
    [low, high, tolerance].iter().all(|v| v.is_finite())
        && (low > 1.0 + tolerance || high < 1.0 - tolerance)
}

fn on_curve(edge: &Edge, point: [f64; 2]) -> Mapping<bool> {
    let Some(conic) = edge.conic()? else {
        let delta = [edge.b[0] - edge.a[0], edge.b[1] - edge.a[1]];
        let length = dot(delta, delta);
        if length == 0.0 {
            return Err("a boundary line has no direction".into());
        }
        let t = dot([point[0] - edge.a[0], point[1] - edge.a[1]], delta) / length;
        let slack = parameter_slack(edge.a.into_iter().chain(edge.b).chain(point), length.sqrt());
        return Ok((-slack..=1.0 + slack).contains(&t));
    };
    if edge.closed {
        return Ok(true);
    }
    let [u, v] = conic.normalize([point[0] - conic.center[0], point[1] - conic.center[1]]);
    let offset = (v.atan2(u) - edge.range[0]).rem_euclid(std::f64::consts::TAU);
    let roundoff = ROUNDOFF_FACTOR * f64::EPSILON * std::f64::consts::TAU;
    Ok(offset <= edge.range[1] - edge.range[0] + roundoff
        || std::f64::consts::TAU - offset <= roundoff)
}

fn intersections(a: &Edge, b: &Edge) -> Mapping<Vec<[f64; 2]>> {
    let candidates = if a.tangent_peers.contains(&b.id) || b.tangent_peers.contains(&a.id) {
        if let Some(point) = tangent_contact(a, b)? {
            vec![point]
        } else {
            carrier_intersections(a, b).map_err(|reason| {
                format!(
                    "the {} and {} boundary intersection is unresolved: {reason}",
                    a.geometry
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown"),
                    b.geometry
                        .get("kind")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                )
            })?
        }
    } else {
        carrier_intersections(a, b).map_err(|reason| {
            format!(
                "the {} and {} boundary intersection is unresolved: {reason}",
                a.geometry
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown"),
                b.geometry
                    .get("kind")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
            )
        })?
    };
    if candidates.iter().flatten().any(|n| !n.is_finite()) {
        return Err("the analytic boundary intersection exceeds the finite range".into());
    }
    candidates
        .into_iter()
        .filter_map(|point| {
            match on_curve(a, point).and_then(|x| on_curve(b, point).map(|y| x && y)) {
                Ok(true) => Some(Ok(point)),
                Ok(false) => None,
                Err(reason) => Some(Err(reason)),
            }
        })
        .collect()
}
fn circular(conic: Conic) -> bool {
    let [a, b] = conic.radii;
    (a - b).abs() <= ROUNDOFF_FACTOR * f64::EPSILON * (a + b)
}

fn tangent_contact(first: &Edge, second: &Edge) -> Mapping<Option<[f64; 2]>> {
    let roundoff = ROUNDOFF_FACTOR * f64::EPSILON;
    match (first.conic()?, second.conic()?) {
        (Some(conic), None) | (None, Some(conic)) => {
            let line = if first.conic()?.is_none() {
                first
            } else {
                second
            };
            let offset =
                conic.normalize([line.a[0] - conic.center[0], line.a[1] - conic.center[1]]);
            let delta = conic.normalize([line.b[0] - line.a[0], line.b[1] - line.a[1]]);
            let length = dot(delta, delta);
            if length == 0.0 || !length.is_finite() {
                return Err("the stored tangent line has no finite direction".into());
            }
            let t = -dot(offset, delta) / length;
            let normal_distance = cross(offset, delta).abs() / length.sqrt();
            if !t.is_finite()
                || !normal_distance.is_finite()
                || (normal_distance - 1.0).abs() > roundoff * (normal_distance + 1.0)
            {
                return Err(
                    "the stored tangent constraint contradicts its line and conic carriers".into(),
                );
            }
            Ok(Some([
                line.a[0] + t * (line.b[0] - line.a[0]),
                line.a[1] + t * (line.b[1] - line.a[1]),
            ]))
        }
        (Some(ca), Some(cb)) if circular(ca) && circular(cb) => {
            let delta = [cb.center[0] - ca.center[0], cb.center[1] - ca.center[1]];
            let center_distance = delta[0].hypot(delta[1]);
            let first_radius = ca.radii[0];
            let second_radius = cb.radii[0];
            if center_distance == 0.0 || !center_distance.is_finite() {
                return Err("the stored tangent circles have no distinct finite centers".into());
            }
            let tolerance = roundoff * (center_distance + first_radius + second_radius);
            let external = (center_distance - first_radius - second_radius).abs() <= tolerance;
            let internal =
                (center_distance - (first_radius - second_radius).abs()).abs() <= tolerance;
            if external == internal {
                return Err("the stored tangent constraint contradicts its circle carriers".into());
            }
            let along = if external || first_radius > second_radius {
                first_radius
            } else {
                -first_radius
            };
            Ok(Some([
                ca.center[0] + along * delta[0] / center_distance,
                ca.center[1] + along * delta[1] / center_distance,
            ]))
        }
        _ => Ok(None),
    }
}
fn carrier_intersections(first: &Edge, second: &Edge) -> Mapping<Vec<[f64; 2]>> {
    let candidates = match (first.conic()?, second.conic()?) {
        (None, None) => {
            let da = [first.b[0] - first.a[0], first.b[1] - first.a[1]];
            let db = [second.b[0] - second.a[0], second.b[1] - second.a[1]];
            let offset = [second.a[0] - first.a[0], second.a[1] - first.a[1]];
            let determinant = cross(da, db);
            let first_numerator = cross(offset, db);
            let second_numerator = cross(offset, da);
            if ![determinant, first_numerator, second_numerator]
                .iter()
                .all(|n| n.is_finite())
            {
                return Err("the analytic line intersection exceeds the finite range".into());
            }
            if determinant == 0.0 {
                if cross(offset, da) == 0.0 {
                    return Err("the analytic boundary lines overlap or touch".into());
                }
                Vec::new()
            } else {
                let t = first_numerator / determinant;
                let u = second_numerator / determinant;
                if !t.is_finite() || !u.is_finite() {
                    return Err(
                        "the analytic line intersection parameters exceed the finite range".into(),
                    );
                }
                let conditioning = 1.0 + offset[0].hypot(offset[1]) / determinant.abs();
                let slack_t = ROUNDOFF_FACTOR * f64::EPSILON * conditioning * db[0].hypot(db[1]);
                let slack_u = ROUNDOFF_FACTOR * f64::EPSILON * conditioning * da[0].hypot(da[1]);
                if (-slack_t..=1.0 + slack_t).contains(&t)
                    && (-slack_u..=1.0 + slack_u).contains(&u)
                {
                    vec![[first.a[0] + t * da[0], first.a[1] + t * da[1]]]
                } else {
                    Vec::new()
                }
            }
        }
        (Some(conic), None) | (None, Some(conic)) => {
            let line = if first.conic()?.is_none() {
                first
            } else {
                second
            };
            let offset =
                conic.normalize([line.a[0] - conic.center[0], line.a[1] - conic.center[1]]);
            let delta = conic.normalize([line.b[0] - line.a[0], line.b[1] - line.a[1]]);
            let slack = ROUNDOFF_FACTOR * f64::EPSILON;
            super::intersection::roots(&[
                dot(offset, offset) - 1.0,
                2.0 * dot(offset, delta),
                dot(delta, delta),
            ])?
            .into_iter()
            .filter(|t| (-slack..=1.0 + slack).contains(t))
            .map(|t| {
                [
                    line.a[0] + t * (line.b[0] - line.a[0]),
                    line.a[1] + t * (line.b[1] - line.a[1]),
                ]
            })
            .collect()
        }
        (Some(source), Some(target)) => {
            let offset = target.normalize([
                source.center[0] - target.center[0],
                source.center[1] - target.center[1],
            ]);
            let (cosine, sine) = source.vectors();
            let cosine_local = target.normalize(cosine);
            let sine_local = target.normalize(sine);
            let x = [
                offset[0] + cosine_local[0],
                2.0 * sine_local[0],
                offset[0] - cosine_local[0],
            ];
            let y = [
                offset[1] + cosine_local[1],
                2.0 * sine_local[1],
                offset[1] - cosine_local[1],
            ];
            let square = |[p0, p1, p2]: [f64; 3]| {
                [
                    p0 * p0,
                    2.0 * p0 * p1,
                    p1 * p1 + 2.0 * p0 * p2,
                    2.0 * p1 * p2,
                    p2 * p2,
                ]
            };
            let xx = square(x);
            let yy = square(y);
            let mut coefficients: Vec<f64> = Vec::with_capacity(5);
            let mut scale = 1.0_f64;
            for ((xs, ys), unit) in xx.into_iter().zip(yy).zip([1.0, 0.0, 2.0, 0.0, 1.0]) {
                coefficients.push(xs + ys - unit);
                scale = scale.max(xs.abs() + ys.abs() + unit);
            }
            if coefficients
                .iter()
                .all(|c| c.abs() <= ROUNDOFF_FACTOR * f64::EPSILON * scale)
            {
                return Err("the analytic intersection carriers coincide".into());
            }
            let at = |angle: f64| {
                [
                    source.center[0] + cosine[0] * angle.cos() + sine[0] * angle.sin(),
                    source.center[1] + cosine[1] * angle.cos() + sine[1] * angle.sin(),
                ]
            };
            // tan(theta/2) omits theta=pi. The leading coefficient is the value of
            // that chart seam; a seam within roundoff of zero is zero.
            let seam = dot([x[2], y[2]], [x[2], y[2]]) - 1.0;
            let on_seam = seam.abs()
                <= ROUNDOFF_FACTOR * f64::EPSILON * (dot([x[2], y[2]], [x[2], y[2]]) + 1.0);
            if on_seam {
                coefficients[4] = 0.0;
            }
            let mut points: Vec<_> = super::intersection::roots(&coefficients)?
                .into_iter()
                .map(|t| at(2.0 * t.atan()))
                .collect();
            if on_seam {
                points.push(at(std::f64::consts::PI));
            }
            points
        }
    };
    Ok(candidates)
}

/// Roundoff of a line parameter that a point at `extent` from the origin sets, in units of the
/// line length. A contact the parameter misses by less than this stays a contact.
fn parameter_slack(coordinates: impl IntoIterator<Item = f64>, length: f64) -> f64 {
    let extent = coordinates
        .into_iter()
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
    ROUNDOFF_FACTOR * f64::EPSILON * (1.0 + extent / length)
}

fn cross([a, b]: [f64; 2], [c, d]: [f64; 2]) -> f64 {
    a * d - b * c
}

fn distance([a, b]: [f64; 2], [c, d]: [f64; 2]) -> f64 {
    (a - c).hypot(b - d)
}

fn boundary_roundoff(edges: &[Edge]) -> f64 {
    ROUNDOFF_FACTOR
        * f64::EPSILON
        * edges
            .iter()
            .flat_map(|e| e.a.into_iter().chain(e.b))
            .map(f64::abs)
            .fold(1.0_f64, f64::max)
}
fn region(mut edges: Vec<Edge>) -> Mapping<Value> {
    let roundoff = boundary_roundoff(&edges);
    let mut rings = Vec::new();
    while let Some(first) = edges.pop() {
        let reverse = first.side < 0;
        let start = if reverse { first.b } else { first.a };
        let mut end = if reverse { first.a } else { first.b };
        let mut closed = first.closed;
        let mut boundary = vec![(first, reverse)];
        while !closed {
            let candidates: Vec<_> = edges
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.closed)
                .filter(|(_, e)| distance(end, if e.side < 0 { e.b } else { e.a }) <= roundoff)
                .map(|(i, _)| i)
                .collect();
            let [index] = candidates.as_slice() else {
                return Err("the directed sketch-region boundary is open or ambiguous".into());
            };
            let edge = edges.remove(*index);
            let reverse = edge.side < 0;
            end = if reverse { edge.a } else { edge.b };
            boundary.push((edge, reverse));
            closed = distance(start, end) <= roundoff;
        }
        let ring = Ring { edges: boundary };
        admit_ring(&ring, roundoff)?;
        rings.push(ring);
    }
    let mut outer_indices = Vec::new();
    for (index, ring) in rings.iter().enumerate() {
        if ring.counterclockwise()? {
            outer_indices.push(index);
        }
    }
    let [outer_index] = outer_indices.as_slice() else {
        return Err("the directed sketch region needs one outer loop".into());
    };
    let outer = rings.remove(*outer_index);
    for (i, hole) in rings.iter().enumerate() {
        admit_separate_rings(&outer, hole)?;
        if !outer.contains(hole.edges.first().ok_or("empty hole")?.0.a)? {
            return Err("the sketch-region hole lies outside its outer boundary".into());
        }
        for other in &rings[..i] {
            admit_separate_rings(other, hole)?;
            if other.contains(hole.edges.first().ok_or("empty hole")?.0.a)?
                || hole.contains(other.edges.first().ok_or("empty hole")?.0.a)?
            {
                return Err("the selected sketch-region holes are nested".into());
            }
        }
    }
    Ok(
        json!({"kind":"trimmed", "outer_boundary":outer.boundary(), "hole_boundaries":rings.iter().map(Ring::boundary).collect::<Vec<_>>()}),
    )
}
fn admit_separate_rings(a: &Ring, b: &Ring) -> Mapping<()> {
    for (first, _) in &a.edges {
        for (second, _) in &b.edges {
            if !separated(first, second)? {
                return Err(
                    "the sketch-region loops intersect or their separation is unresolved".into(),
                );
            }
        }
    }
    Ok(())
}
fn admit_ring(ring: &Ring, roundoff: f64) -> Mapping<()> {
    for (i, (edge, _)) in ring.edges.iter().enumerate() {
        if !edge.closed && distance(edge.a, edge.b) <= roundoff {
            return Err("the sketch-region boundary contains a degenerate edge".into());
        }
        for (j, (other, _)) in ring.edges[..i].iter().enumerate() {
            let adjacent = i == j + 1 || j == 0 && i + 1 == ring.edges.len();
            if separated(edge, other)? {
                continue;
            }
            if !adjacent || !endpoint_contact(edge, other, roundoff)? {
                return Err(
                    "the sketch-region boundary intersects or its intersection is unresolved"
                        .into(),
                );
            }
        }
    }
    Ok(())
}
fn endpoint_contact(first: &Edge, second: &Edge, roundoff: f64) -> Mapping<bool> {
    let endpoint = |point| {
        [first.a, first.b]
            .iter()
            .any(|first| distance(*first, point) <= roundoff)
            && [second.a, second.b]
                .iter()
                .any(|second| distance(*second, point) <= roundoff)
    };
    match (first.conic()?, second.conic()?) {
        (None, None) => {
            let first_delta = [first.b[0] - first.a[0], first.b[1] - first.a[1]];
            let second_delta = [second.b[0] - second.a[0], second.b[1] - second.a[1]];
            let determinant = cross(first_delta, second_delta);
            if determinant.abs()
                <= roundoff
                    * first_delta[0]
                        .hypot(first_delta[1])
                        .max(second_delta[0].hypot(second_delta[1]))
            {
                let shared = [first.a, first.b].into_iter().any(endpoint);
                let axis = usize::from(first_delta[0].abs() < first_delta[1].abs());
                let [a, b, c, d] = [first.a, first.b, second.a, second.b].map(|point| {
                    if axis == 0 {
                        point[0]
                    } else {
                        point[1]
                    }
                });
                let low = a.min(b).max(c.min(d));
                let high = a.max(b).min(c.max(d));
                return Ok(shared && high - low <= roundoff);
            }
            let offset = [second.a[0] - first.a[0], second.a[1] - first.a[1]];
            let parameter = cross(offset, second_delta) / determinant;
            Ok(endpoint([
                first.a[0] + parameter * first_delta[0],
                first.a[1] + parameter * first_delta[1],
            ]))
        }
        (Some(_), None) | (None, Some(_)) => {
            Ok(intersections(first, second)?.into_iter().all(endpoint))
        }
        (Some(ca), Some(cb)) => {
            if ca.center != cb.center
                || ca.radii != cb.radii
                || ca.axis != cb.axis
                || first.closed
                || second.closed
            {
                return Ok(intersections(first, second)?.into_iter().all(endpoint));
            }
            let tau = std::f64::consts::TAU;
            let a_span = first.range[1] - first.range[0];
            let b_span = second.range[1] - second.range[0];
            let offset = (second.range[0] - first.range[0]).rem_euclid(tau);
            let tolerance = ROUNDOFF_FACTOR * f64::EPSILON * tau;
            Ok(offset >= a_span - tolerance && offset + b_span <= tau + tolerance)
        }
    }
}
fn ray_heights(y: f64, ay: f64, by: f64) -> (f64, f64) {
    let tolerance = ROUNDOFF_FACTOR * f64::EPSILON * ay.abs().max(by.abs()).max(y.abs()).max(1.0);
    let height = |v: f64| {
        if (v - y).abs() <= tolerance {
            0.0
        } else {
            v - y
        }
    };
    (height(ay), height(by))
}

fn ray_crosses_segment([x, y]: [f64; 2], [ax, ay]: [f64; 2], [bx, by]: [f64; 2]) -> bool {
    let (da, db) = ray_heights(y, ay, by);
    (da > 0.0) != (db > 0.0) && ax - da * (bx - ax) / (db - da) > x
}

struct Ring {
    edges: Vec<(Edge, bool)>,
}

impl Ring {
    fn boundary(&self) -> Vec<Value> {
        self.edges.iter().map(|(e, r)| e.boundary(*r)).collect()
    }

    /// The boundary uses in the requested sense about the sketch normal. A ring
    /// stored in the other sense is traversed backwards: the edge order reverses
    /// and every use flips.
    fn oriented(&self, counterclockwise: bool) -> Mapping<Vec<Value>> {
        if self.counterclockwise()? == counterclockwise {
            return Ok(self.boundary());
        }
        Ok(self
            .edges
            .iter()
            .rev()
            .map(|(e, r)| e.boundary(!*r))
            .collect())
    }

    fn counterclockwise(&self) -> Mapping<bool> {
        let mut turns = 0.0;
        let mut previous = None;
        let mut first = None;
        let angle = |a: [f64; 2], b: [f64; 2]| -> Mapping<f64> {
            let normalize = |[x, y]: [f64; 2]| -> Mapping<[f64; 2]> {
                let scale = x.abs().max(y.abs());
                if !scale.is_finite() || scale == 0.0 {
                    return Err("the stored curve has no finite tangent direction".into());
                }
                Ok([x / scale, y / scale])
            };
            let a = normalize(a)?;
            let b = normalize(b)?;
            Ok(cross(a, b).atan2(dot(a, b)))
        };
        for (edge, reverse) in &self.edges {
            let (mut start, mut end, mut turn) = if let Some(conic) = edge.conic()? {
                let (a, b) = conic.vectors();
                let tangent = |t: f64| {
                    [
                        -a[0] * t.sin() + b[0] * t.cos(),
                        -a[1] * t.sin() + b[1] * t.cos(),
                    ]
                };
                let [from, to] = edge.range;
                if to <= from || to - from > std::f64::consts::TAU {
                    return Err("the closed boundary has an unresolved curve direction".into());
                }
                let start = tangent(from);
                let end = tangent(to);
                let turn = if edge.closed {
                    std::f64::consts::TAU
                } else {
                    angle(start, end)?.rem_euclid(std::f64::consts::TAU)
                };
                (start, end, turn)
            } else {
                let tangent = [edge.b[0] - edge.a[0], edge.b[1] - edge.a[1]];
                (tangent, tangent, 0.0)
            };
            if *reverse {
                (start, end) = (end.map(|v| -v), start.map(|v| -v));
                turn = -turn;
            }
            if let Some(previous) = previous {
                turns += angle(previous, start)?;
            } else {
                first = Some(start);
            }
            turns += turn;
            previous = Some(end);
        }
        turns += angle(
            previous.ok_or("empty closed boundary")?,
            first.ok_or("empty closed boundary")?,
        )?;
        if !turns.is_finite() || turns.abs() < std::f64::consts::PI {
            return Err("the closed boundary has an unresolved curve direction".into());
        }
        Ok(turns > 0.0)
    }

    fn contains(&self, [point_x, point_y]: [f64; 2]) -> Mapping<bool> {
        let mut crossings = 0;
        for (edge, _) in &self.edges {
            let geometry = &edge.geometry;
            if text(geometry, "kind")? == "line" {
                let [ax, ay] = edge.a;
                let [bx, by] = edge.b;
                if ray_crosses_segment([point_x, point_y], [ax, ay], [bx, by]) {
                    crossings += 1;
                }
                continue;
            }
            let center = geometry
                .get("center")
                .ok_or("the region carrier has no center")?;
            let cx = super::number(center, "u")?;
            let cy = super::number(center, "v")?;
            let (major_radius, minor_radius, angle) = if text(geometry, "kind")? == "ellipse" {
                (
                    super::number(geometry, "major_radius")?,
                    super::number(geometry, "minor_radius")?,
                    super::number(geometry, "major_angle")?,
                )
            } else {
                let radius = super::number(geometry, "radius")?;
                (radius, radius, 0.0)
            };
            let ax = major_radius * angle.cos();
            let ay = major_radius * angle.sin();
            let bx = -minor_radius * angle.sin();
            let by = minor_radius * angle.cos();
            let radius = ay.hypot(by);
            let level = (point_y - cy) / radius;
            if level.abs() > 1.0 || edge.closed && level.abs() == 1.0 {
                continue;
            }
            let phase = by.atan2(ay);
            let [from, to] = edge.range;
            let mut stops = vec![from];
            let pi = std::f64::consts::PI;
            let first = from + (phase - from).rem_euclid(pi);
            for i in 0..3 {
                let parameter = first + f64::from(i) * pi;
                if parameter > from && parameter < to {
                    stops.push(parameter);
                }
            }
            stops.push(to);
            for pair in stops.windows(2) {
                let [lo, hi] = pair else {
                    continue;
                };
                let [_, start_y] = edge.a;
                let [_, end_y] = edge.b;
                let ya = if *lo == from {
                    start_y
                } else {
                    cy + ay * lo.cos() + by * lo.sin()
                };
                let yb = if *hi == to {
                    end_y
                } else {
                    cy + ay * hi.cos() + by * hi.sin()
                };
                let (da, db) = ray_heights(point_y, ya, yb);
                if (da > 0.0) == (db > 0.0) || da == 0.0 && *lo != from || db == 0.0 && *hi != to {
                    continue;
                }
                let rising = yb > ya;
                let parameter = if rising {
                    phase - level.acos()
                } else {
                    phase + level.acos()
                };
                if cx + ax * parameter.cos() + bx * parameter.sin() > point_x {
                    crossings += 1;
                }
            }
        }
        Ok(crossings % 2 == 1)
    }
}
fn source_loops(sketch: &str, model: &Model, budget: &mut Budget) -> Mapping<Loops> {
    let refused = "the outside sketch region has no unique enclosing source loop";
    let mut edges = Vec::new();
    budget.work(model.sketch_entities.len())?;
    for entity in &model.sketch_entities {
        if entity.sketch.as_str() != sketch || entity.construction {
            continue;
        }
        let wire = json::to_value(entity).map_err(|_| refused)?;
        if wire.pointer("/geometry/kind").and_then(Value::as_str) == Some("point") {
            continue;
        }
        edges.push(Edge::new(entity.id().as_str().to_owned(), &wire)?);
    }
    attach_tangencies(sketch, &mut edges, model, budget)?;
    let scale = edges
        .iter()
        .flat_map(|e| e.a.into_iter().chain(e.b))
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
    let roundoff = ROUNDOFF_FACTOR * f64::EPSILON * scale;
    let mut rings = Vec::new();
    let mut open = Vec::new();
    while let Some(first) = edges.pop() {
        let start = first.a;
        let mut end = first.b;
        let mut closed = first.closed || distance(start, end) <= roundoff;
        let mut members = vec![(first, false)];
        while !closed {
            budget.work(edges.len())?;
            let candidates: Vec<_> = edges
                .iter()
                .enumerate()
                .filter(|(_, e)| !e.closed)
                .flat_map(|(i, e)| {
                    [
                        (i, false, distance(end, e.a)),
                        (i, true, distance(end, e.b)),
                    ]
                })
                .filter(|(_, _, gap)| *gap <= roundoff)
                .collect();
            let [(index, reverse, _)] = candidates.as_slice() else {
                if candidates.is_empty() {
                    break;
                }
                return Err("the outside sketch region has tied source-loop connections".into());
            };
            let edge = edges.remove(*index);
            end = if *reverse { edge.a } else { edge.b };
            members.push((edge, *reverse));
            closed = distance(start, end) <= roundoff;
        }
        let retraced_lines = members.len() < 3
            && members
                .iter()
                .all(|(edge, _)| edge.geometry.get("kind").and_then(Value::as_str) == Some("line"));
        if closed && !retraced_lines {
            let ring = Ring { edges: members };
            budget.work(
                ring.edges
                    .len()
                    .checked_mul(ring.edges.len())
                    .ok_or(BUDGET_LOSS)?,
            )?;
            admit_ring(&ring, roundoff)?;
            rings.push(ring);
        } else {
            open.extend(members.into_iter().map(|(edge, _)| edge));
        }
    }
    Ok(Loops { rings, open })
}

fn exterior_region(
    sketch: &str,
    witnesses: &[Edge],
    model: &Model,
    recovery: &mut Recovery,
) -> Mapping<Value> {
    let refused = "the outside sketch region has no unique enclosing source loop";
    let Recovery { budget, loops } = recovery;
    let topology = loops
        .entry(sketch.to_owned())
        .or_insert_with(|| source_loops(sketch, model, budget))
        .as_ref()
        .map_err(Clone::clone)?;
    let rings = &topology.rings;
    let mut candidates = Vec::new();
    for (index, ring) in rings.iter().enumerate() {
        budget.work(
            ring.edges
                .len()
                .checked_mul(witnesses.len())
                .ok_or(BUDGET_LOSS)?,
        )?;
        if ring
            .edges
            .iter()
            .any(|(e, _)| witnesses.iter().any(|w| w.id == e.id))
        {
            continue;
        }
        budget.work(
            ring.edges
                .len()
                .checked_mul(witnesses.len())
                .ok_or(BUDGET_LOSS)?,
        )?;
        if witnesses
            .iter()
            .map(|w| ring.contains(w.a))
            .collect::<Mapping<Vec<_>>>()?
            .iter()
            .all(|v| *v)
        {
            candidates.push(index);
        }
    }
    let [index] = candidates.as_slice() else {
        return Err(refused.into());
    };
    let index = *index;
    let outer = rings.get(index).ok_or(refused)?;
    let mut holes = Vec::new();
    for (i, ring) in rings.iter().enumerate() {
        if i == index {
            continue;
        }
        let point = ring.edges.first().ok_or(refused)?.0.a;
        budget.work(outer.edges.len())?;
        if !outer.contains(point)? {
            continue;
        }
        let mut nested = false;
        for (j, other) in rings.iter().enumerate() {
            budget.work(1)?;
            if j != i && j != index {
                budget.work(
                    other
                        .edges
                        .len()
                        .checked_mul(ring.edges.len())
                        .ok_or(BUDGET_LOSS)?,
                )?;
                budget.work(
                    outer
                        .edges
                        .len()
                        .checked_add(other.edges.len())
                        .ok_or(BUDGET_LOSS)?,
                )?;
                admit_separate_rings(other, ring)?;
                let other_point = other.edges.first().ok_or(refused)?.0.a;
                if outer.contains(other_point)? && other.contains(point)? {
                    nested = true;
                    break;
                }
            }
        }
        if !nested {
            holes.push(ring);
        }
    }
    for edge in &topology.open {
        let mut excluded = false;
        for hole in &holes {
            budget.work(1)?;
            if segment_in_hole(edge, hole)? {
                excluded = true;
                break;
            }
        }
        if !excluded && !isolated(edge, topology, budget)? {
            return Err("the outside sketch region has an open divider whose effect on the selected face is not supported yet".into());
        }
    }
    let boundary_count = std::iter::once(outer)
        .chain(holes.iter().copied())
        .try_fold(0usize, |count, ring| {
            count.checked_add(ring.edges.len()).ok_or(BUDGET_LOSS)
        })?;
    budget.work(
        boundary_count
            .checked_mul(boundary_count)
            .ok_or(BUDGET_LOSS)?,
    )?;
    budget.work(topology.rings.iter().try_fold(0usize, |count, ring| {
        count.checked_add(ring.edges.len()).ok_or(BUDGET_LOSS)
    })?)?;
    let roundoff = ROUNDOFF_FACTOR
        * f64::EPSILON
        * topology
            .rings
            .iter()
            .flat_map(|r| &r.edges)
            .flat_map(|(e, _)| e.a.into_iter().chain(e.b))
            .map(f64::abs)
            .fold(1.0_f64, f64::max);
    admit_ring(outer, roundoff)?;
    for (i, hole) in holes.iter().enumerate() {
        admit_ring(hole, roundoff)?;
        admit_separate_rings(outer, hole)?;
        for other in &holes[..i] {
            admit_separate_rings(other, hole)?;
        }
    }
    for ring in std::iter::once(outer).chain(holes.iter().copied()) {
        for (edge, _) in &ring.edges {
            budget.boundary(edge)?;
        }
    }
    let holes = holes
        .into_iter()
        .map(|hole| hole.oriented(false))
        .collect::<Mapping<Vec<_>>>()?;
    Ok(json!({"kind":"trimmed","outer_boundary":outer.oriented(true)?,"hole_boundaries":holes}))
}

fn segment_in_hole(edge: &Edge, hole: &Ring) -> Mapping<bool> {
    if text(&edge.geometry, "kind")? != "line" {
        return Ok(false);
    }
    let [(carrier, _)] = hole.edges.as_slice() else {
        return Ok(false);
    };
    if !carrier.closed {
        return Ok(false);
    }
    let geometry = &carrier.geometry;
    let center = geometry
        .get("center")
        .ok_or("the region carrier has no center")?;
    let cx = super::number(center, "u")?;
    let cy = super::number(center, "v")?;
    let (major_radius, minor_radius, angle) = if text(geometry, "kind")? == "ellipse" {
        (
            super::number(geometry, "major_radius")?,
            super::number(geometry, "minor_radius")?,
            super::number(geometry, "major_angle")?,
        )
    } else if text(geometry, "kind")? == "circle" {
        let radius = super::number(geometry, "radius")?;
        (radius, radius, 0.0)
    } else {
        return Ok(false);
    };
    Ok([edge.a, edge.b].iter().all(|[point_x, point_y]| {
        let unit_u = ((point_x - cx) * angle.cos() + (point_y - cy) * angle.sin()) / major_radius;
        let unit_v = (-(point_x - cx) * angle.sin() + (point_y - cy) * angle.cos()) / minor_radius;
        unit_u * unit_u + unit_v * unit_v <= 1.0 + 32.0 * f64::EPSILON
    }))
}

#[cfg(test)]
mod tests;
