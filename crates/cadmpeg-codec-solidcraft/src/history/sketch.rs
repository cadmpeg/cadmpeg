// SPDX-License-Identifier: Apache-2.0
//! Sketch planes and sketch geometry.
//!
//! A sketch plane is an origin and orthonormal x and y axes; its normal is
//! x × y, and sketch point `(u, v)` is `origin + u x + v y`. The origin
//! planes are `XY` (normal +Z), `XZ` (x +X, y −Z, normal +Y), and `YZ`
//! (x +Y, y +Z, normal +X). A sketch's points are stored once and its
//! curves index them: a line runs from `a` to `b`, a circle is a centre `c`
//! and a radius `r`, an arc turns counterclockwise about `c` from `a` to
//! `b`, an ellipse is a centre `c`, the end `m` of its major axis, and a
//! minor radius `r`, and a control spline is the clamped uniform B-spline
//! of its `degree` over its points.

use std::f64::consts::TAU;

use cadmpeg_ir::math::portable::Portable;
use serde_json::{json, Value};

use super::expr::{Kind, Scope};
use super::fit;

/// A 3-vector.
pub(super) type V3 = [f64; 3];

/// A sketch plane frame.
#[derive(Debug, Clone, Copy)]
pub(super) struct Frame {
    /// The origin.
    pub(super) origin: V3,
    /// The sketch x axis.
    pub(super) x: V3,
    /// The sketch y axis.
    pub(super) y: V3,
}

