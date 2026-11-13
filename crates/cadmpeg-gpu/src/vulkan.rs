// SPDX-License-Identifier: Apache-2.0
//! Safe cross-platform Vulkan `wgpu` native `SHADER_F64` compute backend.

use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};

use cadmpeg_ir::math::Point3;

use crate::cpu;
use crate::{ClosestPointResult, RayHit3, RayQuery3, TriangleMeshView};

const TRANSFORM_WGSL: &str = "
@group(0) @binding(0) var<storage, read> mat: array<vec4<f64>, 4>;
@group(0) @binding(1) var<storage, read> pts_in: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read_write> pts_out: array<vec4<f64>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= arrayLength(&pts_in) {
        return;
    }
    let p = pts_in[i];
    let r0 = mat[0];
    let r1 = mat[1];
    let r2 = mat[2];
    let r3 = mat[3];
    pts_out[i] = vec4<f64>(
        fma(r0.x, p.x, fma(r0.y, p.y, fma(r0.z, p.z, r0.w * p.w))),
        fma(r1.x, p.x, fma(r1.y, p.y, fma(r1.z, p.z, r1.w * p.w))),
        fma(r2.x, p.x, fma(r2.y, p.y, fma(r2.z, p.z, r2.w * p.w))),
        fma(r3.x, p.x, fma(r3.y, p.y, fma(r3.z, p.z, r3.w * p.w))),
    );
}
";

const PAIRWISE_DIST_WGSL: &str = "
@group(0) @binding(0) var<storage, read> pts_a: array<vec4<f64>>;
@group(0) @binding(1) var<storage, read> pts_b: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read_write> dist_out: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let n = arrayLength(&pts_a);
    let m = arrayLength(&pts_b);
    if idx >= n * m {
        return;
    }
    let row = idx / m;
    let col = idx % m;
    let a = pts_a[row];
    let b = pts_b[col];
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    let dz = a.z - b.z;
    dist_out[idx] = fma(dx, dx, fma(dy, dy, dz * dz));
}
";

const BEZIER_EVAL_WGSL: &str = "
@group(0) @binding(0) var<storage, read> ug: array<vec4<f64>>;
@group(0) @binding(1) var<storage, read> vb: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read_write> pts_out: array<vec4<f64>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let nu = arrayLength(&ug) / 4u;
    let nv = arrayLength(&vb);
    if idx >= nu * nv {
        return;
    }
    let u_idx = idx / nv;
    let v_idx = idx % nv;
    let base = u_idx * 4u;
    let c0 = ug[base + 0u];
    let c1 = ug[base + 1u];
    let c2 = ug[base + 2u];
    let c3 = ug[base + 3u];
    let bv = vb[v_idx];
    let xw = fma(bv.x, c0.x, fma(bv.y, c1.x, fma(bv.z, c2.x, bv.w * c3.x)));
    let yw = fma(bv.x, c0.y, fma(bv.y, c1.y, fma(bv.z, c2.y, bv.w * c3.y)));
    let zw = fma(bv.x, c0.z, fma(bv.y, c1.z, fma(bv.z, c2.z, bv.w * c3.z)));
    let w  = fma(bv.x, c0.w, fma(bv.y, c1.w, fma(bv.z, c2.w, bv.w * c3.w)));
    if abs(w) > 1.0e-30lf {
        let inv_w = 1.0lf / w;
        pts_out[idx] = vec4<f64>(xw * inv_w, yw * inv_w, zw * inv_w, w);
    } else {
        pts_out[idx] = vec4<f64>(xw, yw, zw, w);
    }
}
";

const RAY_MESH_WGSL: &str = "
@group(0) @binding(0) var<storage, read> rays: array<vec4<f64>>;
@group(0) @binding(1) var<storage, read> tris: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read_write> hits_out: array<vec4<f64>>;

fn dop(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    let err = fma(-c, d, cd);
    let f = fma(a, b, -cd);
    return f + err;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let r_idx = gid.x;
    let n_rays = arrayLength(&rays) / 2u;
    if r_idx >= n_rays {
        return;
    }
    let r0 = rays[r_idx * 2u];
    let r1 = rays[r_idx * 2u + 1u];
    let origin = r0.xyz;
    let t_min = r0.w;
    let dir = r1.xyz;
    var best_t = r1.w;
    var best_tri = -1.0lf;
    var best_u = 0.0lf;
    var best_v = 0.0lf;

    let n_tris = arrayLength(&tris) / 3u;
    for (var t_idx = 0u; t_idx < n_tris; t_idx = t_idx + 1u) {
        let base = t_idx * 3u;
        let v0 = tris[base].xyz;
        let e1 = tris[base + 1u].xyz;
        let e2 = tris[base + 2u].xyz;

        let pvec = vec3<f64>(
            dop(dir.y, e2.z, dir.z, e2.y),
            dop(dir.z, e2.x, dir.x, e2.z),
            dop(dir.x, e2.y, dir.y, e2.x),
        );
        let det = fma(e1.x, pvec.x, fma(e1.y, pvec.y, e1.z * pvec.z));
        if abs(det) <= 1.0e-12lf {
            continue;
        }
        let inv_det = 1.0lf / det;
        let tvec = origin - v0;
        let u = fma(tvec.x, pvec.x, fma(tvec.y, pvec.y, tvec.z * pvec.z)) * inv_det;
        if u < -1.0e-11lf || u > 1.0lf + 1.0e-11lf {
            continue;
        }
        let qvec = vec3<f64>(
            dop(tvec.y, e1.z, tvec.z, e1.y),
            dop(tvec.z, e1.x, tvec.x, e1.z),
            dop(tvec.x, e1.y, tvec.y, e1.x),
        );
        let v = fma(dir.x, qvec.x, fma(dir.y, qvec.y, dir.z * qvec.z)) * inv_det;
        if v < -1.0e-11lf || (u + v) > 1.0lf + 1.0e-11lf {
            continue;
        }
        let t = fma(e2.x, qvec.x, fma(e2.y, qvec.y, e2.z * qvec.z)) * inv_det;
        if t >= t_min && t < best_t {
            best_t = t;
            best_tri = f64(t_idx);
            best_u = clamp(u, 0.0lf, 1.0lf);
            best_v = clamp(v, 0.0lf, 1.0lf);
        }
    }
    hits_out[r_idx] = vec4<f64>(best_t, best_tri, best_u, best_v);
}
";

const POINT_IN_SOLID_WGSL: &str = "
@group(0) @binding(0) var<storage, read> points: array<vec4<f64>>;
@group(0) @binding(1) var<storage, read> tris: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read_write> parity_out: array<f64>;

fn dop(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    let err = fma(-c, d, cd);
    let f = fma(a, b, -cd);
    return f + err;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p_idx = gid.x;
    if p_idx >= arrayLength(&points) {
        return;
    }
    let origin = points[p_idx].xyz;
    let dir = vec3<f64>(0.5773502691896258lf, 0.5345224838248488lf, 0.6172133998483676lf);
    var hits = 0u;
    var hit_ts: array<f64, 32>;
    let n_tris = arrayLength(&tris) / 3u;
    for (var t_idx = 0u; t_idx < n_tris; t_idx = t_idx + 1u) {
        let base = t_idx * 3u;
        let v0 = tris[base].xyz;
        let e1 = tris[base + 1u].xyz;
        let e2 = tris[base + 2u].xyz;

        let pvec = vec3<f64>(
            dop(dir.y, e2.z, dir.z, e2.y),
            dop(dir.z, e2.x, dir.x, e2.z),
            dop(dir.x, e2.y, dir.y, e2.x),
        );
        let det = fma(e1.x, pvec.x, fma(e1.y, pvec.y, e1.z * pvec.z));
        if abs(det) <= 1.0e-12lf {
            continue;
        }
        let inv_det = 1.0lf / det;
        let tvec = origin - v0;
        let u = fma(tvec.x, pvec.x, fma(tvec.y, pvec.y, tvec.z * pvec.z)) * inv_det;
        if u < -1.0e-11lf || u > 1.0lf + 1.0e-11lf {
            continue;
        }
        let qvec = vec3<f64>(
            dop(tvec.y, e1.z, tvec.z, e1.y),
            dop(tvec.z, e1.x, tvec.x, e1.z),
            dop(tvec.x, e1.y, tvec.y, e1.x),
        );
        let v = fma(dir.x, qvec.x, fma(dir.y, qvec.y, dir.z * qvec.z)) * inv_det;
        if v < -1.0e-11lf || (u + v) > 1.0lf + 1.0e-11lf {
            continue;
        }
        let t = fma(e2.x, qvec.x, fma(e2.y, qvec.y, e2.z * qvec.z)) * inv_det;
        if t >= 1.0e-9lf {
            var dup = false;
            let tracked = min(hits, 32u);
            for (var k = 0u; k < tracked; k = k + 1u) {
                if abs(t - hit_ts[k]) <= 1.0e-9lf {
                    dup = true;
                    break;
                }
            }
            if !dup {
                if hits < 32u {
                    hit_ts[hits] = t;
                }
                hits = hits + 1u;
            }
        }
    }
    if (hits & 1u) == 1u {
        parity_out[p_idx] = 1.0lf;
    } else {
        parity_out[p_idx] = 0.0lf;
    }
}
";

const BVH_CLOSEST_POINT_WGSL: &str = "
@group(0) @binding(0) var<storage, read> queries: array<vec4<f64>>;
@group(0) @binding(1) var<storage, read> scene: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read_write> cp_out: array<vec4<f64>>;

fn dop(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    let err = fma(-c, d, cd);
    let f = fma(a, b, -cd);
    return f + err;
}

fn vec_dot(a: vec3<f64>, b: vec3<f64>) -> f64 {
    return fma(a.x, b.x, fma(a.y, b.y, a.z * b.z));
}

fn dist_sq3(a: vec3<f64>, b: vec3<f64>) -> f64 {
    let d = a - b;
    return fma(d.x, d.x, fma(d.y, d.y, d.z * d.z));
}

fn aabb_dist_sq(p: vec3<f64>, bmin: vec3<f64>, bmax: vec3<f64>) -> f64 {
    let cx = clamp(p.x, bmin.x, bmax.x);
    let cy = clamp(p.y, bmin.y, bmax.y);
    let cz = clamp(p.z, bmin.z, bmax.z);
    return dist_sq3(p, vec3<f64>(cx, cy, cz));
}

fn closest_on_segment(p: vec3<f64>, a: vec3<f64>, b: vec3<f64>) -> vec3<f64> {
    let ab = b - a;
    let ab_sq = vec_dot(ab, ab);
    if ab_sq <= 0.0lf {
        return a;
    }
    let t = clamp(vec_dot(p - a, ab) / ab_sq, 0.0lf, 1.0lf);
    return fma(vec3<f64>(t, t, t), ab, a);
}

fn closest_point_on_triangle(p: vec3<f64>, a: vec3<f64>, b: vec3<f64>, c: vec3<f64>) -> vec3<f64> {
    let ab = b - a;
    let ac = c - a;
    let n = vec3<f64>(
        dop(ab.y, ac.z, ab.z, ac.y),
        dop(ab.z, ac.x, ab.x, ac.z),
        dop(ab.x, ac.y, ab.y, ac.x),
    );
    let area_sq = vec_dot(n, n);
    let ab_sq = vec_dot(ab, ab);
    let ac_sq = vec_dot(ac, ac);
    if area_sq <= 2.220446049250313e-16lf * ab_sq * ac_sq {
        let p1 = closest_on_segment(p, a, b);
        let p2 = closest_on_segment(p, b, c);
        let p3 = closest_on_segment(p, c, a);
        let d1 = dist_sq3(p, p1);
        let d2 = dist_sq3(p, p2);
        let d3 = dist_sq3(p, p3);
        var best_p = p1;
        var best_d = d1;
        if d2 < best_d {
            best_d = d2;
            best_p = p2;
        }
        if d3 < best_d {
            best_p = p3;
        }
        return best_p;
    }

    let ap = p - a;
    let d1 = vec_dot(ab, ap);
    let d2 = vec_dot(ac, ap);
    if d1 <= 0.0lf && d2 <= 0.0lf {
        return a;
    }

    let bp = p - b;
    let d3 = vec_dot(ab, bp);
    let d4 = vec_dot(ac, bp);
    if d3 >= 0.0lf && d4 <= d3 {
        return b;
    }

    let vc = dop(d1, d4, d3, d2);
    if vc <= 0.0lf && d1 >= 0.0lf && d3 <= 0.0lf {
        let denom = d1 - d3;
        let v = select(0.0lf, clamp(d1 / denom, 0.0lf, 1.0lf), denom > 0.0lf);
        return fma(vec3<f64>(v, v, v), ab, a);
    }

    let cp = p - c;
    let d5 = vec_dot(ab, cp);
    let d6 = vec_dot(ac, cp);
    if d6 >= 0.0lf && d5 <= d6 {
        return c;
    }

    let vb = dop(d5, d2, d1, d6);
    if vb <= 0.0lf && d2 >= 0.0lf && d6 <= 0.0lf {
        let denom = d2 - d6;
        let w = select(0.0lf, clamp(d2 / denom, 0.0lf, 1.0lf), denom > 0.0lf);
        return fma(vec3<f64>(w, w, w), ac, a);
    }

    let va = dop(d3, d6, d5, d4);
    if va <= 0.0lf && (d4 - d3) >= 0.0lf && (d5 - d6) >= 0.0lf {
        let denom = (d4 - d3) + (d5 - d6);
        let w = select(0.0lf, clamp((d4 - d3) / denom, 0.0lf, 1.0lf), denom > 0.0lf);
        let bc = c - b;
        return fma(vec3<f64>(w, w, w), bc, b);
    }

    let t = vec_dot(ap, n) / area_sq;
    return fma(vec3<f64>(-t, -t, -t), n, p);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let q_idx = gid.x;
    if q_idx >= arrayLength(&queries) {
        return;
    }
    let q = queries[q_idx].xyz;
    let hdr = scene[0];
    let n_nodes = u32(hdr.x);
    let tri_base = 1u + n_nodes * 2u;

    var best_sq = hdr.z;
    var best_pt = vec3<f64>(0.0lf, 0.0lf, 0.0lf);
    var best_tri = -1.0lf;

    if n_nodes > 0u {
        var stack_node: array<u32, 64>;
        var stack_end: array<u32, 64>;
        var stack_lb: array<f64, 64>;
        stack_node[0] = 0u;
        stack_end[0] = n_nodes;
        stack_lb[0] = 0.0lf;
        var sp = 1u;

        loop {
            if sp == 0u {
                break;
            }
            sp = sp - 1u;
            let node_idx = stack_node[sp];
            let subtree_end = stack_end[sp];
            let lb = stack_lb[sp];
            if lb >= best_sq {
                continue;
            }

            let n0 = scene[1u + node_idx * 2u];
            let n1 = scene[1u + node_idx * 2u + 1u];
            let tri_f = n0.w;
            let right_child = u32(n1.w);

            if tri_f >= 0.0lf {
                let t_idx = u32(tri_f);
                let tb = tri_base + t_idx * 3u;
                let tv0 = scene[tb];
                if tv0.w > 0.0lf {
                    let v0 = tv0.xyz;
                    let v1 = scene[tb + 1u].xyz;
                    let v2 = scene[tb + 2u].xyz;
                    let cand = closest_point_on_triangle(q, v0, v1, v2);
                    let d_sq = dist_sq3(q, cand);
                    if d_sq < best_sq {
                        best_sq = d_sq;
                        best_pt = cand;
                        best_tri = tri_f;
                    }
                }
            } else {
                let left_idx = node_idx + 1u;
                let left_end = min(right_child, subtree_end);
                let has_left = left_idx < left_end;
                let has_right = right_child < subtree_end;

                if has_left && has_right {
                    let l0 = scene[1u + left_idx * 2u].xyz;
                    let l1 = scene[1u + left_idx * 2u + 1u].xyz;
                    let r0 = scene[1u + right_child * 2u].xyz;
                    let r1 = scene[1u + right_child * 2u + 1u].xyz;
                    let dl = aabb_dist_sq(q, l0, l1);
                    let dr = aabb_dist_sq(q, r0, r1);
                    if dl <= dr {
                        if dr < best_sq && sp < 64u {
                            stack_node[sp] = right_child;
                            stack_end[sp] = subtree_end;
                            stack_lb[sp] = dr;
                            sp = sp + 1u;
                        }
                        if dl < best_sq && sp < 64u {
                            stack_node[sp] = left_idx;
                            stack_end[sp] = left_end;
                            stack_lb[sp] = dl;
                            sp = sp + 1u;
                        }
                    } else {
                        if dl < best_sq && sp < 64u {
                            stack_node[sp] = left_idx;
                            stack_end[sp] = left_end;
                            stack_lb[sp] = dl;
                            sp = sp + 1u;
                        }
                        if dr < best_sq && sp < 64u {
                            stack_node[sp] = right_child;
                            stack_end[sp] = subtree_end;
                            stack_lb[sp] = dr;
                            sp = sp + 1u;
                        }
                    }
                } else if has_left {
                    let l0 = scene[1u + left_idx * 2u].xyz;
                    let l1 = scene[1u + left_idx * 2u + 1u].xyz;
                    let dl = aabb_dist_sq(q, l0, l1);
                    if dl < best_sq && sp < 64u {
                        stack_node[sp] = left_idx;
                        stack_end[sp] = left_end;
                        stack_lb[sp] = dl;
                        sp = sp + 1u;
                    }
                } else if has_right {
                    let r0 = scene[1u + right_child * 2u].xyz;
                    let r1 = scene[1u + right_child * 2u + 1u].xyz;
                    let dr = aabb_dist_sq(q, r0, r1);
                    if dr < best_sq && sp < 64u {
                        stack_node[sp] = right_child;
                        stack_end[sp] = subtree_end;
                        stack_lb[sp] = dr;
                        sp = sp + 1u;
                    }
                }
            }
        }
    }

    cp_out[q_idx * 2u] = vec4<f64>(best_pt.x, best_pt.y, best_pt.z, best_sq);
    cp_out[q_idx * 2u + 1u] = vec4<f64>(best_tri, 0.0lf, 0.0lf, 0.0lf);
}
";

