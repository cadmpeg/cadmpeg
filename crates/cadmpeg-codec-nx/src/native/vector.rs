// SPDX-License-Identifier: Apache-2.0
//! Vector primitives shared by native feature semantics and decode geometry.

use cadmpeg_ir::math::Vector3;

pub(crate) fn cross_vector(first: Vector3, second: Vector3) -> Vector3 {
    first.cross(second)
}

pub(crate) fn dot_vector(first: Vector3, second: Vector3) -> f64 {
    first.dot(second)
}
