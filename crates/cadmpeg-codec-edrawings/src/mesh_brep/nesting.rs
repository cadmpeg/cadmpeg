// SPDX-License-Identifier: Apache-2.0
//! Edge-connected components of a mesh, their containment, and orientation.
//!
//! A closed component bounds a solid region when it is not inside another
//! closed component (even depth) and a void when it is inside one (odd
//! depth). Triangles wind counter-clockwise seen from outside the material, so
//! an outer component has a positive signed volume and a void a negative one.

use super::geom::{cross, dot, sub, Slot, NONE, V3};
use super::mesh::Mesh;
use super::{
    MeshBrepError, MAX_NESTING_RAY_TESTS, MIN_REVERSAL_VOLUME_MM3, MIN_SHEET_AREA_MM2,
    NESTING_BBOX_SLACK_MM, PLANE_DISTANCE_TOLERANCE_MM, THIN_COMPONENT_FACTOR,
};

/// Ray directions of the parity test; a majority of the hits decides.
const RAY_DIRECTIONS: [V3; 3] = [
    [0.371_5, 0.516_3, 0.770_1],
    [-0.618_3, 0.279_1, 0.735_7],
    [0.145_7, -0.884_3, 0.442_9],
];

/// Edge-connected triangles.
pub(super) struct Component {
    /// A triangle of the component.
    pub(super) first: usize,
    pub(super) triangles: usize,
    /// Every half-edge has a twin.
    pub(super) closed: bool,
    /// Signed volume in cubic millimetres.
    pub(super) volume: f64,
    /// Surface area in square millimetres.
    area: f64,
    bbox: [V3; 2],
    /// Number of closed components that contain this one.
    pub(super) depth: usize,
    /// The innermost closed component that contains this one.
    pub(super) parent: Option<usize>,
}

impl Component {
    /// A closed component whose volume is too small to give it an orientation.
    pub(super) fn degenerate(&self) -> bool {
        self.closed && self.volume.abs() < MIN_REVERSAL_VOLUME_MM3
    }

    /// An open component too small to be a part: a flap of a few tiny
    /// triangles.
    pub(super) fn sliver(&self) -> bool {
        !self.closed && self.area < MIN_SHEET_AREA_MM2
    }

    /// A void whose volume is below what merging its triangles into planes of
    /// `PLANE_DISTANCE_TOLERANCE_MM` can move on both sides of its surface; the
    /// merged faces then give the void no reliable volume sign.
    fn thin_void(&self) -> bool {
        self.closed
            && !self.depth.is_multiple_of(2)
            && self.volume.abs() < THIN_COMPONENT_FACTOR * PLANE_DISTANCE_TOLERANCE_MM * self.area
    }

    /// A closed component that takes part in containment.
    fn solid(&self) -> bool {
        self.closed && !self.degenerate()
    }

    /// The sign of the volume that the depth requires.
    fn wrong_sign(&self) -> bool {
        self.solid() && !self.thin_void() && (self.volume > 0.0) != self.depth.is_multiple_of(2)
    }
}

pub(super) struct Nesting {
    pub(super) component_of: Vec<usize>,
    pub(super) components: Vec<Component>,
}

/// The triangles after dropping degenerate components and reversing components
/// of the wrong sign.
pub(super) struct Rewrite {
    pub(super) tris: Vec<[usize; 3]>,
    /// Old triangle index of every new triangle.
    pub(super) old_of_new: Vec<usize>,
    pub(super) dropped_triangles: usize,
    pub(super) dropped_zero_components: usize,
    pub(super) dropped_thin_components: usize,
    pub(super) dropped_sliver_components: usize,
    pub(super) reversed_components: usize,
}

fn find(parent: &mut [usize], item: usize) -> Result<usize, MeshBrepError> {
    let mut root = item;
    while *parent.slot(root)? != root {
        root = *parent.slot(root)?;
    }
    let mut walk = item;
    while *parent.slot(walk)? != root {
        let next = *parent.slot(walk)?;
        *parent.slot_mut(walk)? = root;
        walk = next;
    }
    Ok(root)
}

