// SPDX-License-Identifier: Apache-2.0
//! Refinement of a parameter-plane triangulation until it lies within the
//! chordal and normal-angle limits of the surface.
//!
//! A triangle that misses a limit splits its worst interior edge, together
//! with the triangle across it, or gains a vertex at its centroid when that
//! is worse or every edge is a face boundary. Boundary edges never split, so
//! the faces on both sides of an edge keep the same boundary points.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::compare::floats_agree;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::count::{exceeds_limit, floor_count};
use crate::face::Refusal;
use crate::plane::{angle, centroid3, cross2, distance, mid3, midpoint, Uv};
use crate::surface::Carrier;

/// Ratio of a triangle's parameter area to the domain area below which it
/// is not refined.
const SMALLEST_AREA_RATIO: f64 = 1e-14;
/// Passes of the edge-flip sweep.
const FLIP_PASSES: usize = 32;
/// Angle, in radians, by which two opposite angles must pass a half turn
/// before their edge flips.
const FLIP_SLACK: f64 = 1e-9;
/// Flips one inserted vertex may cause.
const LEGALIZE_STEPS: usize = 4096;
/// Smallest ratio of one metric scale to the other.
const METRIC_FLOOR: f64 = 1e-6;

/// A triangulated domain.
pub(crate) struct Mesh {
    pub(crate) uv: Vec<Uv>,
    pub(crate) points: Vec<Point3>,
    /// Surface normals, unnormalized, where the surface has one.
    pub(crate) normals: Vec<Option<Vector3>>,
    /// The lengths of the two partial derivatives at each vertex: the
    /// metric in which flips judge angles.
    pub(crate) scales: Vec<[f64; 2]>,
    pub(crate) triangles: Vec<Option<[usize; 3]>>,
    /// Face boundary segments, by ordered vertex pair.
    pub(crate) fixed: BTreeSet<(usize, usize)>,
}

fn key(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

/// Which of two interior edges is longer. Last-place length noise does not
/// change the choice: the lower vertex-id pair wins when the lengths agree.
fn longer_edge(
    points: &[Point3],
    first: (usize, usize),
    second: (usize, usize),
) -> std::cmp::Ordering {
    let length = |edge: (usize, usize)| distance(points[edge.0], points[edge.1]);
    let (left, right) = (length(first), length(second));
    if floats_agree(left, right) {
        key(first.0, first.1).cmp(&key(second.0, second.1))
    } else {
        left.total_cmp(&right)
    }
}

/// The lengths of the partial derivatives at `uv`, each kept above a share
/// of the other so that a pole does not collapse the metric.
pub(crate) fn scales(carrier: &Carrier<'_, '_>, uv: Uv) -> Result<[f64; 2], Refusal> {
    let Some(partials) = carrier.partials(uv)? else {
        return Ok([1.0, 1.0]);
    };
    let (u, v) = (
        crate::plane::length(partials.du),
        crate::plane::length(partials.dv),
    );
    let floor = u.max(v) * METRIC_FLOOR;
    if floor.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
        return Ok([1.0, 1.0]);
    }
    Ok([u.max(floor), v.max(floor)])
}

/// The limits of one refinement.
pub(crate) struct Limits {
    pub(crate) chord: f64,
    pub(crate) angle: f64,
    pub(crate) max_vertices: usize,
}

impl Mesh {
    fn add_vertex(&mut self, carrier: &Carrier<'_, '_>, uv: Uv) -> Result<Option<usize>, Refusal> {
        let Some(point) = carrier.point(uv)? else {
            return Ok(None);
        };
        self.uv.push(uv);
        self.points.push(point);
        self.normals.push(carrier.normal(uv)?);
        self.scales.push(scales(carrier, uv)?);
        Ok(Some(self.uv.len() - 1))
    }

    fn edges(&self) -> BTreeMap<(usize, usize), Vec<usize>> {
        let mut map: BTreeMap<(usize, usize), Vec<usize>> = BTreeMap::new();
        for (at, triangle) in self.triangles.iter().enumerate() {
            if let Some([a, b, c]) = triangle {
                for (from, to) in [(*a, *b), (*b, *c), (*c, *a)] {
                    map.entry(key(from, to)).or_default().push(at);
                }
            }
        }
        map
    }