const KDTREE_HAUSDORFF_WGSL: &str = "
@group(0) @binding(0) var<storage, read> nodes: array<vec4<f64>>;
@group(0) @binding(1) var<storage, read> queries: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read_write> best_sq_out: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let q_idx = gid.x;
    if q_idx >= arrayLength(&queries) {
        return;
    }
    let q = queries[q_idx].xyz;
    let n_nodes = arrayLength(&nodes) / 2u;
    var best_sq = nodes[1].y;

    if n_nodes > 0u {
        let seed_idx = min(q_idx, n_nodes - 1u);
        let seed_diff = q - nodes[seed_idx * 2u].xyz;
        best_sq = fma(seed_diff.x, seed_diff.x, fma(seed_diff.y, seed_diff.y, seed_diff.z * seed_diff.z));

        var stack_node: array<u32, 32>;
        var stack_end: array<u32, 32>;
        var stack_lb: array<f64, 32>;
        stack_node[0] = 0u;
        stack_end[0] = n_nodes;
        stack_lb[0] = 0.0lf;
        var sp = 1u;

        loop {
            if sp == 0u {
                break;
            }
            sp = sp - 1u;
            let node_idx = stack_node[sp];
            let subtree_end = stack_end[sp];
            let lb = stack_lb[sp];
            if lb >= best_sq {
                continue;
            }

            let n0 = nodes[node_idx * 2u];
            let n1 = nodes[node_idx * 2u + 1u];
            let np = n0.xyz;
            let split_axis = u32(n0.w);
            let right_child = u32(n1.x);

            let diff = q - np;
            let d_sq = fma(diff.x, diff.x, fma(diff.y, diff.y, diff.z * diff.z));
            if d_sq < best_sq {
                best_sq = d_sq;
            }

            var axis_diff = diff.z;
            if split_axis == 0u {
                axis_diff = diff.x;
            } else if split_axis == 1u {
                axis_diff = diff.y;
            }
            let axis_diff_sq = axis_diff * axis_diff;

            let left_idx = node_idx + 1u;
            let left_end = min(right_child, subtree_end);
            var near_idx = right_child;
            var near_end = subtree_end;
            var far_idx = left_idx;
            var far_end = left_end;
            if axis_diff <= 0.0lf {
                near_idx = left_idx;
                near_end = left_end;
                far_idx = right_child;
                far_end = subtree_end;
            }

            if far_idx < far_end && axis_diff_sq < best_sq && sp < 32u {
                stack_node[sp] = far_idx;
                stack_end[sp] = far_end;
                stack_lb[sp] = axis_diff_sq;
                sp = sp + 1u;
            }
            if near_idx < near_end && sp < 32u {
                stack_node[sp] = near_idx;
                stack_end[sp] = near_end;
                stack_lb[sp] = 0.0lf;
                sp = sp + 1u;
            }
        }
    }

    best_sq_out[q_idx] = best_sq;
}
";

const RT_RAY_MESH_WGSL: &str = "
enable wgpu_ray_query;

@group(0) @binding(0) var acc_struct: acceleration_structure;
@group(0) @binding(1) var<storage, read> rays: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read> scene: array<vec4<f64>>;
@group(0) @binding(3) var<storage, read_write> hits_out: array<vec4<f64>>;

fn dop(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    let err = fma(-c, d, cd);
    let f = fma(a, b, -cd);
    return f + err;
}

fn ray_aabb_hit(
    origin: vec3<f64>,
    dir: vec3<f64>,
    t_min: f64,
    t_max: f64,
    bmin: vec3<f64>,
    bmax: vec3<f64>,
) -> bool {
    var t_near = t_min;
    var t_far = t_max;

    if abs(dir.x) < 1.0e-15lf {
        if origin.x < bmin.x - 1.0e-9lf || origin.x > bmax.x + 1.0e-9lf {
            return false;
        }
    } else {
        let inv_x = 1.0lf / dir.x;
        let tx0 = (bmin.x - 1.0e-9lf - origin.x) * inv_x;
        let tx1 = (bmax.x + 1.0e-9lf - origin.x) * inv_x;
        t_near = max(t_near, min(tx0, tx1));
        t_far = min(t_far, max(tx0, tx1));
        if t_near > t_far {
            return false;
        }
    }

    if abs(dir.y) < 1.0e-15lf {
        if origin.y < bmin.y - 1.0e-9lf || origin.y > bmax.y + 1.0e-9lf {
            return false;
        }
    } else {
        let inv_y = 1.0lf / dir.y;
        let ty0 = (bmin.y - 1.0e-9lf - origin.y) * inv_y;
        let ty1 = (bmax.y + 1.0e-9lf - origin.y) * inv_y;
        t_near = max(t_near, min(ty0, ty1));
        t_far = min(t_far, max(ty0, ty1));
        if t_near > t_far {
            return false;
        }
    }

    if abs(dir.z) < 1.0e-15lf {
        if origin.z < bmin.z - 1.0e-9lf || origin.z > bmax.z + 1.0e-9lf {
            return false;
        }
    } else {
        let inv_z = 1.0lf / dir.z;
        let tz0 = (bmin.z - 1.0e-9lf - origin.z) * inv_z;
        let tz1 = (bmax.z + 1.0e-9lf - origin.z) * inv_z;
        t_near = max(t_near, min(tz0, tz1));
        t_far = min(t_far, max(tz0, tz1));
        if t_near > t_far {
            return false;
        }
    }
    return true;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let r_idx = gid.x;
    let n_rays = arrayLength(&rays) / 2u;
    if r_idx >= n_rays {
        return;
    }
    let r0 = rays[r_idx * 2u];
    let r1 = rays[r_idx * 2u + 1u];
    let origin = r0.xyz;
    let t_min = r0.w;
    let dir = r1.xyz;
    var best_t = r1.w;
    var best_tri = -1.0lf;
    var best_u = 0.0lf;
    var best_v = 0.0lf;

    let hdr = scene[0];
    let n_nodes = u32(hdr.x);
    let n_tris = u32(hdr.y);
    let tri_base = 1u + n_nodes * 2u;

    let orig_f32 = vec3<f32>(
        clamp(f32(origin.x), -1.0e30, 1.0e30),
        clamp(f32(origin.y), -1.0e30, 1.0e30),
        clamp(f32(origin.z), -1.0e30, 1.0e30),
    );
    let dir_f32 = vec3<f32>(
        clamp(f32(dir.x), -1.0e30, 1.0e30),
        clamp(f32(dir.y), -1.0e30, 1.0e30),
        clamp(f32(dir.z), -1.0e30, 1.0e30),
    );
    let tmin_f32 = max(0.0, clamp(f32(t_min), 0.0, 1.0e30) - 1.0e-4);
    let tmax_f32 = clamp(f32(best_t), tmin_f32, 1.0e30);

    var rq: ray_query;
    rayQueryInitialize(&rq, acc_struct, RayDesc(0u, 0xFFu, tmin_f32, tmax_f32, orig_f32, dir_f32));
    while (rayQueryProceed(&rq)) {
        let cand = rayQueryGetCandidateIntersection(&rq);
        let t_idx = cand.primitive_index;
        if t_idx < n_tris {
            let tb = tri_base + t_idx * 3u;
            let tv0 = scene[tb];
            if tv0.w > 0.0lf {
                let v0 = tv0.xyz;
                let v1 = scene[tb + 1u].xyz;
                let v2 = scene[tb + 2u].xyz;
                let e1 = v1 - v0;
                let e2 = v2 - v0;
                let pvec = vec3<f64>(
                    dop(dir.y, e2.z, dir.z, e2.y),
                    dop(dir.z, e2.x, dir.x, e2.z),
                    dop(dir.x, e2.y, dir.y, e2.x),
                );
                let det = fma(e1.x, pvec.x, fma(e1.y, pvec.y, e1.z * pvec.z));
                if abs(det) > 1.0e-12lf {
                    let inv_det = 1.0lf / det;
                    let tvec = origin - v0;
                    let u = fma(tvec.x, pvec.x, fma(tvec.y, pvec.y, tvec.z * pvec.z)) * inv_det;
                    if u >= -1.0e-11lf && u <= 1.0lf + 1.0e-11lf {
                        let qvec = vec3<f64>(
                            dop(tvec.y, e1.z, tvec.z, e1.y),
                            dop(tvec.z, e1.x, tvec.x, e1.z),
                            dop(tvec.x, e1.y, tvec.y, e1.x),
                        );
                        let v = fma(dir.x, qvec.x, fma(dir.y, qvec.y, dir.z * qvec.z)) * inv_det;
                        if v >= -1.0e-11lf && (u + v) <= 1.0lf + 1.0e-11lf {
                            let t = fma(e2.x, qvec.x, fma(e2.y, qvec.y, e2.z * qvec.z)) * inv_det;
                            if t >= t_min && t < best_t {
                                best_t = t;
                                best_tri = f64(t_idx);
                                best_u = clamp(u, 0.0lf, 1.0lf);
                                best_v = clamp(v, 0.0lf, 1.0lf);
                            }
                        }
                    }
                }
            }
        }
    }

    if n_nodes > 0u && n_tris <= 16u {
        var stack_node: array<u32, 64>;
        var stack_end: array<u32, 64>;
        stack_node[0] = 0u;
        stack_end[0] = n_nodes;
        var sp = 1u;

        loop {
            if sp == 0u {
                break;
            }
            sp = sp - 1u;
            let node_idx = stack_node[sp];
            let subtree_end = stack_end[sp];
            let n0 = scene[1u + node_idx * 2u];
            let n1 = scene[1u + node_idx * 2u + 1u];
            if !ray_aabb_hit(origin, dir, t_min, best_t, n0.xyz, n1.xyz) {
                continue;
            }
            let tri_f = n0.w;
            let right_child = u32(n1.w);
            if tri_f >= 0.0lf {
                let t_idx = u32(tri_f);
                let tb = tri_base + t_idx * 3u;
                let tv0 = scene[tb];
                if tv0.w > 0.0lf {
                    let v0 = tv0.xyz;
                    let v1 = scene[tb + 1u].xyz;
                    let v2 = scene[tb + 2u].xyz;
                    let e1 = v1 - v0;
                    let e2 = v2 - v0;
                    let pvec = vec3<f64>(
                        dop(dir.y, e2.z, dir.z, e2.y),
                        dop(dir.z, e2.x, dir.x, e2.z),
                        dop(dir.x, e2.y, dir.y, e2.x),
                    );
                    let det = fma(e1.x, pvec.x, fma(e1.y, pvec.y, e1.z * pvec.z));
                    if abs(det) > 1.0e-12lf {
                        let inv_det = 1.0lf / det;
                        let tvec = origin - v0;
                        let u = fma(tvec.x, pvec.x, fma(tvec.y, pvec.y, tvec.z * pvec.z)) * inv_det;
                        if u >= -1.0e-11lf && u <= 1.0lf + 1.0e-11lf {
                            let qvec = vec3<f64>(
                                dop(tvec.y, e1.z, tvec.z, e1.y),
                                dop(tvec.z, e1.x, tvec.x, e1.z),
                                dop(tvec.x, e1.y, tvec.y, e1.x),
                            );
                            let v = fma(dir.x, qvec.x, fma(dir.y, qvec.y, dir.z * qvec.z)) * inv_det;
                            if v >= -1.0e-11lf && (u + v) <= 1.0lf + 1.0e-11lf {
                                let t = fma(e2.x, qvec.x, fma(e2.y, qvec.y, e2.z * qvec.z)) * inv_det;
                                if t >= t_min && t < best_t {
                                    best_t = t;
                                    best_tri = f64(t_idx);
                                    best_u = clamp(u, 0.0lf, 1.0lf);
                                    best_v = clamp(v, 0.0lf, 1.0lf);
                                }
                            }
                        }
                    }
                }
            } else {
                let left_idx = node_idx + 1u;
                let left_end = min(right_child, subtree_end);
                if right_child < subtree_end && sp < 64u {
                    stack_node[sp] = right_child;
                    stack_end[sp] = subtree_end;
                    sp = sp + 1u;
                }
                if left_idx < left_end && sp < 64u {
                    stack_node[sp] = left_idx;
                    stack_end[sp] = left_end;
                    sp = sp + 1u;
                }
            }
        }
    }

    hits_out[r_idx] = vec4<f64>(best_t, best_tri, best_u, best_v);
}
";

const RT_POINT_IN_SOLID_WGSL: &str = "
enable wgpu_ray_query;

@group(0) @binding(0) var acc_struct: acceleration_structure;
@group(0) @binding(1) var<storage, read> points: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read> scene: array<vec4<f64>>;
@group(0) @binding(3) var<storage, read_write> parity_out: array<f64>;

fn dop(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    let err = fma(-c, d, cd);
    let f = fma(a, b, -cd);
    return f + err;
}