/// Split the triangles into components joined by twin half-edges and measure
/// each one.
pub(super) fn components(mesh: &Mesh, twin: &[Option<usize>]) -> Result<Nesting, MeshBrepError> {
    let count = mesh.tris.len();
    let mut parent: Vec<usize> = (0..count).collect();
    for (he, other) in twin.iter().enumerate() {
        if let Some(other) = other {
            let (a, b) = (find(&mut parent, he / 3)?, find(&mut parent, other / 3)?);
            if a != b {
                *parent.slot_mut(a.max(b))? = a.min(b);
            }
        }
    }
    let mut component_of = mesh.tris.iter().map(|_| NONE).collect::<Vec<_>>();
    let mut index_of_root = component_of.clone();
    let mut list: Vec<Component> = Vec::new();
    for t in 0..count {
        let root = find(&mut parent, t)?;
        let slot = index_of_root.slot_mut(root)?;
        if *slot == NONE {
            *slot = list.len();
            list.push(Component {
                first: t,
                triangles: 0,
                closed: true,
                volume: 0.0,
                area: 0.0,
                bbox: [[f64::INFINITY; 3], [f64::NEG_INFINITY; 3]],
                depth: 0,
                parent: None,
            });
        }
        let index = *slot;
        *component_of.slot_mut(t)? = index;
        let tri = *mesh.tris.slot(t)?;
        let origin = {
            let first = list.slot(index)?.first;
            *mesh
                .pos
                .slot(super::geom::corner(mesh.tris.slot(first)?, 0)?)?
        };
        let component = list.slot_mut(index)?;
        component.triangles += 1;
        component.area += *mesh.area.slot(t)?;
        let [a, b, c] = tri;
        let (pa, pb, pc) = (*mesh.pos.slot(a)?, *mesh.pos.slot(b)?, *mesh.pos.slot(c)?);
        component.volume += dot(sub(pa, origin), cross(sub(pb, origin), sub(pc, origin))) / 6.0;
        for p in [pa, pb, pc] {
            for k in 0..3 {
                let lo = component.bbox[0].slot_mut(k)?;
                *lo = lo.min(*p.slot(k)?);
                let hi = component.bbox[1].slot_mut(k)?;
                *hi = hi.max(*p.slot(k)?);
            }
        }
        for k in 0..3 {
            if twin.slot(3 * t + k)?.is_none() {
                component.closed = false;
            }
        }
    }
    Ok(Nesting {
        component_of,
        components: list,
    })
}

/// Whether the ray `origin + s * direction`, `s > 0`, crosses the triangle.
fn ray_hits(origin: V3, direction: V3, tri: [V3; 3]) -> bool {
    let [p0, p1, p2] = tri;
    let (edge1, edge2) = (sub(p1, p0), sub(p2, p0));
    let pivot = cross(direction, edge2);
    let det = dot(edge1, pivot);
    if det == 0.0 {
        return false;
    }
    let offset = sub(origin, p0);
    let along = dot(offset, pivot) / det;
    if !(0.0..=1.0).contains(&along) {
        return false;
    }
    let side = cross(offset, edge1);
    let across = dot(direction, side) / det;
    if across < 0.0 || along + across > 1.0 {
        return false;
    }
    dot(edge2, side) / det > 0.0
}

impl Nesting {
    /// Whether a point of `inner` lies inside the closed component `outer`.
    fn inside(
        &self,
        mesh: &Mesh,
        probe: V3,
        outer: usize,
        budget: &mut usize,
    ) -> Result<bool, MeshBrepError> {
        let mut votes = 0_usize;
        for direction in RAY_DIRECTIONS {
            let mut crossings = 0_usize;
            for (t, &component) in self.component_of.iter().enumerate() {
                if component != outer {
                    continue;
                }
                let [a, b, c] = *mesh.tris.slot(t)?;
                let tri = [*mesh.pos.slot(a)?, *mesh.pos.slot(b)?, *mesh.pos.slot(c)?];
                if ray_hits(probe, direction, tri) {
                    crossings += 1;
                }
            }
            *budget = budget
                .checked_sub(self.components.slot(outer)?.triangles)
                .unwrap_or(0);
            if !crossings.is_multiple_of(2) {
                votes += 1;
            }
        }
        Ok(votes * 2 > RAY_DIRECTIONS.len())
    }

