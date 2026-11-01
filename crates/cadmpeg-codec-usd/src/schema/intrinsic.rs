// SPDX-License-Identifier: Apache-2.0
//! The `UsdGeom` intrinsic primitives (spec §11.5): their dimensions, in
//! scene units, as B-rep records.

use crate::brep::primitive::{self, Axis, Segment};
use crate::brep::Brep;
use crate::schema::{token, vector, Issues};
use crate::stage::Prim;

/// A dimension of `prim`, or its unauthored value.
fn dimension(prim: &Prim<'_>, name: &str, fallback: f64, issues: &mut Issues) -> f64 {
    vector(prim, name, 1, issues)
        .and_then(|values| values.first().copied())
        .unwrap_or(fallback)
}

fn axis(prim: &Prim<'_>, issues: &mut Issues) -> Option<Axis> {
    match token(prim, "axis", issues).as_deref() {
        None | Some("Z") => Some(Axis::Z),
        Some("X") => Some(Axis::X),
        Some("Y") => Some(Axis::Y),
        Some(_) => None,
    }
}

fn positive(values: &[f64]) -> bool {
    values.iter().all(|value| value.is_finite() && *value > 0.0)
}

/// The profile of a solid of revolution between heights `-height / 2` and
/// `height / 2`, with radius `bottom` and `top` at its ends. A zero radius
/// is an apex.
fn frustum(bottom: f64, top: f64, height: f64, axis: Axis) -> Brep {
    let (low, high) = (-height / 2.0, height / 2.0);
    let mut points = vec![[0.0, low]];
    if bottom > 0.0 {
        points.push([bottom, low]);
    }
    if top > 0.0 {
        points.push([top, high]);
    }
    points.push([0.0, high]);
    let segments = vec![Segment::Line; points.len() - 1];
    primitive::revolved(&points, &segments, axis)
}

/// The records of intrinsic primitive `prim`, or why it has none.
pub(crate) fn records(prim: &Prim<'_>, issues: &mut Issues) -> Result<Brep, String> {
    let type_name = prim.type_name().unwrap_or_default();
    let axis = axis(prim, issues).ok_or("the axis is not X, Y, or Z")?;
    let invalid = || format!("a {type_name} dimension is not a positive number");
    match type_name {
        "Cube" => {
            let size = dimension(prim, "size", 2.0, issues);
            positive(&[size])
                .then(|| primitive::cube(size))
                .ok_or_else(invalid)
        }
        "Sphere" => {
            let radius = dimension(prim, "radius", 1.0, issues);
            positive(&[radius])
                .then(|| primitive::sphere(radius))
                .ok_or_else(invalid)
        }
        "Cylinder" | "Cone" => {
            let radius = dimension(prim, "radius", 1.0, issues);
            let height = dimension(prim, "height", 2.0, issues);
            let top = if type_name == "Cone" { 0.0 } else { radius };
            positive(&[radius, height])
                .then(|| frustum(radius, top, height, axis))
                .ok_or_else(invalid)
        }
        "Cylinder_1" => {
            let bottom = dimension(prim, "radiusBottom", 1.0, issues);
            let top = dimension(prim, "radiusTop", 1.0, issues);
            let height = dimension(prim, "height", 2.0, issues);
            let radii = bottom.is_finite() && top.is_finite() && bottom >= 0.0 && top >= 0.0;
            (radii && bottom.max(top) > 0.0 && positive(&[height]))
                .then(|| frustum(bottom, top, height, axis))
                .ok_or_else(invalid)
        }
        "Capsule" | "Capsule_1" => {
            let (bottom, top) = if type_name == "Capsule" {
                let radius = dimension(prim, "radius", 0.5, issues);
                (radius, radius)
            } else {
                (
                    dimension(prim, "radiusBottom", 0.5, issues),
                    dimension(prim, "radiusTop", 0.5, issues),
                )
            };
            let height = dimension(prim, "height", 1.0, issues);
            if !positive(&[bottom, top, height]) {
                return Err(invalid());
            }
            if bottom != top {
                return Err("a capsule with unequal radii has no spherical caps".to_owned());
            }
            let (low, high) = (-height / 2.0, height / 2.0);
            Ok(primitive::revolved(
                &[
                    [0.0, low - bottom],
                    [bottom, low],
                    [top, high],
                    [0.0, high + top],
                ],
                &[
                    Segment::Arc { center: low },
                    Segment::Line,
                    Segment::Arc { center: high },
                ],
                axis,
            ))
        }
        "Plane" => {
            let width = dimension(prim, "width", 2.0, issues);
            let length = dimension(prim, "length", 2.0, issues);
            positive(&[width, length])
                .then(|| primitive::plane(width, length, axis))
                .ok_or_else(invalid)
        }
        _ => Err(format!("{type_name} is not an intrinsic primitive")),
    }
}
