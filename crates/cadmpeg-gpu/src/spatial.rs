// SPDX-License-Identifier: Apache-2.0
//! Spatial acceleration structures and tree-based geometric query algorithms.
//!
//! Provides a balanced 3D $k$-d tree ([`KdTree3`]) for point-cloud nearest-neighbour
//! search and Hausdorff distance evaluation in $\mathcal{O}(N \log M)$ time, and a
//! bounding volume hierarchy ([`MeshBvh3`]) for triangle-mesh closest-point projection,
//! ray-mesh intersection, and solid containment queries.

use cadmpeg_ir::math::Point3;

use crate::cpu::{
    closest_point_on_triangle, intersect_ray_triangle, PARITY_HIT_DEDUP_TOLERANCE,
    PARITY_RAY_DIRECTION,
};
use crate::{ClosestPointResult, RayHit3, RayQuery3, TriangleMeshView};

/// Minimum ray direction component magnitude before treating ray as parallel to an AABB slab.
pub const SLAB_RAY_DIR_EPSILON: f64 = 1.0e-12;

/// Axis-aligned 3D bounding box defined by minimum and maximum extents.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Aabb3 {
    /// Minimum corner $(x_{\min}, y_{\min}, z_{\min})$.
    pub min: Point3,
    /// Maximum corner $(x_{\max}, y_{\max}, z_{\max})$.
    pub max: Point3,
}

impl Aabb3 {
    /// Creates an empty bounding box with inverted infinite bounds.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            min: Point3 {
                x: f64::INFINITY,
                y: f64::INFINITY,
                z: f64::INFINITY,
            },
            max: Point3 {
                x: f64::NEG_INFINITY,
                y: f64::NEG_INFINITY,
                z: f64::NEG_INFINITY,
            },
        }
    }

    /// Computes the bounding box enclosing three triangle vertices.
    #[must_use]
    pub fn from_triangle(v0: Point3, v1: Point3, v2: Point3) -> Self {
        Self {
            min: Point3::new(
                v0.x.min(v1.x).min(v2.x),
                v0.y.min(v1.y).min(v2.y),
                v0.z.min(v1.z).min(v2.z),
            ),
            max: Point3::new(
                v0.x.max(v1.x).max(v2.x),
                v0.y.max(v1.y).max(v2.y),
                v0.z.max(v1.z).max(v2.z),
            ),
        }
    }

    /// Merges two bounding boxes into their smallest common enclosing box.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            min: Point3::new(
                self.min.x.min(other.min.x),
                self.min.y.min(other.min.y),
                self.min.z.min(other.min.z),
            ),
            max: Point3::new(
                self.max.x.max(other.max.x),
                self.max.y.max(other.max.y),
                self.max.z.max(other.max.z),
            ),
        }
    }

    /// Computes the minimum squared Euclidean distance from `query` to this bounding box.
    #[must_use]
    pub fn distance_squared_to_point(self, query: Point3) -> f64 {
        let dx = if query.x < self.min.x {
            self.min.x - query.x
        } else if query.x > self.max.x {
            query.x - self.max.x
        } else {
            0.0
        };
        let dy = if query.y < self.min.y {
            self.min.y - query.y
        } else if query.y > self.max.y {
            query.y - self.max.y
        } else {
            0.0
        };
        let dz = if query.z < self.min.z {
            self.min.z - query.z
        } else if query.z > self.max.z {
            query.z - self.max.z
        } else {
            0.0
        };
        dx.mul_add(dx, dy.mul_add(dy, dz * dz))
    }

    /// Tests whether a ray intersects this bounding box within $[t_{\min}, t_{\max}]$.
    #[must_use]
    pub fn intersects_ray(self, ray: &RayQuery3) -> bool {
        let mut t_near = ray.t_min;
        let mut t_far = ray.t_max;

        if ray.direction.x.abs() < SLAB_RAY_DIR_EPSILON {
            if ray.origin.x < self.min.x || ray.origin.x > self.max.x {
                return false;
            }
        } else {
            let inv_dir = 1.0 / ray.direction.x;
            let mut t1 = (self.min.x - ray.origin.x) * inv_dir;
            let mut t2 = (self.max.x - ray.origin.x) * inv_dir;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_near = t_near.max(t1);
            t_far = t_far.min(t2);
            if t_near > t_far {
                return false;
            }
        }

        if ray.direction.y.abs() < SLAB_RAY_DIR_EPSILON {
            if ray.origin.y < self.min.y || ray.origin.y > self.max.y {
                return false;
            }
        } else {
            let inv_dir = 1.0 / ray.direction.y;
            let mut t1 = (self.min.y - ray.origin.y) * inv_dir;
            let mut t2 = (self.max.y - ray.origin.y) * inv_dir;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_near = t_near.max(t1);
            t_far = t_far.min(t2);
            if t_near > t_far {
                return false;
            }
        }

        if ray.direction.z.abs() < SLAB_RAY_DIR_EPSILON {
            if ray.origin.z < self.min.z || ray.origin.z > self.max.z {
                return false;
            }
        } else {
            let inv_dir = 1.0 / ray.direction.z;
            let mut t1 = (self.min.z - ray.origin.z) * inv_dir;
            let mut t2 = (self.max.z - ray.origin.z) * inv_dir;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            t_near = t_near.max(t1);
            t_far = t_far.min(t2);
            if t_near > t_far {
                return false;
            }
        }

        true
    }
}

/// Node in a flat array-backed 3D $k$-d tree.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct KdNode3 {
    /// 3D coordinate stored at this node.
    pub point: Point3,
    /// Original index in the source slice passed to [`KdTree3::build`].
    pub original_index: usize,
    /// Index of left child in node array, if present.
    pub left: Option<usize>,
    /// Index of right child in node array, if present.
    pub right: Option<usize>,
    /// Splitting axis: 0 for X, 1 for Y, 2 for Z.
    pub axis: u8,
}

/// Balanced 3D $k$-d tree over point-cloud coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct KdTree3 {
    nodes: Vec<KdNode3>,
    root: Option<usize>,
}

impl KdTree3 {
    /// Constructs a balanced 3D $k$-d tree from `points` using median partitioning.
    #[must_use]
    pub fn build(points: &[Point3]) -> Self {
        if points.is_empty() {
            return Self {
                nodes: Vec::new(),
                root: None,
            };
        }

        let mut indexed: Vec<(Point3, usize)> = points
            .iter()
            .copied()
            .enumerate()
            .map(|(idx, pt)| (pt, idx))
            .collect();

        let mut nodes = Vec::with_capacity(points.len());
        let root = Self::build_subtree(&mut nodes, &mut indexed, 0);

        Self { nodes, root }
    }

    fn build_subtree(
        nodes: &mut Vec<KdNode3>,
        items: &mut [(Point3, usize)],
        depth: usize,
    ) -> Option<usize> {
        if items.is_empty() {
            return None;
        }

        let axis = u8::try_from(depth % 3).unwrap_or(0);
        let mid = items.len() / 2;

        items.select_nth_unstable_by(mid, |(a, _), (b, _)| match axis {
            0 => a.x.total_cmp(&b.x),
            1 => a.y.total_cmp(&b.y),
            _ => a.z.total_cmp(&b.z),
        });

        let (point, original_index) = items[mid];
        let node_idx = nodes.len();
        nodes.push(KdNode3 {
            point,
            original_index,
            left: None,
            right: None,
            axis,
        });

        let (left_items, rest) = items.split_at_mut(mid);
        let right_items = &mut rest[1..];

        let next_depth = depth.checked_add(1).unwrap_or(depth);
        let left = Self::build_subtree(nodes, left_items, next_depth);
        let right = Self::build_subtree(nodes, right_items, next_depth);

        if let Some(node) = nodes.get_mut(node_idx) {
            node.left = left;
            node.right = right;
        }

        Some(node_idx)
    }