pub(super) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(super) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn scaled(a: V3, factor: f64) -> V3 {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn plus(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// The JSON point of a 3-vector.
pub(super) fn point_json(a: V3) -> Value {
    json!({"x": a[0], "y": a[1], "z": a[2]})
}

impl Frame {
    /// The normal, x × y.
    pub(super) fn normal(&self) -> V3 {
        cross(self.x, self.y)
    }

    /// The model point of sketch point `(u, v)`.
    pub(super) fn world(&self, [u, v]: [f64; 2]) -> V3 {
        plus(self.origin, plus(scaled(self.x, u), scaled(self.y, v)))
    }

    /// The model direction of sketch direction `(u, v)`.
    pub(super) fn direction(&self, [u, v]: [f64; 2]) -> V3 {
        plus(scaled(self.x, u), scaled(self.y, v))
    }

    /// The placement record of the frame.
    pub(super) fn placement(&self) -> Value {
        json!({
            "kind": "resolved",
            "frame": {
                "origin": point_json(self.origin),
                "normal": point_json(self.normal()),
                "u_axis": point_json(self.x),
            }
        })
    }
}

fn vector(value: Option<&Value>, scope: &Scope) -> Option<V3> {
    let items = value?.as_array()?;
    let [x, y, z] = items.as_slice() else {
        return None;
    };
    Some([
        scope.eval_json(x, Kind::Length)?,
        scope.eval_json(y, Kind::Length)?,
        scope.eval_json(z, Kind::Length)?,
    ])
}

/// A point of length expressions: `[x, y, z]`.
pub(super) fn point(value: Option<&Value>, scope: &Scope) -> Option<V3> {
    vector(value, scope)
}

/// A direction: `[x, y, z]` numbers.
pub(super) fn direction(value: Option<&Value>) -> Option<V3> {
    let items = value?.as_array()?;
    let [x, y, z] = items.as_slice() else {
        return None;
    };
    Some([x.as_f64()?, y.as_f64()?, z.as_f64()?])
}

/// The frame of a named origin plane.
pub(super) fn origin_plane(name: &str) -> Option<Frame> {
    let origin = [0.0; 3];
    Some(match name.to_ascii_uppercase().as_str() {
        "XY" | "TOP" => Frame {
            origin,
            x: [1.0, 0.0, 0.0],
            y: [0.0, 1.0, 0.0],
        },
        "XZ" | "FRONT" => Frame {
            origin,
            x: [1.0, 0.0, 0.0],
            y: [0.0, 0.0, -1.0],
        },
        "YZ" | "RIGHT" => Frame {
            origin,
            x: [0.0, 1.0, 0.0],
            y: [0.0, 0.0, 1.0],
        },
        _ => return None,
    })
}

/// The frame of a plane reference. A face plane takes the frame it had
/// when it was picked, and a construction plane the frame of its feature
/// in `planes`; another plane that depends on the model has none.
pub(super) fn plane(
    reference: &Value,
    scope: &Scope,
    planes: &std::collections::BTreeMap<u64, Frame>,
    depth: usize,
) -> Option<Frame> {
    if depth > 16 {
        return None;
    }
    let base = |key: &str| plane(reference.get(key)?, scope, planes, depth + 1);
    match reference.get("type")?.as_str()? {
        "face" => {
            let plane = reference.get("plane")?;
            Some(Frame {
                origin: direction(plane.get("origin"))?,
                x: direction(plane.get("x_dir"))?,
                y: direction(plane.get("y_dir"))?,
            })
        }
        "construction" => planes.get(&reference.get("feature")?.as_u64()?).copied(),
        "origin" => origin_plane(reference.get("name")?.as_str()?),
        "custom" => {
            let plane = reference.get("plane")?;
            Some(Frame {
                origin: direction(plane.get("origin"))?,
                x: direction(plane.get("x_dir"))?,
                y: direction(plane.get("y_dir"))?,
            })
        }
        "placed" => Some(Frame {
            origin: point(reference.get("origin"), scope)?,
            x: direction(reference.get("x_dir"))?,
            y: direction(reference.get("y_dir"))?,
        }),
        "offset" => {
            let base = base("base")?;
            let distance = scope.eval_json(reference.get("distance")?, Kind::Length)?;
            Some(Frame {
                origin: plus(base.origin, scaled(base.normal(), distance)),
                ..base
            })
        }
        "flipped" => {
            let base = base("base")?;
            let about_y = reference.get("about_y").and_then(Value::as_bool) == Some(true);
            Some(if about_y {
                Frame {
                    x: scaled(base.x, -1.0),
                    ..base
                }
            } else {
                Frame {
                    y: scaled(base.y, -1.0),
                    ..base
                }
            })
        }
        _ => None,
    }
}

/// A sketch point: its id and position.
pub(super) struct SketchPoint {
    /// The point id.
    pub(super) id: String,
    /// The position.
    pub(super) pos: [f64; 2],
}

/// The points of a sketch record.
pub(super) fn points(sketch: &Value) -> Vec<Option<SketchPoint>> {
    sketch
        .get("points")
        .and_then(Value::as_array)
        .map(|points| {
            points
                .iter()
                .map(|point| {
                    let pos = point.get("pos")?.as_array()?;
                    let [u, v] = pos.as_slice() else {
                        return None;
                    };
                    Some(SketchPoint {
                        id: point.get("id")?.as_str()?.to_owned(),
                        pos: [u.as_f64()?, v.as_f64()?],
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn uv([u, v]: [f64; 2]) -> Value {
    json!({"u": u, "v": v})
}

fn angle_of(centre: [f64; 2], at: [f64; 2]) -> f64 {
    let turn = (at[1] - centre[1]).portable_atan2(at[0] - centre[0]);
    if turn < 0.0 {
        turn + TAU
    } else {
        turn
    }
}

fn distance(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// Whether a spline record is a fit spline of the natural interpolant: it has
/// no tangent handles and is not periodic.
fn fit_spline(curve: &Value) -> bool {
    curve.get("control").and_then(Value::as_bool) != Some(true)
        && curve.get("periodic").and_then(Value::as_bool) != Some(true)
        && curve.get("handles").is_none_or(|handles| {
            handles.is_null() || handles.as_array().is_some_and(Vec::is_empty)
        })
}

/// The positions of the points that a spline record indexes.
fn point_list(curve: &Value, points: &[Option<SketchPoint>]) -> Option<Vec<[f64; 2]>> {
    curve
        .get("pts")?
        .as_array()?
        .iter()
        .map(|index| {
            Some(
                points
                    .get(usize::try_from(index.as_u64()?).ok()?)?
                    .as_ref()?
                    .pos,
            )
        })
        .collect()
}

/// The neutral geometry of a sketch curve record, or `None` for a curve
/// that has no exact neutral form here (a conic, a weighted or periodic
/// spline, a spline with tangent handles).
pub(super) fn curve_geometry(curve: &Value, points: &[Option<SketchPoint>]) -> Option<Value> {
    let at = |key: &str| -> Option<[f64; 2]> {
        let index = usize::try_from(curve.get(key)?.as_u64()?).ok()?;
        Some(points.get(index)?.as_ref()?.pos)
    };
    Some(match curve.get("type")?.as_str()? {
        "line" => json!({"kind": "line", "start": uv(at("a")?), "end": uv(at("b")?)}),
        "circle" => {
            let radius = curve.get("r")?.as_f64()?;
            json!({"kind": "circle", "center": uv(at("c")?), "radius": radius})
        }
        "arc" => {
            let (centre, start, end) = (at("c")?, at("a")?, at("b")?);
            let start_angle = angle_of(centre, start);
            let mut end_angle = angle_of(centre, end);
            if end_angle <= start_angle {
                end_angle += TAU;
            }
            json!({
                "kind": "arc",
                "center": uv(centre),
                "radius": distance(centre, start),
                "start_angle": start_angle,
                "end_angle": end_angle,
            })
        }
        "ellipse" => {
            let (centre, major) = (at("c")?, at("m")?);
            let semi_major = distance(centre, major);
            let semi_minor = curve.get("r")?.as_f64()?;
            let (major_radius, minor_radius, major_angle) = if semi_major >= semi_minor {
                (semi_major, semi_minor, angle_of(centre, major))
            } else {
                (semi_minor, semi_major, angle_of(centre, major) + TAU / 4.0)
            };
            json!({
                "kind": "ellipse",
                "center": uv(centre),
                "major_angle": major_angle,
                "major_radius": major_radius,
                "minor_radius": minor_radius,
            })
        }
        "spline" if fit_spline(curve) => {
            let pieces = fit::bezier_pieces(&point_list(curve, points)?)?;
            let (knots, poles) = fit::bezier_chain(&pieces);
            json!({
                "kind": "nurbs",
                "curve": {
                    "degree": 3,
                    "knots": knots,
                    "periodic": false,
                    "poles": {"kind": "polynomial", "points": poles.into_iter().map(uv).collect::<Vec<_>>()},
                }
            })
        }
        "spline"
            if curve.get("control").and_then(Value::as_bool) == Some(true)
                && curve.get("nurbs").is_none_or(Value::is_null)
                && curve.get("periodic").and_then(Value::as_bool) != Some(true) =>
        {
            let poles = point_list(curve, points)?;
            let degree = usize::from(
                u8::try_from(curve.get("degree").and_then(Value::as_u64).unwrap_or(3)).ok()?,
            )
            .min(poles.len().checked_sub(1)?);
            json!({
                "kind": "nurbs",
                "curve": {
                    "degree": degree,
                    "knots": clamped_uniform(poles.len(), degree),
                    "periodic": false,
                    "poles": {"kind": "polynomial", "points": poles.iter().copied().map(uv).collect::<Vec<_>>()},
                }
            })
        }
        "spline"
            if curve.get("control").and_then(Value::as_bool) == Some(true)
                && curve.get("nurbs").is_some_and(|nurbs| !nurbs.is_null())
                && curve.get("periodic").and_then(Value::as_bool) != Some(true) =>
        {
            let poles = point_list(curve, points)?;
            let degree = usize::from(u8::try_from(curve.get("degree")?.as_u64()?).ok()?);
            let nurbs = curve.get("nurbs")?;
            let knots: Vec<f64> = nurbs
                .get("knots")?
                .as_array()?
                .iter()
                .map(Value::as_f64)
                .collect::<Option<_>>()?;
            let weights: Vec<f64> = match nurbs.get("weights") {
                None | Some(Value::Null) => Vec::new(),
                Some(weights) => weights
                    .as_array()?
                    .iter()
                    .map(Value::as_f64)
                    .collect::<Option<_>>()?,
            };
            if degree == 0 || poles.len() <= degree || knots.len() != poles.len() + degree + 1 {
                return None;
            }
            let poles = if weights.is_empty() {
                json!({"kind": "polynomial", "points": poles.iter().copied().map(uv).collect::<Vec<_>>()})
            } else if weights.len() == poles.len() {
                json!({"kind": "rational", "points": poles
                    .iter()
                    .zip(&weights)
                    .map(|(pole, weight)| json!({"point": uv(*pole), "weight": weight}))
                    .collect::<Vec<_>>()})
            } else {
                return None;
            };
            json!({"kind": "nurbs", "curve": {
                "degree": degree, "knots": knots, "periodic": false, "poles": poles,
            }})
        }
        "conic" => {
            // A conic from `a` to `b` shaped by `apex` is the rational
            // quadratic whose middle weight is rho / (1 - rho): 0.5 is the
            // parabola, below it the ellipse, above it the hyperbola.
            let rho = curve
                .get("rho")?
                .as_f64()
                .filter(|rho| *rho > 0.0 && *rho < 1.0)?;
            let (a, b, apex) = (at("a")?, at("b")?, at("apex")?);
            json!({"kind": "nurbs", "curve": {
                "degree": 2,
                "knots": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                "periodic": false,
                "poles": {"kind": "rational", "points": [
                    {"point": uv(a), "weight": 1.0},
                    {"point": uv(apex), "weight": rho / (1.0 - rho)},
                    {"point": uv(b), "weight": 1.0},
                ]},
            }})
        }
        _ => return None,
    })
}

/// The clamped uniform knot vector of `count` poles of `degree`, on [0, 1].
fn clamped_uniform(count: usize, degree: usize) -> Vec<f64> {
    let spans = count - degree;
    let mut knots = Vec::with_capacity(count + degree + 1);
    let mut step = 0.0;
    let mut steps = 0.0;
    for _ in 0..spans {
        steps += 1.0;
    }
    for index in 0..=(count + degree) {
        if index <= degree {
            knots.push(0.0);
        } else if index >= count {
            knots.push(1.0);
        } else {
            step += 1.0;
            knots.push(step / steps);
        }
    }
    knots
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sketch_points(positions: &[[f64; 2]]) -> Vec<Option<SketchPoint>> {
        positions
            .iter()
            .enumerate()
            .map(|(index, pos)| {
                Some(SketchPoint {
                    id: format!("p{index}"),
                    pos: *pos,
                })
            })
            .collect()
    }

    #[test]
    fn the_origin_planes_have_their_documented_normals() {
        assert_eq!(origin_plane("XY").unwrap().normal(), [0.0, 0.0, 1.0]);
        assert_eq!(origin_plane("XZ").unwrap().normal(), [0.0, 1.0, 0.0]);
        assert_eq!(origin_plane("yz").unwrap().normal(), [1.0, 0.0, 0.0]);
    }

    #[test]
    fn an_offset_flipped_plane_moves_along_its_base_normal() {
        let scope = Scope::default();
        let reference = json!({"type": "flipped", "base": {"type": "offset", "base": {"type": "origin", "name": "XY"}, "distance": "5"}});
        let frame = plane(&reference, &scope, &std::collections::BTreeMap::new(), 0).unwrap();
        assert_eq!(frame.origin, [0.0, 0.0, 5.0]);
        assert_eq!(frame.normal(), [0.0, 0.0, -1.0]);
    }

    #[test]
    fn an_arc_turns_counterclockwise_from_a_to_b() {
        let points = sketch_points(&[[0.0, 0.0], [0.0, 2.0], [2.0, 0.0]]);
        let geometry =
            curve_geometry(&json!({"type": "arc", "c": 0, "a": 1, "b": 2}), &points).unwrap();
        assert_eq!(geometry["radius"], 2.0);
        let start = geometry["start_angle"].as_f64().unwrap();
        let end = geometry["end_angle"].as_f64().unwrap();
        assert!((start - TAU / 4.0).abs() < 1e-12 && (end - TAU).abs() < 1e-12);
    }

    #[test]
    fn a_control_spline_is_clamped_and_uniform() {
        assert_eq!(
            clamped_uniform(5, 3),
            vec![0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0]
        );
        assert_eq!(clamped_uniform(2, 1), vec![0.0, 0.0, 1.0, 1.0]);
    }
}