    /// Fill in depth and parent of every closed component.
    ///
    /// A component is inside another when its bounding box lies within the
    /// other's and a point of its largest triangle has an odd number of ray
    /// crossings with the other. Past `MAX_NESTING_RAY_TESTS` triangle tests the
    /// bounding box and a larger volume decide instead.
    pub(super) fn nest(&mut self, mesh: &Mesh) -> Result<(), MeshBrepError> {
        let solids: Vec<usize> = (0..self.components.len())
            .filter(|&i| self.components.get(i).is_some_and(Component::solid))
            .collect();
        if solids.len() < 2 {
            return Ok(());
        }
        let mut largest = vec![NONE; self.components.len()];
        for (t, &component) in self.component_of.iter().enumerate() {
            let best = largest.slot_mut(component)?;
            if *best == NONE || *mesh.area.slot(t)? > *mesh.area.slot(*best)? {
                *best = t;
            }
        }
        let mut budget = MAX_NESTING_RAY_TESTS;
        let mut found: Vec<(usize, usize, f64)> = Vec::new();
        for &inner in &solids {
            let tri = *mesh.tris.slot(*largest.slot(inner)?)?;
            let mut probe = [0.0; 3];
            for corner in tri {
                let p = *mesh.pos.slot(corner)?;
                for (sum, value) in probe.iter_mut().zip(p) {
                    *sum += value / 3.0;
                }
            }
            let inner_box = self.components.slot(inner)?.bbox;
            for &outer in &solids {
                if outer == inner {
                    continue;
                }
                let candidate = self.components.slot(outer)?;
                let contained = (0..3).all(|k| {
                    inner_box[0]
                        .get(k)
                        .zip(candidate.bbox[0].get(k))
                        .is_some_and(|(i, o)| *i >= *o - NESTING_BBOX_SLACK_MM)
                        && inner_box[1]
                            .get(k)
                            .zip(candidate.bbox[1].get(k))
                            .is_some_and(|(i, o)| *i <= *o + NESTING_BBOX_SLACK_MM)
                });
                if !contained {
                    continue;
                }
                let bigger = candidate.volume.abs() > self.components.slot(inner)?.volume.abs();
                if !bigger {
                    continue;
                }
                let exhausted = candidate
                    .triangles
                    .checked_mul(RAY_DIRECTIONS.len())
                    .is_none_or(|need| budget < need);
                if exhausted || self.inside(mesh, probe, outer, &mut budget)? {
                    found.push((inner, outer, candidate.volume.abs()));
                }
            }
        }
        let mut parent_volume = vec![f64::INFINITY; self.components.len()];
        for (inner, outer, volume) in found {
            let component = self.components.slot_mut(inner)?;
            component.depth += 1;
            let best = parent_volume.slot_mut(inner)?;
            if volume < *best {
                *best = volume;
                component.parent = Some(outer);
            }
        }
        Ok(())
    }

    /// Triangles after dropping degenerate components and reversing components
    /// whose volume sign contradicts their depth; `None` when nothing changes.
    pub(super) fn rewrite(&self, mesh: &Mesh) -> Result<Option<Rewrite>, MeshBrepError> {
        // A mesh made only of slivers keeps them: dropping them leaves no body.
        let drop_slivers = self.components.iter().any(|c| !c.sliver());
        let dropped =
            |c: &Component| c.degenerate() || c.thin_void() || (drop_slivers && c.sliver());
        let changes = self.components.iter().any(|c| dropped(c) || c.wrong_sign());
        if !changes {
            return Ok(None);
        }
        let mut rewrite = Rewrite {
            tris: Vec::with_capacity(mesh.tris.len()),
            old_of_new: Vec::with_capacity(mesh.tris.len()),
            dropped_triangles: 0,
            dropped_zero_components: 0,
            dropped_thin_components: 0,
            dropped_sliver_components: 0,
            reversed_components: 0,
        };
        for c in &self.components {
            if c.wrong_sign() {
                rewrite.reversed_components += 1;
            }
            if c.degenerate() {
                rewrite.dropped_zero_components += 1;
            } else if c.thin_void() {
                rewrite.dropped_thin_components += 1;
            } else if dropped(c) {
                rewrite.dropped_sliver_components += 1;
            }
        }
        for (t, &component) in self.component_of.iter().enumerate() {
            let c = self.components.slot(component)?;
            let [a, b, d] = *mesh.tris.slot(t)?;
            if dropped(c) {
                rewrite.dropped_triangles += 1;
                continue;
            }
            rewrite
                .tris
                .push(if c.wrong_sign() { [a, d, b] } else { [a, b, d] });
            rewrite.old_of_new.push(t);
        }
        Ok(Some(rewrite))
    }
}

/// For every component of `current`, the component of `current` that holds it
/// as a void; `None` for an outer component. `old` is the nesting of the mesh
/// before `rewrite`, and `old_of_new` maps the triangles of `current` to it.
pub(super) fn void_parents(
    old: &Nesting,
    current: &Nesting,
    old_of_new: Option<&[usize]>,
) -> Result<Vec<Option<usize>>, MeshBrepError> {
    let mut new_of_old = vec![NONE; old.component_of.len()];
    if let Some(map) = old_of_new {
        for (new, &previous) in map.iter().enumerate() {
            *new_of_old.slot_mut(previous)? = new;
        }
    } else {
        for (t, slot) in new_of_old.iter_mut().enumerate() {
            *slot = t;
        }
    }
    let mut result = Vec::with_capacity(current.components.len());
    for component in &current.components {
        let previous = match old_of_new {
            Some(map) => *map.slot(component.first)?,
            None => component.first,
        };
        let before = old.components.slot(*old.component_of.slot(previous)?)?;
        let parent = match before.parent {
            Some(p) if !before.depth.is_multiple_of(2) => {
                let moved = *new_of_old.slot(old.components.slot(p)?.first)?;
                (moved != NONE)
                    .then(|| current.component_of.get(moved).copied())
                    .flatten()
            }
            _ => None,
        };
        result.push(parent);
    }
    Ok(result)
}