    /// Returns the number of points stored in this tree.
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether this tree contains zero points.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Finds the nearest point in the tree to `query`, returning `(point, original_index, distance)`.
    #[must_use]
    pub fn nearest(&self, query: Point3) -> Option<(Point3, usize, f64)> {
        let (idx, dist_sq) = self.nearest_sq(query)?;
        let pt = self.nodes.iter().find(|n| n.original_index == idx)?.point;
        Some((pt, idx, dist_sq.sqrt()))
    }

    /// Finds the nearest point in the tree to `query`, returning `(original_index, distance_squared)`.
    #[must_use]
    pub fn nearest_sq(&self, query: Point3) -> Option<(usize, f64)> {
        self.nearest_sq_above_cutoff(query, -1.0)
    }

    /// Finds the nearest point in the tree to `query`, terminating early if any point is found with
    /// squared distance $\le \text{cutoff\_sq}$.
    #[must_use]
    pub fn nearest_sq_above_cutoff(&self, query: Point3, cutoff_sq: f64) -> Option<(usize, f64)> {
        let root = self.root?;
        let mut best_dist_sq = f64::INFINITY;
        let mut best_idx = 0_usize;

        let mut stack = [(0_usize, 0.0_f64); 64];
        stack[0] = (root, 0.0_f64);
        let mut sp = 1_usize;

        while sp > 0 {
            sp -= 1;
            let (node_idx, lower_bound_sq) = stack[sp];
            if lower_bound_sq >= best_dist_sq {
                continue;
            }

            let Some(node) = self.nodes.get(node_idx) else {
                continue;
            };

            let dx = query.x - node.point.x;
            let dy = query.y - node.point.y;
            let dz = query.z - node.point.z;
            let d_sq = dx.mul_add(dx, dy.mul_add(dy, dz * dz));
            if d_sq < best_dist_sq {
                best_dist_sq = d_sq;
                best_idx = node.original_index;
                if best_dist_sq <= cutoff_sq {
                    return Some((best_idx, best_dist_sq));
                }
            }

            let axis_diff = match node.axis {
                0 => dx,
                1 => dy,
                _ => dz,
            };
            let axis_diff_sq = axis_diff * axis_diff;

            let (near_child, far_child) = if axis_diff <= 0.0 {
                (node.left, node.right)
            } else {
                (node.right, node.left)
            };

            if axis_diff_sq < best_dist_sq {
                if let Some(far) = far_child {
                    if sp < 64 {
                        stack[sp] = (far, axis_diff_sq);
                        sp += 1;
                    }
                }
            }
            if let Some(near) = near_child {
                if sp < 64 {
                    stack[sp] = (near, 0.0_f64);
                    sp += 1;
                }
            }
        }

        Some((best_idx, best_dist_sq))
    }

    /// Linearizes the $k$-d tree into a contiguous pre-order array suitable for GPU storage buffers.
    #[must_use]
    pub fn flatten(&self) -> FlatKdTree3 {
        let mut flat = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let right_child = match node.right {
                Some(r) => u32::try_from(r).ok().unwrap_or(0),
                None => u32::MAX,
            };
            flat.push(FlatKdNode {
                point: node.point,
                split_axis: u32::from(node.axis),
                right_child,
            });
        }
        FlatKdTree3 { nodes: flat }
    }
}

/// Flattened 3D $k$-d tree node suitable for contiguous GPU storage buffers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatKdNode {
    /// 3D point coordinate stored at this node.
    pub point: Point3,
    /// Splitting axis: `0` for X, `1` for Y, `2` for Z.
    pub split_axis: u32,
    /// Index of the right child in [`FlatKdTree3::nodes`], or `u32::MAX` when absent.
    pub right_child: u32,
}

/// Contiguous pre-order 3D $k$-d tree representation for GPU compute traversal.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatKdTree3 {
    /// Linearized pre-order node array.
    pub nodes: Vec<FlatKdNode>,
}

impl FlatKdTree3 {
    /// Returns the linearized pre-order node slice.
    #[must_use]
    pub fn nodes(&self) -> &[FlatKdNode] {
        &self.nodes
    }

    /// Finds the minimum squared Euclidean distance from `query` to any point in the flattened tree.
    #[must_use]
    pub fn nearest_sq(&self, query: Point3) -> Option<f64> {
        self.nearest_sq_with_bound(query, f64::INFINITY)
    }

    /// Finds the minimum squared Euclidean distance from `query` using an initial upper bound `initial_bound_sq`.
    #[must_use]
    pub fn nearest_sq_with_bound(&self, query: Point3, initial_bound_sq: f64) -> Option<f64> {
        if self.nodes.is_empty() {
            return None;
        }
        let n_u32 = u32::try_from(self.nodes.len()).ok()?;
        let mut best_dist_sq = initial_bound_sq;
        let mut stack = [(0_u32, 0_u32, 0.0_f64); 64];
        stack[0] = (0, n_u32, 0.0_f64);
        let mut sp = 1_usize;

        while sp > 0 {
            sp -= 1;
            let (node_idx, subtree_end, lower_bound_sq) = stack[sp];
            if lower_bound_sq >= best_dist_sq {
                continue;
            }
            let Ok(idx_usize) = usize::try_from(node_idx) else {
                continue;
            };
            let Some(node) = self.nodes.get(idx_usize) else {
                continue;
            };

            let dx = query.x - node.point.x;
            let dy = query.y - node.point.y;
            let dz = query.z - node.point.z;
            let d_sq = dx.mul_add(dx, dy.mul_add(dy, dz * dz));
            if d_sq < best_dist_sq {
                best_dist_sq = d_sq;
            }

            let axis_diff = match node.split_axis {
                0 => dx,
                1 => dy,
                _ => dz,
            };
            let axis_diff_sq = axis_diff * axis_diff;

            let left_end = node.right_child.min(subtree_end);
            let left_opt = if node_idx + 1 < left_end {
                Some((node_idx + 1, left_end))
            } else {
                None
            };
            let right_opt = if node.right_child < subtree_end {
                Some((node.right_child, subtree_end))
            } else {
                None
            };

            let (near_child, far_child) = if axis_diff <= 0.0 {
                (left_opt, right_opt)
            } else {
                (right_opt, left_opt)
            };

            if axis_diff_sq < best_dist_sq {
                if let Some((far_idx, far_end)) = far_child {
                    if sp < 64 {
                        stack[sp] = (far_idx, far_end, axis_diff_sq);
                        sp += 1;
                    }
                }
            }
            if let Some((near_idx, near_end)) = near_child {
                if sp < 64 {
                    stack[sp] = (near_idx, near_end, 0.0_f64);
                    sp += 1;
                }
            }
        }

        Some(best_dist_sq)
    }
}

#[inline]
fn point_dist_sq(a: Point3, b: Point3) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    dx.mul_add(dx, dy.mul_add(dy, dz * dz))
}

