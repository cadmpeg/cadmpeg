// SPDX-License-Identifier: Apache-2.0
//! Vector primitives shared by native feature semantics and decode geometry.

use cadmpeg_ir::math::Vector3;

pub(crate) fn cross_vector(first: Vector3, second: Vector3) -> Vector3 {
    Vector3::new(
        first.y * second.z - first.z * second.y,
        first.z * second.x - first.x * second.z,
        first.x * second.y - first.y * second.x,
    )
}

pub(crate) fn dot_vector(first: Vector3, second: Vector3) -> f64 {
    first.x * second.x + first.y * second.y + first.z * second.z
}

pub(crate) fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = dot_vector(vector, vector).sqrt();
    (norm.is_finite() && norm > 0.0)
        .then(|| Vector3::new(vector.x / norm, vector.y / norm, vector.z / norm))
}
