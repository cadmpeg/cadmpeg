// SPDX-License-Identifier: Apache-2.0
//! Deterministic CPU reference implementations for all GPU-offloaded operations.

use cadmpeg_ir::math::{Point3, Vector3};
use rayon::prelude::*;

use crate::{ClosestPointResult, RayHit3, RayQuery3, TriangleMeshView};

/// Minimum determinant magnitude for ray-triangle intersection before treating a ray as parallel.
pub const RAY_TRIANGLE_DET_EPSILON: f64 = 1.0e-12;

/// Barycentric boundary tolerance for watertight triangle intersection.
pub const BARYCENTRIC_EDGE_EPSILON: f64 = 1.0e-11;

/// Factor applied to the product of edge lengths squared below which a triangle is treated as degenerate.
pub const DEGENERATE_AREA_FACTOR: f64 = f64::EPSILON;

fn closest_point_on_segment(query: Point3, a: Point3, b: Point3) -> (Point3, f64) {
    let u = Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
    let u_len_sq = u.dot(u);
    let proj = if u_len_sq > 0.0 {
        let ap = Vector3::new(query.x - a.x, query.y - a.y, query.z - a.z);
        let t = (ap.dot(u) / u_len_sq).clamp(0.0, 1.0);
        Point3::new(a.x + t * u.x, a.y + t * u.y, a.z + t * u.z)
    } else {
        a
    };
    let dx = query.x - proj.x;
    let dy = query.y - proj.y;
    let dz = query.z - proj.z;
    (proj, dx.mul_add(dx, dy.mul_add(dy, dz * dz)))
}

/// Minimum homogeneous weight magnitude before falling back to unit weight in rational Bézier evaluation.
pub const RATIONAL_WEIGHT_FLOOR: f64 = 1.0e-12;

/// Minimum distance between two ray hits along the same parity ray before counting them as distinct surfaces.
pub const PARITY_HIT_DEDUP_TOLERANCE: f64 = 1.0e-9;

/// Deterministic irrational-like ray direction used for point-in-solid parity casting to avoid axis-aligned edges.
pub const PARITY_RAY_DIRECTION: Vector3 = Vector3 {
    x: 0.577_350_269_189_625_8,
    y: 0.534_522_483_824_848_8,
    z: 0.617_213_399_848_367_6,
};

/// Applies a $4 \times 4$ row-major transformation matrix to a slice of 4D homogeneous points.
#[must_use]
pub fn batch_transform_4x4(matrix: &[[f64; 4]; 4], points: &[[f64; 4]]) -> Vec<[f64; 4]> {
    let mut out = Vec::with_capacity(points.len());
    for p in points {
        let mut row = [0.0_f64; 4];
        for r in 0..4 {
            row[r] = matrix[r][0].mul_add(
                p[0],
                matrix[r][1].mul_add(p[1], matrix[r][2].mul_add(p[2], matrix[r][3] * p[3])),
            );
        }
        out.push(row);
    }
    out
}

/// Applies a $4 \times 4$ row-major transformation matrix in parallel using Rayon.
#[must_use]
pub fn rayon_batch_transform_4x4(matrix: &[[f64; 4]; 4], points: &[[f64; 4]]) -> Vec<[f64; 4]> {
    points
        .par_iter()
        .map(|p| {
            let mut row = [0.0_f64; 4];
            for r in 0..4 {
                row[r] = matrix[r][0].mul_add(
                    p[0],
                    matrix[r][1].mul_add(p[1], matrix[r][2].mul_add(p[2], matrix[r][3] * p[3])),
                );
            }
            row
        })
        .collect()
}

