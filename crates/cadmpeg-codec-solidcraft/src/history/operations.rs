// SPDX-License-Identifier: Apache-2.0
//! The operations past sketches, extrudes, revolves, and primitives.
//!
//! A fillet, chamfer, shell, move, remove, combine, scale, pattern, mirror,
//! sweep, and loft carry their operation semantics. A construction plane is
//! a datum plane. A pick is a model point that snaps to the nearest edge or
//! face; an edge or face operand keeps its picks and their persistent names
//! as its native selection, and a body operand keeps the body names.

use std::collections::{BTreeMap, BTreeSet};
use std::f64::consts::TAU;

use cadmpeg_ir::math::portable::Portable;
use serde_json::{json, Value};

use super::expr::{Kind, Scope};
use super::sketch::{self, point_json, Frame, V3};
use super::{boolean, id, profile, SketchUse};

/// How far from a full turn a circular pattern's total angle may round
/// and still spread its copies evenly around.
const FULL_TURN_ROUNDING: f64 = 1.0e-9;
/// How small the squared sine between a coil axis and +x may be before the
/// axis counts as along x, whose start direction is then perpendicular.
const COIL_AXIS_ALONG_X: f64 = 1.0e-12;

/// The model state an operation reads.
pub(super) struct Inputs<'i> {
    /// The parameter scope.
    pub(super) scope: &'i Scope,
    /// The frames of the construction planes, by feature id.
    pub(super) planes: &'i BTreeMap<u64, Frame>,
    /// The resolved sketches, by feature id.
    pub(super) sketches: &'i BTreeMap<u64, SketchUse>,
}

/// The native selection of picked edges or faces: the picks and their
/// persistent names.
fn picked(record: &Value, picks: &str, names: &str) -> Option<Value> {
    let points = record.get(picks)?;
    if points.as_array().is_none_or(Vec::is_empty) {
        return None;
    }
    let native = json!({"picks": points, "names": record.get(names).unwrap_or(&Value::Null)});
    Some(json!({"kind": "native", "value": native.to_string()}))
}

/// The native selection of named bodies.
fn bodies(names: &Value) -> Option<Value> {
    let names = match names {
        Value::String(name) => vec![Value::String(name.clone())],
        Value::Array(names) if !names.is_empty() => names.clone(),
        _ => return None,
    };
    Some(json!({"kind": "native", "value": Value::Array(names).to_string()}))
}

fn length(scope: &Scope, value: Option<&Value>) -> Option<f64> {
    scope
        .eval_json(value?, Kind::Length)
        .filter(|value| *value > 0.0)
}

fn unit(vector: V3) -> Option<V3> {
    let norm = sketch::dot(vector, vector).sqrt();
    (norm.is_finite() && norm > 0.0).then(|| vector.map(|component| component / norm))
}

fn count(scope: &Scope, value: Option<&Value>) -> Option<u32> {
    let count = scope.eval_json(value?, Kind::Unitless)?.round();
    if !(1.0..=f64::from(u32::MAX)).contains(&count) {
        return None;
    }
    format!("{count:.0}").parse().ok()
}

