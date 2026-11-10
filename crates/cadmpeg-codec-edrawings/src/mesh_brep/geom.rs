// SPDX-License-Identifier: Apache-2.0
//! Small vector helpers and checked slot access.

use super::MeshBrepError;

/// A point or vector in millimetres.
pub(super) type V3 = [f64; 3];

/// Sentinel for "no index" in dense index tables.
pub(super) const NONE: usize = usize::MAX;

pub(super) fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub(super) fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub(super) fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub(super) fn norm(a: V3) -> f64 {
    dot(a, a).sqrt()
}

/// Distance from `p` to the closed segment `a..b` (a point when `a == b`).
pub(super) fn point_segment_distance(p: V3, a: V3, b: V3) -> f64 {
    let ab = sub(b, a);
    let ap = sub(p, a);
    let len2 = dot(ab, ab);
    let t = if len2 > 0.0 {
        (dot(ap, ab) / len2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let foot = [a[0] + ab[0] * t, a[1] + ab[1] * t, a[2] + ab[2] * t];
    norm(sub(p, foot))
}

/// Bounds-checked element access that reports a broken internal invariant
/// instead of panicking.
pub(super) trait Slot<T> {
    fn slot(&self, index: usize) -> Result<&T, MeshBrepError>;
    fn slot_mut(&mut self, index: usize) -> Result<&mut T, MeshBrepError>;
}

impl<T> Slot<T> for [T] {
    fn slot(&self, index: usize) -> Result<&T, MeshBrepError> {
        self.get(index)
            .ok_or(MeshBrepError::Internal("index out of range"))
    }

    fn slot_mut(&mut self, index: usize) -> Result<&mut T, MeshBrepError> {
        self.get_mut(index)
            .ok_or(MeshBrepError::Internal("index out of range"))
    }
}

/// Vertex corner `0..3` of a triangle.
pub(super) fn corner(tri: &[usize; 3], k: usize) -> Result<usize, MeshBrepError> {
    tri.get(k)
        .copied()
        .ok_or(MeshBrepError::Internal("triangle corner out of range"))
}