/// Computes the $N \times M$ row-major matrix of squared Euclidean distances between `points_a` and `points_b`.
#[must_use]
pub fn pairwise_distance_squared(points_a: &[Point3], points_b: &[Point3]) -> Vec<f64> {
    let total = points_a.len().checked_mul(points_b.len()).unwrap_or(0);
    if total == 0 {
        return Vec::new();
    }
    let m = points_b.len();
    let bx: Vec<f64> = points_b.iter().map(|b| b.x).collect();
    let by: Vec<f64> = points_b.iter().map(|b| b.y).collect();
    let bz: Vec<f64> = points_b.iter().map(|b| b.z).collect();
    let mut out: Vec<f64> = (0..total).map(|_| 0.0_f64).collect();
    for (row, a) in out.chunks_exact_mut(m).zip(points_a.iter()) {
        let ax = a.x;
        let ay = a.y;
        let az = a.z;
        for (((dst, &x), &y), &z) in row.iter_mut().zip(&bx).zip(&by).zip(&bz) {
            let dx = ax - x;
            let dy = ay - y;
            let dz = az - z;
            *dst = dx.mul_add(dx, dy.mul_add(dy, dz * dz));
        }
    }
    out
}

/// Computes the $N \times M$ row-major matrix of squared Euclidean distances in parallel using Rayon.
#[must_use]
pub fn rayon_pairwise_distance_squared(points_a: &[Point3], points_b: &[Point3]) -> Vec<f64> {
    let total = points_a.len().checked_mul(points_b.len()).unwrap_or(0);
    if total == 0 {
        return Vec::new();
    }
    let m = points_b.len();
    let bx: Vec<f64> = points_b.iter().map(|b| b.x).collect();
    let by: Vec<f64> = points_b.iter().map(|b| b.y).collect();
    let bz: Vec<f64> = points_b.iter().map(|b| b.z).collect();
    let mut out: Vec<f64> = (0..total).map(|_| 0.0_f64).collect();
    out.par_chunks_mut(m)
        .zip(points_a.par_iter())
        .for_each(|(row, a)| {
            let ax = a.x;
            let ay = a.y;
            let az = a.z;
            for (((dst, &x), &y), &z) in row.iter_mut().zip(&bx).zip(&by).zip(&bz) {
                let dx = ax - x;
                let dy = ay - y;
                let dz = az - z;
                *dst = dx.mul_add(dx, dy.mul_add(dy, dz * dz));
            }
        });
    out
}

/// Computes the symmetric Hausdorff distance from an $N \times M$ squared distance matrix.
#[must_use]
pub fn hausdorff_from_distance_squared_matrix(dist_sq: &[f64], len_a: usize, len_b: usize) -> f64 {
    let Some(expected_len) = len_a.checked_mul(len_b) else {
        return 0.0;
    };
    if len_a == 0 || len_b == 0 || dist_sq.len() < expected_len {
        return 0.0;
    }
    let mut max_min_a = 0.0_f64;
    for i in 0..len_a {
        let row_start = i * len_b;
        let row = &dist_sq[row_start..row_start + len_b];
        let mut min_sq = f64::INFINITY;
        for &d in row {
            if d < min_sq {
                min_sq = d;
            }
        }
        if min_sq > max_min_a {
            max_min_a = min_sq;
        }
    }
    let mut max_min_b = 0.0_f64;
    for j in 0..len_b {
        let mut min_sq = f64::INFINITY;
        for i in 0..len_a {
            let d = dist_sq[i * len_b + j];
            if d < min_sq {
                min_sq = d;
            }
        }
        if min_sq > max_min_b {
            max_min_b = min_sq;
        }
    }
    max_min_a.max(max_min_b).sqrt()
}

/// Evaluates cubic Bernstein basis polynomials $B_{0..3, 3}(t)$ at parameter $t \in [0, 1]$.
#[must_use]
pub fn cubic_bernstein_basis(t: f64) -> [f64; 4] {
    let s = 1.0 - t;
    let s2 = s * s;
    let t2 = t * t;
    [s2 * s, 3.0 * s2 * t, 3.0 * s * t2, t2 * t]
}

