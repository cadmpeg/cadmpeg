// SPDX-License-Identifier: Apache-2.0
//! `[f64; 3]` storage facade over [`cadmpeg_ir::math::Vector3`].

use cadmpeg_ir::math::Vector3;

const EPS_NEAR_ZERO: f64 = 1.0e-12;

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

pub(crate) use normalize as normalized;
