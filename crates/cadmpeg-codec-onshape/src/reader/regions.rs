// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

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
            let mut entity = serde_json::to_value(entity)
                .map_err(|_| "a sketch-region source curve is unreadable")?;
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

struct Edge {
    id: String,
    a: [f64; 2],
    b: [f64; 2],
    range: [f64; 2],
    closed: bool,
    geometry: Value,
    side: i64,
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
        if a.iter().chain(&b).any(|n| !n.is_finite()) {
            return Err("the sketch-region boundary exceeds the finite range".into());
        }
        Ok(Self {
            id,
            a,
            b,
            range,
            closed,
            geometry: geometry.clone(),
            side: 1,
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
    let tolerance = 64.0 * f64::EPSILON * from.abs().max(to.abs()).max(std::f64::consts::TAU);
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
    let roundoff = 64.0 * f64::EPSILON * scale;
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
    if let (Some(ca), Some(cb)) = (ca, cb) {
        for (edge, source, target) in [(a, ca, cb), (b, cb, ca)] {
            if conic_separated(edge, source, target, scale) {
                return Ok(true);
            }
        }
    }
    Ok(false)
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
    let tolerance = 64.0 * f64::EPSILON * magnitude * (1.0 + scale / major.min(minor));
    let low = constant + l1 + l2;
    let high = constant + h1 + h2;
    [low, high, tolerance].iter().all(|v| v.is_finite())
        && (low > 1.0 + tolerance || high < 1.0 - tolerance)
}

fn cross([a, b]: [f64; 2], [c, d]: [f64; 2]) -> f64 {
    a * d - b * c
}

fn distance([a, b]: [f64; 2], [c, d]: [f64; 2]) -> f64 {
    (a - c).hypot(b - d)
}

const BOUNDARY_ROUNDOFF_FACTOR: f64 = 64.0;
fn boundary_roundoff(edges: &[Edge]) -> f64 {
    BOUNDARY_ROUNDOFF_FACTOR
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
                let low = first.a[axis]
                    .min(first.b[axis])
                    .max(second.a[axis].min(second.b[axis]));
                let high = first.a[axis]
                    .max(first.b[axis])
                    .min(second.a[axis].max(second.b[axis]));
                return Ok(shared && high - low <= roundoff);
            }
            let offset = [second.a[0] - first.a[0], second.a[1] - first.a[1]];
            let parameter = cross(offset, second_delta) / determinant;
            Ok(endpoint([
                first.a[0] + parameter * first_delta[0],
                first.a[1] + parameter * first_delta[1],
            ]))
        }
        (Some(conic), None) | (None, Some(conic)) => {
            let (curve, line) = if first.conic()?.is_some() {
                (first, second)
            } else {
                (second, first)
            };
            let offset =
                conic.normalize([line.a[0] - conic.center[0], line.a[1] - conic.center[1]]);
            let delta = conic.normalize([line.b[0] - line.a[0], line.b[1] - line.a[1]]);
            let qa = dot(delta, delta);
            let qb = 2.0 * dot(offset, delta);
            let qc = dot(offset, offset) - 1.0;
            let discriminant = qb * qb - 4.0 * qa * qc;
            if !discriminant.is_finite() || discriminant < 0.0 || qa == 0.0 {
                return Ok(false);
            }
            for parameter in [
                (-qb - discriminant.sqrt()) / (2.0 * qa),
                (-qb + discriminant.sqrt()) / (2.0 * qa),
            ] {
                if !(0.0..=1.0).contains(&parameter) {
                    continue;
                }
                let point = [
                    line.a[0] + parameter * (line.b[0] - line.a[0]),
                    line.a[1] + parameter * (line.b[1] - line.a[1]),
                ];
                let [local_u, local_v] =
                    conic.normalize([point[0] - conic.center[0], point[1] - conic.center[1]]);
                let angle = local_v.atan2(local_u);
                if (curve.closed
                    || (angle - curve.range[0]).rem_euclid(std::f64::consts::TAU)
                        < curve.range[1] - curve.range[0])
                    && !endpoint(point)
                {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        (Some(ca), Some(cb)) => {
            if ca.center != cb.center
                || ca.radii != cb.radii
                || ca.axis != cb.axis
                || first.closed
                || second.closed
            {
                return Ok(false);
            }
            let tau = std::f64::consts::TAU;
            let a_span = first.range[1] - first.range[0];
            let b_span = second.range[1] - second.range[0];
            let offset = (second.range[0] - first.range[0]).rem_euclid(tau);
            let tolerance = BOUNDARY_ROUNDOFF_FACTOR * f64::EPSILON * tau;
            Ok(offset >= a_span - tolerance && offset + b_span <= tau + tolerance)
        }
    }
}
fn ray_heights(y: f64, ay: f64, by: f64) -> (f64, f64) {
    let tolerance = 64.0 * f64::EPSILON * ay.abs().max(by.abs()).max(y.abs()).max(1.0);
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
        let wire = serde_json::to_value(entity).map_err(|_| refused)?;
        if wire.pointer("/geometry/kind").and_then(Value::as_str) == Some("point") {
            continue;
        }
        edges.push(Edge::new(entity.id().as_str().to_owned(), &wire)?);
    }
    let scale = edges
        .iter()
        .flat_map(|e| e.a.into_iter().chain(e.b))
        .map(f64::abs)
        .fold(1.0_f64, f64::max);
    let roundoff = 64.0 * f64::EPSILON * scale;
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
    let roundoff = BOUNDARY_ROUNDOFF_FACTOR
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
    let holes: Vec<_> = holes.into_iter().map(Ring::boundary).collect();
    Ok(json!({"kind":"trimmed","outer_boundary":outer.boundary(),"hole_boundaries":holes}))
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
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use std::f64::consts::{FRAC_PI_2, PI, TAU};

    fn edge(name: &str, geometry: Value) -> Edge {
        let record = serde_json::Map::from_iter([("geometry".into(), geometry)]);
        Edge::new(name.to_owned(), &Value::Object(record)).unwrap()
    }

    fn half_ring(ellipse: bool, from: f64, to: f64, reverse: bool) -> Ring {
        let geometry = if ellipse {
            json!({"kind":"ellipse","center":{"u":0.0,"v":0.0},"major_radius":2.0,
                "minor_radius":1.0,"major_angle":0.0,"bounds":[from,to]})
        } else {
            json!({"kind":"arc","center":{"u":0.0,"v":0.0},"radius":1.0,"start_angle":from,"end_angle":to})
        };
        let arc = edge("arc", geometry);
        let diameter = edge(
            "diameter",
            json!({"kind":"line", "start":{"u":arc.b[0],"v":arc.b[1]},
            "end":{"u":arc.a[0],"v":arc.a[1]}}),
        );
        let mut edges = vec![(arc, reverse), (diameter, reverse)];
        if reverse {
            edges.reverse();
        }
        Ring { edges }
    }

    #[test]
    fn curved_endpoint_height_crossings_agree_with_lines_and_reversed_uses() {
        for ellipse in [false, true] {
            for reverse in [false, true] {
                let upper = half_ring(ellipse, 0.0, PI, reverse);
                assert!(!upper.contains([-3.0, 0.0]).unwrap());
                assert!(!upper.contains([3.0, 0.0]).unwrap());
                assert!(upper.contains([0.0, 0.5]).unwrap());
                assert!(!upper.contains([0.0, -0.5]).unwrap());
                let lower = half_ring(ellipse, PI, TAU, reverse);
                assert!(!lower.contains([-3.0, 0.0]).unwrap());
                assert!(lower.contains([0.0, -0.5]).unwrap());
                let left = half_ring(ellipse, FRAC_PI_2, 3.0 * FRAC_PI_2, reverse);
                for y in [-1.0, 1.0] {
                    assert!(!left.contains([-3.0, y]).unwrap());
                }
                assert!(left.contains([-0.5, 0.0]).unwrap());
            }
        }
    }

    #[test]
    fn full_carrier_seams_and_interior_tangencies_do_not_double_count() {
        for ellipse in [false, true] {
            let g = if ellipse {
                json!({"kind":"ellipse","center":{"u":0.0,"v":0.0},"major_radius":2.0,"minor_radius":1.0,"major_angle":0.0})
            } else {
                json!({"kind":"circle","center":{"u":0.0,"v":0.0},"radius":1.0})
            };
            let ring = Ring {
                edges: vec![(edge("closed", g), false)],
            };
            assert!(ring.contains([0.0, 0.0]).unwrap());
            assert!(!ring.contains([-3.0, 0.0]).unwrap());
            for y in [-1.0, 1.0] {
                assert!(!ring.contains([-3.0, y]).unwrap());
            }
        }
    }

    #[test]
    fn rotated_ellipse_endpoints_near_extrema_keep_crossing_parity() {
        let angle = 0.3_f64;
        let phase = angle.cos().atan2(2.0 * angle.sin());
        for reverse in [false, true] {
            let arc = edge(
                "rotated",
                json!({"kind":"ellipse","center":{"u":0.0,"v":0.0},
                "major_radius":2.0,"minor_radius":1.0,"major_angle":angle,
                "bounds":[phase - 0.0001, phase + 3.0]}),
            );
            let heights = [arc.a[1], arc.b[1]];
            let chord = edge(
                "chord",
                json!({"kind":"line","start":{"u":arc.b[0],"v":arc.b[1]},
                "end":{"u":arc.a[0],"v":arc.a[1]}}),
            );
            let ring = Ring {
                edges: vec![(arc, reverse), (chord, reverse)],
            };
            for y in heights {
                assert!(!ring.contains([-6.0, y]).unwrap());
            }
        }
    }

    #[test]
    fn isolation_certificates_accept_disjoint_arcs_and_keep_contacts_unproven() {
        let arc = |name, radius, from, to| {
            edge(
                name,
                json!({"kind":"arc","center":{"u":0.0,"v":0.0},
            "radius":radius,"start_angle":from,"end_angle":to}),
            )
        };
        let wide = arc("wide", 3.0, 0.0, 1.5 * PI);
        let gap = arc("gap", 3.0, 5.0 * PI / 3.0, 11.0 * PI / 6.0);
        let inner = arc("inner", 1.0, PI / 4.0, 3.0 * PI / 4.0);
        assert!(separated(&wide, &gap).unwrap());
        assert!(separated(&wide, &inner).unwrap());
        let touching = arc("touching", 3.0, 1.5 * PI, 11.0 * PI / 6.0);
        assert!(!separated(&wide, &touching).unwrap());
        let diameter = edge(
            "diameter",
            json!({"kind":"line","start":{"u":-4.0,"v":0.0},"end":{"u":4.0,"v":0.0}}),
        );
        assert!(!separated(&wide, &diameter).unwrap());
    }

    #[test]
    fn isolation_projections_keep_roundoff_near_translated_contacts_unproven() {
        let origin = 1_000_000_000.0;
        let arc = edge(
            "translated",
            json!({"kind":"arc","center":{"u":origin,"v":origin},"radius":2.0,"start_angle":PI/4.0,"end_angle":3.0*PI/4.0}),
        );
        for (gap, proven) in [(0.0, false), (0.000_001, false), (0.001, true)] {
            let line = edge(
                "line",
                json!({"kind":"line","start":{"u":origin-1.0,"v":origin+2.0+gap},"end":{"u":origin+1.0,"v":origin+2.0+gap}}),
            );
            assert_eq!(separated(&arc, &line).unwrap(), proven);
        }
    }

    fn frame_model() -> Model {
        let mut model = cadmpeg_ir::CadIr::empty().model;
        let mut geometries = vec![
            json!({"kind":"line","start":{"u":-4.0,"v":-1.0},"end":{"u":-2.0,"v":-1.0}}),
            json!({"kind":"line","start":{"u":-2.0,"v":-1.0},"end":{"u":-2.0,"v":1.0}}),
            json!({"kind":"line","start":{"u":-2.0,"v":1.0},"end":{"u":-4.0,"v":1.0}}),
            json!({"kind":"line","start":{"u":-4.0,"v":1.0},"end":{"u":-4.0,"v":-1.0}}),
            json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}),
        ];
        let half = half_ring(false, 0.0, PI, false);
        geometries.extend(half.edges.into_iter().map(|(e, _)| e.geometry));
        for (i, g) in geometries.into_iter().enumerate() {
            model.sketch_entities.push(
                serde_json::from_value(json!({"id":format!("synthetic:model:sketch-entity#{i}"),
                "sketch":"synthetic:model:sketch#frame","construction":false,"endpoint_refs":[],"geometry":g}))
                .unwrap(),
            );
        }
        model
    }

    #[test]
    fn unrelated_semicircle_does_not_enclose_the_rectangle_hole_witness() {
        let model = frame_model();
        let witness = edge(
            "synthetic:model:sketch-entity#4",
            json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}),
        );
        assert_eq!(witness.a, [-3.0, 0.0]);
        let region = exterior_region(
            "synthetic:model:sketch#frame",
            &[witness],
            &model,
            &mut Recovery::default(),
        )
        .unwrap();
        assert_eq!(region["outer_boundary"].as_array().unwrap().len(), 4);
        assert_eq!(region["hole_boundaries"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn long_escaped_ids_exhaust_bytes_before_boundary_expansion() {
        let mut model = frame_model();
        let mut wire = serde_json::to_value(&model.sketch_entities[0]).unwrap();
        wire["id"] = json!(format!(
            "synthetic:model:sketch-entity#{}",
            "quoted\"".repeat(512)
        ));
        model.sketch_entities[0] = serde_json::from_value(wire).unwrap();
        let witness = edge(
            "synthetic:model:sketch-entity#4",
            json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}),
        );
        let mut recovery = Recovery::default();
        recovery.budget.bytes = 1024;
        assert_eq!(
            exterior_region(
                "synthetic:model:sketch#frame",
                &[witness],
                &model,
                &mut recovery
            )
            .unwrap_err(),
            BUDGET_LOSS
        );
        assert_eq!(recovery.budget.bytes, 0);
        assert!(recovery.budget.output > 0);
        assert_eq!(recovery.check().unwrap_err(), BUDGET_LOSS);
        let work = recovery.budget.work;
        assert_eq!(recovery.budget.work(1).unwrap_err(), BUDGET_LOSS);
        assert_eq!(recovery.budget.work, work);
    }

    #[test]
    fn cached_topology_still_charges_cumulative_output_and_work() {
        for output_limited in [false, true] {
            let model = frame_model();
            let witness = edge(
                "synthetic:model:sketch-entity#4",
                json!({"kind":"circle","center":{"u":-3.25,"v":0.0},"radius":0.25}),
            );
            let mut recovery = Recovery::default();
            if output_limited {
                recovery.budget.output = 10;
            } else {
                recovery.budget.work = 200;
            }
            exterior_region(
                "synthetic:model:sketch#frame",
                std::slice::from_ref(&witness),
                &model,
                &mut recovery,
            )
            .unwrap();
            let after_first = recovery.budget.work;
            exterior_region(
                "synthetic:model:sketch#frame",
                std::slice::from_ref(&witness),
                &model,
                &mut recovery,
            )
            .unwrap();
            assert_eq!(recovery.loops.len(), 1);
            if !output_limited {
                eprintln!(
                    "Cached recovery: first {} work units, next {} work units",
                    200 - after_first,
                    after_first - recovery.budget.work
                );
            }
            if !output_limited {
                assert!(after_first - recovery.budget.work < 200 - after_first);
            }
            let mut refused = false;
            for _ in 0..20 {
                if let Err(reason) = exterior_region(
                    "synthetic:model:sketch#frame",
                    std::slice::from_ref(&witness),
                    &model,
                    &mut recovery,
                ) {
                    assert_eq!(reason, BUDGET_LOSS);
                    refused = true;
                    break;
                }
            }
            assert!(refused);
        }
    }
}
