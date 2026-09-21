// SPDX-License-Identifier: Apache-2.0
//! `[f64; 3]` storage facade over [`cadmpeg_ir::math::Vector3`].

use cadmpeg_ir::math::Vector3;

const EPS_NEAR_ZERO: f64 = 1.0e-12;

/// The four row-major vectors of a twelve-slot `[4][3]` local system: the two
/// stored directions, the stored axis, and the origin.
pub(crate) fn local_system_lanes(slots: [f64; 12]) -> [[f64; 3]; 4] {
    let [ax, ay, az, bx, by, bz, cx, cy, cz, ox, oy, oz] = slots;
    [[ax, ay, az], [bx, by, bz], [cx, cy, cz], [ox, oy, oz]]
}

pub(crate) fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    Vector3::from(left).dot(Vector3::from(right))
}

pub(crate) fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    <[f64; 3]>::from(Vector3::from(left).cross(Vector3::from(right)))
}

pub(crate) fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    <[f64; 3]>::from(Vector3::from(left) + Vector3::from(right))
}

pub(crate) fn scale(vector: [f64; 3], factor: f64) -> [f64; 3] {
    <[f64; 3]>::from(Vector3::from(vector).scale(factor))
}

pub(crate) fn normalize(vector: [f64; 3]) -> Option<[f64; 3]> {
    normalize_with_length(vector).map(|(unit, _)| unit)
}

pub(crate) fn normalize_with_length(vector: [f64; 3]) -> Option<([f64; 3], f64)> {
    let vector = Vector3::from(vector);
    let magnitude = vector.norm();
    (magnitude.is_finite() && magnitude > EPS_NEAR_ZERO).then(|| {
        (
            [
                vector.x / magnitude,
                vector.y / magnitude,
                vector.z / magnitude,
            ],
            magnitude,
        )
    })
}
