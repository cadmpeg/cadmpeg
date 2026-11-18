// SPDX-License-Identifier: Apache-2.0
//! Sketch constraints and dimensions.
//!
//! A sketch curve indexes its points, so two curves that index one point
//! share it: each shared point becomes a coincidence of the curve loci that
//! use it, and each entity names its end points in `endpoint_refs`. A
//! constraint refers to points and curves by index; a point is the first
//! locus that uses it, and the sketch origin, when a constraint refers to
//! it, is a fixed construction point. A dimension drives the design
//! parameter that its `param` names, or a parameter of its own that the
//! sketch feature owns. A constraint without a neutral family is a native
//! relation that keeps its scalar fields and its operands.

use std::collections::BTreeMap;

use cadmpeg_ir::sketches::SketchConstraint;
use serde_json::{json, Value};

use super::expr::Scope;
use super::id;

/// The loci of a sketch's points and the entities of its curves.
#[derive(Debug, Default)]
pub(super) struct Loci {
    /// The loci that use each point, by point index.
    points: BTreeMap<u64, Vec<Value>>,
    /// The entity of each curve, by curve index.
    curves: BTreeMap<u64, String>,
}

impl Loci {
    /// Records the entity of the curve at `index` and the loci its points
    /// take on it.
    pub(super) fn curve(&mut self, index: u64, entity: &str, curve: &Value) {
        self.curves.insert(index, entity.to_owned());
        let mut add = |field: &Value, kind: &str| {
            if let Some(point) = field.as_u64() {
                self.points
                    .entry(point)
                    .or_default()
                    .push(json!({"kind": kind, "value": entity}));
            }
        };
        match curve.get("type").and_then(Value::as_str) {
            Some("line" | "arc") => {
                add(&curve["a"], "start");
                add(&curve["b"], "end");
                add(&curve["c"], "center");
            }
            Some("circle" | "ellipse") => add(&curve["c"], "center"),
            Some("spline") => {
                if let Some(pts) = curve.get("pts").and_then(Value::as_array) {
                    if let (Some(first), Some(last)) = (pts.first(), pts.last()) {
                        add(first, "start");
                        add(last, "end");
                    }
                }
            }
            _ => {}
        }
    }

    /// Records the point entity of the point at `index`.
    pub(super) fn point(&mut self, index: u64, entity: &str) {
        self.points
            .entry(index)
            .or_default()
            .push(json!({"kind": "entity", "value": entity}));
    }
}

