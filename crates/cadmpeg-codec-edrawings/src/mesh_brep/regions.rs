// SPDX-License-Identifier: Apache-2.0
//! Coplanar region growing.

use super::geom::{dot, sub, Slot, NONE, V3};
use super::mesh::Mesh;
use super::{MeshBrepError, PLANE_DISTANCE_TOLERANCE_MM};

/// One planar region of triangles.
pub(super) struct Region {
    /// A point on the region plane.
    pub(super) origin: V3,
    /// Unit normal, equal to the normal of the reference triangle.
    pub(super) normal: V3,
}

/// Region table and triangle membership.
pub(super) struct Regions {
    pub(super) list: Vec<Region>,
    /// Region index per triangle.
    pub(super) region_of: Vec<usize>,
}

struct Grower<'a> {
    mesh: &'a Mesh,
    twin: &'a [Option<usize>],
    mark: Vec<u64>,
    stamp: u64,
}

impl Grower<'_> {
    /// Whether triangle `t` lies on the plane `(origin, normal)`: every vertex
    /// within the distance tolerance and the normal within its angular bound.
    fn fits(&self, t: usize, origin: V3, normal: V3) -> Result<bool, MeshBrepError> {
        if dot(normal, *self.mesh.normal.slot(t)?) < *self.mesh.min_cos.slot(t)? {
            return Ok(false);
        }
        for &vertex in self.mesh.tris.slot(t)? {
            let offset = dot(normal, sub(*self.mesh.pos.slot(vertex)?, origin));
            if offset.abs() > PLANE_DISTANCE_TOLERANCE_MM {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Collect the unassigned triangles reachable from `start` across paired
    /// edges that all fit the plane. `start` is accepted without a test.
    fn grow(
        &mut self,
        region_of: &[usize],
        start: usize,
        origin: V3,
        normal: V3,
    ) -> Result<Vec<usize>, MeshBrepError> {
        self.stamp += 1;
        let stamp = self.stamp;
        *self.mark.slot_mut(start)? = stamp;
        let mut stack = vec![start];
        let mut members = Vec::new();
        while let Some(t) = stack.pop() {
            members.push(t);
            for k in 0..3 {
                let Some(neighbour) = *self.twin.slot(3 * t + k)? else {
                    continue;
                };
                let u = neighbour / 3;
                if *region_of.slot(u)? != NONE || *self.mark.slot(u)? == stamp {
                    continue;
                }
                *self.mark.slot_mut(u)? = stamp;
                if self.fits(u, origin, normal)? {
                    stack.push(u);
                }
            }
        }
        members.sort_unstable();
        Ok(members)
    }
}

impl Regions {
    /// Replace each listed region by one region per triangle. The first
    /// triangle keeps the region's index.
    pub(super) fn split_into_triangles(
        &mut self,
        mesh: &Mesh,
        split: &[usize],
    ) -> Result<(), MeshBrepError> {
        for &region in split {
            let members: Vec<usize> = self
                .region_of
                .iter()
                .enumerate()
                .filter(|&(_, &r)| r == region)
                .map(|(t, _)| t)
                .collect();
            for (position, &t) in members.iter().enumerate() {
                let plane = Region {
                    origin: centroid(mesh, t)?,
                    normal: *mesh.normal.slot(t)?,
                };
                if position == 0 {
                    *self.list.slot_mut(region)? = plane;
                } else {
                    *self.region_of.slot_mut(t)? = self.list.len();
                    self.list.push(plane);
                }
            }
        }
        Ok(())
    }
}

fn centroid(mesh: &Mesh, t: usize) -> Result<V3, MeshBrepError> {
    let mut sum = [0.0; 3];
    for &vertex in mesh.tris.slot(t)? {
        let p = *mesh.pos.slot(vertex)?;
        sum = [sum[0] + p[0], sum[1] + p[1], sum[2] + p[2]];
    }
    Ok([sum[0] / 3.0, sum[1] / 3.0, sum[2] / 3.0])
}

/// Partition the triangles into planar regions.
///
/// Seeds are taken in triangle order. A first growth uses the seed's plane to
/// find the region's largest triangle; when that is not the seed, the region
/// is grown again from the largest triangle's plane, which is better
/// conditioned than a sliver's. Every triangle is tested against the region
/// plane itself, never against its neighbour, so a finely tessellated
/// cylinder cannot drift into one region.
pub(super) fn build(mesh: &Mesh, twin: &[Option<usize>]) -> Result<Regions, MeshBrepError> {
    let count = mesh.tris.len();
    let mut grower = Grower {
        mesh,
        twin,
        mark: (0..count).map(|_| 0).collect::<Vec<_>>(),
        stamp: 0,
    };
    let mut regions = Regions {
        list: Vec::new(),
        region_of: (0..count).map(|_| NONE).collect::<Vec<_>>(),
    };
    for seed in 0..count {
        // The seed stays unassigned when it misses the plane of the largest
        // triangle of its first growth; the next round then excludes the
        // triangles taken, so each round makes progress.
        while *regions.region_of.slot(seed)? == NONE {
            grow_region(&mut grower, &mut regions, seed)?;
        }
    }
    Ok(regions)
}

fn grow_region(
    grower: &mut Grower<'_>,
    regions: &mut Regions,
    seed: usize,
) -> Result<(), MeshBrepError> {
    let mesh = grower.mesh;
    let seed_normal = *mesh.normal.slot(seed)?;
    let first = grower.grow(&regions.region_of, seed, centroid(mesh, seed)?, seed_normal)?;
    let mut largest = seed;
    for &t in &first {
        if *mesh.area.slot(t)? > *mesh.area.slot(largest)? {
            largest = t;
        }
    }
    let (members, reference) = if largest == seed {
        (first, seed)
    } else {
        let origin = centroid(mesh, largest)?;
        let normal = *mesh.normal.slot(largest)?;
        (
            grower.grow(&regions.region_of, largest, origin, normal)?,
            largest,
        )
    };
    let index = regions.list.len();
    for &t in &members {
        *regions.region_of.slot_mut(t)? = index;
    }
    regions.list.push(Region {
        origin: centroid(mesh, reference)?,
        normal: *mesh.normal.slot(reference)?,
    });
    Ok(())
}
