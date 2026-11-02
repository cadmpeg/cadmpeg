// SPDX-License-Identifier: Apache-2.0
//! The transform-op stack of a xformable prim.
//!
//! `xformOpOrder` names the ops; the first op is outermost. A matrix here
//! acts on column vectors: a point `p` maps to `M · p`. USD writes matrices
//! for row vectors, so a `transform` op value is transposed on read.

use crate::schema::{attribute, tokens, Issues};
use crate::stage::Prim;
use cadmpeg_ir::math::portable::Portable;

/// An affine map as three rows `[a, b, c, t]` acting on column vectors.
pub(crate) type Affine = [[f64; 4]; 3];

/// The identity map.
pub(crate) const IDENTITY: Affine = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
];

/// Prefix of an inverted op in `xformOpOrder`.
const INVERT_PREFIX: &str = "!invert!";
/// The `xformOpOrder` entry that drops the parent transform.
const RESET_XFORM_STACK: &str = "!resetXformStack!";
/// Namespace of transform-op attributes.
const OP_NAMESPACE: &str = "xformOp:";

/// The local transform of a prim.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LocalTransform {
    pub(crate) matrix: Affine,
    /// The prim drops its parent's transform.
    pub(crate) resets_stack: bool,
}

/// Why an op stack does not resolve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct XformError(pub(crate) String);

/// `left · right`.
pub(crate) fn compose(left: &Affine, right: &Affine) -> Affine {
    let mut out = [[0.0; 4]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        for (column, value) in out_row.iter_mut().enumerate() {
            let mut sum = 0.0;
            for inner in 0..3 {
                sum += left[row][inner] * right[inner][column];
            }
            if column == 3 {
                sum += left[row][3];
            }
            *value = sum;
        }
    }
    out
}

/// The inverse of `matrix`, or `None` when its linear part is singular.
pub(crate) fn invert(matrix: &Affine) -> Option<Affine> {
    let [[m00, m01, m02, tx], [m10, m11, m12, ty], [m20, m21, m22, tz]] = *matrix;
    let co = [
        m11 * m22 - m12 * m21,
        -(m10 * m22 - m12 * m20),
        m10 * m21 - m11 * m20,
        -(m01 * m22 - m02 * m21),
        m00 * m22 - m02 * m20,
        -(m00 * m21 - m01 * m20),
        m01 * m12 - m02 * m11,
        -(m00 * m12 - m02 * m10),
        m00 * m11 - m01 * m10,
    ];
    let det = m00 * co[0] + m01 * co[1] + m02 * co[2];
    if det == 0.0 || !det.is_finite() {
        return None;
    }
    let linear = [
        [co[0] / det, co[3] / det, co[6] / det],
        [co[1] / det, co[4] / det, co[7] / det],
        [co[2] / det, co[5] / det, co[8] / det],
    ];
    let translation = [tx, ty, tz];
    let mut out = [[0.0; 4]; 3];
    for (row, out_row) in out.iter_mut().enumerate() {
        out_row[..3].copy_from_slice(&linear[row]);
        out_row[3] = -(0..3)
            .map(|inner| linear[row][inner] * translation[inner])
            .sum::<f64>();
    }
    out.iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(out)
}

fn translation([x, y, z]: [f64; 3]) -> Affine {
    [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]]
}

fn scaling([x, y, z]: [f64; 3]) -> Affine {
    [[x, 0.0, 0.0, 0.0], [0.0, y, 0.0, 0.0], [0.0, 0.0, z, 0.0]]
}