impl Inputs<'_> {
    /// The operation of a feature record of `kind`; `None` for a kind
    /// without neutral semantics or inputs that do not resolve. The
    /// features it reads are added to `dependencies`.
    pub(super) fn operation(
        &self,
        kind: &str,
        record: &Value,
        dependencies: &mut BTreeSet<u64>,
    ) -> Option<Value> {
        match kind {
            "fillet" => self.fillet(record),
            "chamfer" => self.chamfer(record),
            "shell" => self.shell(record),
            "move" => self.motion(record),
            "remove" => Some(json!({"kind": "delete_body", "value": {
                "bodies": bodies(record.get("bodies")?)?,
                "mode": "delete_selected",
            }})),
            "combine" => Some(json!({"kind": "combine", "value": {
                "operands": {
                    "target": bodies(record.get("target")?)?,
                    "tools": bodies(record.get("tools")?)?,
                },
                "op": boolean(record),
                "keep_tools": record.get("keep_tools").and_then(Value::as_bool) == Some(true),
                "destination": "target_component",
            }})),
            "scale" => self.scale(record),
            "pattern" => self.pattern(record, dependencies),
            "mirror" => self.mirror(record, dependencies),
            "sweep" => self.sweep(record, dependencies),
            "coil" => self.coil(record),
            "delete_face" => Some(json!({"kind": "delete_face", "value": {
                "faces": picked(record, "faces", "face_names")?,
                "heal": record.get("heal").and_then(Value::as_bool) != Some(false),
            }})),
            "offset_face" => Some(json!({"kind": "move_face", "value": {
                "faces": picked(record, "faces", "face_names")?,
                "motion": {
                    "kind": "offset",
                    "distance": self.scope.eval_json(record.get("distance")?, Kind::Length)?,
                },
            }})),
            "loft" => self.loft(record, dependencies),
            "construction_plane" => {
                let frame = sketch::plane(record.get("plane")?, self.scope, self.planes, 0)?;
                Some(json!({"kind": "datum_plane", "value": {"frame": {
                    "origin": point_json(frame.origin),
                    "normal": point_json(unit(frame.normal())?),
                    "u_axis": point_json(unit(frame.x)?),
                }}}))
            }
            _ => None,
        }
    }

    fn fillet(&self, record: &Value) -> Option<Value> {
        let radius = length(self.scope, record.get("radius"))?;
        let radius = match record.get("style").and_then(Value::as_str) {
            None | Some("constant") => json!({"kind": "constant", "radius": radius}),
            Some("chord") => json!({"kind": "chordal", "chord_length": radius}),
            Some(_) => return None,
        };
        Some(json!({"kind": "fillet", "value": {"groups": [{
            "edges": picked(record, "edges", "edge_names")?,
            "radius": radius,
        }]}}))
    }

    fn chamfer(&self, record: &Value) -> Option<Value> {
        let distance = length(self.scope, record.get("distance"))?;
        let second = record.get("distance2").filter(|value| !value.is_null());
        let angle = record.get("angle").filter(|value| !value.is_null());
        let spec = match (second, angle) {
            (Some(second), _) => json!({
                "kind": "two_distances",
                "first": distance,
                "second": length(self.scope, Some(second))?,
            }),
            (None, Some(angle)) => json!({
                "kind": "distance_angle",
                "distance": distance,
                "angle": self.scope.eval_json(angle, Kind::Angle)?,
            }),
            (None, None) => json!({"kind": "distance", "distance": distance}),
        };
        Some(json!({"kind": "chamfer", "value": {
            "groups": [{"edges": picked(record, "edges", "edge_names")?, "spec": spec}],
            "flip_direction": record.get("flip").and_then(Value::as_bool) == Some(true),
        }}))
    }

    fn shell(&self, record: &Value) -> Option<Value> {
        let outward = match record.get("direction").and_then(Value::as_str) {
            None | Some("" | "inside") => Some(false),
            Some("outside") => Some(true),
            Some(_) => None,
        };
        let mut shell = json!({
            "removed_faces": picked(record, "faces", "face_names")
                .unwrap_or_else(|| json!({"kind": "faces", "value": []})),
            "thickness": length(self.scope, record.get("thickness"))?,
        });
        if let Some(outward) = outward {
            shell["outward"] = json!(outward);
        }
        if let Some(body) = record.get("body").and_then(bodies) {
            shell["bodies"] = body;
        }
        Some(json!({"kind": "shell", "value": shell}))
    }

    fn motion(&self, record: &Value) -> Option<Value> {
        let translation = record.get("translate")?.as_array()?;
        let [x, y, z] = translation.as_slice() else {
            return None;
        };
        let along = |value: &Value| self.scope.eval_json(value, Kind::Length);
        let mut motion = json!({
            "bodies": bodies(record.get("bodies")?)?,
            "translation": point_json([along(x)?, along(y)?, along(z)?]),
            "copies": u32::from(record.get("copy").and_then(Value::as_bool) == Some(true)),
        });
        let angle = record
            .get("angle")
            .filter(|value| !value.is_null())
            .map(|angle| self.scope.eval_json(angle, Kind::Angle));
        if let Some(angle) = angle {
            if record
                .get("rotate_about")
                .is_some_and(|about| !about.is_null())
            {
                return None;
            }
            let axis = unit(sketch::direction(record.get("rotate_axis"))?)?;
            let origin = match record.get("origin") {
                None | Some(Value::Null) => [0.0; 3],
                origin => sketch::point(origin, self.scope)?,
            };
            motion["rotation"] = json!({
                "origin": point_json(origin),
                "direction": point_json(axis),
                "angle": angle?,
            });
        }
        Some(json!({"kind": "move_body", "value": motion}))
    }

    fn scale(&self, record: &Value) -> Option<Value> {
        let factor = |value: &Value| {
            self.scope
                .eval_json(value, Kind::Unitless)
                .filter(|factor| *factor != 0.0)
        };
        let factors = match record.get("factors").and_then(Value::as_array) {
            Some(factors) => {
                let [x, y, z] = factors.as_slice() else {
                    return None;
                };
                json!({"kind": "per_axis", "factors": [factor(x)?, factor(y)?, factor(z)?]})
            }
            None => json!({"kind": "uniform", "factor": factor(record.get("factor")?)?}),
        };
        let origin = sketch::direction(record.get("origin")).unwrap_or([0.0; 3]);
        Some(json!({"kind": "scale", "value": {
            "bodies": bodies(record.get("bodies")?)?,
            "center": {"kind": "point", "value": point_json(origin)},
            "factors": factors,
        }}))
    }

    /// The seeds of a pattern or mirror: its features, or its bodies.
    fn seeds(record: &Value, dependencies: &mut BTreeSet<u64>) -> Option<Vec<Value>> {
        if let Some(names) = record
            .get("bodies")
            .filter(|names| names.as_array().is_some_and(|names| !names.is_empty()))
        {
            return Some(vec![json!({"kind": "bodies", "value": bodies(names)?})]);
        }
        let features = record.get("features")?.as_array()?;
        let mut seeds = Vec::new();
        for feature in features {
            let feature = feature.as_u64()?;
            dependencies.insert(feature);
            seeds.push(json!({"kind": "feature", "value": id("feature", &feature.to_string())}));
        }
        (!seeds.is_empty()).then_some(seeds)
    }

    fn pattern(&self, record: &Value, dependencies: &mut BTreeSet<u64>) -> Option<Value> {
        let kind = record.get("pattern")?;
        let pattern = match kind.get("type")?.as_str()? {
            "rectangular" => self.rectangular(kind)?,
            "circular" => self.circular(kind)?,
            "path" => self.path(kind, dependencies)?,
            _ => return None,
        };
        let seeds = Self::seeds(record, dependencies)?;
        Some(json!({"kind": "pattern", "value": {"seeds": seeds, "pattern": pattern}}))
    }

    /// One direction of a rectangular pattern: its direction, count,
    /// spacing, and whether it spreads to both sides.
    fn direction(
        &self,
        pattern: &Value,
        suffix: &str,
        alignment: &str,
        reversed: &str,
    ) -> Option<(V3, u32, f64, bool)> {
        let mut direction = unit(sketch::direction(pattern.get(format!("dir{suffix}")))?)?;
        let count = count(self.scope, pattern.get(format!("count{suffix}")))?;
        let spacing = self
            .scope
            .eval_json(pattern.get(format!("spacing{suffix}"))?, Kind::Length)?;
        let alignment = pattern
            .get(alignment)
            .and_then(Value::as_str)
            .unwrap_or("right");
        let flip =
            (alignment == "left") != (pattern.get(reversed).and_then(Value::as_bool) == Some(true));
        let signed = if flip { -spacing } else { spacing };
        if signed < 0.0 {
            direction = direction.map(|component| -component);
        }
        Some((direction, count, signed.abs(), alignment == "center"))
    }

    fn rectangular(&self, pattern: &Value) -> Option<Value> {
        let (direction, count, spacing, symmetric) =
            self.direction(pattern, "1", "alignment", "reversed")?;
        let mut linear = json!({
            "kind": "linear",
            "direction": point_json(direction),
            "spacing": spacing,
            "count": count,
            "omitted_instances": [],
            "symmetric": symmetric,
        });
        if pattern.get("dir2").is_some_and(|dir| !dir.is_null()) {
            let (direction, count, spacing, symmetric) =
                self.direction(pattern, "2", "alignment2", "reversed2")?;
            linear["second"] = json!({
                "direction": point_json(direction),
                "spacing": spacing,
                "count": count,
                "symmetric": symmetric,
            });
        }
        Some(linear)
    }

    fn circular(&self, pattern: &Value) -> Option<Value> {
        if ["axis_ref", "axis_feature"]
            .iter()
            .any(|field| pattern.get(*field).is_some_and(|value| !value.is_null()))
        {
            return None;
        }
        let origin = sketch::point(pattern.get("origin"), self.scope)?;
        let mut axis = unit(sketch::direction(pattern.get("axis"))?)?;
        let count = count(self.scope, pattern.get("count"))?;
        let total = self.scope.eval_json(pattern.get("angle")?, Kind::Angle)?;
        let steps = f64::from(count) - 1.0;
        let full = (total.abs() - TAU).abs() < FULL_TURN_ROUNDING;
        let step = if full {
            total / f64::from(count)
        } else if steps > 0.0 {
            total / steps
        } else {
            0.0
        };
        let reversed = pattern.get("reversed").and_then(Value::as_bool) == Some(true);
        match pattern.get("alignment").and_then(Value::as_str) {
            None | Some("right" | "left") => {
                let left = pattern.get("alignment").and_then(Value::as_str) == Some("left");
                if (step < 0.0) != (left != reversed) {
                    axis = axis.map(|component| -component);
                }
                Some(json!({
                    "kind": "circular",
                    "axis_origin": point_json(origin),
                    "axis_dir": point_json(axis),
                    "angle": step.abs() * steps,
                    "count": count,
                }))
            }
            Some("center") => {
                let sign = if reversed { -1.0 } else { 1.0 };
                let mut angles = Vec::new();
                let mut index = 0_u32;
                while index < count {
                    let half = f64::from(index.div_ceil(2));
                    let multiplier = if index.is_multiple_of(2) { -half } else { half };
                    angles.push(step * multiplier * sign);
                    index += 1;
                }
                Some(json!({
                    "kind": "circular_angles",
                    "axis_origin": point_json(origin),
                    "axis_dir": point_json(axis),
                    "angles": angles,
                }))
            }
            Some(_) => None,
        }
    }

    /// The sketch curves of a path in the sketch `path_sketch`.
    fn path_curves(&self, record: &Value, dependencies: &mut BTreeSet<u64>) -> Option<Value> {
        let path_sketch = record.get("path_sketch")?.as_u64()?;
        let entities = &self.sketches.get(&path_sketch)?.entities;
        let mut curves = Vec::new();
        for curve in record.get("path")?.as_array()? {
            let curve = curve.as_str()?;
            if !entities.contains(curve) {
                return None;
            }
            let curve = id("entity", &format!("{path_sketch}:{curve}"));
            if curves.contains(&curve) {
                return None;
            }
            curves.push(curve);
        }
        (!curves.is_empty()).then_some(())?;
        dependencies.insert(path_sketch);
        Some(json!({"kind": "sketch_curves", "value": {
            "sketch": id("sketch", &path_sketch.to_string()),
            "curves": curves,
        }}))
    }

    /// A pattern along a path that translates its copies. A copy that turns
    /// with the path, or a reversed path, has no neutral form.
    fn path(&self, pattern: &Value, dependencies: &mut BTreeSet<u64>) -> Option<Value> {
        if ["flip", "orient"]
            .iter()
            .any(|field| pattern.get(*field).and_then(Value::as_bool) == Some(true))
        {
            return None;
        }
        let count = count(self.scope, pattern.get("count"))?;
        let distance = length(self.scope, pattern.get("spacing"))?;
        let spacing = if pattern.get("extent").and_then(Value::as_bool) == Some(true) {
            distance / (f64::from(count) - 1.0)
        } else {
            distance
        };
        Some(json!({
            "kind": "curve_driven",
            "path": self.path_curves(pattern, dependencies)?,
            "spacing": spacing,
            "count": count,
        }))
    }

    fn mirror(&self, record: &Value, dependencies: &mut BTreeSet<u64>) -> Option<Value> {
        if record.get("combine").and_then(Value::as_bool) == Some(true) {
            return None;
        }
        let frame = sketch::plane(record.get("plane")?, self.scope, self.planes, 0)?;
        if let Some(feature) = record
            .get("plane")
            .and_then(|plane| plane.get("feature"))
            .and_then(Value::as_u64)
        {
            dependencies.insert(feature);
        }
        let seeds = Self::seeds(record, dependencies)?;
        Some(
            json!({"kind": "pattern", "value": {"seeds": seeds, "pattern": {
                "kind": "mirror",
                "plane_origin": point_json(frame.origin),
                "plane_normal": point_json(unit(frame.normal())?),
            }}}),
        )
    }

    /// The profile section of the sketch that `record` names: a planar
    /// profile for a loft, a profile for a sweep.
    fn section(
        &self,
        record: &Value,
        planar: bool,
        dependencies: &mut BTreeSet<u64>,
    ) -> Option<Value> {
        let sketch = record.get("sketch")?.as_u64()?;
        self.sketches.contains_key(&sketch).then_some(())?;
        dependencies.insert(sketch);
        let profile = profile(record, sketch);
        Some(if planar {
            json!({"kind": "profile", "value": {"kind": "planar", "value": profile}})
        } else {
            json!({"kind": "profile", "value": profile})
        })
    }

    fn sweep(&self, record: &Value, dependencies: &mut BTreeSet<u64>) -> Option<Value> {
        let unsupported = ["guide", "twist", "taper", "scale", "orientation", "thin"]
            .iter()
            .any(|field| record.get(*field).is_some_and(|value| !value.is_null()));
        if unsupported {
            return None;
        }
        let section = self.section(record, false, dependencies)?;
        let path = self.path_curves(record, dependencies)?;
        Some(json!({"kind": "sweep", "value": {
            "shape": {"kind": "solid", "op": boolean(record), "section": section, "sections": []},
            "path": path,
            "path_tangent": false,
            "linearize": false,
        }}))
    }

    /// A coil of a generated section. Its turn is right-handed about its
    /// axis, clockwise when seen along the axis, unless `clockwise` turns
    /// it the other way; its start lies toward +x, turned by `start_angle`.
    fn coil(&self, record: &Value) -> Option<Value> {
        if record.get("sketch").is_some_and(|sketch| !sketch.is_null()) {
            return None;
        }
        let axis = unit(sketch::direction(record.get("axis")).unwrap_or([0.0, 0.0, 1.0]))?;
        let base = sketch::point(record.get("base"), self.scope)?;
        let size = length(self.scope, record.get("section_size"))?;
        let pitch = self.scope.eval_json(record.get("pitch")?, Kind::Length)?;
        let turns = self
            .scope
            .eval_json(record.get("turns")?, Kind::Unitless)
            .filter(|turns| *turns > 0.0)?;
        let start = match record.get("start_angle") {
            None | Some(Value::Null) => 0.0,
            Some(angle) => self.scope.eval_json(angle, Kind::Angle)?,
        };
        let along = sketch::dot([1.0, 0.0, 0.0], axis);
        let x0 = unit([1.0 - axis[0] * along, -axis[1] * along, -axis[2] * along])
            .filter(|_| {
                sketch::dot(
                    sketch::cross(axis, [1.0, 0.0, 0.0]),
                    sketch::cross(axis, [1.0, 0.0, 0.0]),
                ) > COIL_AXIS_ALONG_X
            })
            .or_else(|| unit(sketch::cross(axis, [0.0, 1.0, 0.0])))?;
        let turned = sketch::cross(axis, x0);
        let (sin, cos) = (start.portable_sin(), start.portable_cos());
        let radial = [
            x0[0] * cos + turned[0] * sin,
            x0[1] * cos + turned[1] * sin,
            x0[2] * cos + turned[2] * sin,
        ];
        let section = match record.get("section").and_then(Value::as_str) {
            None | Some("circular") => json!({"kind": "circular", "diameter": size}),
            Some("square") => json!({"kind": "square", "size": size}),
            Some(_) => return None,
        };
        let placement = match record.get("position").and_then(Value::as_i64).unwrap_or(0) {
            0 => "center",
            position if position < 0 => "inside",
            _ => "outside",
        };
        let result = match boolean(record) {
            "new_body" => json!({"kind": "new_body"}),
            operation => json!({
                "kind": "boolean",
                "operation": operation,
                "targets": record.get("targets").and_then(bodies).unwrap_or_else(|| json!({"kind": "unresolved"})),
            }),
        };
        Some(json!({"kind": "coil", "value": {
            "construction": {
                "placement": {
                    "kind": "explicit",
                    "origin": point_json(base),
                    "axis": point_json(axis),
                    "radial": point_json(radial),
                },
                "diameter": length(self.scope, record.get("diameter"))?,
                "extent": {"kind": "revolutions_pitch", "revolutions": turns, "pitch": pitch},
                "section": section,
                "section_placement": placement,
                "clockwise": record.get("clockwise").and_then(Value::as_bool) != Some(true),
                "taper": 0.0,
            },
            "result": result,
        }}))
    }

    fn loft(&self, record: &Value, dependencies: &mut BTreeSet<u64>) -> Option<Value> {
        let unsupported = ["rails", "guides", "centerline", "start", "end"]
            .iter()
            .any(|field| {
                record
                    .get(*field)
                    .is_some_and(|value| !value.is_null() && value != &json!([]))
            });
        if unsupported {
            return None;
        }
        let sections = record
            .get("sections")?
            .as_array()?
            .iter()
            .map(|section| self.section(section, true, dependencies))
            .collect::<Option<Vec<_>>>()?;
        (sections.len() >= 2).then_some(())?;
        Some(json!({"kind": "loft", "value": {
            "sections": sections,
            "guidance": {"kind": "guides", "value": []},
            "closed": record.get("closed").and_then(Value::as_bool) == Some(true),
            "ruled": record.get("ruled").and_then(Value::as_bool) == Some(true),
            "solid": true,
            "linearize": false,
            "op": boolean(record),
        }}))
    }
}