    /// Refines every triangle to `limits`.
    pub(crate) fn refine(
        &mut self,
        carrier: &Carrier<'_, '_>,
        limits: &Limits,
    ) -> Result<(), Refusal> {
        let total: f64 = self
            .triangles
            .iter()
            .flatten()
            .map(|[a, b, c]| cross2(self.uv[*a], self.uv[*b], self.uv[*c]).abs())
            .sum();
        let smallest = total * SMALLEST_AREA_RATIO;
        let mut edges = self.edges();
        let mut queue: Vec<usize> = (0..self.triangles.len()).collect();
        while let Some(at) = queue.pop() {
            if self.uv.len() >= limits.max_vertices {
                break;
            }
            let Some([a, b, c]) = self.triangles[at] else {
                continue;
            };
            if cross2(self.uv[a], self.uv[b], self.uv[c]).abs() <= smallest {
                continue;
            }
            let mut worst_edge: Option<f64> = None;
            for (from, to) in [(a, b), (b, c), (c, a)] {
                let uv = midpoint(self.uv[from], self.uv[to]);
                let Some(on) = carrier.point(uv)? else {
                    continue;
                };
                let error = distance(on, mid3(self.points[from], self.points[to]));
                if self.fixed.contains(&key(from, to)) {
                    continue;
                }
                if worst_edge.is_none_or(|worst| error > worst) {
                    worst_edge = Some(error);
                }
            }
            let centre = [
                (self.uv[a][0] + self.uv[b][0] + self.uv[c][0]) / 3.0,
                (self.uv[a][1] + self.uv[b][1] + self.uv[c][1]) / 3.0,
            ];
            let centre_error = carrier.point(centre)?.map_or(0.0, |on| {
                distance(
                    on,
                    centroid3(self.points[a], self.points[b], self.points[c]),
                )
            });
            // Boundary sampling already bounds the turn along a fixed edge.
            let turned = [(a, b), (b, c), (c, a)].into_iter().any(|(from, to)| {
                if self.fixed.contains(&key(from, to)) {
                    return false;
                }
                match (self.normals[from], self.normals[to]) {
                    (Some(first), Some(second)) => {
                        exceeds_limit(angle(first, second), limits.angle)
                    }
                    _ => false,
                }
            });
            let edge_error = worst_edge.unwrap_or(0.0);
            if !exceeds_limit(edge_error, limits.chord)
                && !exceeds_limit(centre_error, limits.chord)
                && !turned
            {
                continue;
            }
            // The longest interior edge splits, which keeps triangles from
            // thinning; a triangle bounded only by fixed edges gains a
            // vertex at its centroid.
            let longest = [(a, b), (b, c), (c, a)]
                .into_iter()
                .filter(|(from, to)| !self.fixed.contains(&key(*from, *to)))
                .max_by(|first, second| longer_edge(&self.points, *first, *second));
            match longest {
                Some(edge) => {
                    let uv = midpoint(self.uv[edge.0], self.uv[edge.1]);
                    self.split_edge(carrier, &mut edges, &mut queue, edge, uv)?;
                }
                None => self.split_centre(carrier, &mut edges, &mut queue, at, centre)?,
            }
        }
        Ok(())
    }

    /// Flips interior edges toward a Delaunay triangulation of the model
    /// points.
    pub(crate) fn flip(&mut self) {
        let mut edges = self.edges();
        let mut ignored = Vec::new();
        for _ in 0..FLIP_PASSES {
            let mut flipped = false;
            let candidates: Vec<(usize, usize)> = edges.keys().copied().collect();
            for edge in candidates {
                flipped |= self.try_flip(&mut edges, &mut ignored, edge).is_some();
            }
            if !flipped {
                break;
            }
        }
        self.triangles.retain(Option::is_some);
    }