fn directed_kd_hausdorff_sq(
    queries: &[Point3],
    targets: &[Point3],
    tree: &KdTree3,
    initial_max_sq: f64,
) -> f64 {
    let mut max_sq = initial_max_sq;
    let mut last_witness = targets[0];
    let n_targets = targets.len();

    for (idx, &q) in queries.iter().enumerate() {
        if point_dist_sq(q, last_witness) <= max_sq {
            continue;
        }
        let corr = targets[idx % n_targets];
        if point_dist_sq(q, corr) <= max_sq {
            last_witness = corr;
            continue;
        }
        if let Some((best_idx, d_sq)) = tree.nearest_sq_above_cutoff(q, max_sq) {
            if let Some(&w) = targets.get(best_idx) {
                last_witness = w;
            }
            if d_sq > max_sq {
                max_sq = d_sq;
            }
        }
    }
    max_sq
}

/// Computes the exact symmetric Hausdorff distance between two 3D point sets using $k$-d trees
/// with Taha–Hanbury early-break and spatial-coherence witness pruning.
#[must_use]
pub fn kd_hausdorff_distance(points_a: &[Point3], points_b: &[Point3]) -> f64 {
    if points_a.is_empty() || points_b.is_empty() {
        return 0.0;
    }

    let tree_b = KdTree3::build(points_b);
    let max_a_to_b_sq = directed_kd_hausdorff_sq(points_a, points_b, &tree_b, 0.0);

    let tree_a = KdTree3::build(points_a);
    let max_b_to_a_sq = directed_kd_hausdorff_sq(points_b, points_a, &tree_a, max_a_to_b_sq);

    max_b_to_a_sq.sqrt()
}

/// Node in a flat array-backed triangle mesh bounding volume hierarchy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BvhNode3 {
    /// Bounding box enclosing all triangles under this subtree.
    pub bounds: Aabb3,
    /// Left child index in node array, if internal.
    pub left: Option<usize>,
    /// Right child index in node array, if internal.
    pub right: Option<usize>,
    /// Triangle index in `mesh.triangles`, or `u32::MAX` for internal nodes.
    pub triangle_index: u32,
}

/// Bounding volume hierarchy over an indexed triangle mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct MeshBvh3 {
    nodes: Vec<BvhNode3>,
    root: Option<usize>,
}

#[derive(Clone, Copy)]
struct TriItem {
    idx: u32,
    bounds: Aabb3,
    centroid: Point3,
}

impl MeshBvh3 {
    /// Builds a balanced bounding volume hierarchy from `mesh`.
    #[must_use]
    pub fn build(mesh: TriangleMeshView<'_>) -> Self {
        if mesh.triangles.is_empty() {
            return Self {
                nodes: Vec::new(),
                root: None,
            };
        }

        let mut items = Vec::with_capacity(mesh.triangles.len());
        for (tri_idx, tri) in mesh.triangles.iter().enumerate() {
            let Ok(idx) = u32::try_from(tri_idx) else {
                continue;
            };
            let Ok(i0) = usize::try_from(tri[0]) else {
                continue;
            };
            let Ok(i1) = usize::try_from(tri[1]) else {
                continue;
            };
            let Ok(i2) = usize::try_from(tri[2]) else {
                continue;
            };
            let Some(&v0) = mesh.vertices.get(i0) else {
                continue;
            };
            let Some(&v1) = mesh.vertices.get(i1) else {
                continue;
            };
            let Some(&v2) = mesh.vertices.get(i2) else {
                continue;
            };

            let bounds = Aabb3::from_triangle(v0, v1, v2);
            let centroid = Point3::new(
                (v0.x + v1.x + v2.x) / 3.0,
                (v0.y + v1.y + v2.y) / 3.0,
                (v0.z + v1.z + v2.z) / 3.0,
            );
            items.push(TriItem {
                idx,
                bounds,
                centroid,
            });
        }

        if items.is_empty() {
            return Self {
                nodes: Vec::new(),
                root: None,
            };
        }

        let mut nodes = Vec::with_capacity(items.len().checked_mul(2).unwrap_or(items.len()));
        let root = Self::build_subtree(&mut nodes, &mut items);

        Self { nodes, root }
    }

    fn build_subtree(nodes: &mut Vec<BvhNode3>, items: &mut [TriItem]) -> Option<usize> {
        if items.is_empty() {
            return None;
        }

        if items.len() == 1 {
            let item = items[0];
            let node_idx = nodes.len();
            nodes.push(BvhNode3 {
                bounds: item.bounds,
                left: None,
                right: None,
                triangle_index: item.idx,
            });
            return Some(node_idx);
        }

        let mut c_min = Point3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
        let mut c_max = Point3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
        let mut total_bounds = Aabb3::empty();

        for it in items.iter() {
            total_bounds = total_bounds.union(it.bounds);
            c_min.x = c_min.x.min(it.centroid.x);
            c_min.y = c_min.y.min(it.centroid.y);
            c_min.z = c_min.z.min(it.centroid.z);
            c_max.x = c_max.x.max(it.centroid.x);
            c_max.y = c_max.y.max(it.centroid.y);
            c_max.z = c_max.z.max(it.centroid.z);
        }

        let span_x = c_max.x - c_min.x;
        let span_y = c_max.y - c_min.y;
        let span_z = c_max.z - c_min.z;

        let axis = if span_x >= span_y && span_x >= span_z {
            0_u8
        } else if span_y >= span_z {
            1_u8
        } else {
            2_u8
        };

        let mid = items.len() / 2;
        items.select_nth_unstable_by(mid, |a, b| match axis {
            0 => a.centroid.x.total_cmp(&b.centroid.x),
            1 => a.centroid.y.total_cmp(&b.centroid.y),
            _ => a.centroid.z.total_cmp(&b.centroid.z),
        });

        let node_idx = nodes.len();
        nodes.push(BvhNode3 {
            bounds: total_bounds,
            left: None,
            right: None,
            triangle_index: u32::MAX,
        });

        let (left_items, right_items) = items.split_at_mut(mid);
        let left = Self::build_subtree(nodes, left_items);
        let right = Self::build_subtree(nodes, right_items);

        if let Some(node) = nodes.get_mut(node_idx) {
            node.left = left;
            node.right = right;
        }

        Some(node_idx)
    }