/// The ids of the points a curve ends on, in its stored direction.
pub(super) fn endpoint_refs(curve: &Value, ids: &[Option<String>]) -> Vec<String> {
    let id_of = |field: &Value| {
        field
            .as_u64()
            .and_then(|index| usize::try_from(index).ok())
            .and_then(|index| ids.get(index).cloned().flatten())
    };
    match curve.get("type").and_then(Value::as_str) {
        Some("line" | "arc") => [id_of(&curve["a"]), id_of(&curve["b"])]
            .into_iter()
            .flatten()
            .collect(),
        Some("spline") => curve
            .get("pts")
            .and_then(Value::as_array)
            .and_then(|pts| Some(vec![id_of(pts.first()?)?, id_of(pts.last()?)?]))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// The relations of one sketch.
#[derive(Debug, Default)]
pub(super) struct Relations {
    /// The neutral constraints.
    pub(super) constraints: Vec<SketchConstraint>,
    /// The parameters that dimensions without a design parameter drive, as
    /// wire records without an ordinal.
    pub(super) parameters: Vec<Value>,
    /// The design parameters that a dimension drives.
    pub(super) driven: Vec<String>,
    /// Whether a relation uses the sketch origin.
    pub(super) origin: bool,
    /// The count of native relations.
    pub(super) native: usize,
    /// The count of relations that neither a neutral nor a native record
    /// admits.
    pub(super) dropped: usize,
}

/// The sketch whose relations are decoded.
pub(super) struct SketchScope<'s> {
    /// The key of the sketch feature.
    pub(super) key: &'s str,
    /// The neutral sketch id.
    pub(super) sketch: &'s str,
    /// The loci of the sketch.
    pub(super) loci: &'s Loci,
    /// The parameter scope.
    pub(super) scope: &'s Scope,
}

/// Dimensions whose value is an angle.
const ANGULAR: &[&str] = &["angle", "line_angle", "arc_sweep"];

impl SketchScope<'_> {
    fn origin_entity(&self) -> String {
        id("entity", &format!("{}:origin", self.key))
    }

    /// The locus of the point at `index`.
    fn locus(&self, index: u64, relations: &mut Relations) -> Option<Value> {
        if let Some(locus) = self.loci.points.get(&index).and_then(|loci| loci.first()) {
            return Some(locus.clone());
        }
        (index == 0).then(|| {
            relations.origin = true;
            json!({"kind": "entity", "value": self.origin_entity()})
        })
    }

    fn curve(&self, record: &Value, field: &str) -> Option<String> {
        self.loci.curves.get(&record.get(field)?.as_u64()?).cloned()
    }

    /// The relations of the sketch record `body`.
    pub(super) fn relations(&self, body: &Value) -> Relations {
        let mut relations = Relations::default();
        for (index, loci) in &self.loci.points {
            if loci.len() < 2 {
                continue;
            }
            let point_id = body
                .get("points")
                .and_then(|points| points.get(usize::try_from(*index).ok()?))
                .and_then(|point| point.get("id"))
                .and_then(Value::as_str)
                .map_or_else(|| index.to_string(), str::to_owned);
            self.push(
                &mut relations,
                &format!("shared:{point_id}"),
                json!({"kind": "coincident_loci", "loci": loci}),
                &json!({}),
            );
        }
        for curve in body
            .get("curves")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            if curve.get("fixed").and_then(Value::as_bool) != Some(true) {
                continue;
            }
            let Some(name) = curve.get("id").and_then(Value::as_str) else {
                continue;
            };
            let entity = id("entity", &format!("{}:{name}", self.key));
            if self.loci.curves.values().any(|known| *known == entity) {
                self.push(
                    &mut relations,
                    &format!("fixed:{name}"),
                    json!({"kind": "fixed", "entity": entity}),
                    &json!({}),
                );
            }
        }
        for record in body
            .get("constraints")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
        {
            self.constraint(record, &mut relations);
        }
        if relations.origin {
            self.push(
                &mut relations,
                "origin",
                json!({"kind": "fixed", "entity": self.origin_entity()}),
                &json!({}),
            );
        }
        relations
    }

    fn constraint(&self, record: &Value, relations: &mut Relations) {
        let (Some(kind), Some(name)) = (
            record.get("type").and_then(Value::as_str),
            record.get("id").and_then(Value::as_str),
        ) else {
            relations.dropped += 1;
            return;
        };
        let value = record.get("value").and_then(Value::as_f64);
        let driven = record.get("driven").and_then(Value::as_bool) == Some(true);
        let parameter = value.map(|value| self.parameter(record, name, kind, value, relations));
        let mut state = json!({});
        if value.is_some() {
            state["driving"] = json!(!driven);
        }
        let definition = self.definition(kind, record, parameter.as_deref(), relations);
        let neutral =
            definition.is_some_and(|definition| self.push(relations, name, definition, &state));
        if neutral {
            return;
        }
        let native = self.native(kind, record, parameter.as_deref(), relations);
        if self.push(relations, name, native, &state) {
            relations.native += 1;
        } else {
            relations.dropped += 1;
        }
    }

    /// The parameter a dimension drives: the design parameter its `param`
    /// names, or one of its own.
    fn parameter(
        &self,
        record: &Value,
        name: &str,
        kind: &str,
        value: f64,
        relations: &mut Relations,
    ) -> String {
        if let Some(param) = record.get("param").and_then(Value::as_str) {
            if self.scope.knows(param) {
                relations.driven.push(param.to_owned());
                return id("parameter", param);
            }
        }
        let key = format!("{}:{name}", self.key);
        let parameter = id("parameter", &key);
        let (expression, value) = if ANGULAR.contains(&kind) {
            (
                format!("{value} rad"),
                json!({"kind": "angle", "value": value}),
            )
        } else {
            (
                format!("{value} mm"),
                json!({"kind": "length", "value": value}),
            )
        };
        relations.parameters.push(json!({
            "id": parameter,
            "owner": id("feature", self.key),
            "name": name,
            "expression": expression,
            "value": value,
            "dependencies": [],
            "properties": {},
        }));
        parameter
    }

    fn definition(
        &self,
        kind: &str,
        record: &Value,
        parameter: Option<&str>,
        relations: &mut Relations,
    ) -> Option<Value> {
        let mut locus = |field: &str| self.locus(record.get(field)?.as_u64()?, relations);
        let curve = |field: &str| self.curve(record, field);
        Some(match kind {
            "coincident" => json!({"kind": "coincident_loci", "loci": [locus("p")?, locus("q")?]}),
            "point_on_curve" => {
                json!({"kind": "point_on_object", "point": locus("p")?, "entity": curve("c")?})
            }
            "horizontal" | "vertical" => json!({"kind": kind, "entity": curve("l")?}),
            "horizontal_points" | "vertical_points" => {
                json!({"kind": "same_coordinate", "relation": {
                    "first": locus("p")?,
                    "second": locus("q")?,
                    "axis": if kind == "horizontal_points" { "v" } else { "u" },
                }})
            }
            "parallel" | "perpendicular" | "collinear" | "tangent" | "equal" | "concentric" => {
                json!({"kind": kind, "first": curve("a")?, "second": curve("b")?})
            }
            "smooth" => json!({"kind": "curvature", "first": curve("a")?, "second": curve("b")?}),
            "midpoint" => json!({"kind": "midpoint", "point": locus("p")?, "entity": curve("l")?}),
            "symmetric" => json!({
                "kind": "symmetric",
                "first": locus("p")?,
                "second": locus("q")?,
                "axis": curve("l")?,
            }),
            "fix" => {
                let point = locus("p")?;
                (point["kind"] == "entity").then_some(())?;
                json!({"kind": "fixed", "entity": point["value"]})
            }
            "distance" => json!({
                "kind": "distance_loci",
                "first": locus("p")?,
                "second": locus("q")?,
                "parameter": parameter?,
            }),
            "distance_x" | "distance_y" => json!({
                "kind": if kind == "distance_x" { "horizontal_distance" } else { "vertical_distance" },
                "first": locus("p")?,
                "second": locus("q")?,
                "parameter": parameter?,
            }),
            "point_line_distance" => json!({
                "kind": "distance_loci",
                "first": locus("p")?,
                "second": {"kind": "entity", "value": curve("l")?},
                "parameter": parameter?,
            }),
            "length" => {
                json!({"kind": "distance", "entities": [curve("l")?], "parameter": parameter?})
            }
            "line_angle" => json!({
                "kind": "angle_to_axis",
                "entity": curve("l")?,
                "axis": "horizontal",
                "parameter": parameter?,
            }),
            "angle" if record.get("flip").and_then(Value::as_bool) != Some(true) => json!({
                "kind": "angle",
                "first": curve("a")?,
                "second": curve("b")?,
                "parameter": parameter?,
            }),
            "radius" | "diameter" => {
                json!({"kind": kind, "entity": curve("c")?, "parameter": parameter?})
            }
            _ => return None,
        })
    }

    /// A native relation of `kind`: its scalar fields, the entities of its
    /// curves, and its point and curve operands.
    fn native(
        &self,
        kind: &str,
        record: &Value,
        parameter: Option<&str>,
        relations: &mut Relations,
    ) -> Value {
        let mut properties = BTreeMap::new();
        let mut entities = Vec::new();
        let mut operands = Vec::new();
        if let Some(object) = record.as_object() {
            for (field, value) in object {
                if matches!(field.as_str(), "id" | "type" | "param" | "driven" | "text")
                    || field.is_empty()
                {
                    continue;
                }
                let curve = matches!(field.as_str(), "l" | "c" | "a" | "b");
                let point = matches!(field.as_str(), "p" | "q" | "center");
                if let (true, Some(index)) = (curve || point, value.as_u64()) {
                    let entity = if curve {
                        self.loci.curves.get(&index).cloned()
                    } else {
                        self.locus(index, relations)
                            .and_then(|locus| locus["value"].as_str().map(str::to_owned))
                    };
                    if let Some(entity) = entity {
                        if !entities.contains(&entity) {
                            entities.push(entity);
                        }
                    }
                    operands.push(json!({
                        "native_kind": if curve { "curve" } else { "point" },
                        "field": {"name": field},
                        "object_index": u32::try_from(index).ok(),
                    }));
                    continue;
                }
                let text = match value {
                    Value::String(text) => text.clone(),
                    Value::Number(_) | Value::Bool(_) => value.to_string(),
                    _ => continue,
                };
                properties.insert(field.clone(), text);
            }
        }
        json!({
            "kind": "native",
            "native_kind": format!("solidcraft:{kind}"),
            "native_properties": properties,
            "entities": entities,
            "parameter": parameter,
            "operands": operands,
        })
    }

    /// Adds a constraint; `false` when the neutral model refuses it.
    fn push(
        &self,
        relations: &mut Relations,
        name: &str,
        mut definition: Value,
        state: &Value,
    ) -> bool {
        if let Value::Object(fields) = &mut definition {
            fields.retain(|_, value| !value.is_null());
        }
        let mut record = serde_json::Map::new();
        record.insert(
            "id".to_owned(),
            json!(id("constraint", &format!("{}:{name}", self.key))),
        );
        record.insert("sketch".to_owned(), json!(self.sketch));
        record.insert("definition".to_owned(), definition);
        if let Some(state) = state.as_object() {
            record.extend(state.clone());
        }
        let record = Value::Object(record);
        match serde_json::from_value::<SketchConstraint>(record) {
            Ok(constraint) => {
                relations.constraints.push(constraint);
                true
            }
            Err(_) => false,
        }
    }
}