fn ray_aabb_hit(
    origin: vec3<f64>,
    dir: vec3<f64>,
    bmin: vec3<f64>,
    bmax: vec3<f64>,
) -> bool {
    var t_near = 0.0lf;
    var t_far = 1.0e300lf;

    let inv_x = 1.0lf / dir.x;
    let tx0 = (bmin.x - 1.0e-9lf - origin.x) * inv_x;
    let tx1 = (bmax.x + 1.0e-9lf - origin.x) * inv_x;
    t_near = max(t_near, min(tx0, tx1));
    t_far = min(t_far, max(tx0, tx1));
    if t_near > t_far {
        return false;
    }

    let inv_y = 1.0lf / dir.y;
    let ty0 = (bmin.y - 1.0e-9lf - origin.y) * inv_y;
    let ty1 = (bmax.y + 1.0e-9lf - origin.y) * inv_y;
    t_near = max(t_near, min(ty0, ty1));
    t_far = min(t_far, max(ty0, ty1));
    if t_near > t_far {
        return false;
    }

    let inv_z = 1.0lf / dir.z;
    let tz0 = (bmin.z - 1.0e-9lf - origin.z) * inv_z;
    let tz1 = (bmax.z + 1.0e-9lf - origin.z) * inv_z;
    t_near = max(t_near, min(tz0, tz1));
    t_far = min(t_far, max(tz0, tz1));
    return t_near <= t_far;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p_idx = gid.x;
    if p_idx >= arrayLength(&points) {
        return;
    }
    let origin = points[p_idx].xyz;
    let dir = vec3<f64>(0.5773502691896258lf, 0.5345224838248488lf, 0.6172133998483676lf);
    let hdr = scene[0];
    let n_nodes = u32(hdr.x);
    let n_tris = u32(hdr.y);
    let tri_base = 1u + n_nodes * 2u;

    var hits = 0u;
    var hit_ts: array<f64, 32>;

    let orig_f32 = vec3<f32>(
        clamp(f32(origin.x), -1.0e30, 1.0e30),
        clamp(f32(origin.y), -1.0e30, 1.0e30),
        clamp(f32(origin.z), -1.0e30, 1.0e30),
    );
    let dir_f32 = vec3<f32>(0.57735026, 0.5345225, 0.6172134);

    var rq: ray_query;
    rayQueryInitialize(&rq, acc_struct, RayDesc(0u, 0xFFu, 0.0, 1.0e30, orig_f32, dir_f32));
    while (rayQueryProceed(&rq)) {
        let cand = rayQueryGetCandidateIntersection(&rq);
        let t_idx = cand.primitive_index;
        if t_idx < n_tris {
            let tb = tri_base + t_idx * 3u;
            let tv0 = scene[tb];
            if tv0.w > 0.0lf {
                let v0 = tv0.xyz;
                let v1 = scene[tb + 1u].xyz;
                let v2 = scene[tb + 2u].xyz;
                let e1 = v1 - v0;
                let e2 = v2 - v0;
                let pvec = vec3<f64>(
                    dop(dir.y, e2.z, dir.z, e2.y),
                    dop(dir.z, e2.x, dir.x, e2.z),
                    dop(dir.x, e2.y, dir.y, e2.x),
                );
                let det = fma(e1.x, pvec.x, fma(e1.y, pvec.y, e1.z * pvec.z));
                if abs(det) > 1.0e-12lf {
                    let inv_det = 1.0lf / det;
                    let tvec = origin - v0;
                    let u = fma(tvec.x, pvec.x, fma(tvec.y, pvec.y, tvec.z * pvec.z)) * inv_det;
                    if u >= -1.0e-11lf && u <= 1.0lf + 1.0e-11lf {
                        let qvec = vec3<f64>(
                            dop(tvec.y, e1.z, tvec.z, e1.y),
                            dop(tvec.z, e1.x, tvec.x, e1.z),
                            dop(tvec.x, e1.y, tvec.y, e1.x),
                        );
                        let v = fma(dir.x, qvec.x, fma(dir.y, qvec.y, dir.z * qvec.z)) * inv_det;
                        if v >= -1.0e-11lf && (u + v) <= 1.0lf + 1.0e-11lf {
                            let t = fma(e2.x, qvec.x, fma(e2.y, qvec.y, e2.z * qvec.z)) * inv_det;
                            if t >= 1.0e-9lf {
                                var dup = false;
                                let tracked = min(hits, 32u);
                                for (var k = 0u; k < tracked; k = k + 1u) {
                                    if abs(t - hit_ts[k]) <= 1.0e-9lf {
                                        dup = true;
                                        break;
                                    }
                                }
                                if !dup {
                                    if hits < 32u {
                                        hit_ts[hits] = t;
                                    }
                                    hits = hits + 1u;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if n_nodes > 0u && n_tris <= 16u {
        var stack_node: array<u32, 64>;
        var stack_end: array<u32, 64>;
        stack_node[0] = 0u;
        stack_end[0] = n_nodes;
        var sp = 1u;

        loop {
            if sp == 0u {
                break;
            }
            sp = sp - 1u;
            let node_idx = stack_node[sp];
            let subtree_end = stack_end[sp];
            let n0 = scene[1u + node_idx * 2u];
            let n1 = scene[1u + node_idx * 2u + 1u];
            if !ray_aabb_hit(origin, dir, n0.xyz, n1.xyz) {
                continue;
            }
            let tri_f = n0.w;
            let right_child = u32(n1.w);
            if tri_f >= 0.0lf {
                let t_idx = u32(tri_f);
                let tb = tri_base + t_idx * 3u;
                let tv0 = scene[tb];
                if tv0.w > 0.0lf {
                    let v0 = tv0.xyz;
                    let v1 = scene[tb + 1u].xyz;
                    let v2 = scene[tb + 2u].xyz;
                    let e1 = v1 - v0;
                    let e2 = v2 - v0;
                    let pvec = vec3<f64>(
                        dop(dir.y, e2.z, dir.z, e2.y),
                        dop(dir.z, e2.x, dir.x, e2.z),
                        dop(dir.x, e2.y, dir.y, e2.x),
                    );
                    let det = fma(e1.x, pvec.x, fma(e1.y, pvec.y, e1.z * pvec.z));
                    if abs(det) > 1.0e-12lf {
                        let inv_det = 1.0lf / det;
                        let tvec = origin - v0;
                        let u = fma(tvec.x, pvec.x, fma(tvec.y, pvec.y, tvec.z * pvec.z)) * inv_det;
                        if u >= -1.0e-11lf && u <= 1.0lf + 1.0e-11lf {
                            let qvec = vec3<f64>(
                                dop(tvec.y, e1.z, tvec.z, e1.y),
                                dop(tvec.z, e1.x, tvec.x, e1.z),
                                dop(tvec.x, e1.y, tvec.y, e1.x),
                            );
                            let v = fma(dir.x, qvec.x, fma(dir.y, qvec.y, dir.z * qvec.z)) * inv_det;
                            if v >= -1.0e-11lf && (u + v) <= 1.0lf + 1.0e-11lf {
                                let t = fma(e2.x, qvec.x, fma(e2.y, qvec.y, e2.z * qvec.z)) * inv_det;
                                if t >= 1.0e-9lf {
                                    var dup = false;
                                    let tracked = min(hits, 32u);
                                    for (var k = 0u; k < tracked; k = k + 1u) {
                                        if abs(t - hit_ts[k]) <= 1.0e-9lf {
                                            dup = true;
                                            break;
                                        }
                                    }
                                    if !dup {
                                        if hits < 32u {
                                            hit_ts[hits] = t;
                                        }
                                        hits = hits + 1u;
                                    }
                                }
                            }
                        }
                    }
                }
            } else {
                let left_idx = node_idx + 1u;
                let left_end = min(right_child, subtree_end);
                if right_child < subtree_end && sp < 64u {
                    stack_node[sp] = right_child;
                    stack_end[sp] = subtree_end;
                    sp = sp + 1u;
                }
                if left_idx < left_end && sp < 64u {
                    stack_node[sp] = left_idx;
                    stack_end[sp] = left_end;
                    sp = sp + 1u;
                }
            }
        }
    }

    if (hits & 1u) == 1u {
        parity_out[p_idx] = 1.0lf;
    } else {
        parity_out[p_idx] = 0.0lf;
    }
}
";

const RT_CLOSEST_POINT_WGSL: &str = "
enable wgpu_ray_query;

@group(0) @binding(0) var acc_struct: acceleration_structure;
@group(0) @binding(1) var<storage, read> queries: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read> scene: array<vec4<f64>>;
@group(0) @binding(3) var<storage, read_write> cp_out: array<vec4<f64>>;

fn dop(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let cd = c * d;
    let err = fma(-c, d, cd);
    let f = fma(a, b, -cd);
    return f + err;
}

fn vec_dot(a: vec3<f64>, b: vec3<f64>) -> f64 {
    return fma(a.x, b.x, fma(a.y, b.y, a.z * b.z));
}

fn dist_sq3(a: vec3<f64>, b: vec3<f64>) -> f64 {
    let d = a - b;
    return fma(d.x, d.x, fma(d.y, d.y, d.z * d.z));
}

fn aabb_dist_sq(p: vec3<f64>, bmin: vec3<f64>, bmax: vec3<f64>) -> f64 {
    let cx = clamp(p.x, bmin.x, bmax.x);
    let cy = clamp(p.y, bmin.y, bmax.y);
    let cz = clamp(p.z, bmin.z, bmax.z);
    return dist_sq3(p, vec3<f64>(cx, cy, cz));
}

fn closest_on_segment(p: vec3<f64>, a: vec3<f64>, b: vec3<f64>) -> vec3<f64> {
    let ab = b - a;
    let ab_sq = vec_dot(ab, ab);
    if ab_sq <= 0.0lf {
        return a;
    }
    let t = clamp(vec_dot(p - a, ab) / ab_sq, 0.0lf, 1.0lf);
    return fma(vec3<f64>(t, t, t), ab, a);
}

fn closest_point_on_triangle(p: vec3<f64>, a: vec3<f64>, b: vec3<f64>, c: vec3<f64>) -> vec3<f64> {
    let ab = b - a;
    let ac = c - a;
    let n = vec3<f64>(
        dop(ab.y, ac.z, ab.z, ac.y),
        dop(ab.z, ac.x, ab.x, ac.z),
        dop(ab.x, ac.y, ab.y, ac.x),
    );
    let area_sq = vec_dot(n, n);
    let ab_sq = vec_dot(ab, ab);
    let ac_sq = vec_dot(ac, ac);
    if area_sq <= 2.220446049250313e-16lf * ab_sq * ac_sq {
        let p1 = closest_on_segment(p, a, b);
        let p2 = closest_on_segment(p, b, c);
        let p3 = closest_on_segment(p, c, a);
        let d1 = dist_sq3(p, p1);
        let d2 = dist_sq3(p, p2);
        let d3 = dist_sq3(p, p3);
        var best_p = p1;
        var best_d = d1;
        if d2 < best_d {
            best_d = d2;
            best_p = p2;
        }
        if d3 < best_d {
            best_p = p3;
        }
        return best_p;
    }

    let ap = p - a;
    let d1 = vec_dot(ab, ap);
    let d2 = vec_dot(ac, ap);
    if d1 <= 0.0lf && d2 <= 0.0lf {
        return a;
    }

    let bp = p - b;
    let d3 = vec_dot(ab, bp);
    let d4 = vec_dot(ac, bp);
    if d3 >= 0.0lf && d4 <= d3 {
        return b;
    }

    let vc = dop(d1, d4, d3, d2);
    if vc <= 0.0lf && d1 >= 0.0lf && d3 <= 0.0lf {
        let denom = d1 - d3;
        let v = select(0.0lf, clamp(d1 / denom, 0.0lf, 1.0lf), denom > 0.0lf);
        return fma(vec3<f64>(v, v, v), ab, a);
    }

    let cp = p - c;
    let d5 = vec_dot(ab, cp);
    let d6 = vec_dot(ac, cp);
    if d6 >= 0.0lf && d5 <= d6 {
        return c;
    }

    let vb = dop(d5, d2, d1, d6);
    if vb <= 0.0lf && d2 >= 0.0lf && d6 <= 0.0lf {
        let denom = d2 - d6;
        let w = select(0.0lf, clamp(d2 / denom, 0.0lf, 1.0lf), denom > 0.0lf);
        return fma(vec3<f64>(w, w, w), ac, a);
    }

    let va = dop(d3, d6, d5, d4);
    if va <= 0.0lf && (d4 - d3) >= 0.0lf && (d5 - d6) >= 0.0lf {
        let denom = (d4 - d3) + (d5 - d6);
        let w = select(0.0lf, clamp((d4 - d3) / denom, 0.0lf, 1.0lf), denom > 0.0lf);
        let bc = c - b;
        return fma(vec3<f64>(w, w, w), bc, b);
    }

    let t = vec_dot(ap, n) / area_sq;
    return fma(vec3<f64>(-t, -t, -t), n, p);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let q_idx = gid.x;
    if q_idx >= arrayLength(&queries) {
        return;
    }
    let q = queries[q_idx].xyz;
    let hdr = scene[0];
    let n_nodes = u32(hdr.x);
    let n_tris = u32(hdr.y);
    let r_pad_sq = hdr.w;
    let tri_base = 1u + n_nodes * 2u;

    var best_sq = hdr.z;
    var best_pt = vec3<f64>(0.0lf, 0.0lf, 0.0lf);
    var best_tri = -1.0lf;

    if n_tris > 0u {
        let r_reach = clamp(f32(scene[tri_base + 1u].w), 1.0e-4, 1.0e29);
        let qx_f32 = clamp(f32(q.x), -1.0e30, 1.0e30);
        let qy_f32 = clamp(f32(q.y), -1.0e30, 1.0e30);
        let qz_f32 = clamp(f32(q.z), -1.0e30, 1.0e30);
        let orig_f32 = vec3<f32>(qx_f32 - r_reach, qy_f32, qz_f32);
        let dir_f32 = vec3<f32>(1.0, 0.0, 0.0);
        var rq: ray_query;
        rayQueryInitialize(&rq, acc_struct, RayDesc(0u, 0xFFu, 0.0, 2.0 * r_reach, orig_f32, dir_f32));
        while (rayQueryProceed(&rq)) {
            let cand_hit = rayQueryGetCandidateIntersection(&rq);
            let t_idx = cand_hit.primitive_index;
            if t_idx < n_tris {
                let tb = tri_base + t_idx * 3u;
                let tv0 = scene[tb];
                if tv0.w > 0.0lf {
                    let v0 = tv0.xyz;
                    let v1 = scene[tb + 1u].xyz;
                    let v2 = scene[tb + 2u].xyz;
                    let cand = closest_point_on_triangle(q, v0, v1, v2);
                    let d_sq = dist_sq3(q, cand);
                    if d_sq < best_sq {
                        best_sq = d_sq;
                        best_pt = cand;
                        best_tri = f64(t_idx);
                    }
                }
            }
        }
    }

    if n_nodes > 0u && (n_tris <= 16u || best_sq >= r_pad_sq) {
        var stack_node: array<u32, 64>;
        var stack_end: array<u32, 64>;
        var stack_lb: array<f64, 64>;
        stack_node[0] = 0u;
        stack_end[0] = n_nodes;
        stack_lb[0] = 0.0lf;
        var sp = 1u;

        loop {
            if sp == 0u {
                break;
            }
            sp = sp - 1u;
            let node_idx = stack_node[sp];
            let subtree_end = stack_end[sp];
            let lb = stack_lb[sp];
            if lb >= best_sq {
                continue;
            }

            let n0 = scene[1u + node_idx * 2u];
            let n1 = scene[1u + node_idx * 2u + 1u];
            let tri_f = n0.w;
            let right_child = u32(n1.w);

            if tri_f >= 0.0lf {
                let t_idx = u32(tri_f);
                let tb = tri_base + t_idx * 3u;
                let tv0 = scene[tb];
                if tv0.w > 0.0lf {
                    let v0 = tv0.xyz;
                    let v1 = scene[tb + 1u].xyz;
                    let v2 = scene[tb + 2u].xyz;
                    let cand = closest_point_on_triangle(q, v0, v1, v2);
                    let d_sq = dist_sq3(q, cand);
                    if d_sq < best_sq {
                        best_sq = d_sq;
                        best_pt = cand;
                        best_tri = tri_f;
                    }
                }
            } else {
                let left_idx = node_idx + 1u;
                let left_end = min(right_child, subtree_end);
                let has_left = left_idx < left_end;
                let has_right = right_child < subtree_end;

                if has_left && has_right {
                    let l0 = scene[1u + left_idx * 2u].xyz;
                    let l1 = scene[1u + left_idx * 2u + 1u].xyz;
                    let r0 = scene[1u + right_child * 2u].xyz;
                    let r1 = scene[1u + right_child * 2u + 1u].xyz;
                    let dl = aabb_dist_sq(q, l0, l1);
                    let dr = aabb_dist_sq(q, r0, r1);
                    if dl <= dr {
                        if dr < best_sq && sp < 64u {
                            stack_node[sp] = right_child;
                            stack_end[sp] = subtree_end;
                            stack_lb[sp] = dr;
                            sp = sp + 1u;
                        }
                        if dl < best_sq && sp < 64u {
                            stack_node[sp] = left_idx;
                            stack_end[sp] = left_end;
                            stack_lb[sp] = dl;
                            sp = sp + 1u;
                        }
                    } else {
                        if dl < best_sq && sp < 64u {
                            stack_node[sp] = left_idx;
                            stack_end[sp] = left_end;
                            stack_lb[sp] = dl;
                            sp = sp + 1u;
                        }
                        if dr < best_sq && sp < 64u {
                            stack_node[sp] = right_child;
                            stack_end[sp] = subtree_end;
                            stack_lb[sp] = dr;
                            sp = sp + 1u;
                        }
                    }
                } else if has_left {
                    let l0 = scene[1u + left_idx * 2u].xyz;
                    let l1 = scene[1u + left_idx * 2u + 1u].xyz;
                    let dl = aabb_dist_sq(q, l0, l1);
                    if dl < best_sq && sp < 64u {
                        stack_node[sp] = left_idx;
                        stack_end[sp] = left_end;
                        stack_lb[sp] = dl;
                        sp = sp + 1u;
                    }
                } else if has_right {
                    let r0 = scene[1u + right_child * 2u].xyz;
                    let r1 = scene[1u + right_child * 2u + 1u].xyz;
                    let dr = aabb_dist_sq(q, r0, r1);
                    if dr < best_sq && sp < 64u {
                        stack_node[sp] = right_child;
                        stack_end[sp] = subtree_end;
                        stack_lb[sp] = dr;
                        sp = sp + 1u;
                    }
                }
            }
        }
    }

    cp_out[q_idx * 2u] = vec4<f64>(best_pt.x, best_pt.y, best_pt.z, best_sq);
    cp_out[q_idx * 2u + 1u] = vec4<f64>(best_tri, 0.0lf, 0.0lf, 0.0lf);
}
";

const RT_HAUSDORFF_WGSL: &str = "
enable wgpu_ray_query;

@group(0) @binding(0) var acc_struct: acceleration_structure;
@group(0) @binding(1) var<storage, read> nodes: array<vec4<f64>>;
@group(0) @binding(2) var<storage, read> queries: array<vec4<f64>>;
@group(0) @binding(3) var<storage, read_write> best_sq_out: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let q_idx = gid.x;
    if q_idx >= arrayLength(&queries) {
        return;
    }
    let q = queries[q_idx].xyz;
    let hdr = nodes[0];
    let n_nodes = u32(hdr.x);
    var best_sq = hdr.y;
    let r_pad_f32 = clamp(f32(hdr.z), 1.0e-4, 1.0e29);
    let r_pad_sq = hdr.w;

    if n_nodes > 0u {
        let seed_idx = min(q_idx, n_nodes - 1u);
        let seed_diff = q - nodes[1u + seed_idx * 2u].xyz;
        best_sq = fma(seed_diff.x, seed_diff.x, fma(seed_diff.y, seed_diff.y, seed_diff.z * seed_diff.z));
    }

    if n_nodes > 32u {
        let qx_f32 = clamp(f32(q.x), -1.0e30, 1.0e30);
        let qy_f32 = clamp(f32(q.y), -1.0e30, 1.0e30);
        let qz_f32 = clamp(f32(q.z), -1.0e30, 1.0e30);
        let orig_f32 = vec3<f32>(qx_f32 - r_pad_f32, qy_f32, qz_f32);
        let dir_f32 = vec3<f32>(1.0, 0.0, 0.0);
        var rq: ray_query;
        rayQueryInitialize(&rq, acc_struct, RayDesc(0u, 0xFFu, 0.0, 2.0 * r_pad_f32, orig_f32, dir_f32));
        while (rayQueryProceed(&rq)) {
            let cand = rayQueryGetCandidateIntersection(&rq);
            let p_idx = cand.primitive_index;
            if p_idx < n_nodes {
                let np = nodes[1u + p_idx * 2u].xyz;
                let diff = q - np;
                let d_sq = fma(diff.x, diff.x, fma(diff.y, diff.y, diff.z * diff.z));
                if d_sq < best_sq {
                    best_sq = d_sq;
                }
            }
        }
    }

    if n_nodes > 0u && (n_nodes <= 32u || best_sq > r_pad_sq) {
        var stack_node: array<u32, 32>;
        var stack_end: array<u32, 32>;
        var stack_lb: array<f64, 32>;
        stack_node[0] = 0u;
        stack_end[0] = n_nodes;
        stack_lb[0] = 0.0lf;
        var sp = 1u;

        loop {
            if sp == 0u {
                break;
            }
            sp = sp - 1u;
            let node_idx = stack_node[sp];
            let subtree_end = stack_end[sp];
            let lb = stack_lb[sp];
            if lb >= best_sq {
                continue;
            }

            let n0 = nodes[1u + node_idx * 2u];
            let n1 = nodes[1u + node_idx * 2u + 1u];
            let np = n0.xyz;
            let split_axis = u32(n0.w);
            let right_child = u32(n1.x);

            let diff = q - np;
            let d_sq = fma(diff.x, diff.x, fma(diff.y, diff.y, diff.z * diff.z));
            if d_sq < best_sq {
                best_sq = d_sq;
            }

            var axis_diff = diff.z;
            if split_axis == 0u {
                axis_diff = diff.x;
            } else if split_axis == 1u {
                axis_diff = diff.y;
            }
            let axis_diff_sq = axis_diff * axis_diff;

            let left_idx = node_idx + 1u;
            let left_end = min(right_child, subtree_end);
            var near_idx = right_child;
            var near_end = subtree_end;
            var far_idx = left_idx;
            var far_end = left_end;
            if axis_diff <= 0.0lf {
                near_idx = left_idx;
                near_end = left_end;
                far_idx = right_child;
                far_end = subtree_end;
            }

            if far_idx < far_end && axis_diff_sq < best_sq && sp < 32u {
                stack_node[sp] = far_idx;
                stack_end[sp] = far_end;
                stack_lb[sp] = axis_diff_sq;
                sp = sp + 1u;
            }
            if near_idx < near_end && sp < 32u {
                stack_node[sp] = near_idx;
                stack_end[sp] = near_end;
                stack_lb[sp] = 0.0lf;
                sp = sp + 1u;
            }
        }
    }

    best_sq_out[q_idx] = best_sq;
}
";

struct CachedVulkanRtMesh {
    hash: u64,
    n_tris: usize,
    _blas_surface: wgpu::Blas,
    tlas_surface: wgpu::Tlas,
    _blas_proxy: wgpu::Blas,
    tlas_proxy: wgpu::Tlas,
    b_scene: wgpu::Buffer,
}

struct CachedVulkanRtCloud {
    hash: u64,
    n_pts: usize,
    _blas: wgpu::Blas,
    tlas: wgpu::Tlas,
    b_nodes: wgpu::Buffer,
    b_queries: wgpu::Buffer,
}

struct CachedVulkanComputeMesh {
    hash: u64,
    n_tris: usize,
    b_tris: wgpu::Buffer,
    b_scene: wgpu::Buffer,
}

struct CachedVulkanKdCloud {
    hash: u64,
    n_pts: usize,
    b_nodes: wgpu::Buffer,
    b_queries: wgpu::Buffer,
}

struct RtPipelines {
    bgl_rt: wgpu::BindGroupLayout,
    pipe_rt_ray_mesh: wgpu::ComputePipeline,
    pipe_rt_point_in_solid: wgpu::ComputePipeline,
    pipe_rt_closest_point: wgpu::ComputePipeline,
    pipe_rt_hausdorff: wgpu::ComputePipeline,
    mesh_cache: Mutex<Option<CachedVulkanRtMesh>>,
    cloud_cache: Mutex<[Option<CachedVulkanRtCloud>; 2]>,
}

/// Hardware context for safe `wgpu` native `SHADER_F64` and `EXPERIMENTAL_RAY_QUERY` compute execution.
pub struct VulkanContext {
    device_name: String,
    device: wgpu::Device,
    queue: wgpu::Queue,
    bgl_compute: wgpu::BindGroupLayout,
    pipe_transform: wgpu::ComputePipeline,
    pipe_pairwise_dist: wgpu::ComputePipeline,
    pipe_bezier_eval: wgpu::ComputePipeline,
    pipe_ray_mesh: wgpu::ComputePipeline,
    pipe_point_in_solid: wgpu::ComputePipeline,
    pipe_bvh_closest_point: wgpu::ComputePipeline,
    pipe_kdtree_hausdorff: wgpu::ComputePipeline,
    compute_mesh_cache: Mutex<Option<CachedVulkanComputeMesh>>,
    compute_kd_cache: Mutex<[Option<CachedVulkanKdCloud>; 2]>,
    rt: Option<RtPipelines>,
}

static GLOBAL_VULKAN: OnceLock<Option<VulkanContext>> = OnceLock::new();

/// Returns the process-wide [`VulkanContext`] if a `wgpu` adapter with native `SHADER_F64` is available.
#[must_use]
pub fn global_vulkan() -> Option<&'static VulkanContext> {
    GLOBAL_VULKAN.get_or_init(VulkanContext::probe).as_ref()
}

fn slice_f64_to_bytes(slice: &[f64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(slice.len().checked_mul(8).unwrap_or(0));
    for val in slice {
        bytes.extend_from_slice(&val.to_ne_bytes());
    }
    bytes
}

fn slice_f32x3_to_bytes(slice: &[[f32; 3]]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(slice.len().checked_mul(12).unwrap_or(0));
    for item in slice {
        for val in item {
            bytes.extend_from_slice(&val.to_ne_bytes());
        }
    }
    bytes
}

fn slice_u32_to_bytes(slice: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(slice.len().checked_mul(4).unwrap_or(0));
    for val in slice {
        bytes.extend_from_slice(&val.to_ne_bytes());
    }
    bytes
}

fn bytes_to_vec_f64(bytes: &[u8], count: usize) -> Vec<f64> {
    let mut out = Vec::with_capacity(count);
    for chunk in bytes.chunks_exact(8).take(count) {
        out.push(f64::from_ne_bytes([
            chunk[0], chunk[1], chunk[2], chunk[3], chunk[4], chunk[5], chunk[6], chunk[7],
        ]));
    }
    out
}

fn f64_to_blas_f32(val: f64) -> f32 {
    if !val.is_finite() || val == 0.0 {
        return 0.0;
    }
    let clamped = val.clamp(-1.0e30, 1.0e30);
    let bits = clamped.to_bits();
    let sign_u32 = u32::try_from(bits >> 63).unwrap_or(0) << 31;
    let exp_i32 = i32::try_from((bits >> 52) & 0x7ff).unwrap_or(0) - 1023;
    if exp_i32 < -126 {
        return 0.0;
    }
    if exp_i32 > 127 {
        return f32::from_bits(sign_u32 | (254 << 23) | 0x007f_ffff);
    }
    let exp_f32 = u32::try_from(exp_i32 + 127).unwrap_or(0) << 23;
    let mant_24 = (bits & 0x000f_ffff_ffff_ffff) >> 28;
    let sticky = (bits & ((1_u64 << 28) - 1)) != 0;
    let frac_23 = mant_24 >> 1;
    let round_bit = mant_24 & 1;
    let round_up = round_bit == 1 && (sticky || (frac_23 & 1) == 1);
    let f32_bits = sign_u32 | (exp_f32 + u32::try_from(frac_23 + u64::from(round_up)).unwrap_or(0));
    f32::from_bits(f32_bits)
}

impl VulkanContext {
    /// Probes the primary `wgpu` high-performance adapter for `SHADER_F64` and `EXPERIMENTAL_RAY_QUERY` support.
    #[must_use]
    pub fn probe() -> Option<Self> {
        let instance = wgpu::Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .ok()?;

        let info = adapter.get_info();
        let adapter_features = adapter.features();
        if !adapter_features.contains(wgpu::Features::SHADER_F64) {
            return None;
        }
        let supports_ray_query = adapter_features.contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY);

        let mut required_features = wgpu::Features::SHADER_F64;
        let mut required_limits = wgpu::Limits::default();
        if supports_ray_query {
            let adapter_limits = adapter.limits();
            required_features |= wgpu::Features::EXPERIMENTAL_RAY_QUERY;
            required_limits.max_blas_primitive_count = adapter_limits.max_blas_primitive_count;
            required_limits.max_blas_geometry_count = adapter_limits.max_blas_geometry_count;
            required_limits.max_tlas_instance_count = adapter_limits.max_tlas_instance_count;
            required_limits.max_acceleration_structures_per_shader_stage =
                adapter_limits.max_acceleration_structures_per_shader_stage;
            required_limits.max_buffers_and_acceleration_structures_per_shader_stage =
                adapter_limits.max_buffers_and_acceleration_structures_per_shader_stage;
        }
        let experimental_features = if supports_ray_query {
            unsafe { wgpu::ExperimentalFeatures::enabled() }
        } else {
            wgpu::ExperimentalFeatures::disabled()
        };

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("cadmpeg-gpu-device"),
            required_features,
            required_limits,
            memory_hints: wgpu::MemoryHints::Performance,
            trace: wgpu::Trace::Off,
            experimental_features,
        }))
        .ok()?;

        let bgl_compute = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("cadmpeg-gpu-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("cadmpeg-gpu-pipe-layout"),
            bind_group_layouts: &[Some(&bgl_compute)],
            immediate_size: 0,
        });

        let compile_pipe = |label: &str, wgsl: &str| -> wgpu::ComputePipeline {
            let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(wgsl.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: Some(&pipeline_layout),
                module: &sm,
                entry_point: Some("main"),
                compilation_options: wgpu::PipelineCompilationOptions::default(),
                cache: None,
            })
        };

        let rt = if supports_ray_query {
            let bgl_rt = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("cadmpeg-gpu-bgl-rt"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::AccelerationStructure {
                            vertex_return: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: false },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
            let rt_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("cadmpeg-gpu-rt-pipe-layout"),
                bind_group_layouts: &[Some(&bgl_rt)],
                immediate_size: 0,
            });
            let compile_rt_pipe = |label: &str, wgsl: &str| -> wgpu::ComputePipeline {
                let sm = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some(label),
                    source: wgpu::ShaderSource::Wgsl(wgsl.into()),
                });
                device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(label),
                    layout: Some(&rt_layout),
                    module: &sm,
                    entry_point: Some("main"),
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                    cache: None,
                })
            };
            Some(RtPipelines {
                pipe_rt_ray_mesh: compile_rt_pipe("cadmpeg-rt-ray-mesh", RT_RAY_MESH_WGSL),
                pipe_rt_point_in_solid: compile_rt_pipe(
                    "cadmpeg-rt-point-in-solid",
                    RT_POINT_IN_SOLID_WGSL,
                ),
                pipe_rt_closest_point: compile_rt_pipe(
                    "cadmpeg-rt-closest-point",
                    RT_CLOSEST_POINT_WGSL,
                ),
                pipe_rt_hausdorff: compile_rt_pipe("cadmpeg-rt-hausdorff", RT_HAUSDORFF_WGSL),
                bgl_rt,
                mesh_cache: Mutex::new(None),
                cloud_cache: Mutex::new([None, None]),
            })
        } else {
            None
        };

        Some(Self {
            device_name: info.name,
            pipe_transform: compile_pipe("cadmpeg-wgsl-transform", TRANSFORM_WGSL),
            pipe_pairwise_dist: compile_pipe("cadmpeg-wgsl-pairwise-dist", PAIRWISE_DIST_WGSL),
            pipe_bezier_eval: compile_pipe("cadmpeg-wgsl-bezier-eval", BEZIER_EVAL_WGSL),
            pipe_ray_mesh: compile_pipe("cadmpeg-wgsl-ray-mesh", RAY_MESH_WGSL),
            pipe_point_in_solid: compile_pipe("cadmpeg-wgsl-point-in-solid", POINT_IN_SOLID_WGSL),
            pipe_bvh_closest_point: compile_pipe("cadmpeg-wgsl-bvh-cp", BVH_CLOSEST_POINT_WGSL),
            pipe_kdtree_hausdorff: compile_pipe("cadmpeg-wgsl-kdtree", KDTREE_HAUSDORFF_WGSL),
            compute_mesh_cache: Mutex::new(None),
            compute_kd_cache: Mutex::new([None, None]),
            rt,
            device,
            queue,
            bgl_compute,
        })
    }

    /// Name of the active `wgpu` physical adapter.
    #[must_use]
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Returns whether hardware RT Core ray queries (`EXPERIMENTAL_RAY_QUERY`) are active.
    #[must_use]
    pub const fn supports_ray_query(&self) -> bool {
        self.rt.is_some()
    }

    fn execute_f64_pipeline(
        &self,
        pipeline: &wgpu::ComputePipeline,
        buf0_f64: &[f64],
        buf1_f64: &[f64],
        out_f64_len: usize,
        work_items: usize,
    ) -> Option<Vec<f64>> {
        if work_items == 0 || out_f64_len == 0 {
            return Some(Vec::new());
        }
        let bytes0 = slice_f64_to_bytes(buf0_f64);
        let bytes1 = slice_f64_to_bytes(buf1_f64);
        let size0 = u64::try_from(bytes0.len().max(32)).ok()?;
        let size1 = u64::try_from(bytes1.len().max(32)).ok()?;
        let out_byte_len = out_f64_len.checked_mul(8)?;
        let out_size = u64::try_from(out_byte_len.max(32)).ok()?;

        let b0 = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b0"),
            size: size0,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b1 = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b1"),
            size: size1,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b_out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let b_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b-staging"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        self.queue.write_buffer(&b0, 0, &bytes0);
        self.queue.write_buffer(&b1, 0, &bytes1);

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadmpeg-gpu-bg"),
            layout: &self.bgl_compute,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: b0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: b1.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: b_out.as_entire_binding(),
                },
            ],
        });

        let total_groups = u32::try_from(work_items).ok()?.div_ceil(64);
        if total_groups > 65_535 {
            return None;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cadmpeg-gpu-encoder"),
            });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cadmpeg-gpu-cpass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(pipeline);
            cpass.set_bind_group(0, &bg, &[]);
            cpass.dispatch_workgroups(total_groups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&b_out, 0, &b_staging, 0, out_size);
        self.queue.submit(Some(encoder.finish()));

        let slice = b_staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            tx.send(res).ok();
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let out = bytes_to_vec_f64(&mapped, out_f64_len);
        drop(mapped);
        b_staging.unmap();
        Some(out)
    }

    fn build_blas_tlas_pair(
        &self,
        verts_f32: &[[f32; 3]],
        indices_u32: &[u32],
        opaque_geometry: bool,
    ) -> Option<(wgpu::Blas, wgpu::Tlas)> {
        let vertex_count = u32::try_from(verts_f32.len()).ok()?;
        let index_count = u32::try_from(indices_u32.len()).ok()?;
        let v_bytes = slice_f32x3_to_bytes(verts_f32);
        let i_bytes = slice_u32_to_bytes(indices_u32);
        let v_size = u64::try_from(v_bytes.len().max(12)).ok()?;
        let i_size = u64::try_from(i_bytes.len().max(12)).ok()?;

        let v_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-blas-vbuf"),
            size: v_size,
            usage: wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let i_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-blas-ibuf"),
            size: i_size,
            usage: wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&v_buf, 0, &v_bytes);
        self.queue.write_buffer(&i_buf, 0, &i_bytes);

        let geom_flags = if opaque_geometry {
            wgpu::AccelerationStructureGeometryFlags::OPAQUE
        } else {
            wgpu::AccelerationStructureGeometryFlags::empty()
        };
        let size_desc = wgpu::BlasTriangleGeometrySizeDescriptor {
            vertex_format: wgpu::VertexFormat::Float32x3,
            vertex_count,
            index_format: Some(wgpu::IndexFormat::Uint32),
            index_count: Some(index_count),
            flags: geom_flags,
        };
        let blas = self.device.create_blas(
            &wgpu::CreateBlasDescriptor {
                label: Some("cadmpeg-gpu-blas"),
                flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
                update_mode: wgpu::AccelerationStructureUpdateMode::Build,
            },
            wgpu::BlasGeometrySizeDescriptors::Triangles {
                descriptors: vec![size_desc.clone()],
            },
        );
        let mut tlas = self.device.create_tlas(&wgpu::CreateTlasDescriptor {
            label: Some("cadmpeg-gpu-tlas"),
            max_instances: 1,
            flags: wgpu::AccelerationStructureFlags::PREFER_FAST_TRACE,
            update_mode: wgpu::AccelerationStructureUpdateMode::Build,
        });
        tlas[0] = Some(wgpu::TlasInstance::new(
            &blas,
            [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
            0,
            0xFF,
        ));

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cadmpeg-gpu-blas-build-encoder"),
            });
        {
            let blas_entry = wgpu::BlasBuildEntry {
                blas: &blas,
                geometry: wgpu::BlasGeometries::TriangleGeometries(vec![
                    wgpu::BlasTriangleGeometry {
                        size: &size_desc,
                        vertex_buffer: &v_buf,
                        first_vertex: 0,
                        vertex_stride: 12,
                        index_buffer: Some(&i_buf),
                        first_index: Some(0),
                        transform_buffer: None,
                        transform_buffer_offset: None,
                    },
                ]),
            };
            encoder.build_acceleration_structures(
                std::iter::once(&blas_entry),
                std::iter::once(&tlas),
            );
        }
        self.queue.submit(Some(encoder.finish()));
        Some((blas, tlas))
    }

    fn execute_cached_rt_pipeline(
        &self,
        pipeline: &wgpu::ComputePipeline,
        tlas: &wgpu::Tlas,
        b_scene: &wgpu::Buffer,
        queries_f64: &[f64],
        out_f64_len: usize,
        work_items: usize,
    ) -> Option<Vec<f64>> {
        let rt_bgl = &self.rt.as_ref()?.bgl_rt;
        if work_items == 0 || out_f64_len == 0 {
            return Some(Vec::new());
        }
        let q_bytes = slice_f64_to_bytes(queries_f64);
        let q_size = u64::try_from(q_bytes.len().max(32)).ok()?;
        let out_byte_len = out_f64_len.checked_mul(8)?;
        let out_size = u64::try_from(out_byte_len.max(32)).ok()?;

        let b_queries = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-queries"),
            size: q_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b_out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-b-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let b_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-b-staging"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&b_queries, 0, &q_bytes);

        let (b1_res, b2_res) = (b_queries.as_entire_binding(), b_scene.as_entire_binding());

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadmpeg-gpu-rt-bg"),
            layout: rt_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: tlas.as_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: b1_res,
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: b2_res,
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: b_out.as_entire_binding(),
                },
            ],
        });

        let groups = u32::try_from(work_items).ok()?.div_ceil(64);
        if groups > 65_535 {
            return None;
        }
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cadmpeg-gpu-rt-encoder"),
            });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cadmpeg-gpu-rt-cpass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(pipeline);
            cpass.set_bind_group(0, &bg, &[]);
            cpass.dispatch_workgroups(groups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&b_out, 0, &b_staging, 0, out_size);
        self.queue.submit(Some(encoder.finish()));

        let slice = b_staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            tx.send(res).ok();
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let out = bytes_to_vec_f64(&mapped, out_f64_len);
        drop(mapped);
        b_staging.unmap();
        Some(out)
    }

    /// Executes native `f64` 4x4 homogeneous point transformation via Vulkan `wgpu`.
    #[must_use]
    pub fn compute_batch_transform_4x4(
        &self,
        matrix: &[[f64; 4]; 4],
        points: &[[f64; 4]],
    ) -> Option<Vec<[f64; 4]>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        let flat_pts: Vec<f64> = points.iter().flat_map(|p| *p).collect();
        let raw = self.execute_f64_pipeline(
            &self.pipe_transform,
            matrix.as_flattened(),
            &flat_pts,
            flat_pts.len(),
            points.len(),
        )?;
        Some(
            raw.chunks_exact(4)
                .map(|c| [c[0], c[1], c[2], c[3]])
                .collect(),
        )
    }

    /// Executes native `f64` pairwise squared Euclidean distance matrix computation via Vulkan `wgpu`.
    #[must_use]
    pub fn compute_pairwise_distance_squared(
        &self,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<Vec<f64>> {
        let total = points_a.len().checked_mul(points_b.len())?;
        if total == 0 {
            return Some(Vec::new());
        }
        let flat_a: Vec<f64> = points_a.iter().flat_map(|p| [p.x, p.y, p.z, 0.0]).collect();
        let flat_b: Vec<f64> = points_b.iter().flat_map(|p| [p.x, p.y, p.z, 0.0]).collect();
        self.execute_f64_pipeline(&self.pipe_pairwise_dist, &flat_a, &flat_b, total, total)
    }

    /// Executes native `f64` bicubic rational Bézier surface patch grid evaluation via Vulkan `wgpu`.
    #[must_use]
    pub fn compute_eval_bicubic_bezier_grid(
        &self,
        control_points: &[[[f64; 4]; 4]; 4],
        u_params: &[f64],
        v_params: &[f64],
    ) -> Option<Vec<Point3>> {
        let total = u_params.len().checked_mul(v_params.len())?;
        if total == 0 {
            return Some(Vec::new());
        }
        let u_bases: Vec<[f64; 4]> = u_params
            .iter()
            .map(|&u| cpu::cubic_bernstein_basis(u))
            .collect();
        let mut flat_ug: Vec<f64> = Vec::with_capacity(u_params.len().checked_mul(16)?);
        for bu in &u_bases {
            for (c0, c1, c2, c3) in control_points[0]
                .iter()
                .zip(&control_points[1])
                .zip(&control_points[2])
                .zip(&control_points[3])
                .map(|(((a, b), c), d)| (a, b, c, d))
            {
                for (((&v0, &v1), &v2), &v3) in c0.iter().zip(c1).zip(c2).zip(c3) {
                    flat_ug
                        .push(bu[0].mul_add(v0, bu[1].mul_add(v1, bu[2].mul_add(v2, bu[3] * v3))));
                }
            }
        }
        let flat_vb: Vec<f64> = v_params
            .iter()
            .flat_map(|&v| cpu::cubic_bernstein_basis(v))
            .collect();
        let out_f64_len = total.checked_mul(4)?;
        let raw = self.execute_f64_pipeline(
            &self.pipe_bezier_eval,
            &flat_ug,
            &flat_vb,
            out_f64_len,
            total,
        )?;
        Some(
            raw.chunks_exact(4)
                .map(|p| Point3::new(p[0], p[1], p[2]))
                .collect(),
        )
    }

    fn execute_f64_pipeline_with_cached_b1(
        &self,
        pipeline: &wgpu::ComputePipeline,
        buf0_f64: &[f64],
        b1: &wgpu::Buffer,
        out_f64_len: usize,
        work_items: usize,
    ) -> Option<Vec<f64>> {
        if work_items == 0 || out_f64_len == 0 {
            return Some(Vec::new());
        }
        let total_groups = u32::try_from(work_items).ok()?.div_ceil(64);
        if total_groups > 65_535 {
            return None;
        }
        let bytes0 = slice_f64_to_bytes(buf0_f64);
        let size0 = u64::try_from(bytes0.len().max(32)).ok()?;
        let out_byte_len = out_f64_len.checked_mul(8)?;
        let out_size = u64::try_from(out_byte_len.max(32)).ok()?;

        let b0 = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b0"),
            size: size0,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b_out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b-out"),
            size: out_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let b_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-b-staging"),
            size: out_size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&b0, 0, &bytes0);

        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadmpeg-gpu-bg-cached-b1"),
            layout: &self.bgl_compute,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: b0.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: b1.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: b_out.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cadmpeg-gpu-encoder-cached-b1"),
            });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cadmpeg-gpu-cpass-cached-b1"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(pipeline);
            cpass.set_bind_group(0, &bg, &[]);
            cpass.dispatch_workgroups(total_groups, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&b_out, 0, &b_staging, 0, out_size);
        self.queue.submit(Some(encoder.finish()));

        let slice = b_staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            tx.send(res).ok();
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let result = bytes_to_vec_f64(&mapped, out_f64_len);
        drop(mapped);
        b_staging.unmap();
        Some(result)
    }

    fn build_cached_compute_mesh(
        &self,
        mesh: TriangleMeshView<'_>,
        hash: u64,
    ) -> Option<CachedVulkanComputeMesh> {
        let packed = pack_mesh_bvh_scene_and_blas(mesh)?;
        let mut flat_tris: Vec<f64> = Vec::with_capacity(mesh.triangles.len().checked_mul(12)?);
        for tri in mesh.triangles {
            let i0 = usize::try_from(tri[0]).ok()?;
            let i1 = usize::try_from(tri[1]).ok()?;
            let i2 = usize::try_from(tri[2]).ok()?;
            let (Some(&v0), Some(&v1), Some(&v2)) = (
                mesh.vertices.get(i0),
                mesh.vertices.get(i1),
                mesh.vertices.get(i2),
            ) else {
                flat_tris.extend_from_slice(&[0.0; 12]);
                continue;
            };
            flat_tris.extend_from_slice(&[
                v0.x,
                v0.y,
                v0.z,
                0.0,
                v1.x - v0.x,
                v1.y - v0.y,
                v1.z - v0.z,
                0.0,
                v2.x - v0.x,
                v2.y - v0.y,
                v2.z - v0.z,
                0.0,
            ]);
        }
        let tris_bytes = slice_f64_to_bytes(&flat_tris);
        let scene_bytes = slice_f64_to_bytes(&packed.scene);
        let tris_size = u64::try_from(tris_bytes.len().max(32)).ok()?;
        let scene_size = u64::try_from(scene_bytes.len().max(32)).ok()?;

        let b_tris = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-compute-cached-tris"),
            size: tris_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b_scene = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-compute-cached-scene"),
            size: scene_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&b_tris, 0, &tris_bytes);
        self.queue.write_buffer(&b_scene, 0, &scene_bytes);

        Some(CachedVulkanComputeMesh {
            hash,
            n_tris: mesh.triangles.len(),
            b_tris,
            b_scene,
        })
    }

    fn with_cached_compute_mesh<R>(
        &self,
        mesh: TriangleMeshView<'_>,
        f: impl FnOnce(&CachedVulkanComputeMesh) -> Option<R>,
    ) -> Option<R> {
        let hash = Self::hash_mesh(mesh);
        let mut guard = self.compute_mesh_cache.lock().ok()?;
        let needs_rebuild = match guard.as_ref() {
            Some(cached) => cached.hash != hash || cached.n_tris != mesh.triangles.len(),
            None => true,
        };
        if needs_rebuild {
            *guard = Some(self.build_cached_compute_mesh(mesh, hash)?);
        }
        f(guard.as_ref()?)
    }

    /// Executes native `f64` batch ray-mesh intersection via Vulkan `wgpu`.
    #[must_use]
    pub fn compute_ray_mesh_intersect(
        &self,
        rays: &[RayQuery3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<RayHit3>> {
        if rays.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some((0..rays.len()).map(|_| RayHit3::miss()).collect());
        }
        let flat_rays: Vec<f64> = rays
            .iter()
            .flat_map(|r| {
                [
                    r.origin.x,
                    r.origin.y,
                    r.origin.z,
                    r.t_min,
                    r.direction.x,
                    r.direction.y,
                    r.direction.z,
                    r.t_max,
                ]
            })
            .collect();
        let out_f64_len = rays.len().checked_mul(4)?;
        let raw = self.with_cached_compute_mesh(mesh, |cached| {
            self.execute_f64_pipeline_with_cached_b1(
                &self.pipe_ray_mesh,
                &flat_rays,
                &cached.b_tris,
                out_f64_len,
                rays.len(),
            )
        })?;
        Some(
            raw.chunks_exact(4)
                .map(|r| {
                    let Some(tri_idx) = f64_int_to_u32(r[1]) else {
                        return RayHit3::miss();
                    };
                    RayHit3 {
                        hit: true,
                        t: r[0],
                        triangle_index: tri_idx,
                        barycentric_u: r[2],
                        barycentric_v: r[3],
                    }
                })
                .collect(),
        )
    }

    /// Executes native `f64` batch point-in-solid parity containment via Vulkan `wgpu`.
    #[must_use]
    pub fn compute_point_in_solid_parity(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<bool>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some(vec![false; points.len()]);
        }
        let flat_pts: Vec<f64> = points.iter().flat_map(|p| [p.x, p.y, p.z, 0.0]).collect();
        let raw = self.with_cached_compute_mesh(mesh, |cached| {
            self.execute_f64_pipeline_with_cached_b1(
                &self.pipe_point_in_solid,
                &flat_pts,
                &cached.b_tris,
                points.len(),
                points.len(),
            )
        })?;
        Some(raw.into_iter().map(|v| v > 0.5).collect())
    }

    /// Executes native `f64` closest-point-on-mesh queries via Vulkan `wgpu` flattened BVH traversal.
    #[must_use]
    pub fn compute_spatial_closest_point(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<ClosestPointResult>> {
        if points.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() {
            return Some(cpu::closest_point_mesh(points, mesh));
        }
        let flat_queries: Vec<f64> = points.iter().flat_map(|q| [q.x, q.y, q.z, 0.0]).collect();
        let out_f64_len = points.len().checked_mul(8)?;
        let raw = self.with_cached_compute_mesh(mesh, |cached| {
            self.execute_f64_pipeline_with_cached_b1(
                &self.pipe_bvh_closest_point,
                &flat_queries,
                &cached.b_scene,
                out_f64_len,
                points.len(),
            )
        })?;
        Some(
            raw.chunks_exact(8)
                .map(|r| {
                    let Some(tri_idx) = f64_int_to_u32(r[4]) else {
                        return ClosestPointResult {
                            closest_point: Point3::new(r[0], r[1], r[2]),
                            distance_sq: r[3],
                            triangle_index: u32::MAX,
                        };
                    };
                    ClosestPointResult {
                        closest_point: Point3::new(r[0], r[1], r[2]),
                        distance_sq: r[3],
                        triangle_index: tri_idx,
                    }
                })
                .collect(),
        )
    }

    fn serialize_compute_kd_cloud(flat_tree: &crate::spatial::FlatKdTree3) -> (Vec<f64>, Vec<f64>) {
        let n = flat_tree.nodes().len();
        let mut flat_nodes = Vec::with_capacity(n.checked_mul(8).unwrap_or(0));
        let mut flat_queries = Vec::with_capacity(n.checked_mul(4).unwrap_or(0));
        for node in flat_tree.nodes() {
            flat_nodes.extend_from_slice(&[
                node.point.x,
                node.point.y,
                node.point.z,
                f64::from(node.split_axis),
                f64::from(node.right_child),
                f64::INFINITY,
                0.0,
                0.0,
            ]);
            flat_queries.extend_from_slice(&[node.point.x, node.point.y, node.point.z, 0.0]);
        }
        (flat_nodes, flat_queries)
    }

    fn upload_compute_kd_cloud(
        &self,
        flat_nodes: &[f64],
        flat_queries: &[f64],
        n_pts: usize,
        hash: u64,
    ) -> Option<CachedVulkanKdCloud> {
        let nodes_bytes = slice_f64_to_bytes(flat_nodes);
        let queries_bytes = slice_f64_to_bytes(flat_queries);
        let nodes_size = u64::try_from(nodes_bytes.len().max(32)).ok()?;
        let queries_size = u64::try_from(queries_bytes.len().max(32)).ok()?;

        let b_nodes = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-compute-kd-nodes"),
            size: nodes_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b_queries = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-compute-kd-queries"),
            size: queries_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&b_nodes, 0, &nodes_bytes);
        self.queue.write_buffer(&b_queries, 0, &queries_bytes);

        Some(CachedVulkanKdCloud {
            hash,
            n_pts,
            b_nodes,
            b_queries,
        })
    }

    /// Executes native `f64` symmetric Hausdorff distance via Vulkan `wgpu` KD-tree traversal.
    #[must_use]
    pub fn compute_spatial_hausdorff(
        &self,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<f64> {
        if points_a.is_empty() || points_b.is_empty() {
            return Some(0.0);
        }
        let groups_ab = u32::try_from(points_a.len()).ok()?.div_ceil(64);
        let groups_ba = u32::try_from(points_b.len()).ok()?.div_ceil(64);
        if groups_ab > 65_535 || groups_ba > 65_535 {
            return None;
        }

        let hash_a = Self::hash_point_cloud(points_a, 0);
        let hash_b = Self::hash_point_cloud(points_b, 0);
        let mut guard = self.compute_kd_cache.lock().ok()?;
        let rebuild_a = match guard[0].as_ref() {
            Some(c) => c.hash != hash_a || c.n_pts != points_a.len(),
            None => true,
        };
        let rebuild_b = match guard[1].as_ref() {
            Some(c) => c.hash != hash_b || c.n_pts != points_b.len(),
            None => true,
        };
        match (rebuild_a, rebuild_b) {
            (true, true) => {
                let ((nodes_a, queries_a), (nodes_b, queries_b)) = rayon::join(
                    || {
                        let tree = crate::spatial::KdTree3::build(points_a).flatten();
                        Self::serialize_compute_kd_cloud(&tree)
                    },
                    || {
                        let tree = crate::spatial::KdTree3::build(points_b).flatten();
                        Self::serialize_compute_kd_cloud(&tree)
                    },
                );
                guard[0] = Some(self.upload_compute_kd_cloud(
                    &nodes_a,
                    &queries_a,
                    points_a.len(),
                    hash_a,
                )?);
                guard[1] = Some(self.upload_compute_kd_cloud(
                    &nodes_b,
                    &queries_b,
                    points_b.len(),
                    hash_b,
                )?);
            }
            (true, false) => {
                let tree = crate::spatial::KdTree3::build(points_a).flatten();
                let (nodes_a, queries_a) = Self::serialize_compute_kd_cloud(&tree);
                guard[0] = Some(self.upload_compute_kd_cloud(
                    &nodes_a,
                    &queries_a,
                    points_a.len(),
                    hash_a,
                )?);
            }
            (false, true) => {
                let tree = crate::spatial::KdTree3::build(points_b).flatten();
                let (nodes_b, queries_b) = Self::serialize_compute_kd_cloud(&tree);
                guard[1] = Some(self.upload_compute_kd_cloud(
                    &nodes_b,
                    &queries_b,
                    points_b.len(),
                    hash_b,
                )?);
            }
            (false, false) => {}
        }
        let cloud_a = guard[0].as_ref()?;
        let cloud_b = guard[1].as_ref()?;

        let out_a_bytes = u64::try_from(points_a.len().checked_mul(8)?.max(32)).ok()?;
        let out_b_bytes = u64::try_from(points_b.len().checked_mul(8)?.max(32)).ok()?;
        let total_out_f64 = points_a.len().checked_add(points_b.len())?;
        let staging_bytes = out_a_bytes.checked_add(out_b_bytes)?;

        let b_out_ab = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-compute-hd-ab"),
            size: out_a_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let b_out_ba = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-compute-hd-ba"),
            size: out_b_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let b_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-compute-hd-staging"),
            size: staging_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg_ab = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadmpeg-gpu-compute-hd-bg-ab"),
            layout: &self.bgl_compute,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cloud_b.b_nodes.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cloud_a.b_queries.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: b_out_ab.as_entire_binding(),
                },
            ],
        });
        let bg_ba = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadmpeg-gpu-compute-hd-bg-ba"),
            layout: &self.bgl_compute,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cloud_a.b_nodes.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cloud_b.b_queries.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: b_out_ba.as_entire_binding(),
                },
            ],
        });

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cadmpeg-gpu-compute-hd-encoder"),
            });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cadmpeg-gpu-compute-hd-cpass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&self.pipe_kdtree_hausdorff);
            cpass.set_bind_group(0, &bg_ab, &[]);
            cpass.dispatch_workgroups(groups_ab, 1, 1);
            cpass.set_bind_group(0, &bg_ba, &[]);
            cpass.dispatch_workgroups(groups_ba, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&b_out_ab, 0, &b_staging, 0, out_a_bytes);
        encoder.copy_buffer_to_buffer(&b_out_ba, 0, &b_staging, out_a_bytes, out_b_bytes);
        self.queue.submit(Some(encoder.finish()));

        let slice = b_staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            tx.send(res).ok();
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let dists = bytes_to_vec_f64(&mapped, total_out_f64);
        drop(mapped);
        b_staging.unmap();

        let mut max_sq = 0.0_f64;
        for &d in &dists {
            if d > max_sq {
                max_sq = d;
            }
        }
        Some(max_sq.sqrt())
    }

    fn hash_mesh(mesh: TriangleMeshView<'_>) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        mesh.vertices.len().hash(&mut hasher);
        mesh.triangles.len().hash(&mut hasher);
        let v_step = (mesh.vertices.len() / 256).max(1);
        for v in mesh.vertices.iter().step_by(v_step) {
            v.x.to_bits().hash(&mut hasher);
            v.y.to_bits().hash(&mut hasher);
            v.z.to_bits().hash(&mut hasher);
        }
        if let Some(last_v) = mesh.vertices.last() {
            last_v.x.to_bits().hash(&mut hasher);
            last_v.y.to_bits().hash(&mut hasher);
            last_v.z.to_bits().hash(&mut hasher);
        }
        let t_step = (mesh.triangles.len() / 256).max(1);
        for tri in mesh.triangles.iter().step_by(t_step) {
            tri.hash(&mut hasher);
        }
        if let Some(last_t) = mesh.triangles.last() {
            last_t.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn hash_point_cloud(points: &[Point3], r_pad_bits: u64) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        points.len().hash(&mut hasher);
        r_pad_bits.hash(&mut hasher);
        let step = (points.len() / 256).max(1);
        for p in points.iter().step_by(step) {
            p.x.to_bits().hash(&mut hasher);
            p.y.to_bits().hash(&mut hasher);
            p.z.to_bits().hash(&mut hasher);
        }
        if let Some(last_p) = points.last() {
            last_p.x.to_bits().hash(&mut hasher);
            last_p.y.to_bits().hash(&mut hasher);
            last_p.z.to_bits().hash(&mut hasher);
        }
        hasher.finish()
    }

    fn build_cached_rt_mesh(
        &self,
        mesh: TriangleMeshView<'_>,
        hash: u64,
    ) -> Option<CachedVulkanRtMesh> {
        let packed = pack_mesh_bvh_scene_and_blas(mesh)?;
        let (blas_surface, tlas_surface) =
            self.build_blas_tlas_pair(&packed.verts_f32, &packed.indices_u32, false)?;
        let (blas_proxy, tlas_proxy) =
            self.build_blas_tlas_pair(&packed.proxy_verts_f32, &packed.proxy_indices_u32, false)?;
        let scene_bytes = slice_f64_to_bytes(&packed.scene);
        let scene_size = u64::try_from(scene_bytes.len().max(32)).ok()?;
        let b_scene = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-cached-scene"),
            size: scene_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&b_scene, 0, &scene_bytes);

        Some(CachedVulkanRtMesh {
            hash,
            n_tris: mesh.triangles.len(),
            _blas_surface: blas_surface,
            tlas_surface,
            _blas_proxy: blas_proxy,
            tlas_proxy,
            b_scene,
        })
    }

    fn with_cached_rt_mesh<R>(
        &self,
        rt: &RtPipelines,
        mesh: TriangleMeshView<'_>,
        f: impl FnOnce(&CachedVulkanRtMesh) -> Option<R>,
    ) -> Option<R> {
        let hash = Self::hash_mesh(mesh);
        let mut guard = rt.mesh_cache.lock().ok()?;
        let needs_rebuild = match guard.as_ref() {
            Some(cached) => cached.hash != hash || cached.n_tris != mesh.triangles.len(),
            None => true,
        };
        if needs_rebuild {
            *guard = Some(self.build_cached_rt_mesh(mesh, hash)?);
        }
        f(guard.as_ref()?)
    }

    /// Executes hardware RT Core (`wgpu_ray_query` + `SHADER_F64`) ray-mesh intersection.
    #[must_use]
    pub fn rt_core_ray_mesh_intersect(
        &self,
        rays: &[RayQuery3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<RayHit3>> {
        let rt = self.rt.as_ref()?;
        if rays.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() || mesh.vertices.is_empty() {
            return Some((0..rays.len()).map(|_| RayHit3::miss()).collect());
        }
        let flat_rays: Vec<f64> = rays
            .iter()
            .flat_map(|r| {
                [
                    r.origin.x,
                    r.origin.y,
                    r.origin.z,
                    r.t_min,
                    r.direction.x,
                    r.direction.y,
                    r.direction.z,
                    r.t_max,
                ]
            })
            .collect();
        let out_f64_len = rays.len().checked_mul(4)?;
        let raw = self.with_cached_rt_mesh(rt, mesh, |cached| {
            self.execute_cached_rt_pipeline(
                &rt.pipe_rt_ray_mesh,
                &cached.tlas_surface,
                &cached.b_scene,
                &flat_rays,
                out_f64_len,
                rays.len(),
            )
        })?;
        Some(
            raw.chunks_exact(4)
                .map(|r| {
                    let Some(tri_idx) = f64_int_to_u32(r[1]) else {
                        return RayHit3::miss();
                    };
                    RayHit3 {
                        hit: true,
                        t: r[0],
                        triangle_index: tri_idx,
                        barycentric_u: r[2],
                        barycentric_v: r[3],
                    }
                })
                .collect(),
        )
    }

    /// Executes hardware RT Core (`wgpu_ray_query` + `SHADER_F64`) point-in-solid parity containment.
    #[must_use]
    pub fn rt_core_point_in_solid_parity(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<bool>> {
        let rt = self.rt.as_ref()?;
        if points.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() || mesh.vertices.is_empty() {
            return Some(vec![false; points.len()]);
        }
        let flat_pts: Vec<f64> = points.iter().flat_map(|p| [p.x, p.y, p.z, 0.0]).collect();
        let raw = self.with_cached_rt_mesh(rt, mesh, |cached| {
            self.execute_cached_rt_pipeline(
                &rt.pipe_rt_point_in_solid,
                &cached.tlas_surface,
                &cached.b_scene,
                &flat_pts,
                points.len(),
                points.len(),
            )
        })?;
        Some(raw.into_iter().map(|v| v > 0.5).collect())
    }

    /// Executes hardware RT Core (`wgpu_ray_query` + `SHADER_F64`) closest-point-on-mesh projection.
    #[must_use]
    pub fn rt_core_closest_point_mesh(
        &self,
        points: &[Point3],
        mesh: TriangleMeshView<'_>,
    ) -> Option<Vec<ClosestPointResult>> {
        let rt = self.rt.as_ref()?;
        if points.is_empty() {
            return Some(Vec::new());
        }
        if mesh.triangles.is_empty() || mesh.vertices.is_empty() {
            return Some(cpu::closest_point_mesh(points, mesh));
        }
        let flat_queries: Vec<f64> = points.iter().flat_map(|q| [q.x, q.y, q.z, 0.0]).collect();
        let out_f64_len = points.len().checked_mul(8)?;
        let raw = self.with_cached_rt_mesh(rt, mesh, |cached| {
            self.execute_cached_rt_pipeline(
                &rt.pipe_rt_closest_point,
                &cached.tlas_proxy,
                &cached.b_scene,
                &flat_queries,
                out_f64_len,
                points.len(),
            )
        })?;
        Some(
            raw.chunks_exact(8)
                .map(|r| {
                    let Some(tri_idx) = f64_int_to_u32(r[4]) else {
                        return ClosestPointResult {
                            closest_point: Point3::new(r[0], r[1], r[2]),
                            distance_sq: r[3],
                            triangle_index: u32::MAX,
                        };
                    };
                    ClosestPointResult {
                        closest_point: Point3::new(r[0], r[1], r[2]),
                        distance_sq: r[3],
                        triangle_index: tri_idx,
                    }
                })
                .collect(),
        )
    }

    fn estimate_cloud_r_pad(points_a: &[Point3], points_b: &[Point3]) -> f64 {
        let min_len = points_a.len().min(points_b.len());
        let step_corresp = (min_len / 32).max(1);
        let mut max_corresp_sq = 0.0_f64;
        for idx in (0..min_len).step_by(step_corresp) {
            let pa = points_a[idx];
            let pb = points_b[idx];
            let dx = pa.x - pb.x;
            let dy = pa.y - pb.y;
            let dz = pa.z - pb.z;
            let d2 = dx * dx + dy * dy + dz * dz;
            if d2.is_finite() && d2 > max_corresp_sq {
                max_corresp_sq = d2;
            }
        }
        let mut sum_adj_sq = 0.0_f64;
        let mut adj_cnt = 0_u32;
        if points_a.len() > 1 {
            let step_adj = ((points_a.len() - 1) / 32).max(1);
            for idx in (0..(points_a.len() - 1)).step_by(step_adj) {
                let p0 = points_a[idx];
                let p1 = points_a[idx + 1];
                let dx = p1.x - p0.x;
                let dy = p1.y - p0.y;
                let dz = p1.z - p0.z;
                let d2 = dx * dx + dy * dy + dz * dz;
                if d2.is_finite() {
                    sum_adj_sq += d2;
                    adj_cnt += 1;
                }
            }
        }
        let avg_adj = if adj_cnt > 0 {
            (sum_adj_sq / f64::from(adj_cnt)).sqrt()
        } else {
            0.1
        };
        let corresp = max_corresp_sq.sqrt();
        if corresp > 0.0 && corresp <= avg_adj * 8.0 {
            (corresp * 1.15).max(1.0e-4)
        } else {
            (avg_adj * 1.5).max(corresp.min(1.0)).max(1.0e-4)
        }
    }

    fn build_cached_rt_cloud_from_tree(
        &self,
        flat_tree: &crate::spatial::FlatKdTree3,
        r_pad: f64,
        hash: u64,
    ) -> Option<CachedVulkanRtCloud> {
        let n_pts = flat_tree.nodes().len();
        let n_pts_u32 = u32::try_from(n_pts).ok()?;
        let r_pad_aabb = r_pad * 1.002 + 1.0e-5;
        let r_f32 = f64_to_blas_f32(r_pad_aabb).max(1.0e-4);
        let r3_f32 = r_f32 * 3.0;

        let mut verts_f32: Vec<[f32; 3]> = Vec::with_capacity(n_pts.checked_mul(3)?);
        let mut indices_u32: Vec<u32> = Vec::with_capacity(n_pts.checked_mul(3)?);
        let mut flat_nodes: Vec<f64> =
            Vec::with_capacity((n_pts.checked_mul(2)? + 1).checked_mul(4)?);
        let mut flat_queries: Vec<f64> = Vec::with_capacity(n_pts.checked_mul(4)?);

        flat_nodes.extend_from_slice(&[
            f64::from(n_pts_u32),
            f64::INFINITY,
            r_pad_aabb,
            r_pad * r_pad,
        ]);

        for (idx, n) in flat_tree.nodes().iter().enumerate() {
            let p = n.point;
            let base_v = u32::try_from(idx.checked_mul(3)?).ok()?;
            let px = f64_to_blas_f32(p.x);
            let py = f64_to_blas_f32(p.y);
            let pz = f64_to_blas_f32(p.z);
            verts_f32.push([px, py - r_f32, pz - r_f32]);
            verts_f32.push([px, py + r3_f32, pz - r_f32]);
            verts_f32.push([px, py - r_f32, pz + r3_f32]);
            indices_u32.extend_from_slice(&[base_v, base_v + 1, base_v + 2]);
            flat_nodes.extend_from_slice(&[
                p.x,
                p.y,
                p.z,
                f64::from(n.split_axis),
                f64::from(n.right_child),
                0.0,
                0.0,
                0.0,
            ]);
            flat_queries.extend_from_slice(&[p.x, p.y, p.z, 0.0]);
        }

        let (blas, tlas) = self.build_blas_tlas_pair(&verts_f32, &indices_u32, false)?;
        let nodes_bytes = slice_f64_to_bytes(&flat_nodes);
        let queries_bytes = slice_f64_to_bytes(&flat_queries);
        let nodes_size = u64::try_from(nodes_bytes.len().max(32)).ok()?;
        let queries_size = u64::try_from(queries_bytes.len().max(32)).ok()?;

        let b_nodes = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-cloud-nodes"),
            size: nodes_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let b_queries = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-cloud-queries"),
            size: queries_size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&b_nodes, 0, &nodes_bytes);
        self.queue.write_buffer(&b_queries, 0, &queries_bytes);

        Some(CachedVulkanRtCloud {
            hash,
            n_pts,
            _blas: blas,
            tlas,
            b_nodes,
            b_queries,
        })
    }

    /// Executes hardware RT Core (`wgpu_ray_query` + `SHADER_F64`) symmetric Hausdorff distance.
    #[must_use]
    pub fn rt_core_spatial_hausdorff(
        &self,
        points_a: &[Point3],
        points_b: &[Point3],
    ) -> Option<f64> {
        let rt = self.rt.as_ref()?;
        if points_a.is_empty() || points_b.is_empty() {
            return Some(0.0);
        }
        let r_pad = Self::estimate_cloud_r_pad(points_a, points_b);
        let r_pad_bits = r_pad.to_bits();
        let hash_a = Self::hash_point_cloud(points_a, r_pad_bits);
        let hash_b = Self::hash_point_cloud(points_b, r_pad_bits);

        let mut guard = rt.cloud_cache.lock().ok()?;
        let rebuild_a = match guard[0].as_ref() {
            Some(c) => c.hash != hash_a || c.n_pts != points_a.len(),
            None => true,
        };
        let rebuild_b = match guard[1].as_ref() {
            Some(c) => c.hash != hash_b || c.n_pts != points_b.len(),
            None => true,
        };
        match (rebuild_a, rebuild_b) {
            (true, true) => {
                let (tree_a, tree_b) = rayon::join(
                    || crate::spatial::KdTree3::build(points_a).flatten(),
                    || crate::spatial::KdTree3::build(points_b).flatten(),
                );
                guard[0] = Some(self.build_cached_rt_cloud_from_tree(&tree_a, r_pad, hash_a)?);
                guard[1] = Some(self.build_cached_rt_cloud_from_tree(&tree_b, r_pad, hash_b)?);
            }
            (true, false) => {
                let tree_a = crate::spatial::KdTree3::build(points_a).flatten();
                guard[0] = Some(self.build_cached_rt_cloud_from_tree(&tree_a, r_pad, hash_a)?);
            }
            (false, true) => {
                let tree_b = crate::spatial::KdTree3::build(points_b).flatten();
                guard[1] = Some(self.build_cached_rt_cloud_from_tree(&tree_b, r_pad, hash_b)?);
            }
            (false, false) => {}
        }
        let cloud_a = guard[0].as_ref()?;
        let cloud_b = guard[1].as_ref()?;

        let out_a_bytes = u64::try_from(points_a.len().checked_mul(8)?.max(32)).ok()?;
        let out_b_bytes = u64::try_from(points_b.len().checked_mul(8)?.max(32)).ok()?;
        let total_out_f64 = points_a.len().checked_add(points_b.len())?;
        let staging_bytes = out_a_bytes.checked_add(out_b_bytes)?;

        let b_out_ab = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-hd-ab"),
            size: out_a_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let b_out_ba = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-hd-ba"),
            size: out_b_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let b_staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cadmpeg-gpu-rt-hd-staging"),
            size: staging_bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bg_ab = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadmpeg-gpu-rt-hd-bg-ab"),
            layout: &rt.bgl_rt,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cloud_b.tlas.as_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cloud_b.b_nodes.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: cloud_a.b_queries.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: b_out_ab.as_entire_binding(),
                },
            ],
        });
        let bg_ba = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cadmpeg-gpu-rt-hd-bg-ba"),
            layout: &rt.bgl_rt,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: cloud_a.tlas.as_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: cloud_a.b_nodes.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: cloud_b.b_queries.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: b_out_ba.as_entire_binding(),
                },
            ],
        });

        let groups_ab = u32::try_from(points_a.len()).ok()?.div_ceil(64);
        let groups_ba = u32::try_from(points_b.len()).ok()?.div_ceil(64);

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("cadmpeg-gpu-rt-hd-encoder"),
            });
        {
            let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("cadmpeg-gpu-rt-hd-cpass"),
                timestamp_writes: None,
            });
            cpass.set_pipeline(&rt.pipe_rt_hausdorff);
            cpass.set_bind_group(0, &bg_ab, &[]);
            cpass.dispatch_workgroups(groups_ab, 1, 1);
            cpass.set_bind_group(0, &bg_ba, &[]);
            cpass.dispatch_workgroups(groups_ba, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&b_out_ab, 0, &b_staging, 0, out_a_bytes);
        encoder.copy_buffer_to_buffer(&b_out_ba, 0, &b_staging, out_a_bytes, out_b_bytes);
        self.queue.submit(Some(encoder.finish()));

        let slice = b_staging.slice(..);
        let (tx, rx) = mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            tx.send(res).ok();
        });
        self.device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        rx.recv().ok()?.ok()?;

        let mapped = slice.get_mapped_range().ok()?;
        let dists = bytes_to_vec_f64(&mapped, total_out_f64);
        drop(mapped);
        b_staging.unmap();

        let mut max_sq = 0.0_f64;
        for &d in &dists {
            if d > max_sq {
                max_sq = d;
            }
        }
        Some(max_sq.sqrt())
    }
}