    /// Finds the closest point on `mesh` to `query` using branch-and-bound pruning.
    #[must_use]
    pub fn closest_point(&self, query: Point3, mesh: TriangleMeshView<'_>) -> ClosestPointResult {
        let Some(root) = self.root else {
            return ClosestPointResult {
                closest_point: Point3::new(0.0, 0.0, 0.0),
                distance_sq: f64::INFINITY,
                triangle_index: u32::MAX,
            };
        };

        let mut best_pt = Point3::new(0.0, 0.0, 0.0);
        let mut best_dist_sq = f64::INFINITY;
        let mut best_tri = u32::MAX;

        let mut stack = [(0_usize, 0.0_f64); 64];
        stack[0] = (root, 0.0_f64);
        let mut sp = 1_usize;

        while sp > 0 {
            sp -= 1;
            let (node_idx, lower_bound_sq) = stack[sp];
            if lower_bound_sq >= best_dist_sq {
                continue;
            }

            let Some(node) = self.nodes.get(node_idx) else {
                continue;
            };

            if node.triangle_index == u32::MAX {
                let dl = node
                    .left
                    .and_then(|l| self.nodes.get(l))
                    .map(|n| n.bounds.distance_squared_to_point(query));
                let dr = node
                    .right
                    .and_then(|r| self.nodes.get(r))
                    .map(|n| n.bounds.distance_squared_to_point(query));

                match (node.left.zip(dl), node.right.zip(dr)) {
                    (Some((l, d_l)), Some((r, d_r))) => {
                        if d_l < d_r {
                            if d_r < best_dist_sq && sp < 64 {
                                stack[sp] = (r, d_r);
                                sp += 1;
                            }
                            if d_l < best_dist_sq && sp < 64 {
                                stack[sp] = (l, d_l);
                                sp += 1;
                            }
                        } else {
                            if d_l < best_dist_sq && sp < 64 {
                                stack[sp] = (l, d_l);
                                sp += 1;
                            }
                            if d_r < best_dist_sq && sp < 64 {
                                stack[sp] = (r, d_r);
                                sp += 1;
                            }
                        }
                    }
                    (Some((l, d_l)), None) => {
                        if d_l < best_dist_sq && sp < 64 {
                            stack[sp] = (l, d_l);
                            sp += 1;
                        }
                    }
                    (None, Some((r, d_r))) => {
                        if d_r < best_dist_sq && sp < 64 {
                            stack[sp] = (r, d_r);
                            sp += 1;
                        }
                    }
                    (None, None) => {}
                }
            } else {
                let Ok(tri_idx) = usize::try_from(node.triangle_index) else {
                    continue;
                };
                let Some(tri) = mesh.triangles.get(tri_idx) else {
                    continue;
                };
                let Ok(i0) = usize::try_from(tri[0]) else {
                    continue;
                };
                let Ok(i1) = usize::try_from(tri[1]) else {
                    continue;
                };
                let Ok(i2) = usize::try_from(tri[2]) else {
                    continue;
                };
                let (Some(&v0), Some(&v1), Some(&v2)) = (
                    mesh.vertices.get(i0),
                    mesh.vertices.get(i1),
                    mesh.vertices.get(i2),
                ) else {
                    continue;
                };

                let (q, d_sq) = closest_point_on_triangle(query, v0, v1, v2);
                if d_sq < best_dist_sq {
                    best_dist_sq = d_sq;
                    best_pt = q;
                    best_tri = node.triangle_index;
                }
            }
        }

        ClosestPointResult {
            closest_point: best_pt,
            distance_sq: best_dist_sq,
            triangle_index: best_tri,
        }
    }

    /// Finds the closest intersection along `ray` against `mesh`.
    #[must_use]
    pub fn intersect_ray(&self, ray: &RayQuery3, mesh: TriangleMeshView<'_>) -> RayHit3 {
        let Some(root) = self.root else {
            return RayHit3::miss();
        };

        let mut closest = RayHit3::miss();
        let mut stack = [0_usize; 64];
        stack[0] = root;
        let mut sp = 1_usize;

        while sp > 0 {
            sp -= 1;
            let node_idx = stack[sp];
            let Some(node) = self.nodes.get(node_idx) else {
                continue;
            };

            let bounded_ray = RayQuery3 {
                origin: ray.origin,
                direction: ray.direction,
                t_min: ray.t_min,
                t_max: closest.t.min(ray.t_max),
            };

            if !node.bounds.intersects_ray(&bounded_ray) {
                continue;
            }

            if node.triangle_index == u32::MAX {
                if let Some(r) = node.right {
                    if sp < 64 {
                        stack[sp] = r;
                        sp += 1;
                    }
                }
                if let Some(l) = node.left {
                    if sp < 64 {
                        stack[sp] = l;
                        sp += 1;
                    }
                }
            } else {
                let Ok(tri_idx) = usize::try_from(node.triangle_index) else {
                    continue;
                };
                let Some(tri) = mesh.triangles.get(tri_idx) else {
                    continue;
                };
                let Ok(i0) = usize::try_from(tri[0]) else {
                    continue;
                };
                let Ok(i1) = usize::try_from(tri[1]) else {
                    continue;
                };
                let Ok(i2) = usize::try_from(tri[2]) else {
                    continue;
                };
                let (Some(&v0), Some(&v1), Some(&v2)) = (
                    mesh.vertices.get(i0),
                    mesh.vertices.get(i1),
                    mesh.vertices.get(i2),
                ) else {
                    continue;
                };

                if let Some((t, u, v)) = intersect_ray_triangle(&bounded_ray, v0, v1, v2) {
                    if t < closest.t {
                        closest = RayHit3 {
                            hit: true,
                            t,
                            triangle_index: node.triangle_index,
                            barycentric_u: u,
                            barycentric_v: v,
                        };
                    }
                }
            }
        }

        closest
    }

    /// Counts ray surface crossings for parity solid containment testing.
    #[must_use]
    pub fn count_surface_crossings(&self, ray: &RayQuery3, mesh: TriangleMeshView<'_>) -> usize {
        let Some(root) = self.root else {
            return 0;
        };

        let mut hit_buf = [0.0_f64; 64];
        let mut hit_len = 0_usize;
        let mut stack = [0_usize; 64];
        stack[0] = root;
        let mut sp = 1_usize;

        while sp > 0 {
            sp -= 1;
            let node_idx = stack[sp];
            let Some(node) = self.nodes.get(node_idx) else {
                continue;
            };

            if !node.bounds.intersects_ray(ray) {
                continue;
            }

            if node.triangle_index == u32::MAX {
                if let Some(r) = node.right {
                    if sp < 64 {
                        stack[sp] = r;
                        sp += 1;
                    }
                }
                if let Some(l) = node.left {
                    if sp < 64 {
                        stack[sp] = l;
                        sp += 1;
                    }
                }
            } else {
                let Ok(tri_idx) = usize::try_from(node.triangle_index) else {
                    continue;
                };
                let Some(tri) = mesh.triangles.get(tri_idx) else {
                    continue;
                };
                let Ok(i0) = usize::try_from(tri[0]) else {
                    continue;
                };
                let Ok(i1) = usize::try_from(tri[1]) else {
                    continue;
                };
                let Ok(i2) = usize::try_from(tri[2]) else {
                    continue;
                };
                let (Some(&v0), Some(&v1), Some(&v2)) = (
                    mesh.vertices.get(i0),
                    mesh.vertices.get(i1),
                    mesh.vertices.get(i2),
                ) else {
                    continue;
                };

                if let Some((t, _, _)) = intersect_ray_triangle(ray, v0, v1, v2) {
                    if hit_len < 64 {
                        hit_buf[hit_len] = t;
                        hit_len += 1;
                    }
                }
            }
        }

        let hits = &mut hit_buf[..hit_len];
        hits.sort_unstable_by(f64::total_cmp);
        let mut count = 0_usize;
        let mut last_t = -1.0_f64;
        for &t in hits.iter() {
            if last_t < 0.0 || (t - last_t).abs() > PARITY_HIT_DEDUP_TOLERANCE {
                count = count.checked_add(1).unwrap_or(count);
                last_t = t;
            }
        }
        count
    }

    /// Linearizes the bounding volume hierarchy into a contiguous pre-order array suitable for GPU storage buffers.
    #[must_use]
    pub fn flatten(&self) -> FlatMeshBvh3 {
        let mut flat = Vec::with_capacity(self.nodes.len());
        for node in &self.nodes {
            let right_child = match node.right {
                Some(r) => u32::try_from(r).ok().unwrap_or(0),
                None => u32::MAX,
            };
            flat.push(FlatBvhNode {
                aabb_min: node.bounds.min,
                aabb_max: node.bounds.max,
                triangle_index: node.triangle_index,
                right_child,
            });
        }
        FlatMeshBvh3 { nodes: flat }
    }
}

