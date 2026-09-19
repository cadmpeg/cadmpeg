// SPDX-License-Identifier: Apache-2.0
//! Byte-identical `[f64; 3]` vector helpers shared by the b5 parse graph and
//! its IR transfer passes.
//!
//! `add`, `scale`, and `cross` go through [`cadmpeg_ir::math::Vector3`].
//! Normalization is not shared: the graph side has
//! `unit_by_reciprocal_multiply` and the transfer side has
//! `unit_by_component_division`. They have the same refusal set but different
//! last-bit results, so each side keeps its own.

use cadmpeg_ir::math::Vector3;

pub(super) fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    (Vector3::from(left) + Vector3::from(right)).into()
}

pub(super) fn scale(value: [f64; 3], scalar: f64) -> [f64; 3] {
    Vector3::from(value).scale(scalar).into()
}

pub(super) fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    Vector3::from(left).cross(Vector3::from(right)).into()
}