struct PackedBvhSceneAndBlas {
    scene: Vec<f64>,
    verts_f32: Vec<[f32; 3]>,
    indices_u32: Vec<u32>,
    proxy_verts_f32: Vec<[f32; 3]>,
    proxy_indices_u32: Vec<u32>,
}

fn pack_mesh_bvh_scene_and_blas(mesh: TriangleMeshView<'_>) -> Option<PackedBvhSceneAndBlas> {
    let flat_bvh = crate::spatial::MeshBvh3::build(mesh).flatten();
    let n_nodes = flat_bvh.nodes().len();
    let n_tris = mesh.triangles.len();
    let n_tris_u32 = u32::try_from(n_tris).ok()?;

    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for v in mesh.vertices {
        if v.x.is_finite() && v.y.is_finite() && v.z.is_finite() {
            min_x = min_x.min(v.x);
            min_y = min_y.min(v.y);
            min_z = min_z.min(v.z);
            max_x = max_x.max(v.x);
            max_y = max_y.max(v.y);
            max_z = max_z.max(v.z);
        }
    }
    let diag = if min_x <= max_x {
        let dx = max_x - min_x;
        let dy = max_y - min_y;
        let dz = max_z - min_z;
        (dx * dx + dy * dy + dz * dz).sqrt().max(1.0e-3)
    } else {
        1.0
    };
    let sqrt_tris = f64::from(n_tris_u32.max(1)).sqrt();
    let r_pad = (diag * 0.42 / sqrt_tris).max(1.0e-3);
    let r_pad_sq = r_pad * r_pad;
    let r_pad_aabb = r_pad * 1.002 + 1.0e-5;

    let mut max_hx = 0.0_f64;
    for tri in mesh.triangles {
        let (Ok(i0), Ok(i1), Ok(i2)) = (
            usize::try_from(tri[0]),
            usize::try_from(tri[1]),
            usize::try_from(tri[2]),
        ) else {
            continue;
        };
        let (Some(&v0), Some(&v1), Some(&v2)) = (
            mesh.vertices.get(i0),
            mesh.vertices.get(i1),
            mesh.vertices.get(i2),
        ) else {
            continue;
        };
        if v0.x.is_finite() && v1.x.is_finite() && v2.x.is_finite() {
            let tx_min = v0.x.min(v1.x).min(v2.x);
            let tx_max = v0.x.max(v1.x).max(v2.x);
            max_hx = max_hx.max(0.5 * (tx_max - tx_min));
        }
    }
    let r_reach = r_pad_aabb + max_hx;

    let mut scene: Vec<f64> = Vec::with_capacity(4 + n_nodes * 8 + n_tris * 12);
    scene.extend_from_slice(&[
        f64::from(u32::try_from(n_nodes).ok()?),
        f64::from(n_tris_u32),
        f64::INFINITY,
        r_pad_sq,
    ]);
    for node in flat_bvh.nodes() {
        let tri_f = if node.triangle_index == u32::MAX {
            -1.0
        } else {
            f64::from(node.triangle_index)
        };
        scene.extend_from_slice(&[
            node.aabb_min.x,
            node.aabb_min.y,
            node.aabb_min.z,
            tri_f,
            node.aabb_max.x,
            node.aabb_max.y,
            node.aabb_max.z,
            f64::from(node.right_child),
        ]);
    }
    let mut verts_f32: Vec<[f32; 3]> = mesh
        .vertices
        .iter()
        .map(|v| {
            [
                f64_to_blas_f32(v.x),
                f64_to_blas_f32(v.y),
                f64_to_blas_f32(v.z),
            ]
        })
        .collect();
    if verts_f32.is_empty() {
        verts_f32.push([0.0, 0.0, 0.0]);
    }
    let mut indices_u32: Vec<u32> = Vec::with_capacity(n_tris.checked_mul(3)?);
    let mut proxy_verts_f32: Vec<[f32; 3]> = Vec::with_capacity(n_tris.checked_mul(3)?.max(3));
    let mut proxy_indices_u32: Vec<u32> = Vec::with_capacity(n_tris.checked_mul(3)?.max(3));

    for (t_idx, tri) in mesh.triangles.iter().enumerate() {
        let base_pv = u32::try_from(t_idx.checked_mul(3)?).ok()?;
        let i0 = usize::try_from(tri[0]).ok()?;
        let i1 = usize::try_from(tri[1]).ok()?;
        let i2 = usize::try_from(tri[2]).ok()?;
        let (Some(&v0), Some(&v1), Some(&v2)) = (
            mesh.vertices.get(i0),
            mesh.vertices.get(i1),
            mesh.vertices.get(i2),
        ) else {
            scene.extend_from_slice(&[0.0; 12]);
            indices_u32.extend_from_slice(&[0, 0, 0]);
            proxy_verts_f32.extend_from_slice(&[[0.0, 0.0, 0.0]; 3]);
            proxy_indices_u32.extend_from_slice(&[0, 0, 0]);
            continue;
        };
        let valid = v0.x.is_finite()
            && v0.y.is_finite()
            && v0.z.is_finite()
            && v1.x.is_finite()
            && v1.y.is_finite()
            && v1.z.is_finite()
            && v2.x.is_finite()
            && v2.y.is_finite()
            && v2.z.is_finite();
        scene.extend_from_slice(&[
            v0.x, v0.y, v0.z, 1.0, v1.x, v1.y, v1.z, r_reach, v2.x, v2.y, v2.z, 1.0,
        ]);
        indices_u32.extend_from_slice(&[tri[0], tri[1], tri[2]]);
        if valid {
            let cx = 0.5 * (v0.x.min(v1.x).min(v2.x) + v0.x.max(v1.x).max(v2.x));
            let y0 = v0.y.min(v1.y).min(v2.y) - r_pad_aabb;
            let z0 = v0.z.min(v1.z).min(v2.z) - r_pad_aabb;
            let dy2 = ((v0.y.max(v1.y).max(v2.y) + r_pad_aabb) - y0) * 2.0;
            let dz2 = ((v0.z.max(v1.z).max(v2.z) + r_pad_aabb) - z0) * 2.0;
            let cx_f32 = f64_to_blas_f32(cx);
            let y0_f32 = f64_to_blas_f32(y0);
            let z0_f32 = f64_to_blas_f32(z0);
            let y1_f32 = f64_to_blas_f32(y0 + dy2);
            let z1_f32 = f64_to_blas_f32(z0 + dz2);
            proxy_verts_f32.push([cx_f32, y0_f32, z0_f32]);
            proxy_verts_f32.push([cx_f32, y1_f32, z0_f32]);
            proxy_verts_f32.push([cx_f32, y0_f32, z1_f32]);
            proxy_indices_u32.extend_from_slice(&[base_pv, base_pv + 1, base_pv + 2]);
        } else {
            proxy_verts_f32.extend_from_slice(&[[0.0, 0.0, 0.0]; 3]);
            proxy_indices_u32.extend_from_slice(&[0, 0, 0]);
        }
    }
    if proxy_verts_f32.is_empty() {
        proxy_verts_f32.extend_from_slice(&[[0.0, 0.0, 0.0]; 3]);
        proxy_indices_u32.extend_from_slice(&[0, 0, 0]);
    }
    Some(PackedBvhSceneAndBlas {
        scene,
        verts_f32,
        indices_u32,
        proxy_verts_f32,
        proxy_indices_u32,
    })
}