/// Flattened 3D bounding volume hierarchy node suitable for contiguous GPU storage buffers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlatBvhNode {
    /// Minimum corner of the node bounding box.
    pub aabb_min: Point3,
    /// Maximum corner of the node bounding box.
    pub aabb_max: Point3,
    /// Triangle index in `mesh.triangles` for leaf nodes, or `u32::MAX` for internal nodes.
    pub triangle_index: u32,
    /// Index of the right child in [`FlatMeshBvh3::nodes`] for internal nodes, or `u32::MAX` for leaves.
    pub right_child: u32,
}

/// Contiguous pre-order 3D mesh BVH representation for GPU compute traversal.
#[derive(Debug, Clone, PartialEq)]
pub struct FlatMeshBvh3 {
    /// Linearized pre-order node array.
    pub nodes: Vec<FlatBvhNode>,
}

impl FlatMeshBvh3 {
    /// Returns the linearized pre-order node slice.
    #[must_use]
    pub fn nodes(&self) -> &[FlatBvhNode] {
        &self.nodes
    }

    /// Finds the closest point on `mesh` to `query` by traversing the flattened BVH array.
    #[must_use]
    pub fn closest_point(&self, query: Point3, mesh: TriangleMeshView<'_>) -> ClosestPointResult {
        self.closest_point_with_bound(query, mesh, f64::INFINITY)
    }

    /// Finds the closest point on `mesh` to `query` using an initial distance-squared upper bound.
    #[must_use]
    pub fn closest_point_with_bound(
        &self,
        query: Point3,
        mesh: TriangleMeshView<'_>,
        initial_bound_sq: f64,
    ) -> ClosestPointResult {
        if self.nodes.is_empty() {
            return ClosestPointResult {
                closest_point: Point3::new(0.0, 0.0, 0.0),
                distance_sq: f64::INFINITY,
                triangle_index: u32::MAX,
            };
        }

        let mut best_pt = Point3::new(0.0, 0.0, 0.0);
        let mut best_dist_sq = initial_bound_sq;
        let mut best_tri = u32::MAX;

        let mut stack = [(0_u32, 0.0_f64); 64];
        stack[0] = (0, 0.0_f64);
        let mut sp = 1_usize;

        while sp > 0 {
            sp -= 1;
            let (node_idx, lower_bound_sq) = stack[sp];
            if lower_bound_sq >= best_dist_sq && best_tri != u32::MAX {
                continue;
            }
            let Ok(idx_usize) = usize::try_from(node_idx) else {
                continue;
            };
            let Some(node) = self.nodes.get(idx_usize) else {
                continue;
            };

            if node.triangle_index == u32::MAX {
                let left_idx = node_idx + 1;
                let right_idx = node.right_child;

                let dl = usize::try_from(left_idx)
                    .ok()
                    .and_then(|l| self.nodes.get(l))
                    .map(|n| {
                        Aabb3 {
                            min: n.aabb_min,
                            max: n.aabb_max,
                        }
                        .distance_squared_to_point(query)
                    });
                let dr = usize::try_from(right_idx)
                    .ok()
                    .and_then(|r| self.nodes.get(r))
                    .map(|n| {
                        Aabb3 {
                            min: n.aabb_min,
                            max: n.aabb_max,
                        }
                        .distance_squared_to_point(query)
                    });

                match (dl, dr) {
                    (Some(d_l), Some(d_r)) => {
                        if d_l < d_r {
                            if d_r < best_dist_sq && sp < 64 {
                                stack[sp] = (right_idx, d_r);
                                sp += 1;
                            }
                            if d_l < best_dist_sq && sp < 64 {
                                stack[sp] = (left_idx, d_l);
                                sp += 1;
                            }
                        } else {
                            if d_l < best_dist_sq && sp < 64 {
                                stack[sp] = (left_idx, d_l);
                                sp += 1;
                            }
                            if d_r < best_dist_sq && sp < 64 {
                                stack[sp] = (right_idx, d_r);
                                sp += 1;
                            }
                        }
                    }
                    (Some(d_l), None) => {
                        if d_l < best_dist_sq && sp < 64 {
                            stack[sp] = (left_idx, d_l);
                            sp += 1;
                        }
                    }
                    (None, Some(d_r)) => {
                        if d_r < best_dist_sq && sp < 64 {
                            stack[sp] = (right_idx, d_r);
                            sp += 1;
                        }
                    }
                    (None, None) => {}
                }
            } else {
                let Ok(tri_idx) = usize::try_from(node.triangle_index) else {
                    continue;
                };
                let Some(tri) = mesh.triangles.get(tri_idx) else {
                    continue;
                };
                let Ok(i0) = usize::try_from(tri[0]) else {
                    continue;
                };
                let Ok(i1) = usize::try_from(tri[1]) else {
                    continue;
                };
                let Ok(i2) = usize::try_from(tri[2]) else {
                    continue;
                };
                let (Some(&v0), Some(&v1), Some(&v2)) = (
                    mesh.vertices.get(i0),
                    mesh.vertices.get(i1),
                    mesh.vertices.get(i2),
                ) else {
                    continue;
                };

                let (q, d_sq) = closest_point_on_triangle(query, v0, v1, v2);
                if d_sq < best_dist_sq {
                    best_dist_sq = d_sq;
                    best_pt = q;
                    best_tri = node.triangle_index;
                }
            }
        }

        ClosestPointResult {
            closest_point: best_pt,
            distance_sq: best_dist_sq,
            triangle_index: best_tri,
        }
    }
}

/// Finds the closest point on `mesh` for each point in `points` using a bounding volume hierarchy.
#[must_use]
pub fn closest_points_on_mesh(
    points: &[Point3],
    mesh: TriangleMeshView<'_>,
) -> Vec<ClosestPointResult> {
    if points.is_empty() || mesh.triangles.is_empty() {
        return crate::cpu::closest_point_mesh(points, mesh);
    }
    let bvh = MeshBvh3::build(mesh);
    points.iter().map(|&p| bvh.closest_point(p, mesh)).collect()
}

/// Intersects a batch of rays against an indexed triangle mesh using a bounding volume hierarchy.
#[must_use]
pub fn intersect_rays_mesh(rays: &[RayQuery3], mesh: TriangleMeshView<'_>) -> Vec<RayHit3> {
    if rays.is_empty() || mesh.triangles.is_empty() {
        return crate::cpu::ray_mesh_intersect(rays, mesh);
    }
    let bvh = MeshBvh3::build(mesh);
    let mut hits = Vec::with_capacity(rays.len());
    for ray in rays {
        hits.push(bvh.intersect_ray(ray, mesh));
    }
    hits
}

/// Determines inside/outside solid containment for query points using BVH ray casting.
#[must_use]
pub fn points_in_solid_parity(points: &[Point3], mesh: TriangleMeshView<'_>) -> Vec<bool> {
    if points.is_empty() || mesh.triangles.is_empty() {
        return crate::cpu::point_in_solid_parity(points, mesh);
    }
    let bvh = MeshBvh3::build(mesh);
    let mut parity = Vec::with_capacity(points.len());
    for &p in points {
        let ray = RayQuery3 {
            origin: p,
            direction: PARITY_RAY_DIRECTION,
            t_min: 0.0,
            t_max: f64::INFINITY,
        };
        let crossings = bvh.count_surface_crossings(&ray, mesh);
        parity.push(crossings % 2 == 1);
    }
    parity
}

