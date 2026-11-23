// SPDX-License-Identifier: Apache-2.0
//! Cleaned triangle soup: welded vertices, no degenerate triangles.

use super::geom::{corner, cross, dot, norm, sub, Slot, NONE, V3};
use super::weld::Welded;
use super::{
    MeshBrepError, MIN_TRIANGLE_AREA_MM2, PLANE_ANGLE_TOLERANCE_RAD, PLANE_DISTANCE_TOLERANCE_MM,
    SLIVER_ANGLE_LIMIT_RAD,
};

/// Welded, non-degenerate triangles. Half-edge `3 * t + k` runs from corner
/// `k` of triangle `t` to corner `(k + 1) % 3`.
pub(super) struct Mesh {
    pub(super) pos: Vec<V3>,
    pub(super) tris: Vec<[usize; 3]>,
    /// Unit normal per triangle (right-hand rule).
    pub(super) normal: Vec<V3>,
    pub(super) area: Vec<f64>,
    /// Smallest admissible dot product between a region normal and this
    /// triangle's normal. Thin triangles get a looser bound because their
    /// computed normal is poorly conditioned.
    pub(super) min_cos: Vec<f64>,
    pub(super) dropped: usize,
    pub(super) signed_volume: f64,
}

impl Mesh {
    pub(super) fn build(welded: Welded, triangles: &[[u32; 3]]) -> Result<Self, MeshBrepError> {
        let Welded { positions, map } = welded;
        let mut mesh = Self {
            pos: positions,
            tris: Vec::new(),
            normal: Vec::new(),
            area: Vec::new(),
            min_cos: Vec::new(),
            dropped: 0,
            signed_volume: 0.0,
        };
        for triangle in triangles {
            let mut ids = [0_usize; 3];
            for (slot, &index) in ids.iter_mut().zip(triangle) {
                let i = usize::try_from(index)
                    .map_err(|_| MeshBrepError::Internal("index conversion"))?;
                *slot = *map.slot(i)?;
            }
            mesh.push(ids, MIN_TRIANGLE_AREA_MM2)?;
        }
        Ok(mesh)
    }

    /// Append triangle `ids` unless it is degenerate: a repeated or missing
    /// vertex, or an area below `min_area` (a zero area is always degenerate).
    fn push(&mut self, ids: [usize; 3], min_area: f64) -> Result<(), MeshBrepError> {
        let [a, b, c] = ids;
        if a == b || b == c || a == c || a == NONE || b == NONE || c == NONE {
            self.dropped += 1;
            return Ok(());
        }
        let (pa, pb, pc) = (*self.pos.slot(a)?, *self.pos.slot(b)?, *self.pos.slot(c)?);
        let normal_vector = cross(sub(pb, pa), sub(pc, pa));
        let double_area = norm(normal_vector);
        let area = 0.5 * double_area;
        if area.is_nan() || area < min_area || area <= 0.0 {
            self.dropped += 1;
            return Ok(());
        }
        let longest = norm(sub(pb, pa))
            .max(norm(sub(pc, pb)))
            .max(norm(sub(pa, pc)));
        let altitude = double_area / longest;
        let slack = (2.0 * PLANE_DISTANCE_TOLERANCE_MM / altitude)
            .atan()
            .clamp(PLANE_ANGLE_TOLERANCE_RAD, SLIVER_ANGLE_LIMIT_RAD);
        self.tris.push(ids);
        self.normal.push([
            normal_vector[0] / double_area,
            normal_vector[1] / double_area,
            normal_vector[2] / double_area,
        ]);
        self.area.push(area);
        self.min_cos.push(slack.cos());
        self.signed_volume += dot(pa, cross(pb, pc)) / 6.0;
        Ok(())
    }

    /// The same points with the triangles `tris`. Only triangles with a
    /// repeated vertex or zero area are dropped.
    pub(super) fn with_triangles(&self, tris: &[[usize; 3]]) -> Result<Self, MeshBrepError> {
        let mut mesh = Self {
            pos: self.pos.clone(),
            tris: Vec::new(),
            normal: Vec::new(),
            area: Vec::new(),
            min_cos: Vec::new(),
            dropped: self.dropped,
            signed_volume: 0.0,
        };
        for &ids in tris {
            mesh.push(ids, 0.0)?;
        }
        Ok(mesh)
    }

    /// Start vertex of half-edge `he`.
    pub(super) fn from(&self, he: usize) -> Result<usize, MeshBrepError> {
        corner(self.tris.slot(he / 3)?, he % 3)
    }

    /// End vertex of half-edge `he`.
    pub(super) fn to(&self, he: usize) -> Result<usize, MeshBrepError> {
        corner(self.tris.slot(he / 3)?, (he % 3 + 1) % 3)
    }
}

/// Next half-edge around the same triangle.
pub(super) const fn next_in_triangle(he: usize) -> usize {
    3 * (he / 3) + (he % 3 + 1) % 3
}