/// Evaluates a bicubic rational Bézier surface patch over the tensor-product grid `u_params` $\times$ `v_params`.
///
/// Each entry `control_points[i][j]` holds homogeneous coordinates `[x * w, y * w, z * w, w]`.
#[must_use]
pub fn eval_bicubic_bezier_grid(
    control_points: &[[[f64; 4]; 4]; 4],
    u_params: &[f64],
    v_params: &[f64],
) -> Vec<Point3> {
    let total = u_params.len().checked_mul(v_params.len()).unwrap_or(0);
    let mut out = Vec::with_capacity(total);
    let v_bases: Vec<[f64; 4]> = v_params.iter().map(|&v| cubic_bernstein_basis(v)).collect();
    for &u in u_params {
        let bu = cubic_bernstein_basis(u);
        let mut u_contracted = [[0.0_f64; 4]; 4];
        for j in 0..4 {
            for c in 0..4 {
                u_contracted[j][c] = bu[0].mul_add(
                    control_points[0][j][c],
                    bu[1].mul_add(
                        control_points[1][j][c],
                        bu[2].mul_add(control_points[2][j][c], bu[3] * control_points[3][j][c]),
                    ),
                );
            }
        }
        for bv in &v_bases {
            let mut hom = [0.0_f64; 4];
            for c in 0..4 {
                hom[c] = bv[0].mul_add(
                    u_contracted[0][c],
                    bv[1].mul_add(
                        u_contracted[1][c],
                        bv[2].mul_add(u_contracted[2][c], bv[3] * u_contracted[3][c]),
                    ),
                );
            }
            let inv_w = if hom[3].abs() > RATIONAL_WEIGHT_FLOOR {
                1.0 / hom[3]
            } else {
                1.0
            };
            out.push(Point3::new(hom[0] * inv_w, hom[1] * inv_w, hom[2] * inv_w));
        }
    }
    out
}

/// Evaluates a bicubic rational Bézier surface patch in parallel using Rayon.
#[must_use]
pub fn rayon_eval_bicubic_bezier_grid(
    control_points: &[[[f64; 4]; 4]; 4],
    u_params: &[f64],
    v_params: &[f64],
) -> Vec<Point3> {
    let total = u_params.len().checked_mul(v_params.len()).unwrap_or(0);
    if total == 0 {
        return Vec::new();
    }
    let nv = v_params.len();
    let v_bases: Vec<[f64; 4]> = v_params.iter().map(|&v| cubic_bernstein_basis(v)).collect();
    let mut out: Vec<Point3> = (0..total).map(|_| Point3::new(0.0, 0.0, 0.0)).collect();
    out.par_chunks_mut(nv)
        .zip(u_params.par_iter())
        .for_each(|(row_out, &u)| {
            let bu = cubic_bernstein_basis(u);
            let mut u_contracted = [[0.0_f64; 4]; 4];
            for j in 0..4 {
                for c in 0..4 {
                    u_contracted[j][c] = bu[0].mul_add(
                        control_points[0][j][c],
                        bu[1].mul_add(
                            control_points[1][j][c],
                            bu[2].mul_add(control_points[2][j][c], bu[3] * control_points[3][j][c]),
                        ),
                    );
                }
            }
            for (dst, bv) in row_out.iter_mut().zip(&v_bases) {
                let mut hom = [0.0_f64; 4];
                for c in 0..4 {
                    hom[c] = bv[0].mul_add(
                        u_contracted[0][c],
                        bv[1].mul_add(
                            u_contracted[1][c],
                            bv[2].mul_add(u_contracted[2][c], bv[3] * u_contracted[3][c]),
                        ),
                    );
                }
                let inv_w = if hom[3].abs() > RATIONAL_WEIGHT_FLOOR {
                    1.0 / hom[3]
                } else {
                    1.0
                };
                *dst = Point3::new(hom[0] * inv_w, hom[1] * inv_w, hom[2] * inv_w);
            }
        });
    out
}