fn parallel_directed_kd_hausdorff_sq(
    queries: &[Point3],
    targets: &[Point3],
    tree: &KdTree3,
    shared_max_bits: &std::sync::atomic::AtomicU64,
) {
    use rayon::prelude::*;
    use std::sync::atomic::Ordering;

    let n_targets = targets.len();
    queries
        .par_chunks(256)
        .enumerate()
        .for_each(|(chunk_idx, chunk)| {
            let base_idx = chunk_idx * 256;
            let mut local_max_sq = f64::from_bits(shared_max_bits.load(Ordering::Relaxed));
            let mut last_witness = targets[base_idx % n_targets];

            for (offset, &q) in chunk.iter().enumerate() {
                if offset.is_multiple_of(16) {
                    let global_sq = f64::from_bits(shared_max_bits.load(Ordering::Relaxed));
                    if global_sq > local_max_sq {
                        local_max_sq = global_sq;
                    }
                }
                if point_dist_sq(q, last_witness) <= local_max_sq {
                    continue;
                }
                let corr = targets[(base_idx + offset) % n_targets];
                if point_dist_sq(q, corr) <= local_max_sq {
                    last_witness = corr;
                    continue;
                }
                if let Some((best_idx, d_sq)) = tree.nearest_sq_above_cutoff(q, local_max_sq) {
                    if let Some(&w) = targets.get(best_idx) {
                        last_witness = w;
                    }
                    if d_sq > local_max_sq {
                        local_max_sq = d_sq;
                        let new_bits = local_max_sq.to_bits();
                        let mut cur_bits = shared_max_bits.load(Ordering::Relaxed);
                        while f64::from_bits(cur_bits) < local_max_sq {
                            match shared_max_bits.compare_exchange_weak(
                                cur_bits,
                                new_bits,
                                Ordering::Relaxed,
                                Ordering::Relaxed,
                            ) {
                                Ok(_) => break,
                                Err(actual) => cur_bits = actual,
                            }
                        }
                    }
                }
            }
        });
}

/// Computes the exact symmetric Hausdorff distance between two 3D point sets using Rayon
/// multi-threaded parallel $k$-d tree queries with lock-free Taha–Hanbury early-break.
#[must_use]
pub fn rayon_hausdorff_distance(points_a: &[Point3], points_b: &[Point3]) -> f64 {
    use std::sync::atomic::{AtomicU64, Ordering};

    if points_a.is_empty() || points_b.is_empty() {
        return 0.0;
    }

    let shared_max_bits = AtomicU64::new(0.0_f64.to_bits());
    let (tree_a, tree_b) = rayon::join(|| KdTree3::build(points_a), || KdTree3::build(points_b));
    parallel_directed_kd_hausdorff_sq(points_a, points_b, &tree_b, &shared_max_bits);
    parallel_directed_kd_hausdorff_sq(points_b, points_a, &tree_a, &shared_max_bits);

    f64::from_bits(shared_max_bits.load(Ordering::Relaxed)).sqrt()
}

/// Finds the closest point on `mesh` for each point using Rayon multi-threaded BVH search.
#[must_use]
pub fn rayon_closest_points_on_mesh(
    points: &[Point3],
    mesh: TriangleMeshView<'_>,
) -> Vec<ClosestPointResult> {
    use rayon::prelude::*;

    if points.is_empty() || mesh.triangles.is_empty() {
        return closest_points_on_mesh(points, mesh);
    }

    let bvh = MeshBvh3::build(mesh);
    points
        .par_iter()
        .map(|&p| bvh.closest_point(p, mesh))
        .collect()
}

/// Intersects a batch of rays against an indexed triangle mesh using Rayon multi-threaded BVH traversal.
#[must_use]
pub fn rayon_intersect_rays_mesh(rays: &[RayQuery3], mesh: TriangleMeshView<'_>) -> Vec<RayHit3> {
    use rayon::prelude::*;

    if rays.is_empty() || mesh.triangles.is_empty() {
        return intersect_rays_mesh(rays, mesh);
    }

    let bvh = MeshBvh3::build(mesh);
    rays.par_iter()
        .map(|ray| bvh.intersect_ray(ray, mesh))
        .collect()
}