/// Rotation by `degrees` about axis `axis` (0, 1, 2 for X, Y, Z).
fn rotation(axis: usize, degrees: f64) -> Affine {
    let (sin, cos) = degrees.to_radians().portable_sin_cos();
    match axis {
        0 => [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, cos, -sin, 0.0],
            [0.0, sin, cos, 0.0],
        ],
        1 => [
            [cos, 0.0, sin, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [-sin, 0.0, cos, 0.0],
        ],
        _ => [
            [cos, -sin, 0.0, 0.0],
            [sin, cos, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    }
}

/// Rotation of a quaternion `(real, i, j, k)`, normalized.
pub(crate) fn quaternion([w, x, y, z]: [f64; 4]) -> Option<Affine> {
    let norm = (w * w + x * x + y * y + z * z).sqrt();
    if norm == 0.0 || !norm.is_finite() {
        return None;
    }
    let (w, x, y, z) = (w / norm, x / norm, y / norm, z / norm);
    Some([
        [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y - w * z),
            2.0 * (x * z + w * y),
            0.0,
        ],
        [
            2.0 * (x * y + w * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z - w * x),
            0.0,
        ],
        [
            2.0 * (x * z - w * y),
            2.0 * (y * z + w * x),
            1.0 - 2.0 * (x * x + y * y),
            0.0,
        ],
    ])
}

/// The axis index of an axis letter.
fn axis(letter: u8) -> Option<usize> {
    match letter {
        b'X' => Some(0),
        b'Y' => Some(1),
        b'Z' => Some(2),
        _ => None,
    }
}

/// The axis that the last letter of a single-axis op type names.
fn axis_of(op_type: &str) -> Result<usize, String> {
    op_type
        .as_bytes()
        .last()
        .copied()
        .and_then(axis)
        .ok_or_else(|| format!("unknown op `{op_type}`"))
}

/// The matrix of one op from its type token and value components.
fn op_matrix(op_type: &str, components: &[f64]) -> Result<Affine, String> {
    let vector3 = || match components {
        [x, y, z] => Ok([*x, *y, *z]),
        _ => Err(format!("`{op_type}` needs three components")),
    };
    let scalar = || match components {
        [value] => Ok(*value),
        _ => Err(format!("`{op_type}` needs one component")),
    };
    match op_type {
        "translate" => Ok(translation(vector3()?)),
        "scale" => Ok(scaling(vector3()?)),
        "orient" => match components {
            [w, x, y, z] => quaternion([*w, *x, *y, *z])
                .ok_or_else(|| "`orient` has a zero quaternion".to_owned()),
            _ => Err("`orient` needs a quaternion".to_owned()),
        },
        "transform" => match components {
            [m00, m01, m02, m03, m10, m11, m12, m13, m20, m21, m22, m23, m30, m31, m32, w] => {
                if *m03 != 0.0 || *m13 != 0.0 || *m23 != 0.0 || *w == 0.0 {
                    return Err("`transform` is not affine".to_owned());
                }
                Ok([
                    [*m00 / w, *m10 / w, *m20 / w, *m30 / w],
                    [*m01 / w, *m11 / w, *m21 / w, *m31 / w],
                    [*m02 / w, *m12 / w, *m22 / w, *m32 / w],
                ])
            }
            _ => Err("`transform` needs a 4x4 matrix".to_owned()),
        },
        "translateX" | "translateY" | "translateZ" => {
            let mut offset = [0.0; 3];
            offset[axis_of(op_type)?] = scalar()?;
            Ok(translation(offset))
        }
        "scaleX" | "scaleY" | "scaleZ" => {
            let mut factors = [1.0; 3];
            factors[axis_of(op_type)?] = scalar()?;
            Ok(scaling(factors))
        }
        "rotateX" | "rotateY" | "rotateZ" => Ok(rotation(axis_of(op_type)?, scalar()?)),
        _ => {
            let order = op_type
                .strip_prefix("rotate")
                .filter(|order| order.len() == 3)
                .ok_or_else(|| format!("unknown op `{op_type}`"))?
                .as_bytes();
            let [Some(a), Some(b), Some(c)] = [order[0], order[1], order[2]].map(axis) else {
                return Err(format!("unknown op `{op_type}`"));
            };
            let angles = vector3()?;
            let first = rotation(a, angles[a]);
            let second = rotation(b, angles[b]);
            let third = rotation(c, angles[c]);
            Ok(compose(&compose(&third, &second), &first))
        }
    }
}

/// The local transform of `prim`, in scene units. A prim with no
/// `xformOpOrder` has the identity transform.
pub(crate) fn local_transform(
    prim: &Prim<'_>,
    issues: &mut Issues,
) -> Result<LocalTransform, XformError> {
    let mut local = LocalTransform {
        matrix: IDENTITY,
        resets_stack: false,
    };
    let Some(order) = tokens(prim, "xformOpOrder", issues) else {
        return Ok(local);
    };
    for (position, entry) in order.iter().enumerate() {
        if entry == RESET_XFORM_STACK {
            if position != 0 {
                return Err(XformError(format!(
                    "`{RESET_XFORM_STACK}` is entry {position} of xformOpOrder, not the first"
                )));
            }
            local.resets_stack = true;
            continue;
        }
        let (inverted, name) = match entry.strip_prefix(INVERT_PREFIX) {
            Some(name) => (true, name),
            None => (false, entry.as_str()),
        };
        let Some(op) = name.strip_prefix(OP_NAMESPACE) else {
            return Err(XformError(format!("`{entry}` is not a transform op")));
        };
        let op_type = op.split(':').next().unwrap_or(op);
        let Some(value) = attribute(prim, name, issues) else {
            continue;
        };
        let components = value
            .numbers()
            .ok_or_else(|| XformError(format!("op `{name}` has no numeric value")))?;
        let matrix = op_matrix(op_type, &components).map_err(XformError)?;
        let matrix = if inverted {
            invert(&matrix).ok_or_else(|| XformError(format!("op `{name}` is not invertible")))?
        } else {
            matrix
        };
        local.matrix = compose(&local.matrix, &matrix);
    }
    if local.matrix.iter().flatten().all(|value| value.is_finite()) {
        Ok(local)
    } else {
        Err(XformError("the op stack is not finite".to_owned()))
    }
}

#[cfg(test)]
mod tests;