/// Tests a single ray against a triangle (`v0`, `v1`, `v2`) using the Möller–Trumbore algorithm.
#[must_use]
pub fn intersect_ray_triangle(
    ray: &RayQuery3,
    v0: Point3,
    v1: Point3,
    v2: Point3,
) -> Option<(f64, f64, f64)> {
    let edge1 = Vector3::new(v1.x - v0.x, v1.y - v0.y, v1.z - v0.z);
    let edge2 = Vector3::new(v2.x - v0.x, v2.y - v0.y, v2.z - v0.z);
    let pvec = ray.direction.cross(edge2);
    let det = edge1.dot(pvec);
    if det.abs() <= RAY_TRIANGLE_DET_EPSILON {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = Vector3::new(
        ray.origin.x - v0.x,
        ray.origin.y - v0.y,
        ray.origin.z - v0.z,
    );
    let u = tvec.dot(pvec) * inv_det;
    if !(-BARYCENTRIC_EDGE_EPSILON..=1.0 + BARYCENTRIC_EDGE_EPSILON).contains(&u) {
        return None;
    }
    let qvec = tvec.cross(edge1);
    let v = ray.direction.dot(qvec) * inv_det;
    if v < -BARYCENTRIC_EDGE_EPSILON || u + v > 1.0 + BARYCENTRIC_EDGE_EPSILON {
        return None;
    }
    let t = edge2.dot(qvec) * inv_det;
    if t >= ray.t_min && t <= ray.t_max {
        Some((t, u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
    } else {
        None
    }
}

/// Intersects a batch of rays against an indexed triangle mesh and returns the closest hit per ray.
#[must_use]
pub fn ray_mesh_intersect(rays: &[RayQuery3], mesh: TriangleMeshView<'_>) -> Vec<RayHit3> {
    let mut hits = Vec::with_capacity(rays.len());
    for ray in rays {
        let mut best = RayHit3::miss();
        for (tri_idx, tri) in mesh.triangles.iter().enumerate() {
            let Ok(i0) = usize::try_from(tri[0]) else {
                continue;
            };
            let Ok(i1) = usize::try_from(tri[1]) else {
                continue;
            };
            let Ok(i2) = usize::try_from(tri[2]) else {
                continue;
            };
            let Ok(tri_u32) = u32::try_from(tri_idx) else {
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
            if let Some((t, u, v)) = intersect_ray_triangle(ray, v0, v1, v2) {
                if t < best.t {
                    best = RayHit3 {
                        hit: true,
                        t,
                        triangle_index: tri_u32,
                        barycentric_u: u,
                        barycentric_v: v,
                    };
                }
            }
        }
        hits.push(best);
    }
    hits
}

/// Evaluates point-in-solid containment for each query point against a closed triangle mesh
/// using ray-casting intersection parity with deduplication of shared-edge hits.
#[must_use]
pub fn point_in_solid_parity(points: &[Point3], mesh: TriangleMeshView<'_>) -> Vec<bool> {
    let mut inside = Vec::with_capacity(points.len());
    let mut hit_ts = Vec::new();
    for &pt in points {
        hit_ts.clear();
        let ray = RayQuery3 {
            origin: pt,
            direction: PARITY_RAY_DIRECTION,
            t_min: PARITY_HIT_DEDUP_TOLERANCE,
            t_max: f64::INFINITY,
        };
        for tri in mesh.triangles {
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
            if let Some((t, _, _)) = intersect_ray_triangle(&ray, v0, v1, v2) {
                hit_ts.push(t);
            }
        }
        hit_ts.sort_by(f64::total_cmp);
        let mut distinct_hits = 0_usize;
        let mut last_t = -f64::INFINITY;
        for &t in &hit_ts {
            if (t - last_t).abs() > PARITY_HIT_DEDUP_TOLERANCE {
                distinct_hits += 1;
                last_t = t;
            }
        }
        inside.push(distinct_hits % 2 == 1);
    }
    inside
}

/// Evaluates `prod1_a * prod1_b - prod2_a * prod2_b` using Kahan's two-FMA difference of products.
#[inline]
fn diff_of_products(prod1_a: f64, prod1_b: f64, prod2_a: f64, prod2_b: f64) -> f64 {
    let cd = prod2_a * prod2_b;
    let err = (-prod2_a).mul_add(prod2_b, cd);
    let dop = prod1_a.mul_add(prod1_b, -cd);
    dop + err
}

/// Computes the closest point on triangle (`v0`, `v1`, `v2`) to `query`.
#[must_use]
pub fn closest_point_on_triangle(
    query: Point3,
    v0: Point3,
    v1: Point3,
    v2: Point3,
) -> (Point3, f64) {
    let ab = Vector3::new(v1.x - v0.x, v1.y - v0.y, v1.z - v0.z);
    let ac = Vector3::new(v2.x - v0.x, v2.y - v0.y, v2.z - v0.z);

    let n = Vector3::new(
        diff_of_products(ab.y, ac.z, ab.z, ac.y),
        diff_of_products(ab.z, ac.x, ab.x, ac.z),
        diff_of_products(ab.x, ac.y, ab.y, ac.x),
    );
    let n_sq = n.dot(n);
    let ab_sq = ab.dot(ab);
    let ac_sq = ac.dot(ac);
    if n_sq <= DEGENERATE_AREA_FACTOR * ab_sq * ac_sq {
        let (p_ab, d_ab) = closest_point_on_segment(query, v0, v1);
        let (p_bc, d_bc) = closest_point_on_segment(query, v1, v2);
        let (p_ca, d_ca) = closest_point_on_segment(query, v2, v0);
        let mut best_p = p_ab;
        let mut best_d = d_ab;
        if d_bc < best_d {
            best_p = p_bc;
            best_d = d_bc;
        }
        if d_ca < best_d {
            best_p = p_ca;
            best_d = d_ca;
        }
        return (best_p, best_d);
    }

    let ap = Vector3::new(query.x - v0.x, query.y - v0.y, query.z - v0.z);

    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        let dx = query.x - v0.x;
        let dy = query.y - v0.y;
        let dz = query.z - v0.z;
        return (v0, dx.mul_add(dx, dy.mul_add(dy, dz * dz)));
    }

    let bp = Vector3::new(query.x - v1.x, query.y - v1.y, query.z - v1.z);
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        let dx = query.x - v1.x;
        let dy = query.y - v1.y;
        let dz = query.z - v1.z;
        return (v1, dx.mul_add(dx, dy.mul_add(dy, dz * dz)));
    }

    let vc = d1.mul_add(d4, -(d3 * d2));
    let denom_ab = d1 - d3;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 && denom_ab > 0.0 {
        let bary_v = d1 / denom_ab;
        let proj = Point3::new(
            v0.x + bary_v * ab.x,
            v0.y + bary_v * ab.y,
            v0.z + bary_v * ab.z,
        );
        let dx = query.x - proj.x;
        let dy = query.y - proj.y;
        let dz = query.z - proj.z;
        return (proj, dx.mul_add(dx, dy.mul_add(dy, dz * dz)));
    }

    let cp = Vector3::new(query.x - v2.x, query.y - v2.y, query.z - v2.z);
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        let dx = query.x - v2.x;
        let dy = query.y - v2.y;
        let dz = query.z - v2.z;
        return (v2, dx.mul_add(dx, dy.mul_add(dy, dz * dz)));
    }

    let vb = d5.mul_add(d2, -(d1 * d6));
    let denom_ac = d2 - d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 && denom_ac > 0.0 {
        let bary_w = d2 / denom_ac;
        let proj = Point3::new(
            v0.x + bary_w * ac.x,
            v0.y + bary_w * ac.y,
            v0.z + bary_w * ac.z,
        );
        let dx = query.x - proj.x;
        let dy = query.y - proj.y;
        let dz = query.z - proj.z;
        return (proj, dx.mul_add(dx, dy.mul_add(dy, dz * dz)));
    }

    let va = d3.mul_add(d6, -(d5 * d4));
    let denom_bc = (d4 - d3) + (d5 - d6);
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 && denom_bc > 0.0 {
        let bary_w = (d4 - d3) / denom_bc;
        let proj = Point3::new(
            v1.x + bary_w * (v2.x - v1.x),
            v1.y + bary_w * (v2.y - v1.y),
            v1.z + bary_w * (v2.z - v1.z),
        );
        let dx = query.x - proj.x;
        let dy = query.y - proj.y;
        let dz = query.z - proj.z;
        return (proj, dx.mul_add(dx, dy.mul_add(dy, dz * dz)));
    }

    let k = ap.dot(n) / n_sq;
    let proj = Point3::new(query.x - k * n.x, query.y - k * n.y, query.z - k * n.z);
    let dx = query.x - proj.x;
    let dy = query.y - proj.y;
    let dz = query.z - proj.z;
    (proj, dx.mul_add(dx, dy.mul_add(dy, dz * dz)))
}

/// Finds the closest point on an indexed triangle mesh for each query point, using per-triangle
/// AABB lower-bound culling against a running upper-bound distance.
#[must_use]
pub fn closest_point_mesh(
    points: &[Point3],
    mesh: TriangleMeshView<'_>,
) -> Vec<ClosestPointResult> {
    closest_point_mesh_with_upper_bounds(points, mesh, None)
}

/// Finds the closest point on an indexed triangle mesh using optional per-point upper-bound
/// squared distances (for example from RT Core directional probe rays) to accelerate AABB culling.
#[must_use]
pub fn closest_point_mesh_with_upper_bounds(
    points: &[Point3],
    mesh: TriangleMeshView<'_>,
    upper_bounds_sq: Option<&[f64]>,
) -> Vec<ClosestPointResult> {
    #[derive(Clone, Copy)]
    struct TriRecord {
        v0: Point3,
        v1: Point3,
        v2: Point3,
        min: Point3,
        max: Point3,
        idx: u32,
    }

    let mut tris = Vec::with_capacity(mesh.triangles.len());
    for (tri_idx, tri) in mesh.triangles.iter().enumerate() {
        let Ok(i0) = usize::try_from(tri[0]) else {
            continue;
        };
        let Ok(i1) = usize::try_from(tri[1]) else {
            continue;
        };
        let Ok(i2) = usize::try_from(tri[2]) else {
            continue;
        };
        let Ok(idx) = u32::try_from(tri_idx) else {
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
        let min = Point3::new(
            v0.x.min(v1.x).min(v2.x),
            v0.y.min(v1.y).min(v2.y),
            v0.z.min(v1.z).min(v2.z),
        );
        let max = Point3::new(
            v0.x.max(v1.x).max(v2.x),
            v0.y.max(v1.y).max(v2.y),
            v0.z.max(v1.z).max(v2.z),
        );
        tris.push(TriRecord {
            v0,
            v1,
            v2,
            min,
            max,
            idx,
        });
    }

    let mut results = Vec::with_capacity(points.len());
    for (pt_idx, &p) in points.iter().enumerate() {
        let mut best_pt = Point3::new(0.0, 0.0, 0.0);
        let mut best_dist_sq = f64::INFINITY;
        let mut best_tri = u32::MAX;

        let hint_sq = upper_bounds_sq
            .and_then(|ub| ub.get(pt_idx).copied())
            .unwrap_or(f64::INFINITY);

        for t in &tris {
            let dx = if p.x < t.min.x {
                t.min.x - p.x
            } else if p.x > t.max.x {
                p.x - t.max.x
            } else {
                0.0
            };
            let dy = if p.y < t.min.y {
                t.min.y - p.y
            } else if p.y > t.max.y {
                p.y - t.max.y
            } else {
                0.0
            };
            let dz = if p.z < t.min.z {
                t.min.z - p.z
            } else if p.z > t.max.z {
                p.z - t.max.z
            } else {
                0.0
            };
            let aabb_dist_sq = dx.mul_add(dx, dy.mul_add(dy, dz * dz));
            let cull_limit = best_dist_sq.min(hint_sq);
            if aabb_dist_sq > cull_limit && best_tri != u32::MAX {
                continue;
            }
            let (q, d_sq) = closest_point_on_triangle(p, t.v0, t.v1, t.v2);
            if d_sq < best_dist_sq {
                best_dist_sq = d_sq;
                best_pt = q;
                best_tri = t.idx;
            }
        }
        results.push(ClosestPointResult {
            closest_point: best_pt,
            distance_sq: best_dist_sq,
            triangle_index: best_tri,
        });
    }
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNIT_TEST_TOLERANCE: f64 = 1.0e-12;

    #[test]
    fn batch_transform_applies_translation_and_scale() {
        let matrix = [
            [2.0, 0.0, 0.0, 3.0],
            [0.0, 4.0, 0.0, -1.0],
            [0.0, 0.0, 0.5, 2.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let pts = [[1.0, 2.0, 4.0, 1.0], [-1.0, 0.5, 2.0, 1.0]];
        let out = batch_transform_4x4(&matrix, &pts);
        assert_eq!(out.len(), 2);
        assert!((out[0][0] - 5.0).abs() < UNIT_TEST_TOLERANCE);
        assert!((out[0][1] - 7.0).abs() < UNIT_TEST_TOLERANCE);
        assert!((out[0][2] - 4.0).abs() < UNIT_TEST_TOLERANCE);
        assert!((out[1][0] - 1.0).abs() < UNIT_TEST_TOLERANCE);
        assert!((out[1][1] - 1.0).abs() < UNIT_TEST_TOLERANCE);
        assert!((out[1][2] - 3.0).abs() < UNIT_TEST_TOLERANCE);
    }

    #[test]
    fn closest_point_on_triangle_handles_interior_and_vertices() {
        let a = Point3::new(0.0, 0.0, 0.0);
        let b = Point3::new(2.0, 0.0, 0.0);
        let c = Point3::new(0.0, 2.0, 0.0);
        let (q_int, d_int) = closest_point_on_triangle(Point3::new(0.5, 0.5, 3.0), a, b, c);
        assert!((q_int.x - 0.5).abs() < UNIT_TEST_TOLERANCE);
        assert!((q_int.y - 0.5).abs() < UNIT_TEST_TOLERANCE);
        assert!(q_int.z.abs() < UNIT_TEST_TOLERANCE);
        assert!((d_int - 9.0).abs() < UNIT_TEST_TOLERANCE);

        let (q_v0, d_v0) = closest_point_on_triangle(Point3::new(-1.0, -2.0, 0.0), a, b, c);
        assert!((q_v0.x - a.x).abs() < UNIT_TEST_TOLERANCE);
        assert!((q_v0.y - a.y).abs() < UNIT_TEST_TOLERANCE);
        assert!((d_v0 - 5.0).abs() < UNIT_TEST_TOLERANCE);
    }

    #[test]
    fn closest_point_on_triangle_degenerate_zero_area_and_collinear() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(1.0, 0.0, 0.0);
        let v2 = Point3::new(1.0, 0.0, 0.0);
        let query = Point3::new(0.1, 0.5, 0.0);
        let (proj1, dist1_sq) = closest_point_on_triangle(query, v0, v1, v2);
        assert!((proj1.x - 0.1).abs() < UNIT_TEST_TOLERANCE);
        assert!(proj1.y.abs() < UNIT_TEST_TOLERANCE);
        assert!(proj1.z.abs() < UNIT_TEST_TOLERANCE);
        assert!((dist1_sq - 0.25).abs() < UNIT_TEST_TOLERANCE);

        let v2_collinear = Point3::new(2.0, 0.0, 0.0);
        let (proj2, dist2_sq) = closest_point_on_triangle(query, v0, v1, v2_collinear);
        assert!((proj2.x - 0.1).abs() < UNIT_TEST_TOLERANCE);
        assert!(proj2.y.abs() < UNIT_TEST_TOLERANCE);
        assert!(proj2.z.abs() < UNIT_TEST_TOLERANCE);
        assert!((dist2_sq - 0.25).abs() < UNIT_TEST_TOLERANCE);
    }

    #[test]
    fn closest_point_on_triangle_sliver_sharp_vertex() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(1000.0, 0.0, 0.0);
        let v2 = Point3::new(1000.0, 2.0e-5, 0.0);
        let query = Point3::new(900.0, 9.0e-6, 0.0);
        let (proj, dist_sq) = closest_point_on_triangle(query, v0, v1, v2);
        assert!((proj.x - 900.0).abs() < UNIT_TEST_TOLERANCE);
        assert!((proj.y - 9.0e-6).abs() < UNIT_TEST_TOLERANCE);
        assert!(proj.z.abs() < UNIT_TEST_TOLERANCE);
        assert!(dist_sq < UNIT_TEST_TOLERANCE);

        let query_off_plane = Point3::new(900.0, 9.0e-6, 3.0);
        let (proj_off, dist_off_sq) = closest_point_on_triangle(query_off_plane, v0, v1, v2);
        assert!((proj_off.x - 900.0).abs() < UNIT_TEST_TOLERANCE);
        assert!((proj_off.y - 9.0e-6).abs() < UNIT_TEST_TOLERANCE);
        assert!(proj_off.z.abs() < UNIT_TEST_TOLERANCE);
        assert!((dist_off_sq - 9.0).abs() < UNIT_TEST_TOLERANCE);
    }

    #[test]
    fn closest_point_on_triangle_rotated_sliver_sharp_vertex() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(
            844.029_628_745_985_4,
            449.098_785_111_286_93,
            -293.128_413_857_272_2,
        );
        let v2 = Point3::new(
            844.029_622_883_417_1,
            449.098_801_991_879_5,
            -293.128_404_875_296_5,
        );
        let query = Point3::new(
            759.626_663_233_231_2,
            404.188_914_196_424_9,
            -263.815_568_429_655_97,
        );
        let (proj, dist_sq) = closest_point_on_triangle(query, v0, v1, v2);
        assert!((proj.x - query.x).abs() < UNIT_TEST_TOLERANCE);
        assert!((proj.y - query.y).abs() < UNIT_TEST_TOLERANCE);
        assert!((proj.z - query.z).abs() < UNIT_TEST_TOLERANCE);
        assert!(dist_sq < UNIT_TEST_TOLERANCE);
    }

    #[test]
    fn closest_point_on_triangle_degeneracy_threshold_normal_precision() {
        let v0 = Point3::new(0.0, 0.0, 0.0);
        let v1 = Point3::new(
            2_943.919_949_616_599_8,
            -2_484.031_550_366_579_7,
            -3_187.933_905_687_633,
        );
        let v2 = Point3::new(
            2_060.743_987_198_398,
            -1_738.822_040_387_564_4,
            -2_231.553_748_196_103_2,
        );
        let nx = diff_of_products(v1.y, v2.z, v1.z, v2.y);
        let ny = diff_of_products(v1.z, v2.x, v1.x, v2.z);
        let nz = diff_of_products(v1.x, v2.y, v1.y, v2.x);
        let n_len = nx.hypot(ny).hypot(nz);
        let centroid = Point3::new(
            (v0.x + v1.x + v2.x) / 3.0,
            (v0.y + v1.y + v2.y) / 3.0,
            (v0.z + v1.z + v2.z) / 3.0,
        );
        let query = Point3::new(
            centroid.x + nx / n_len,
            centroid.y + ny / n_len,
            centroid.z + nz / n_len,
        );
        let (proj, dist_sq) = closest_point_on_triangle(query, v0, v1, v2);
        assert!(proj.x.is_finite() && proj.y.is_finite() && proj.z.is_finite());
        assert!(dist_sq >= 1.0 - 1.0e-9);
    }

    #[test]
    fn rayon_parallel_kernels_match_serial_cpu() {
        let matrix = [
            [1.5, -0.5, 0.25, 4.0],
            [0.0, 2.0, -1.0, -2.0],
            [-0.25, 0.5, 3.0, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let pts = [
            [1.0, 2.0, 3.0, 1.0],
            [-4.0, 0.5, 2.5, 1.0],
            [0.0, -1.5, 4.0, 1.0],
        ];
        assert_eq!(
            rayon_batch_transform_4x4(&matrix, &pts),
            batch_transform_4x4(&matrix, &pts)
        );

        let pts_a = [Point3::new(0.0, 1.0, 2.0), Point3::new(-1.0, 3.0, 0.5)];
        let pts_b = [
            Point3::new(2.0, -1.0, 0.0),
            Point3::new(0.5, 0.5, 0.5),
            Point3::new(-2.0, 4.0, 1.0),
        ];
        assert_eq!(
            rayon_pairwise_distance_squared(&pts_a, &pts_b),
            pairwise_distance_squared(&pts_a, &pts_b)
        );

        let mut cp = [[[0.0_f64; 4]; 4]; 4];
        for (i, row) in cp.iter_mut().enumerate() {
            for (j, cell) in row.iter_mut().enumerate() {
                let u = f64::from(u8::try_from(i).unwrap_or(0));
                let v = f64::from(u8::try_from(j).unwrap_or(0));
                *cell = [u, v, u * v, 1.0];
            }
        }
        let u_params = [0.0, 0.5, 1.0];
        let v_params = [0.0, 0.25, 0.75, 1.0];
        assert_eq!(
            rayon_eval_bicubic_bezier_grid(&cp, &u_params, &v_params),
            eval_bicubic_bezier_grid(&cp, &u_params, &v_params)
        );
    }
}