/// Determines inside/outside solid containment for query points using Rayon multi-threaded BVH ray casting.
#[must_use]
pub fn rayon_points_in_solid_parity(points: &[Point3], mesh: TriangleMeshView<'_>) -> Vec<bool> {
    use rayon::prelude::*;

    if points.is_empty() || mesh.triangles.is_empty() {
        return points_in_solid_parity(points, mesh);
    }

    let bvh = MeshBvh3::build(mesh);
    points
        .par_iter()
        .map(|&p| {
            let ray = RayQuery3 {
                origin: p,
                direction: PARITY_RAY_DIRECTION,
                t_min: 0.0,
                t_max: f64::INFINITY,
            };
            let crossings = bvh.count_surface_crossings(&ray, mesh);
            crossings % 2 == 1
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPATIAL_TEST_TOLERANCE: f64 = 1.0e-12;

    #[test]
    fn kdtree_exact_nearest_neighbour_matches_linear_scan() {
        let points = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
            Point3::new(0.0, 10.0, 0.0),
            Point3::new(0.0, 0.0, 10.0),
            Point3::new(5.0, 5.0, 5.0),
            Point3::new(-3.0, 2.0, 4.0),
        ];
        let tree = KdTree3::build(&points);
        assert_eq!(tree.len(), 6);

        let query = Point3::new(4.8, 5.1, 4.9);
        let (pt, idx, dist) = tree.nearest(query).expect("nearest point found");
        assert_eq!(idx, 4);
        assert!((pt.x - 5.0).abs() < SPATIAL_TEST_TOLERANCE);
        assert!((pt.y - 5.0).abs() < SPATIAL_TEST_TOLERANCE);
        assert!((pt.z - 5.0).abs() < SPATIAL_TEST_TOLERANCE);
        assert!(dist < 0.3);
    }

    #[test]
    fn kd_hausdorff_identical_sets_is_zero() {
        let pts = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(999.99997, 0.0, 0.0),
            Point3::new(1000.00003, 0.0, 0.0),
        ];
        let d = kd_hausdorff_distance(&pts, &pts);
        assert_eq!(d, 0.0);
    }

    #[test]
    fn mesh_bvh_closest_point_matches_triangle_projection() {
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
        ];
        let triangles = [[0_u32, 1, 2]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };

        let pts = [Point3::new(0.5, 0.5, 3.0), Point3::new(-1.0, -1.0, 0.0)];
        let results = closest_points_on_mesh(&pts, mesh);
        assert_eq!(results.len(), 2);
        assert!((results[0].distance_sq - 9.0).abs() < SPATIAL_TEST_TOLERANCE);
        assert!((results[1].distance_sq - 2.0).abs() < SPATIAL_TEST_TOLERANCE);
    }

    const PARITY_TOLERANCE: f64 = 1.0e-11;

    #[test]
    fn spatial_kdtree_and_bvh_edge_cases() {
        let empty_tree = KdTree3::build(&[]);
        assert_eq!(empty_tree.len(), 0);
        assert!(empty_tree.is_empty());
        assert!(empty_tree.nearest(Point3::new(1.0, 2.0, 3.0)).is_none());

        let empty_mesh = TriangleMeshView {
            vertices: &[],
            triangles: &[],
        };
        let empty_bvh = MeshBvh3::build(empty_mesh);
        let res = empty_bvh.closest_point(Point3::new(0.0, 0.0, 0.0), empty_mesh);
        assert_eq!(res.triangle_index, u32::MAX);

        let single = [Point3::new(7.0, -3.0, 4.0)];
        let single_tree = KdTree3::build(&single);
        assert_eq!(single_tree.len(), 1);
        let (pt, idx, dist) = single_tree
            .nearest(Point3::new(7.0, -3.0, 5.0))
            .expect("found");
        assert_eq!(idx, 0);
        assert!((pt.z - 4.0).abs() < PARITY_TOLERANCE);
        assert!((dist - 1.0).abs() < PARITY_TOLERANCE);

        let coplanar: Vec<Point3> = (0..20)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                Point3::new(f, f * 0.5, 0.0)
            })
            .collect();
        let tree = KdTree3::build(&coplanar);
        assert_eq!(tree.len(), 20);
        let (pt_cop, idx_cop, _) = tree
            .nearest(Point3::new(5.1, 2.5, 0.0))
            .expect("coplanar nearest");
        assert_eq!(idx_cop, 5);
        assert!((pt_cop.x - 5.0).abs() < PARITY_TOLERANCE);

        let h_single = kd_hausdorff_distance(&coplanar, &single);
        let h_rayon = rayon_hausdorff_distance(&coplanar, &single);
        assert_eq!(h_single, h_rayon);
    }

    #[test]
    fn flattened_kdtree_and_bvh_match_tree_queries() {
        let points: Vec<Point3> = (0..37)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                Point3::new(f * 0.4 - 3.0, (f * 0.3).sin() * 5.0, (f * 0.2).cos() * 4.0)
            })
            .collect();
        let tree = KdTree3::build(&points);
        let flat_tree = tree.flatten();
        assert_eq!(flat_tree.nodes.len(), points.len());

        for j in 0..25_u32 {
            let qf = f64::from(j) * 0.5 - 4.0;
            let q = Point3::new(qf, qf * -0.7, qf * 0.3);
            let (_, expected_sq) = tree.nearest_sq(q).expect("tree nearest");
            let actual_sq = flat_tree.nearest_sq(q).expect("flat nearest");
            assert!((expected_sq - actual_sq).abs() < PARITY_TOLERANCE);
        }

        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
        ];
        let triangles = [[0_u32, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let bvh = MeshBvh3::build(mesh);
        let flat_bvh = bvh.flatten();
        assert_eq!(flat_bvh.nodes.len(), 7);

        for k in 0..15_u32 {
            let qf = f64::from(k) * 0.3 - 1.5;
            let q = Point3::new(qf, qf * 0.5, qf * -0.4);
            let exp_cp = bvh.closest_point(q, mesh);
            let act_cp = flat_bvh.closest_point(q, mesh);
            assert!((exp_cp.distance_sq - act_cp.distance_sq).abs() < PARITY_TOLERANCE);
            assert_eq!(exp_cp.triangle_index, act_cp.triangle_index);
        }
    }

    #[test]
    fn degenerate_coincident_triangle_closest_point() {
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(6.0, 0.0, 0.0),
            Point3::new(5.0, 1.0, 0.0),
        ];
        let triangles = [[0_u32, 0, 1], [2, 3, 4]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let query = Point3::new(0.5, 0.5, 0.0);

        let cpu_cp = crate::cpu::closest_point_mesh(&[query], mesh);
        assert_eq!(cpu_cp.len(), 1);
        assert_eq!(cpu_cp[0].triangle_index, 0);
        assert!((cpu_cp[0].distance_sq - 0.25).abs() < PARITY_TOLERANCE);
        assert!((cpu_cp[0].closest_point.x - 0.5).abs() < PARITY_TOLERANCE);
        assert!((cpu_cp[0].closest_point.y - 0.0).abs() < PARITY_TOLERANCE);
        assert!((cpu_cp[0].closest_point.z - 0.0).abs() < PARITY_TOLERANCE);

        let bvh = MeshBvh3::build(mesh);
        let bvh_cp = bvh.closest_point(query, mesh);
        assert_eq!(bvh_cp.triangle_index, 0);
        assert!((bvh_cp.distance_sq - 0.25).abs() < PARITY_TOLERANCE);
        assert!((bvh_cp.closest_point.x - 0.5).abs() < PARITY_TOLERANCE);
        assert!((bvh_cp.closest_point.y - 0.0).abs() < PARITY_TOLERANCE);
        assert!((bvh_cp.closest_point.z - 0.0).abs() < PARITY_TOLERANCE);

        let flat_bvh = bvh.flatten();
        let flat_cp = flat_bvh.closest_point(query, mesh);
        assert_eq!(flat_cp.triangle_index, 0);
        assert!((flat_cp.distance_sq - 0.25).abs() < PARITY_TOLERANCE);
        assert!((flat_cp.closest_point.x - 0.5).abs() < PARITY_TOLERANCE);
        assert!((flat_cp.closest_point.y - 0.0).abs() < PARITY_TOLERANCE);
        assert!((flat_cp.closest_point.z - 0.0).abs() < PARITY_TOLERANCE);
    }

    #[test]
    fn large_magnitude_coordinate_hausdorff_precision() {
        let pts_a = [Point3::new(1.0e5, 0.0, 0.0)];
        let pts_b = [Point3::new(1.0e5 + 0.0035, 0.0, 0.0)];
        let h1 = kd_hausdorff_distance(&pts_a, &pts_b);
        assert!((h1 - 0.0035).abs() < PARITY_TOLERANCE);

        let pts_c = [Point3::new(1.0e6, 0.0, 0.0)];
        let pts_d = [Point3::new(1.0e6 + 0.03, 0.0, 0.0)];
        let expected2 = pts_d[0].x - pts_c[0].x;
        let h2 = kd_hausdorff_distance(&pts_c, &pts_d);
        assert!((h2 - expected2).abs() < PARITY_TOLERANCE);
    }

    #[test]
    fn degenerate_zero_area_and_collinear_mesh_closest_point() {
        let v_in1 = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(6.0, 0.0, 0.0),
            Point3::new(5.0, 1.0, 0.0),
        ];
        let t_in1 = [[0_u32, 1, 1], [2, 3, 4]];
        let mesh1 = TriangleMeshView {
            vertices: &v_in1,
            triangles: &t_in1,
        };
        let query = Point3::new(0.1, 0.5, 0.0);

        let cpu_cp1 = crate::cpu::closest_point_mesh(&[query], mesh1);
        assert_eq!(cpu_cp1[0].triangle_index, 0);
        assert!((cpu_cp1[0].distance_sq - 0.25).abs() < PARITY_TOLERANCE);
        assert!((cpu_cp1[0].closest_point.x - 0.1).abs() < PARITY_TOLERANCE);
        assert!(cpu_cp1[0].closest_point.y.abs() < PARITY_TOLERANCE);

        let flat_bvh1 = MeshBvh3::build(mesh1).flatten();
        let flat_cp1 = flat_bvh1.closest_point(query, mesh1);
        assert_eq!(flat_cp1.triangle_index, 0);
        assert!((flat_cp1.distance_sq - 0.25).abs() < PARITY_TOLERANCE);
        assert!((flat_cp1.closest_point.x - 0.1).abs() < PARITY_TOLERANCE);
        assert!(flat_cp1.closest_point.y.abs() < PARITY_TOLERANCE);

        let v_in2 = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(6.0, 0.0, 0.0),
            Point3::new(5.0, 1.0, 0.0),
        ];
        let t_in2 = [[0_u32, 1, 2], [3, 4, 5]];
        let mesh2 = TriangleMeshView {
            vertices: &v_in2,
            triangles: &t_in2,
        };
        let cpu_cp2 = crate::cpu::closest_point_mesh(&[query], mesh2);
        assert_eq!(cpu_cp2[0].triangle_index, 0);
        assert!((cpu_cp2[0].distance_sq - 0.25).abs() < PARITY_TOLERANCE);
        assert!((cpu_cp2[0].closest_point.x - 0.1).abs() < PARITY_TOLERANCE);
        assert!(cpu_cp2[0].closest_point.y.abs() < PARITY_TOLERANCE);

        let flat_bvh2 = MeshBvh3::build(mesh2).flatten();
        let flat_cp2 = flat_bvh2.closest_point(query, mesh2);
        assert_eq!(flat_cp2.triangle_index, 0);
        assert!((flat_cp2.distance_sq - 0.25).abs() < PARITY_TOLERANCE);
        assert!((flat_cp2.closest_point.x - 0.1).abs() < PARITY_TOLERANCE);
        assert!(flat_cp2.closest_point.y.abs() < PARITY_TOLERANCE);
    }

    #[test]
    fn sliver_sharp_vertex_mesh_closest_point() {
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1000.0, 0.0, 0.0),
            Point3::new(1000.0, 2.0e-5, 0.0),
        ];
        let triangles = [[0_u32, 1, 2]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let query = Point3::new(900.0, 9.0e-6, 0.0);

        let cpu_cp = crate::cpu::closest_point_mesh(&[query], mesh);
        assert_eq!(cpu_cp[0].triangle_index, 0);
        assert!(cpu_cp[0].distance_sq < PARITY_TOLERANCE);
        assert!((cpu_cp[0].closest_point.x - 900.0).abs() < PARITY_TOLERANCE);
        assert!((cpu_cp[0].closest_point.y - 9.0e-6).abs() < PARITY_TOLERANCE);
        assert!(cpu_cp[0].closest_point.z.abs() < PARITY_TOLERANCE);

        let flat_bvh = MeshBvh3::build(mesh).flatten();
        let flat_cp = flat_bvh.closest_point(query, mesh);
        assert_eq!(flat_cp.triangle_index, 0);
        assert!(flat_cp.distance_sq < PARITY_TOLERANCE);
        assert!((flat_cp.closest_point.x - 900.0).abs() < PARITY_TOLERANCE);
        assert!((flat_cp.closest_point.y - 9.0e-6).abs() < PARITY_TOLERANCE);
        assert!(flat_cp.closest_point.z.abs() < PARITY_TOLERANCE);

        let disp_cp = closest_points_on_mesh(&[query], mesh);
        assert_eq!(disp_cp[0].triangle_index, 0);
        assert!(disp_cp[0].distance_sq < PARITY_TOLERANCE);
        assert!((disp_cp[0].closest_point.x - 900.0).abs() < PARITY_TOLERANCE);
        assert!((disp_cp[0].closest_point.y - 9.0e-6).abs() < PARITY_TOLERANCE);
        assert!(disp_cp[0].closest_point.z.abs() < PARITY_TOLERANCE);
    }

    #[test]
    fn rotated_sliver_sharp_vertex_mesh_closest_point() {
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(
                844.029_628_745_985_4,
                449.098_785_111_286_93,
                -293.128_413_857_272_2,
            ),
            Point3::new(
                844.029_622_883_417_1,
                449.098_801_991_879_5,
                -293.128_404_875_296_5,
            ),
        ];
        let triangles = [[0_u32, 1, 2]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let query = Point3::new(
            759.626_663_233_231_2,
            404.188_914_196_424_9,
            -263.815_568_429_655_97,
        );

        let cpu_cp = crate::cpu::closest_point_mesh(&[query], mesh);
        assert_eq!(cpu_cp[0].triangle_index, 0);
        assert!(cpu_cp[0].distance_sq < PARITY_TOLERANCE);
        assert!((cpu_cp[0].closest_point.x - query.x).abs() < PARITY_TOLERANCE);
        assert!((cpu_cp[0].closest_point.y - query.y).abs() < PARITY_TOLERANCE);
        assert!((cpu_cp[0].closest_point.z - query.z).abs() < PARITY_TOLERANCE);

        let flat_bvh = MeshBvh3::build(mesh).flatten();
        let flat_cp = flat_bvh.closest_point(query, mesh);
        assert_eq!(flat_cp.triangle_index, 0);
        assert!(flat_cp.distance_sq < PARITY_TOLERANCE);
        assert!((flat_cp.closest_point.x - query.x).abs() < PARITY_TOLERANCE);
        assert!((flat_cp.closest_point.y - query.y).abs() < PARITY_TOLERANCE);
        assert!((flat_cp.closest_point.z - query.z).abs() < PARITY_TOLERANCE);

        let disp_cp = closest_points_on_mesh(&[query], mesh);
        assert_eq!(disp_cp[0].triangle_index, 0);
        assert!(disp_cp[0].distance_sq < PARITY_TOLERANCE);
        assert!((disp_cp[0].closest_point.x - query.x).abs() < PARITY_TOLERANCE);
        assert!((disp_cp[0].closest_point.y - query.y).abs() < PARITY_TOLERANCE);
        assert!((disp_cp[0].closest_point.z - query.z).abs() < PARITY_TOLERANCE);
    }

    #[test]
    fn rayon_parallel_spatial_queries_match_single_threaded() {
        let vertices = [
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(2.0, 0.0, 0.0),
            Point3::new(0.0, 2.0, 0.0),
            Point3::new(0.0, 0.0, 2.0),
        ];
        let triangles = [[0_u32, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mesh = TriangleMeshView {
            vertices: &vertices,
            triangles: &triangles,
        };
        let queries: Vec<Point3> = (0..32_u32)
            .map(|i| {
                let f = f64::from(i) * 0.1 - 0.5;
                Point3::new(f, f * 0.8 + 0.2, f * -0.4 + 0.5)
            })
            .collect();
        let rays: Vec<RayQuery3> = queries
            .iter()
            .map(|&origin| RayQuery3 {
                origin,
                direction: crate::cpu::PARITY_RAY_DIRECTION,
                t_min: 0.0,
                t_max: 100.0,
            })
            .collect();

        let h_st = kd_hausdorff_distance(&queries, &vertices);
        let h_mt = rayon_hausdorff_distance(&queries, &vertices);
        assert!((h_st - h_mt).abs() < PARITY_TOLERANCE);

        let cp_st = closest_points_on_mesh(&queries, mesh);
        let cp_mt = rayon_closest_points_on_mesh(&queries, mesh);
        for (a, b) in cp_st.iter().zip(&cp_mt) {
            assert_eq!(a.triangle_index, b.triangle_index);
            assert!((a.distance_sq - b.distance_sq).abs() < PARITY_TOLERANCE);
        }

        let hits_st = intersect_rays_mesh(&rays, mesh);
        let hits_mt = rayon_intersect_rays_mesh(&rays, mesh);
        for (a, b) in hits_st.iter().zip(&hits_mt) {
            assert_eq!(a.hit, b.hit);
            if a.hit {
                assert_eq!(a.triangle_index, b.triangle_index);
                assert!((a.t - b.t).abs() < PARITY_TOLERANCE);
            }
        }

        let par_st = points_in_solid_parity(&queries, mesh);
        let par_mt = rayon_points_in_solid_parity(&queries, mesh);
        assert_eq!(par_st, par_mt);
    }
}