fn f64_int_to_u32(val: f64) -> Option<u32> {
    if val < 0.0 || !val.is_finite() {
        return None;
    }
    if val == 0.0 {
        return Some(0);
    }
    let bits = val.to_bits();
    let exp = u32::try_from((bits >> 52) & 0x7ff).ok()?;
    if !(1023..=1054).contains(&exp) {
        return None;
    }
    let shift = exp - 1023;
    let mantissa = (bits & 0x000f_ffff_ffff_ffff) | (1_u64 << 52);
    let int_val = mantissa >> (52 - shift);
    if int_val == u64::from(u32::MAX) {
        return None;
    }
    u32::try_from(int_val).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VULKAN_PARITY_TOLERANCE: f64 = 1.0e-11;

    #[test]
    fn vulkan_transform_and_1e20_extreme_magnitude_match_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        assert!(!vk.device_name().is_empty());

        let matrix = [
            [1.25, -0.5, 0.25, 4.0],
            [0.5, 2.0, -0.75, -2.5],
            [-0.25, 0.125, 1.5, 1.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let points: Vec<[f64; 4]> = (0..19)
            .map(|i| {
                let f = f64::from(i);
                [f * 0.3 - 1.0, f * -0.7 + 2.0, f * 0.5, 1.0]
            })
            .collect();
        let expected = cpu::batch_transform_4x4(&matrix, &points);
        let actual = vk
            .compute_batch_transform_4x4(&matrix, &points)
            .expect("Vulkan f64 transform");
        for (e, a) in expected.iter().zip(&actual) {
            for c in 0..4 {
                assert!((e[c] - a[c]).abs() < VULKAN_PARITY_TOLERANCE);
            }
        }

        let scale = 1.0e20_f64;
        let big_mat = [
            [scale, 0.0, 0.0, 0.0],
            [0.0, scale, 0.0, 0.0],
            [0.0, 0.0, scale, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let big_pts = [[scale, -scale, 0.5 * scale, 1.0]];
        let big_out = vk
            .compute_batch_transform_4x4(&big_mat, &big_pts)
            .expect("Vulkan 1e20 transform");
        assert_eq!(big_out[0][0], 1.0e40);
        assert!(!big_out[0][0].is_nan());
    }

    #[test]
    fn vulkan_pairwise_distance_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        let pts_a: Vec<Point3> = (0..17)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(1000.0 + f * 0.25, -500.0 + f * 0.5, 250.0 - f * 0.125)
            })
            .collect();
        let pts_b: Vec<Point3> = (0..13)
            .map(|j| {
                let f = f64::from(j);
                Point3::new(1000.5 + f * 0.125, -499.0 - f * 0.25, 249.5 + f * 0.375)
            })
            .collect();
        let expected = cpu::pairwise_distance_squared(&pts_a, &pts_b);
        let actual = vk
            .compute_pairwise_distance_squared(&pts_a, &pts_b)
            .expect("Vulkan f64 pairwise dist");
        assert_eq!(expected.len(), actual.len());
        for (&e, &a) in expected.iter().zip(&actual) {
            assert!((e - a).abs() < VULKAN_PARITY_TOLERANCE);
        }
    }

    #[test]
    fn vulkan_bezier_eval_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        let mut ctrl = [[[0.0_f64; 4]; 4]; 4];
        for (i, row) in ctrl.iter_mut().enumerate() {
            for (j, pole) in row.iter_mut().enumerate() {
                let u = f64::from(u32::try_from(i).unwrap_or(0));
                let v = f64::from(u32::try_from(j).unwrap_or(0));
                let w = 1.0 + 0.1 * (u + v);
                *pole = [u * w, v * w, (u * u - v) * 0.25 * w, w];
            }
        }
        let u_params: Vec<f64> = (0..11).map(|i| f64::from(i) / 10.0).collect();
        let v_params: Vec<f64> = (0..9).map(|j| f64::from(j) / 8.0).collect();
        let expected = cpu::eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params);
        let actual = vk
            .compute_eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params)
            .expect("Vulkan f64 Bezier eval");
        assert_eq!(expected.len(), actual.len());
        for (e, a) in expected.iter().zip(&actual) {
            assert!((e.x - a.x).abs() < VULKAN_PARITY_TOLERANCE);
            assert!((e.y - a.y).abs() < VULKAN_PARITY_TOLERANCE);
            assert!((e.z - a.z).abs() < VULKAN_PARITY_TOLERANCE);
        }
    }

    #[test]
    fn vulkan_ray_mesh_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
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
        let rays: Vec<RayQuery3> = (0_u32..25)
            .map(|i| {
                let f = f64::from(i) * 0.1;
                RayQuery3 {
                    origin: Point3::new(0.1 + f, 0.2, -1.0),
                    direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    t_min: 0.0,
                    t_max: 10.0,
                }
            })
            .collect();
        let expected_rays = cpu::ray_mesh_intersect(&rays, mesh);
        let actual_rays = vk
            .compute_ray_mesh_intersect(&rays, mesh)
            .expect("Vulkan f64 ray-mesh");
        for (e, a) in expected_rays.iter().zip(&actual_rays) {
            assert_eq!(e.hit, a.hit);
            assert_eq!(e.triangle_index, a.triangle_index);
            if e.hit {
                assert!((e.t - a.t).abs() < VULKAN_PARITY_TOLERANCE);
                assert!((e.barycentric_u - a.barycentric_u).abs() < VULKAN_PARITY_TOLERANCE);
                assert!((e.barycentric_v - a.barycentric_v).abs() < VULKAN_PARITY_TOLERANCE);
            }
        }
    }

    #[test]
    fn vulkan_point_in_solid_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
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
        let pts = [
            Point3::new(0.25, 0.25, 0.25),
            Point3::new(0.1, 0.1, 0.1),
            Point3::new(1.5, 1.5, 1.5),
            Point3::new(-0.5, 0.2, 0.2),
        ];
        let expected_parity = cpu::point_in_solid_parity(&pts, mesh);
        let actual_parity = vk
            .compute_point_in_solid_parity(&pts, mesh)
            .expect("Vulkan f64 point-in-solid");
        assert_eq!(expected_parity, actual_parity);
    }

    #[test]
    fn vulkan_spatial_closest_point_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        let pts_a: Vec<Point3> = (0_u32..64)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 2.0, (f * 0.3).sin(), (f * 0.2).cos())
            })
            .collect();
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
        let expected_cp = cpu::closest_point_mesh(&pts_a, mesh);
        let actual_cp = vk
            .compute_spatial_closest_point(&pts_a, mesh)
            .expect("Vulkan f64 spatial closest point");
        for (e, a) in expected_cp.iter().zip(&actual_cp) {
            assert!((e.distance_sq - a.distance_sq).abs() < VULKAN_PARITY_TOLERANCE);
            assert!((e.closest_point.x - a.closest_point.x).abs() < VULKAN_PARITY_TOLERANCE);
            assert!((e.closest_point.y - a.closest_point.y).abs() < VULKAN_PARITY_TOLERANCE);
            assert!((e.closest_point.z - a.closest_point.z).abs() < VULKAN_PARITY_TOLERANCE);
        }
    }

    #[test]
    fn vulkan_spatial_hausdorff_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        let pts_a: Vec<Point3> = (0_u32..64)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 2.0, (f * 0.3).sin(), (f * 0.2).cos())
            })
            .collect();
        let pts_b: Vec<Point3> = (0_u32..53)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 1.92, (f * 0.3).sin() - 0.04, (f * 0.2).cos())
            })
            .collect();
        let expected_h = crate::spatial::kd_hausdorff_distance(&pts_a, &pts_b);
        let actual_h = vk
            .compute_spatial_hausdorff(&pts_a, &pts_b)
            .expect("Vulkan f64 spatial Hausdorff");
        assert!((expected_h - actual_h).abs() < VULKAN_PARITY_TOLERANCE);

        let close_a = [Point3::new(0.0, 0.0, 0.0), Point3::new(1000.0, 0.0, 0.0)];
        let close_b = [Point3::new(0.0, 0.0, 0.0), Point3::new(1000.0, 1.0e-6, 0.0)];
        let close_h = vk
            .compute_spatial_hausdorff(&close_a, &close_b)
            .expect("Vulkan close-points Hausdorff");
        assert!((close_h - 1.0e-6).abs() < VULKAN_PARITY_TOLERANCE);
    }

    #[test]
    fn vulkan_rt_core_ray_mesh_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        if !vk.supports_ray_query() {
            return;
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
        let rays: Vec<RayQuery3> = (0_u32..25)
            .map(|i| {
                let f = f64::from(i) * 0.1;
                RayQuery3 {
                    origin: Point3::new(0.1 + f, 0.2, -1.0),
                    direction: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    t_min: 0.0,
                    t_max: 10.0,
                }
            })
            .collect();
        let expected_rays = cpu::ray_mesh_intersect(&rays, mesh);
        let actual_rays = vk
            .rt_core_ray_mesh_intersect(&rays, mesh)
            .expect("RT Core ray-mesh");
        for (e, a) in expected_rays.iter().zip(&actual_rays) {
            assert_eq!(e.hit, a.hit);
            assert_eq!(e.triangle_index, a.triangle_index);
            if e.hit {
                assert!((e.t - a.t).abs() < VULKAN_PARITY_TOLERANCE);
                assert!((e.barycentric_u - a.barycentric_u).abs() < VULKAN_PARITY_TOLERANCE);
                assert!((e.barycentric_v - a.barycentric_v).abs() < VULKAN_PARITY_TOLERANCE);
            }
        }
    }

    #[test]
    fn vulkan_rt_core_point_in_solid_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        if !vk.supports_ray_query() {
            return;
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
        let pts = [
            Point3::new(0.25, 0.25, 0.25),
            Point3::new(0.1, 0.1, 0.1),
            Point3::new(1.5, 1.5, 1.5),
            Point3::new(-0.5, 0.2, 0.2),
        ];
        let expected_parity = cpu::point_in_solid_parity(&pts, mesh);
        let actual_parity = vk
            .rt_core_point_in_solid_parity(&pts, mesh)
            .expect("RT Core point-in-solid");
        assert_eq!(expected_parity, actual_parity);
    }

    #[test]
    fn vulkan_rt_core_closest_point_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        if !vk.supports_ray_query() {
            return;
        }
        let pts_a: Vec<Point3> = (0_u32..64)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 2.0, (f * 0.3).sin(), (f * 0.2).cos())
            })
            .collect();
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
        let expected_cp = cpu::closest_point_mesh(&pts_a, mesh);
        let actual_cp = vk
            .rt_core_closest_point_mesh(&pts_a, mesh)
            .expect("RT Core spatial closest point");
        for (e, a) in expected_cp.iter().zip(&actual_cp) {
            assert!((e.distance_sq - a.distance_sq).abs() < VULKAN_PARITY_TOLERANCE);
            assert!((e.closest_point.x - a.closest_point.x).abs() < VULKAN_PARITY_TOLERANCE);
            assert!((e.closest_point.y - a.closest_point.y).abs() < VULKAN_PARITY_TOLERANCE);
            assert!((e.closest_point.z - a.closest_point.z).abs() < VULKAN_PARITY_TOLERANCE);
        }
    }

    #[test]
    fn vulkan_rt_core_hausdorff_matches_cpu() {
        let Some(vk) = global_vulkan() else {
            return;
        };
        if !vk.supports_ray_query() {
            return;
        }
        let pts_a: Vec<Point3> = (0_u32..64)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 2.0, (f * 0.3).sin(), (f * 0.2).cos())
            })
            .collect();
        let pts_b: Vec<Point3> = (0_u32..53)
            .map(|i| {
                let f = f64::from(i);
                Point3::new(f * 0.15 - 1.92, (f * 0.3).sin() - 0.04, (f * 0.2).cos())
            })
            .collect();
        let expected_h = crate::spatial::kd_hausdorff_distance(&pts_a, &pts_b);
        let actual_h = vk
            .rt_core_spatial_hausdorff(&pts_a, &pts_b)
            .expect("RT Core spatial Hausdorff");
        assert!((expected_h - actual_h).abs() < VULKAN_PARITY_TOLERANCE);

        let close_a = [Point3::new(0.0, 0.0, 0.0), Point3::new(1000.0, 0.0, 0.0)];
        let close_b = [Point3::new(0.0, 0.0, 0.0), Point3::new(1000.0, 1.0e-6, 0.0)];
        let close_h = vk
            .rt_core_spatial_hausdorff(&close_a, &close_b)
            .expect("RT Core close-points Hausdorff");
        assert!((close_h - 1.0e-6).abs() < VULKAN_PARITY_TOLERANCE);
    }
}