    /// Flips `edge` when the two angles opposite it sum past a half turn and
    /// both new triangles keep their parameter-plane orientation. Returns the
    /// two vertices opposite the old edge, which the new edge joins.
    fn try_flip(
        &mut self,
        edges: &mut BTreeMap<(usize, usize), Vec<usize>>,
        queue: &mut Vec<usize>,
        edge: (usize, usize),
    ) -> Option<(usize, usize)> {
        if self.fixed.contains(&edge) {
            return None;
        }
        let &[first, second] = edges.get(&edge).map(Vec::as_slice)? else {
            return None;
        };
        let (t1, t2) = (self.triangles[first]?, self.triangles[second]?);
        let opposite = |triangle: [usize; 3]| {
            (0..3)
                .find(|k| key(triangle[*k], triangle[(k + 1) % 3]) == edge)
                .map(|k| (triangle[k], triangle[(k + 1) % 3], triangle[(k + 2) % 3]))
        };
        let ((a, b, c), (_, _, d)) = (opposite(t1)?, opposite(t2)?);
        // Angles in the parameter plane stretched by the surface metric of
        // the four corners.
        let mut metric = [0.0; 2];
        for vertex in [a, b, c, d] {
            for (axis, value) in metric.iter_mut().enumerate() {
                *value += self.scales[vertex][axis] / 4.0;
            }
        }
        let corner = |at: usize, from: usize, to: usize| {
            let lift = |vertex: usize| {
                cadmpeg_ir::math::Point3::new(
                    self.uv[vertex][0] * metric[0],
                    self.uv[vertex][1] * metric[1],
                    0.0,
                )
            };
            angle(
                crate::plane::sub(lift(from), lift(at)),
                crate::plane::sub(lift(to), lift(at)),
            )
        };
        if corner(c, a, b) + corner(d, b, a) <= std::f64::consts::PI + FLIP_SLACK {
            return None;
        }
        let new = [[c, a, d], [c, d, b]];
        if new
            .iter()
            .any(|[p, q, r]| cross2(self.uv[*p], self.uv[*q], self.uv[*r]) <= 0.0)
        {
            return None;
        }
        self.replace(edges, queue, first, &new[..1]);
        self.replace(edges, queue, second, &new[1..]);
        Some((c, d))
    }

    /// Restores the Delaunay property around a new vertex `vertex`, whose
    /// triangles have the edges `opposite` across from it.
    fn legalize(
        &mut self,
        edges: &mut BTreeMap<(usize, usize), Vec<usize>>,
        queue: &mut Vec<usize>,
        vertex: usize,
        opposite: Vec<(usize, usize)>,
    ) {
        let mut stack = opposite;
        let mut steps = 0_usize;
        while let Some((p, q)) = stack.pop() {
            steps += 1;
            if steps > LEGALIZE_STEPS {
                break;
            }
            if let Some((c, d)) = self.try_flip(edges, queue, key(p, q)) {
                let other = if c == vertex { d } else { c };
                stack.push((p, other));
                stack.push((other, q));
            }
        }
    }

    fn replace(
        &mut self,
        edges: &mut BTreeMap<(usize, usize), Vec<usize>>,
        queue: &mut Vec<usize>,
        at: usize,
        triangles: &[[usize; 3]],
    ) {
        if let Some([a, b, c]) = self.triangles[at].take() {
            for (from, to) in [(a, b), (b, c), (c, a)] {
                if let Some(list) = edges.get_mut(&key(from, to)) {
                    list.retain(|other| *other != at);
                }
            }
        }
        for triangle in triangles {
            let index = self.triangles.len();
            self.triangles.push(Some(*triangle));
            let [a, b, c] = *triangle;
            for (from, to) in [(a, b), (b, c), (c, a)] {
                edges.entry(key(from, to)).or_default().push(index);
            }
            queue.push(index);
        }
    }

    fn split_edge(
        &mut self,
        carrier: &Carrier<'_, '_>,
        edges: &mut BTreeMap<(usize, usize), Vec<usize>>,
        queue: &mut Vec<usize>,
        (from, to): (usize, usize),
        uv: Uv,
    ) -> Result<(), Refusal> {
        let Some(middle) = self.add_vertex(carrier, uv)? else {
            return Ok(());
        };
        let sharing = edges.get(&key(from, to)).cloned().unwrap_or_default();
        let mut opposite = Vec::new();
        for at in sharing {
            let Some(corners) = self.triangles[at] else {
                continue;
            };
            // Rotate the corners so the split edge is the first one.
            let Some(start) =
                (0..3).find(|k| key(corners[*k], corners[(k + 1) % 3]) == key(from, to))
            else {
                continue;
            };
            let (first, second, apex) = (
                corners[start],
                corners[(start + 1) % 3],
                corners[(start + 2) % 3],
            );
            self.replace(
                edges,
                queue,
                at,
                &[[first, middle, apex], [middle, second, apex]],
            );
            opposite.push((first, apex));
            opposite.push((second, apex));
        }
        self.legalize(edges, queue, middle, opposite);
        Ok(())
    }

