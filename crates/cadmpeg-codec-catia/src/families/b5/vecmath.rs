// SPDX-License-Identifier: Apache-2.0
//! Byte-identical `[f64; 3]` vector helpers shared by the b5 parse graph and
//! its IR transfer passes.
//!
//! Only the operations whose implementations are identical across both sides
//! live here. Each side keeps its own `unit` because they normalize by
//! bit-level-distinct arithmetic (reciprocal-multiply on the graph side,
//! per-component division on the transfer side) that must not be unified.

pub(super) fn add(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

pub(super) fn scale(value: [f64; 3], scalar: f64) -> [f64; 3] {
    [value[0] * scalar, value[1] * scalar, value[2] * scalar]
}

pub(super) fn cross(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}