    /// Inserts interior vertices on a grid whose spacing along each
    /// parameter is what the chord and angle limits allow for the surface's
    /// curvature in that direction, and never less than the median boundary
    /// segment, so that refinement starts from well-shaped triangles.
    pub(crate) fn seed(
        &mut self,
        carrier: &Carrier<'_, '_>,
        limits: &Limits,
    ) -> Result<(), Refusal> {
        let mut lengths: Vec<f64> = self
            .fixed
            .iter()
            .map(|(from, to)| distance(self.points[*from], self.points[*to]))
            .filter(|length| *length > 0.0)
            .collect();
        if lengths.is_empty() || self.uv.is_empty() {
            return Ok(());
        }
        lengths.sort_by(f64::total_cmp);
        let spacing = lengths[lengths.len() / 2];
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for uv in &self.uv {
            for axis in 0..2 {
                low[axis] = low[axis].min(uv[axis]);
                high[axis] = high[axis].max(uv[axis]);
            }
        }
        let centre = [
            f64::midpoint(low[0], high[0]),
            f64::midpoint(low[1], high[1]),
        ];
        let metric = scales(carrier, centre)?;
        let mut step = [0.0; 2];
        for axis in 0..2 {
            let extent = (high[axis] - low[axis]) * metric[axis];
            let curvature =
                carrier.curvature(centre, axis, high[axis] - low[axis], metric[axis])?;
            let allowed = if curvature > 0.0 {
                (8.0 * limits.chord / curvature)
                    .sqrt()
                    .min(limits.angle / curvature)
            } else {
                extent
            };
            step[axis] = allowed.max(spacing) / metric[axis];
        }
        let counts = [0, 1].map(|axis| floor_count((high[axis] - low[axis]) / step[axis]));
        let [Some(columns), Some(rows)] = counts else {
            return Ok(());
        };
        if columns
            .checked_mul(rows)
            .is_none_or(|count| count > limits.max_vertices / 2)
        {
            return Ok(());
        }
        let mut edges = self.edges();
        let mut queue = Vec::new();
        for column in 1..=columns {
            for row in 1..=rows {
                let (Some(column), Some(row)) = (
                    cadmpeg_core::convert::f64_from_index(column),
                    cadmpeg_core::convert::f64_from_index(row),
                ) else {
                    continue;
                };
                let uv = [
                    low[0] + step[0] * (column - 0.5),
                    low[1] + step[1] * (row - 0.5),
                ];
                let Some(at) = self.triangles.iter().position(|triangle| {
                    triangle.is_some_and(|[a, b, c]| {
                        cross2(self.uv[a], self.uv[b], uv) > 0.0
                            && cross2(self.uv[b], self.uv[c], uv) > 0.0
                            && cross2(self.uv[c], self.uv[a], uv) > 0.0
                    })
                }) else {
                    continue;
                };
                let Some([a, b, c]) = self.triangles[at] else {
                    continue;
                };
                // Keep half a step from the triangle's corners.
                let near = [a, b, c].into_iter().any(|corner| {
                    let du = (self.uv[corner][0] - uv[0]) / step[0];
                    let dv = (self.uv[corner][1] - uv[1]) / step[1];
                    du.hypot(dv) < 0.5
                });
                if !near {
                    self.split_centre(carrier, &mut edges, &mut queue, at, uv)?;
                }
            }
        }
        Ok(())
    }

    fn split_centre(
        &mut self,
        carrier: &Carrier<'_, '_>,
        edges: &mut BTreeMap<(usize, usize), Vec<usize>>,
        queue: &mut Vec<usize>,
        at: usize,
        uv: Uv,
    ) -> Result<(), Refusal> {
        let Some([a, b, c]) = self.triangles[at] else {
            return Ok(());
        };
        let Some(centre) = self.add_vertex(carrier, uv)? else {
            return Ok(());
        };
        self.replace(
            edges,
            queue,
            at,
            &[[a, b, centre], [b, c, centre], [c, a, centre]],
        );
        self.legalize(edges, queue, centre, vec![(a, b), (b, c), (c, a)]);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
