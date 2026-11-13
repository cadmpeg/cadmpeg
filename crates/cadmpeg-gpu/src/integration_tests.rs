// SPDX-License-Identifier: Apache-2.0
//! End-to-end integration and CPU-parity tests for `cadmpeg-gpu`.

use cadmpeg_ir::math::{Point3, Vector3};

use crate::{
    closest_points_on_mesh, cpu, eval_bicubic_bezier_grid, global_dispatcher, hausdorff_distance,
    intersect_rays_mesh, pairwise_squared_distances, points_in_solid_parity, transform_points_4x4,
    BackendKind, OperationKind, RayQuery3, TriangleMeshView,
};

const PARITY_TOLERANCE: f64 = 1.0e-11;

fn closed_octahedron_mesh() -> ([Point3; 6], [[u32; 3]; 8]) {
    let vertices = [
        Point3::new(2.0, 0.0, 0.0),
        Point3::new(-2.0, 0.0, 0.0),
        Point3::new(0.0, 2.0, 0.0),
        Point3::new(0.0, -2.0, 0.0),
        Point3::new(0.0, 0.0, 2.0),
        Point3::new(0.0, 0.0, -2.0),
    ];
    let triangles = [
        [0, 2, 4],
        [2, 1, 4],
        [1, 3, 4],
        [3, 0, 4],
        [2, 0, 5],
        [1, 2, 5],
        [3, 1, 5],
        [0, 3, 5],
    ];
    (vertices, triangles)
}

#[test]
fn dispatcher_calibration_discovers_backends_and_builds_schedules() {
    let disp = global_dispatcher();
    let report = disp.report();
    assert!(report.available_backends.contains(&BackendKind::Cpu));
    assert!(!report.samples.is_empty());

    for op in OperationKind::ALL {
        let sched = disp.schedule(op);
        assert_eq!(sched.operation, op);
        assert!(!sched.intervals.is_empty());
        assert_eq!(
            sched
                .intervals
                .first()
                .expect("schedule has first interval")
                .min_size,
            0
        );
        assert_eq!(
            sched
                .intervals
                .last()
                .expect("schedule has last interval")
                .max_size,
            usize::MAX
        );
    }
}

#[test]
fn all_backends_match_cpu_for_batch_transform_across_sizes() {
    let disp = global_dispatcher();
    let matrix = [
        [0.85, -0.35, 0.4, 12.5],
        [0.45, 0.9, -0.15, -7.25],
        [-0.25, 0.25, 0.93, 3.75],
        [0.0, 0.0, 0.0, 1.0],
    ];

    for &size in &[0_usize, 1, 7, 8, 15, 16, 65, 257, 1025] {
        let points: Vec<[f64; 4]> = (0..size)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                [f * 0.125 - 5.0, f * -0.375 + 2.0, f * 0.625 - 1.0, 1.0]
            })
            .collect();
        let cpu_ref = cpu::batch_transform_4x4(&matrix, &points);

        for backend in BackendKind::ALL {
            if let Some(out) = disp.execute_transform_on(backend, &matrix, &points) {
                assert_eq!(out.len(), cpu_ref.len());
                for (r, a) in cpu_ref.iter().zip(&out) {
                    for c in 0..4 {
                        assert!(
                            (r[c] - a[c]).abs() < PARITY_TOLERANCE,
                            "mismatch on backend {backend:?} at size {size}: {} vs {}",
                            r[c],
                            a[c]
                        );
                    }
                }
            }
        }

        let auto_out = transform_points_4x4(&matrix, &points);
        for (r, a) in cpu_ref.iter().zip(&auto_out) {
            for c in 0..4 {
                assert!((r[c] - a[c]).abs() < PARITY_TOLERANCE);
            }
        }
    }
}

#[test]
fn all_backends_match_cpu_for_pairwise_distances_and_hausdorff() {
    let disp = global_dispatcher();
    for &(na, nb) in &[(1_usize, 1_usize), (5, 7), (8, 8), (19, 23), (65, 37)] {
        let pts_a: Vec<Point3> = (0..na)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                Point3::new(50.0 + f * 0.3, -25.0 + f * 0.7, 10.0 - f * 0.2)
            })
            .collect();
        let pts_b: Vec<Point3> = (0..nb)
            .map(|j| {
                let f = f64::from(u32::try_from(j).unwrap_or(0));
                Point3::new(50.5 + f * 0.2, -24.5 - f * 0.4, 9.5 + f * 0.5)
            })
            .collect();

        let cpu_ref = cpu::pairwise_distance_squared(&pts_a, &pts_b);
        for backend in BackendKind::ALL {
            if let Some(out) = disp.execute_pairwise_distance_on(backend, &pts_a, &pts_b) {
                assert_eq!(out.len(), cpu_ref.len());
                for (&r, &a) in cpu_ref.iter().zip(&out) {
                    assert!(
                        (r - a).abs() < PARITY_TOLERANCE,
                        "distance mismatch on backend {backend:?} ({na}x{nb}): {r} vs {a}",
                    );
                }
            }
        }

        let auto_dist = pairwise_squared_distances(&pts_a, &pts_b);
        for (&r, &a) in cpu_ref.iter().zip(&auto_dist) {
            assert!((r - a).abs() < PARITY_TOLERANCE);
        }

        let expected_h = cpu::hausdorff_from_distance_squared_matrix(&cpu_ref, na, nb);
        for backend in BackendKind::ALL {
            if let Some(h) = disp.execute_hausdorff_on(backend, &pts_a, &pts_b) {
                assert!(
                    (expected_h - h).abs() < PARITY_TOLERANCE,
                    "hausdorff mismatch on backend {backend:?}: {expected_h} vs {h}"
                );
            }
        }
        let actual_h = hausdorff_distance(&pts_a, &pts_b);
        assert!((expected_h - actual_h).abs() < PARITY_TOLERANCE);
    }
}

#[test]
fn all_backends_match_cpu_for_bicubic_bezier_surface_evaluation() {
    let disp = global_dispatcher();
    let mut ctrl = [[[0.0_f64; 4]; 4]; 4];
    for (i, row) in ctrl.iter_mut().enumerate() {
        for (j, pole) in row.iter_mut().enumerate() {
            let u = f64::from(u32::try_from(i).unwrap_or(0));
            let v = f64::from(u32::try_from(j).unwrap_or(0));
            let w = 1.0 + 0.15 * (u + v);
            *pole = [
                (u * 1.5 - 0.5) * w,
                (v * 2.0 + 0.25) * w,
                ((u - 1.5) * (v - 1.5)) * w,
                w,
            ];
        }
    }

    for &(nu, nv) in &[(1_usize, 1_usize), (4, 4), (9, 11), (16, 16), (25, 19)] {
        let u_params: Vec<f64> = (0..nu)
            .map(|i| {
                f64::from(u32::try_from(i).unwrap_or(0))
                    / f64::from(u32::try_from(nu.max(2) - 1).unwrap_or(1))
            })
            .collect();
        let v_params: Vec<f64> = (0..nv)
            .map(|j| {
                f64::from(u32::try_from(j).unwrap_or(0))
                    / f64::from(u32::try_from(nv.max(2) - 1).unwrap_or(1))
            })
            .collect();

        let cpu_ref = cpu::eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params);
        for backend in BackendKind::ALL {
            if let Some(out) = disp.execute_bezier_eval_on(backend, &ctrl, &u_params, &v_params) {
                assert_eq!(out.len(), cpu_ref.len());
                for (r, a) in cpu_ref.iter().zip(&out) {
                    assert!((r.x - a.x).abs() < PARITY_TOLERANCE);
                    assert!((r.y - a.y).abs() < PARITY_TOLERANCE);
                    assert!((r.z - a.z).abs() < PARITY_TOLERANCE);
                }
            }
        }

        let auto_out = eval_bicubic_bezier_grid(&ctrl, &u_params, &v_params);
        for (r, a) in cpu_ref.iter().zip(&auto_out) {
            assert!((r.x - a.x).abs() < PARITY_TOLERANCE);
            assert!((r.y - a.y).abs() < PARITY_TOLERANCE);
            assert!((r.z - a.z).abs() < PARITY_TOLERANCE);
        }
    }
}

#[test]
fn all_backends_match_cpu_for_ray_mesh_and_point_in_solid_and_closest_point() {
    let disp = global_dispatcher();
    let (vertices, triangles) = closed_octahedron_mesh();
    let mesh = TriangleMeshView {
        vertices: &vertices,
        triangles: &triangles,
    };

    let rays: Vec<RayQuery3> = (0..35)
        .map(|i| {
            let f = f64::from(i) * 0.1 - 1.7;
            RayQuery3 {
                origin: Point3::new(f * 0.5, f * 0.3, -5.0),
                direction: Vector3::new(0.05, -0.02, 1.0),
                t_min: 0.0,
                t_max: 20.0,
            }
        })
        .collect();

    let cpu_hits = cpu::ray_mesh_intersect(&rays, mesh);
    for backend in BackendKind::ALL {
        if let Some(hits) = disp.execute_ray_mesh_on(backend, &rays, mesh) {
            assert_eq!(hits.len(), cpu_hits.len());
            for (r, a) in cpu_hits.iter().zip(&hits) {
                assert_eq!(r.hit, a.hit, "hit flag mismatch on {backend:?}");
                if r.hit {
                    assert_eq!(r.triangle_index, a.triangle_index);
                    assert!((r.t - a.t).abs() < PARITY_TOLERANCE);
                    assert!((r.barycentric_u - a.barycentric_u).abs() < PARITY_TOLERANCE);
                    assert!((r.barycentric_v - a.barycentric_v).abs() < PARITY_TOLERANCE);
                }
            }
        }
    }
    let auto_hits = intersect_rays_mesh(&rays, mesh);
    for (r, a) in cpu_hits.iter().zip(&auto_hits) {
        assert_eq!(r.hit, a.hit);
    }

    let query_pts: Vec<Point3> = (0..29)
        .map(|i| {
            let f = f64::from(i) * 0.15 - 2.0;
            Point3::new(f * 0.6, f * -0.4, f * 0.5)
        })
        .collect();

    let cpu_parity = cpu::point_in_solid_parity(&query_pts, mesh);
    for backend in BackendKind::ALL {
        if let Some(parity) = disp.execute_point_in_solid_on(backend, &query_pts, mesh) {
            assert_eq!(parity, cpu_parity);
        }
    }
    assert_eq!(points_in_solid_parity(&query_pts, mesh), cpu_parity);

    let cpu_closest = cpu::closest_point_mesh(&query_pts, mesh);
    for backend in BackendKind::ALL {
        if let Some(closest) = disp.execute_closest_point_on(backend, &query_pts, mesh) {
            assert_eq!(closest.len(), cpu_closest.len());
            for (r, a) in cpu_closest.iter().zip(&closest) {
                assert!((r.distance_sq - a.distance_sq).abs() < PARITY_TOLERANCE);
                assert!((r.closest_point.x - a.closest_point.x).abs() < PARITY_TOLERANCE);
                assert!((r.closest_point.y - a.closest_point.y).abs() < PARITY_TOLERANCE);
                assert!((r.closest_point.z - a.closest_point.z).abs() < PARITY_TOLERANCE);
            }
        }
    }
    let auto_closest = closest_points_on_mesh(&query_pts, mesh);
    for (r, a) in cpu_closest.iter().zip(&auto_closest) {
        assert!((r.distance_sq - a.distance_sq).abs() < PARITY_TOLERANCE);
    }
}

#[test]
fn hausdorff_empty_and_single_point_inputs() {
    let p1 = Point3::new(1.0, 2.0, 3.0);
    let p2 = Point3::new(4.0, 6.0, 3.0);
    let p3 = Point3::new(10.0, 20.0, 30.0);

    assert_eq!(hausdorff_distance(&[], &[]), 0.0);
    assert_eq!(hausdorff_distance(&[p1], &[]), 0.0);
    assert_eq!(hausdorff_distance(&[], &[p2]), 0.0);

    let dist_11 = hausdorff_distance(&[p1], &[p2]);
    assert!((dist_11 - 5.0).abs() < PARITY_TOLERANCE);

    let dist_1m = hausdorff_distance(&[p1], &[p2, p3]);
    let dist_m1 = hausdorff_distance(&[p2, p3], &[p1]);
    assert_eq!(dist_1m, dist_m1);
    let expected_max_min = 5.0_f64.max((p3.x - p1.x).hypot(p3.y - p1.y).hypot(p3.z - p1.z));
    assert!((dist_1m - expected_max_min).abs() < PARITY_TOLERANCE);
}

#[test]
fn hausdorff_identical_and_collinear_sets() {
    let pts: Vec<Point3> = (0..32)
        .map(|i| {
            let f = f64::from(i);
            Point3::new(f * 2.5, f * -1.25, f * 0.75)
        })
        .collect();

    assert_eq!(hausdorff_distance(&pts, &pts), 0.0);

    let shifted: Vec<Point3> = pts
        .iter()
        .map(|p| Point3::new(p.x + 3.0, p.y, p.z))
        .collect();
    let dist_shifted = hausdorff_distance(&pts, &shifted);
    assert!((dist_shifted - 3.0).abs() < PARITY_TOLERANCE);

    let pcurve_pts = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(999.99997, 0.0, 0.0),
        Point3::new(1000.00003, 0.0, 0.0),
    ];
    assert_eq!(hausdorff_distance(&pcurve_pts, &pcurve_pts), 0.0);
}

#[test]
fn hausdorff_boundary_block_sizes() {
    for &na in &[1_usize, 255, 256, 257] {
        for &nb in &[1_usize, 7, 255, 256, 257] {
            let pts_a: Vec<Point3> = (0..na)
                .map(|i| {
                    let f = f64::from(u32::try_from(i).unwrap_or(0));
                    Point3::new(f * 0.1, (f * 0.05).sin(), (f * 0.02).cos())
                })
                .collect();
            let pts_b: Vec<Point3> = (0..nb)
                .map(|j| {
                    let f = f64::from(u32::try_from(j).unwrap_or(0));
                    Point3::new(f * -0.08 + 1.0, (f * 0.03).cos(), (f * 0.04).sin())
                })
                .collect();

            let dist_sq = cpu::pairwise_distance_squared(&pts_a, &pts_b);
            let expected_h = cpu::hausdorff_from_distance_squared_matrix(&dist_sq, na, nb);
            let actual_h = hausdorff_distance(&pts_a, &pts_b);
            assert!(
                (expected_h - actual_h).abs() < PARITY_TOLERANCE,
                "Hausdorff mismatch at boundary ({na} x {nb}): expected {expected_h}, got {actual_h}"
            );
        }
    }
}

#[test]
fn hausdorff_large_dynamic_range_and_close_pairs() {
    let offset_x = 1_000_000.0;
    let offset_y = -2_000_000.0;
    let offset_z = 500_000.0;

    let pts_a: Vec<Point3> = (0..16)
        .map(|i| {
            let f = f64::from(i);
            Point3::new(
                offset_x + f * 0.01,
                offset_y + f * 0.02,
                offset_z + f * 0.03,
            )
        })
        .collect();
    let pts_b: Vec<Point3> = pts_a
        .iter()
        .map(|p| Point3::new(p.x + 1.0e-6, p.y - 2.0e-6, p.z + 2.0e-6))
        .collect();

    let dist_sq = cpu::pairwise_distance_squared(&pts_a, &pts_b);
    let expected_h =
        cpu::hausdorff_from_distance_squared_matrix(&dist_sq, pts_a.len(), pts_b.len());
    let actual_h = hausdorff_distance(&pts_a, &pts_b);

    assert!((actual_h - 3.0e-6).abs() < 1.0e-9);
    assert!((expected_h - actual_h).abs() < PARITY_TOLERANCE);
}

#[test]
fn batch_transform_degenerate_and_special_matrices() {
    let pts = [
        [1.0, 2.0, 3.0, 1.0],
        [-4.0, 5.0, -6.0, 1.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let out_ident = transform_points_4x4(&identity, &pts);
    assert_eq!(out_ident.len(), pts.len());
    for (p, o) in pts.iter().zip(&out_ident) {
        for c in 0..4 {
            assert!((p[c] - o[c]).abs() < PARITY_TOLERANCE);
        }
    }

    let reflection = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let out_reflect = transform_points_4x4(&reflection, &pts);
    for (p, o) in pts.iter().zip(&out_reflect) {
        assert!((p[0] + o[0]).abs() < PARITY_TOLERANCE);
        assert!((p[1] - o[1]).abs() < PARITY_TOLERANCE);
        assert!((p[2] + o[2]).abs() < PARITY_TOLERANCE);
        assert!((p[3] - o[3]).abs() < PARITY_TOLERANCE);
    }

    let out_empty = transform_points_4x4(&identity, &[]);
    assert!(out_empty.is_empty());
}

#[test]
fn bicubic_bezier_corner_and_degenerate_cases() {
    let mut ctrl = [[[0.0_f64; 4]; 4]; 4];
    for (i, row) in ctrl.iter_mut().enumerate() {
        for (j, pole) in row.iter_mut().enumerate() {
            let u = f64::from(u32::try_from(i).unwrap_or(0));
            let v = f64::from(u32::try_from(j).unwrap_or(0));
            *pole = [u * 10.0, v * 5.0, (u + v) * 2.0, 1.0];
        }
    }

    let corner_00 = eval_bicubic_bezier_grid(&ctrl, &[0.0], &[0.0]);
    assert_eq!(corner_00.len(), 1);
    assert!((corner_00[0].x - ctrl[0][0][0]).abs() < PARITY_TOLERANCE);
    assert!((corner_00[0].y - ctrl[0][0][1]).abs() < PARITY_TOLERANCE);
    assert!((corner_00[0].z - ctrl[0][0][2]).abs() < PARITY_TOLERANCE);

    let corner_10 = eval_bicubic_bezier_grid(&ctrl, &[1.0], &[0.0]);
    assert!((corner_10[0].x - ctrl[3][0][0]).abs() < PARITY_TOLERANCE);
    assert!((corner_10[0].y - ctrl[3][0][1]).abs() < PARITY_TOLERANCE);
    assert!((corner_10[0].z - ctrl[3][0][2]).abs() < PARITY_TOLERANCE);

    let corner_01 = eval_bicubic_bezier_grid(&ctrl, &[0.0], &[1.0]);
    assert!((corner_01[0].x - ctrl[0][3][0]).abs() < PARITY_TOLERANCE);
    assert!((corner_01[0].y - ctrl[0][3][1]).abs() < PARITY_TOLERANCE);
    assert!((corner_01[0].z - ctrl[0][3][2]).abs() < PARITY_TOLERANCE);

    let corner_11 = eval_bicubic_bezier_grid(&ctrl, &[1.0], &[1.0]);
    assert!((corner_11[0].x - ctrl[3][3][0]).abs() < PARITY_TOLERANCE);
    assert!((corner_11[0].y - ctrl[3][3][1]).abs() < PARITY_TOLERANCE);
    assert!((corner_11[0].z - ctrl[3][3][2]).abs() < PARITY_TOLERANCE);

    let p_fixed = [7.5, -3.2, 4.8, 1.0];
    let ctrl_collapsed = [[p_fixed; 4]; 4];
    let collapsed_grid =
        eval_bicubic_bezier_grid(&ctrl_collapsed, &[0.0, 0.25, 0.5, 0.75, 1.0], &[0.1, 0.9]);
    for pt in &collapsed_grid {
        assert!((pt.x - p_fixed[0]).abs() < PARITY_TOLERANCE);
        assert!((pt.y - p_fixed[1]).abs() < PARITY_TOLERANCE);
        assert!((pt.z - p_fixed[2]).abs() < PARITY_TOLERANCE);
    }

    assert!(eval_bicubic_bezier_grid(&ctrl, &[], &[0.5]).is_empty());
    assert!(eval_bicubic_bezier_grid(&ctrl, &[0.5], &[]).is_empty());
}

#[test]
fn ray_mesh_and_containment_degenerate_cases() {
    let (vertices, triangles) = closed_octahedron_mesh();
    let mesh = TriangleMeshView {
        vertices: &vertices,
        triangles: &triangles,
    };

    let origin = [Point3::new(0.0, 0.0, 0.0)];
    let origin_inside = points_in_solid_parity(&origin, mesh);
    assert_eq!(origin_inside, vec![true]);

    let outside = [Point3::new(100.0, 100.0, 100.0)];
    let outside_result = points_in_solid_parity(&outside, mesh);
    assert_eq!(outside_result, vec![false]);

    let away_ray = [RayQuery3 {
        origin: Point3::new(10.0, 0.0, 0.0),
        direction: Vector3::new(1.0, 0.0, 0.0),
        t_min: 0.0,
        t_max: 1000.0,
    }];
    let hit_away = intersect_rays_mesh(&away_ray, mesh);
    assert_eq!(hit_away.len(), 1);
    assert!(!hit_away[0].hit);

    let hit_apex_ray = [RayQuery3 {
        origin: Point3::new(10.0, 0.0, 0.0),
        direction: Vector3::new(-1.0, 0.0, 0.0),
        t_min: 0.0,
        t_max: 1000.0,
    }];
    let hit_apex = intersect_rays_mesh(&hit_apex_ray, mesh);
    assert_eq!(hit_apex.len(), 1);
    assert!(hit_apex[0].hit);
    assert!((hit_apex[0].t - 8.0).abs() < PARITY_TOLERANCE);

    assert!(intersect_rays_mesh(&[], mesh).is_empty());
    assert!(points_in_solid_parity(&[], mesh).is_empty());
    assert!(closest_points_on_mesh(&[], mesh).is_empty());

    let tri_vertices = [
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(1.0, 0.0, 0.0),
        Point3::new(0.0, 1.0, 0.0),
    ];
    let tri_indices = [[0, 1, 2]];
    let single_tri_mesh = TriangleMeshView {
        vertices: &tri_vertices,
        triangles: &tri_indices,
    };
    let eps_ray = [RayQuery3 {
        origin: Point3::new(0.5 + 5e-11, 0.5 + 5e-11, -1.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
        t_min: 0.0,
        t_max: 10.0,
    }];
    let cpu_hit = cpu::ray_mesh_intersect(&eps_ray, single_tri_mesh);
    assert!(!cpu_hit[0].hit);
    let disp_hit = intersect_rays_mesh(&eps_ray, single_tri_mesh);
    assert!(!disp_hit[0].hit);
    if let Some(cuda) = crate::cuda::global_cuda() {
        let cuda_hit = cuda
            .simt_ray_mesh_intersect(&eps_ray, single_tri_mesh)
            .expect("cuda ray hit");
        assert!(!cuda_hit[0].hit);
    }
}

#[test]
fn batch_transform_and_distance_extreme_magnitude_1e20() {
    let disp = global_dispatcher();
    let scale = 1.0e20_f64;
    let matrix = [
        [scale, 0.0, 0.0, 0.0],
        [0.0, scale, 0.0, 0.0],
        [0.0, 0.0, scale, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let points = [[scale, -scale, 0.5 * scale, 1.0]];
    let cpu_ref = cpu::batch_transform_4x4(&matrix, &points);
    assert_eq!(cpu_ref[0][0], 1.0e40);

    for backend in BackendKind::ALL {
        if let Some(out) = disp.execute_transform_on(backend, &matrix, &points) {
            assert_eq!(out.len(), 1);
            for c in 0..4 {
                assert!(!out[0][c].is_nan(), "NaN on backend {backend:?}");
                assert_eq!(out[0][c], cpu_ref[0][c], "mismatch on backend {backend:?}");
            }
        }
    }

    let pts_a = [Point3::new(scale, 0.0, 0.0)];
    let pts_b = [Point3::new(scale + 1.0e10, 0.0, 0.0)];
    let expected_h = 1.0e10_f64;
    for backend in BackendKind::ALL {
        if let Some(h) = disp.execute_hausdorff_on(backend, &pts_a, &pts_b) {
            assert!(!h.is_nan(), "NaN Hausdorff on backend {backend:?}");
            assert!((h - expected_h).abs() <= 1.0e4);
        }
    }
}

#[test]
fn hausdorff_gram_cancellation_and_infinite_closest_point_query() {
    let disp = global_dispatcher();
    let pts_a = [Point3::new(0.0, 0.0, 0.0), Point3::new(1000.0, 0.0, 0.0)];
    let pts_b = [Point3::new(0.0, 0.0, 0.0), Point3::new(1000.0, 1.0e-6, 0.0)];
    for backend in BackendKind::ALL {
        if let Some(h) = disp.execute_hausdorff_on(backend, &pts_a, &pts_b) {
            assert!(
                (h - 1.0e-6).abs() < PARITY_TOLERANCE,
                "Hausdorff cancellation on backend {backend:?}: expected 1e-6, got {h}"
            );
        }
    }
    let auto_h = hausdorff_distance(&pts_a, &pts_b);
    assert!((auto_h - 1.0e-6).abs() < PARITY_TOLERANCE);

    let (vertices, triangles) = closed_octahedron_mesh();
    let mesh = TriangleMeshView {
        vertices: &vertices,
        triangles: &triangles,
    };
    let inf_query = [Point3::new(f64::INFINITY, 0.0, 0.0)];
    let cpu_ref = cpu::closest_point_mesh(&inf_query, mesh);
    for backend in BackendKind::ALL {
        if let Some(res) = disp.execute_closest_point_on(backend, &inf_query, mesh) {
            assert_eq!(res.len(), 1);
            assert!(
                !res[0].distance_sq.is_nan(),
                "NaN distance_sq on {backend:?}"
            );
            assert!(
                !res[0].closest_point.x.is_nan(),
                "NaN closest_point.x on {backend:?}"
            );
            assert!(
                !res[0].closest_point.y.is_nan(),
                "NaN closest_point.y on {backend:?}"
            );
            assert!(
                !res[0].closest_point.z.is_nan(),
                "NaN closest_point.z on {backend:?}"
            );
            assert_eq!(res, cpu_ref, "mismatch on {backend:?}");
        }
    }
    let auto_cp = closest_points_on_mesh(&inf_query, mesh);
    assert_eq!(auto_cp, cpu_ref);

    let ctrl = [[[0.0_f64, 0.0, 0.0, 1.0]; 4]; 4];
    let params = [0.0, 1.0];
    let rays = [RayQuery3 {
        origin: Point3::new(0.0, 0.0, -5.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
        t_min: 0.0,
        t_max: 10.0,
    }];
    let matrix = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let hom_pts = [[1.0, 2.0, 3.0, 1.0]];
    assert!(disp
        .execute_point_in_solid_on(BackendKind::RtxTensorCore, &pts_a, mesh)
        .is_none());
    assert!(disp
        .execute_closest_point_on(BackendKind::RtxTensorCore, &pts_a, mesh)
        .is_none());
    assert!(disp
        .execute_hausdorff_on(BackendKind::RtxTensorCore, &pts_a, &pts_b)
        .is_none());
    assert!(disp
        .execute_pairwise_distance_on(BackendKind::RtxTensorCore, &pts_a, &pts_b)
        .is_none());
    assert!(disp
        .execute_ray_mesh_on(BackendKind::RtxTensorCore, &rays, mesh)
        .is_none());
    assert!(disp
        .execute_transform_on(BackendKind::CpuSpatialIndex, &matrix, &hom_pts)
        .is_none());
    assert!(disp
        .execute_pairwise_distance_on(BackendKind::CpuSpatialIndex, &pts_a, &pts_b)
        .is_none());
    assert!(disp
        .execute_bezier_eval_on(BackendKind::CpuSpatialIndex, &ctrl, &params, &params)
        .is_none());
    assert!(disp
        .execute_transform_on(BackendKind::VulkanRtCore, &matrix, &hom_pts)
        .is_none());
    assert!(disp
        .execute_pairwise_distance_on(BackendKind::VulkanRtCore, &pts_a, &pts_b)
        .is_none());
    assert!(disp
        .execute_bezier_eval_on(BackendKind::VulkanRtCore, &ctrl, &params, &params)
        .is_none());
    assert!(disp
        .execute_transform_on(BackendKind::RtxRtCore, &matrix, &hom_pts)
        .is_none());
    assert!(disp
        .execute_pairwise_distance_on(BackendKind::RtxRtCore, &pts_a, &pts_b)
        .is_none());
    assert!(disp
        .execute_bezier_eval_on(BackendKind::RtxRtCore, &ctrl, &params, &params)
        .is_none());
}

#[test]
fn benchmark_scaling_all_kernels() {
    use std::time::Instant;

    let vk = crate::vulkan::global_vulkan();
    let cu = crate::cuda::global_cuda();

    eprintln!("\n=== Benchmark: Hausdorff Distance Scaling Across Backends (ms) ===");
    eprintln!(
        "| {:>6} | {:>13} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} |",
        "N",
        "CPU All-Pairs",
        "CPU KD-Tree",
        "Rayon KD",
        "Vulkan f64",
        "Vulkan RT",
        "CUDA SIMT",
        "PTX SM86",
        "OptiX RT",
        "Max Speedup"
    );
    eprintln!(
        "|:-------|:--------------|:------------|:------------|:------------|:------------|:------------|:------------|:------------|:------------|"
    );

    for &size in &[64_usize, 256, 1024, 4096, 16384, 65536] {
        let pts_a: Vec<Point3> = (0..size)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                Point3::new(
                    f * 0.05 - 10.0,
                    (f * 0.08).sin() * 5.0,
                    (f * 0.03).cos() * 4.0,
                )
            })
            .collect();
        let pts_b: Vec<Point3> = (0..size)
            .map(|j| {
                let f = f64::from(u32::try_from(j).unwrap_or(0));
                Point3::new(
                    f * 0.05 - 9.85,
                    (f * 0.08).sin() * 5.0 + 0.12,
                    (f * 0.03).cos() * 4.0 - 0.08,
                )
            })
            .collect();

        let (cpu_ms_str, cpu_ms_val) = if size <= 4096 {
            let t0 = Instant::now();
            let dist_sq = cpu::pairwise_distance_squared(&pts_a, &pts_b);
            let cpu_h = cpu::hausdorff_from_distance_squared_matrix(&dist_sq, size, size);
            let cpu_ms = t0.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box(cpu_h);
            (format!("{cpu_ms:>13.3}"), Some(cpu_ms))
        } else {
            ("   OOM (>8GB)".to_string(), None)
        };

        let t_kd = Instant::now();
        let kd_h = crate::spatial::kd_hausdorff_distance(&pts_a, &pts_b);
        let kd_ms = t_kd.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(kd_h);

        let t_rayon = Instant::now();
        let rayon_h = crate::spatial::rayon_hausdorff_distance(&pts_a, &pts_b);
        let rayon_ms = t_rayon.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(rayon_h);
        assert!((kd_h - rayon_h).abs() < PARITY_TOLERANCE);

        let mut best_ms = kd_ms.min(rayon_ms);

        let vk_str = if let Some(vk_ctx) = vk {
            std::hint::black_box(vk_ctx.compute_spatial_hausdorff(&pts_a, &pts_b));
            let t_vk = Instant::now();
            if let Some(vh) = vk_ctx.compute_spatial_hausdorff(&pts_a, &pts_b) {
                let ms = t_vk.elapsed().as_secs_f64() * 1000.0;
                assert!((kd_h - vh).abs() < PARITY_TOLERANCE);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let vk_rt_str = if let Some(vk_ctx) = vk {
            std::hint::black_box(vk_ctx.rt_core_spatial_hausdorff(&pts_a, &pts_b));
            let t_rt = Instant::now();
            if let Some(rh) = vk_ctx.rt_core_spatial_hausdorff(&pts_a, &pts_b) {
                let ms = t_rt.elapsed().as_secs_f64() * 1000.0;
                assert!((kd_h - rh).abs() < PARITY_TOLERANCE);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let simt_str = if let Some(cu_ctx) = cu {
            std::hint::black_box(cu_ctx.simt_spatial_hausdorff(&pts_a, &pts_b));
            let t_simt = Instant::now();
            if let Some(sh) = cu_ctx.simt_spatial_hausdorff(&pts_a, &pts_b) {
                let ms = t_simt.elapsed().as_secs_f64() * 1000.0;
                assert!((kd_h - sh).abs() < PARITY_TOLERANCE);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let sm86_str = if let Some(cu_ctx) = cu {
            let t_sm86 = Instant::now();
            if let Some(sh) = cu_ctx.ptx_sm86_spatial_hausdorff(&pts_a, &pts_b) {
                let ms = t_sm86.elapsed().as_secs_f64() * 1000.0;
                assert!((kd_h - sh).abs() < PARITY_TOLERANCE);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let optix_rt_str = if let Some(cu_ctx) = cu {
            std::hint::black_box(cu_ctx.optix_rt_spatial_hausdorff(&pts_a, &pts_b));
            let t_rt = Instant::now();
            if let Some(rh) = cu_ctx.optix_rt_spatial_hausdorff(&pts_a, &pts_b) {
                let ms = t_rt.elapsed().as_secs_f64() * 1000.0;
                assert!((kd_h - rh).abs() < PARITY_TOLERANCE);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let speedup_str = cpu_ms_val.map_or_else(
            || "     >1000x".to_string(),
            |c| format!("{:>10.1}x", c / best_ms.max(0.0001)),
        );

        eprintln!(
            "| {size:>6} | {cpu_ms_str} | {kd_ms:>11.3} | {rayon_ms:>11.3} | {vk_str} | {vk_rt_str} | {simt_str} | {sm86_str} | {optix_rt_str} | {speedup_str} |"
        );
    }

    let nx = 16_u32;
    let ny = 8_u32;
    let stride = nx + 1;
    let mut dome_verts = Vec::with_capacity(usize::try_from(stride * (ny + 1) * 2).unwrap_or(0));
    let mut dome_tris = Vec::with_capacity(usize::try_from(nx * ny * 4).unwrap_or(0));
    for half in 0..2_u32 {
        let sign = if half == 0 { 1.0 } else { -1.0 };
        for iy in 0..=ny {
            let v_frac = f64::from(iy) / f64::from(ny);
            let py = -5.0 + v_frac * 10.0;
            for ix in 0..=nx {
                let u_frac = f64::from(ix) / f64::from(nx);
                let px = -5.0 + u_frac * 10.0;
                let r2 = (px * px + py * py) / 50.0;
                let pz = sign * (0.2 + 2.3 * (1.0 - r2).max(0.0));
                dome_verts.push(Point3::new(px, py, pz));
            }
        }
    }
    let top_base = 0_u32;
    let bot_base = stride * (ny + 1);
    for iy in 0..ny {
        for ix in 0..nx {
            let t00 = top_base + iy * stride + ix;
            let t10 = t00 + 1;
            let t01 = t00 + stride;
            let t11 = t01 + 1;
            dome_tris.push([t00, t10, t11]);
            dome_tris.push([t00, t11, t01]);

            let b00 = bot_base + iy * stride + ix;
            let b10 = b00 + 1;
            let b01 = b00 + stride;
            let b11 = b01 + 1;
            dome_tris.push([b00, b11, b10]);
            dome_tris.push([b00, b01, b11]);
        }
    }
    let mesh = TriangleMeshView {
        vertices: &dome_verts,
        triangles: &dome_tris,
    };

    eprintln!("\n=== Benchmark: Ray-Mesh Intersection Scaling (512 Tris, ms) ===");
    eprintln!(
        "| {:>6} | {:>13} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} |",
        "N",
        "CPU All-Tris",
        "CPU BVH",
        "Rayon BVH",
        "Vulkan f64",
        "Vulkan RT",
        "CUDA SIMT",
        "PTX SM86",
        "OptiX RT",
        "Max Speedup"
    );
    eprintln!(
        "|:-------|:--------------|:------------|:------------|:------------|:------------|:------------|:------------|:------------|:------------|"
    );
    let warm_ray = [RayQuery3 {
        origin: Point3::new(0.0, 0.0, -5.0),
        direction: Vector3::new(0.0, 0.0, 1.0),
        t_min: 0.0,
        t_max: 20.0,
    }];
    if let Some(vk_ctx) = vk {
        std::hint::black_box(vk_ctx.compute_ray_mesh_intersect(&warm_ray, mesh));
        std::hint::black_box(vk_ctx.rt_core_ray_mesh_intersect(&warm_ray, mesh));
    }
    if let Some(cu_ctx) = cu {
        std::hint::black_box(cu_ctx.simt_ray_mesh_intersect(&warm_ray, mesh));
        std::hint::black_box(cu_ctx.optix_rt_ray_mesh_intersect(&warm_ray, mesh));
    }
    for &n_rays in &[64_usize, 256, 1024, 4096, 16384, 65536] {
        let rays: Vec<RayQuery3> = (0..n_rays)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                let x = (f * 0.13).sin() * 4.2;
                let y = (f * 0.17).cos() * 4.2;
                RayQuery3 {
                    origin: Point3::new(x, y, -5.0),
                    direction: Vector3::new(0.0, 0.0, 1.0),
                    t_min: 0.0,
                    t_max: 20.0,
                }
            })
            .collect();
        let t0 = Instant::now();
        let cpu_res = cpu::ray_mesh_intersect(&rays, mesh);
        let cpu_ms = t0.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&cpu_res);

        let t1 = Instant::now();
        let bvh_res = crate::spatial::intersect_rays_mesh(&rays, mesh);
        let bvh_ms = t1.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&bvh_res);

        let t2 = Instant::now();
        let rayon_res = crate::spatial::rayon_intersect_rays_mesh(&rays, mesh);
        let rayon_ms = t2.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&rayon_res);

        let mut best_ms = bvh_ms.min(rayon_ms);

        let vk_str = if let Some(vk_ctx) = vk {
            let t_vk = Instant::now();
            if let Some(r) = vk_ctx.compute_ray_mesh_intersect(&rays, mesh) {
                let ms = t_vk.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let vk_rt_str = if let Some(vk_ctx) = vk {
            let t_rt = Instant::now();
            if let Some(r) = vk_ctx.rt_core_ray_mesh_intersect(&rays, mesh) {
                let ms = t_rt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let simt_str = if let Some(cu_ctx) = cu {
            let t_simt = Instant::now();
            if let Some(r) = cu_ctx.simt_ray_mesh_intersect(&rays, mesh) {
                let ms = t_simt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let sm86_str = if let Some(cu_ctx) = cu {
            let t_sm86 = Instant::now();
            if let Some(r) = cu_ctx.ptx_sm86_ray_mesh_intersect(&rays, mesh) {
                let ms = t_sm86.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let optix_rt_str = if let Some(cu_ctx) = cu {
            let t_rt = Instant::now();
            if let Some(r) = cu_ctx.optix_rt_ray_mesh_intersect(&rays, mesh) {
                let ms = t_rt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let speedup = cpu_ms / best_ms.max(0.0001);
        eprintln!(
            "| {n_rays:>6} | {cpu_ms:>13.3} | {bvh_ms:>11.3} | {rayon_ms:>11.3} | {vk_str} | {vk_rt_str} | {simt_str} | {sm86_str} | {optix_rt_str} | {speedup:>10.1}x |"
        );
    }

    eprintln!("\n=== Benchmark: Point-in-Solid Parity Scaling (512 Tris, ms) ===");
    eprintln!(
        "| {:>6} | {:>13} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} |",
        "N",
        "CPU All-Tris",
        "CPU BVH",
        "Rayon BVH",
        "Vulkan f64",
        "Vulkan RT",
        "CUDA SIMT",
        "PTX SM86",
        "OptiX RT",
        "Max Speedup"
    );
    eprintln!(
        "|:-------|:--------------|:------------|:------------|:------------|:------------|:------------|:------------|:------------|:------------|"
    );
    for &n_pts in &[64_usize, 256, 1024, 4096, 16384, 65536] {
        let q_pts: Vec<Point3> = (0..n_pts)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                let x = (f * 0.13).sin() * 4.2;
                let y = (f * 0.17).cos() * 4.2;
                let r2 = (x * x + y * y) / 50.0;
                let z_dome = 0.2 + 2.3 * (1.0 - r2).max(0.0);
                let sign = if i % 2 == 0 { -1.0 } else { 1.0 };
                Point3::new(x, y, z_dome + sign * 0.05)
            })
            .collect();
        let t0 = Instant::now();
        let cpu_res = cpu::point_in_solid_parity(&q_pts, mesh);
        let cpu_ms = t0.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&cpu_res);

        let t1 = Instant::now();
        let bvh_res = crate::spatial::points_in_solid_parity(&q_pts, mesh);
        let bvh_ms = t1.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&bvh_res);

        let t2 = Instant::now();
        let rayon_res = crate::spatial::rayon_points_in_solid_parity(&q_pts, mesh);
        let rayon_ms = t2.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&rayon_res);

        let mut best_ms = bvh_ms.min(rayon_ms);

        let vk_str = if let Some(vk_ctx) = vk {
            let t_vk = Instant::now();
            if let Some(r) = vk_ctx.compute_point_in_solid_parity(&q_pts, mesh) {
                let ms = t_vk.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let vk_rt_str = if let Some(vk_ctx) = vk {
            let t_rt = Instant::now();
            if let Some(r) = vk_ctx.rt_core_point_in_solid_parity(&q_pts, mesh) {
                let ms = t_rt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let simt_str = if let Some(cu_ctx) = cu {
            let t_simt = Instant::now();
            if let Some(r) = cu_ctx.simt_point_in_solid_parity(&q_pts, mesh) {
                let ms = t_simt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let sm86_str = if let Some(cu_ctx) = cu {
            let t_sm86 = Instant::now();
            if let Some(r) = cu_ctx.ptx_sm86_point_in_solid_parity(&q_pts, mesh) {
                let ms = t_sm86.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let optix_rt_str = if let Some(cu_ctx) = cu {
            let t_rt = Instant::now();
            if let Some(r) = cu_ctx.optix_rt_point_in_solid_parity(&q_pts, mesh) {
                let ms = t_rt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let speedup = cpu_ms / best_ms.max(0.0001);
        eprintln!(
            "| {n_pts:>6} | {cpu_ms:>13.3} | {bvh_ms:>11.3} | {rayon_ms:>11.3} | {vk_str} | {vk_rt_str} | {simt_str} | {sm86_str} | {optix_rt_str} | {speedup:>10.1}x |"
        );
    }

    eprintln!("\n=== Benchmark: Triangle Mesh Closest-Point Scaling (512 Tris, ms) ===");
    eprintln!(
        "| {:>6} | {:>13} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} |",
        "N",
        "CPU All-Tris",
        "CPU BVH",
        "Rayon BVH",
        "Vulkan f64",
        "Vulkan RT",
        "CUDA SIMT",
        "PTX SM86",
        "OptiX RT",
        "Max Speedup"
    );
    eprintln!(
        "|:-------|:--------------|:------------|:------------|:------------|:------------|:------------|:------------|:------------|:------------|"
    );
    if let Some(vk_ctx) = vk {
        std::hint::black_box(
            vk_ctx.rt_core_closest_point_mesh(&[Point3::new(0.0, 0.0, 2.5)], mesh),
        );
    }
    if let Some(cu_ctx) = cu {
        std::hint::black_box(
            cu_ctx.optix_rt_closest_point_mesh(&[Point3::new(0.0, 0.0, 2.5)], mesh),
        );
    }
    for &n_queries in &[64_usize, 256, 1024, 4096, 16384, 65536] {
        let q_pts: Vec<Point3> = (0..n_queries)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                let x = (f * 0.13).sin() * 4.2;
                let y = (f * 0.17).cos() * 4.2;
                let r2 = (x * x + y * y) / 50.0;
                let z_dome = 0.2 + 2.3 * (1.0 - r2).max(0.0);
                Point3::new(x, y, z_dome + (f * 0.07).sin() * 0.04)
            })
            .collect();
        let t0 = Instant::now();
        let cpu_res = cpu::closest_point_mesh(&q_pts, mesh);
        let cpu_ms = t0.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&cpu_res);

        let t1 = Instant::now();
        let bvh_res = crate::spatial::closest_points_on_mesh(&q_pts, mesh);
        let bvh_ms = t1.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&bvh_res);

        let t2 = Instant::now();
        let rayon_res = crate::spatial::rayon_closest_points_on_mesh(&q_pts, mesh);
        let rayon_ms = t2.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&rayon_res);

        let mut best_ms = bvh_ms.min(rayon_ms);

        let vk_str = if let Some(vk_ctx) = vk {
            let t_vk = Instant::now();
            if let Some(r) = vk_ctx.compute_spatial_closest_point(&q_pts, mesh) {
                let ms = t_vk.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let vk_rt_str = if let Some(vk_ctx) = vk {
            let t_rt = Instant::now();
            if let Some(r) = vk_ctx.rt_core_closest_point_mesh(&q_pts, mesh) {
                let ms = t_rt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let simt_str = if let Some(cu_ctx) = cu {
            let t_simt = Instant::now();
            if let Some(r) = cu_ctx.simt_spatial_closest_point(&q_pts, mesh) {
                let ms = t_simt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let sm86_str = if let Some(cu_ctx) = cu {
            let t_sm86 = Instant::now();
            if let Some(r) = cu_ctx.ptx_sm86_spatial_closest_point(&q_pts, mesh) {
                let ms = t_sm86.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let optix_rt_str = if let Some(cu_ctx) = cu {
            let t_rt = Instant::now();
            if let Some(r) = cu_ctx.optix_rt_closest_point_mesh(&q_pts, mesh) {
                let ms = t_rt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let speedup = cpu_ms / best_ms.max(0.0001);
        eprintln!(
            "| {n_queries:>6} | {cpu_ms:>13.3} | {bvh_ms:>11.3} | {rayon_ms:>11.3} | {vk_str} | {vk_rt_str} | {simt_str} | {sm86_str} | {optix_rt_str} | {speedup:>10.1}x |"
        );
    }

    let report = global_dispatcher().report();
    for &scale in &[16_usize, 64, 256, 1024, 4096] {
        eprintln!("\n=== Benchmark: 33-Kernel Latency Matrix at N = {scale} (ns) ===");
        eprintln!(
            "| {:<24} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} | {:>10} |",
            "Operation",
            "Cpu",
            "CpuSpatial",
            "Rayon",
            "Vulkan f64",
            "Vulkan RT",
            "CUDA SIMT",
            "PTX SM86",
            "OptiX RT",
            "TensorCore"
        );
        eprintln!(
            "|:-------------------------|:-----------|:-----------|:-----------|:-----------|:-----------|:-----------|:-----------|:-----------|:-----------|"
        );
        for op in OperationKind::ALL {
            let fmt_cell = |b: BackendKind| -> String {
                report
                    .samples
                    .iter()
                    .find(|s| s.operation == op && s.backend == b && s.problem_size == scale)
                    .map_or_else(
                        || "       N/A".to_string(),
                        |s| format!("{:>10}", s.latency_ns),
                    )
            };
            eprintln!(
                "| {:<24} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
                format!("{op:?}"),
                fmt_cell(BackendKind::Cpu),
                fmt_cell(BackendKind::CpuSpatialIndex),
                fmt_cell(BackendKind::CpuParallelSpatialIndex),
                fmt_cell(BackendKind::VulkanCompute),
                fmt_cell(BackendKind::VulkanRtCore),
                fmt_cell(BackendKind::CudaCompute),
                fmt_cell(BackendKind::PtxSm86),
                fmt_cell(BackendKind::RtxRtCore),
                fmt_cell(BackendKind::RtxTensorCore),
            );
        }
    }

    eprintln!("\n=== Benchmark: BatchTransform4x4 Scaling (ms) ===");
    eprintln!(
        "| {:>8} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} |",
        "N", "CPU", "Rayon", "Vulkan f64", "CUDA SIMT", "PTX SM86", "Max Speedup"
    );
    eprintln!(
        "|:---------|:------------|:------------|:------------|:------------|:------------|:------------|"
    );
    let matrix = [
        [1.0, 0.25, -0.125, 2.0],
        [-0.25, 1.5, 0.5, -1.0],
        [0.125, -0.5, 1.25, 0.5],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for &n_pts in &[1024_usize, 4096, 16384, 65536, 262_144, 1_048_576] {
        let pts: Vec<[f64; 4]> = (0..n_pts)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                [f * 0.01, f * -0.02, f * 0.03, 1.0]
            })
            .collect();
        let t0 = Instant::now();
        let cpu_res = cpu::batch_transform_4x4(&matrix, &pts);
        let cpu_ms = t0.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&cpu_res);

        let t1 = Instant::now();
        let rayon_res = cpu::rayon_batch_transform_4x4(&matrix, &pts);
        let rayon_ms = t1.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&rayon_res);

        let mut best_ms = cpu_ms.min(rayon_ms);

        let vk_str = if let Some(vk_ctx) = vk {
            let t_vk = Instant::now();
            if let Some(r) = vk_ctx.compute_batch_transform_4x4(&matrix, &pts) {
                let ms = t_vk.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let simt_str = if let Some(cu_ctx) = cu {
            let t_simt = Instant::now();
            if let Some(r) = cu_ctx.simt_batch_transform_4x4(&matrix, &pts) {
                let ms = t_simt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let sm86_str = if let Some(cu_ctx) = cu {
            let t_sm86 = Instant::now();
            if let Some(r) = cu_ctx.ptx_sm86_batch_transform_4x4(&matrix, &pts) {
                let ms = t_sm86.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let speedup = cpu_ms / best_ms.max(0.0001);
        eprintln!(
            "| {n_pts:>8} | {cpu_ms:>11.3} | {rayon_ms:>11.3} | {vk_str} | {simt_str} | {sm86_str} | {speedup:>10.1}x |"
        );
    }

    eprintln!("\n=== Benchmark: PairwiseDistanceMatrix Scaling (ms) ===");
    eprintln!(
        "| {:>8} | {:>13} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} |",
        "N (pts)", "CPU (Dense)", "Rayon", "Vulkan f64", "CUDA SIMT", "PTX SM86", "Max Speedup"
    );
    eprintln!(
        "|:---------|:--------------|:------------|:------------|:------------|:------------|:------------|"
    );
    for &side in &[64_usize, 256, 1024, 2048, 4096, 8192] {
        let pts_a: Vec<Point3> = (0..side)
            .map(|i| {
                let f = f64::from(u32::try_from(i).unwrap_or(0));
                Point3::new(f * 0.1, f * 0.2, f * -0.1)
            })
            .collect();
        let pts_b: Vec<Point3> = (0..side)
            .map(|j| {
                let f = f64::from(u32::try_from(j).unwrap_or(0));
                Point3::new(f * -0.15 + 1.0, f * 0.05, f * 0.25)
            })
            .collect();

        let (cpu_ms_str, cpu_ms_val) = if side <= 4096 {
            let t0 = Instant::now();
            let cpu_res = cpu::pairwise_distance_squared(&pts_a, &pts_b);
            let cpu_ms = t0.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box(&cpu_res);
            (format!("{cpu_ms:>13.3}"), Some(cpu_ms))
        } else {
            ("   OOM (>8GB)".to_string(), None)
        };

        let (rayon_ms_str, rayon_ms_val) = if side <= 4096 {
            let t1 = Instant::now();
            let rayon_res = cpu::rayon_pairwise_distance_squared(&pts_a, &pts_b);
            let rayon_ms = t1.elapsed().as_secs_f64() * 1000.0;
            std::hint::black_box(&rayon_res);
            (format!("{rayon_ms:>11.3}"), Some(rayon_ms))
        } else {
            ("   OOM (>8GB)".to_string(), None)
        };

        let mut best_ms = cpu_ms_val
            .unwrap_or(f64::INFINITY)
            .min(rayon_ms_val.unwrap_or(f64::INFINITY));

        let vk_str = if side <= 2048 {
            if let Some(vk_ctx) = vk {
                let t_vk = Instant::now();
                if let Some(r) = vk_ctx.compute_pairwise_distance_squared(&pts_a, &pts_b) {
                    let ms = t_vk.elapsed().as_secs_f64() * 1000.0;
                    std::hint::black_box(&r);
                    best_ms = best_ms.min(ms);
                    format!("{ms:>11.3}")
                } else {
                    "        N/A".to_string()
                }
            } else {
                "        N/A".to_string()
            }
        } else {
            "   OOM (>8GB)".to_string()
        };

        let simt_str = if side <= 4096 {
            if let Some(cu_ctx) = cu {
                let t_simt = Instant::now();
                if let Some(r) = cu_ctx.simt_pairwise_distance_squared(&pts_a, &pts_b) {
                    let ms = t_simt.elapsed().as_secs_f64() * 1000.0;
                    std::hint::black_box(&r);
                    best_ms = best_ms.min(ms);
                    format!("{ms:>11.3}")
                } else {
                    "        N/A".to_string()
                }
            } else {
                "        N/A".to_string()
            }
        } else {
            "   OOM (>8GB)".to_string()
        };

        let sm86_str = if side <= 4096 {
            if let Some(cu_ctx) = cu {
                let t_sm86 = Instant::now();
                if let Some(r) = cu_ctx.ptx_sm86_pairwise_distance_squared(&pts_a, &pts_b) {
                    let ms = t_sm86.elapsed().as_secs_f64() * 1000.0;
                    std::hint::black_box(&r);
                    best_ms = best_ms.min(ms);
                    format!("{ms:>11.3}")
                } else {
                    "        N/A".to_string()
                }
            } else {
                "        N/A".to_string()
            }
        } else {
            "   OOM (>8GB)".to_string()
        };

        let speedup_str = cpu_ms_val.map_or_else(
            || "     >1000x".to_string(),
            |c| format!("{:>10.1}x", c / best_ms.max(0.0001)),
        );

        eprintln!(
            "| {side:>8} | {cpu_ms_str} | {rayon_ms_str} | {vk_str} | {simt_str} | {sm86_str} | {speedup_str} |"
        );
    }

    eprintln!("\n=== Benchmark: BezierSurfaceEval Scaling (ms) ===");
    eprintln!(
        "| {:>8} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} | {:>11} |",
        "Total Pts", "CPU", "Rayon", "Vulkan f64", "CUDA SIMT", "PTX SM86", "Max Speedup"
    );
    eprintln!(
        "|:---------|:------------|:------------|:------------|:------------|:------------|:------------|"
    );
    let ctrl = [
        [
            [0.0, 0.0, 0.0, 1.0],
            [0.0, 1.0, 0.2, 1.0],
            [0.0, 2.0, 0.2, 1.0],
            [0.0, 3.0, 0.0, 1.0],
        ],
        [
            [1.0, 0.0, 0.2, 1.0],
            [1.0, 1.0, 0.8, 1.2],
            [1.0, 2.0, 0.8, 1.2],
            [1.0, 3.0, 0.2, 1.0],
        ],
        [
            [2.0, 0.0, 0.2, 1.0],
            [2.0, 1.0, 0.8, 1.2],
            [2.0, 2.0, 0.8, 1.2],
            [2.0, 3.0, 0.2, 1.0],
        ],
        [
            [3.0, 0.0, 0.0, 1.0],
            [3.0, 1.0, 0.2, 1.0],
            [3.0, 2.0, 0.2, 1.0],
            [3.0, 3.0, 0.0, 1.0],
        ],
    ];
    for &side in &[32_usize, 64, 128, 256, 512, 1024] {
        let total_pts = side * side;
        let u_params: Vec<f64> = (0..side)
            .map(|i| {
                f64::from(u32::try_from(i).unwrap_or(0))
                    / f64::from(u32::try_from(side - 1).unwrap_or(1))
            })
            .collect();
        let t0 = Instant::now();
        let cpu_res = cpu::eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params);
        let cpu_ms = t0.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&cpu_res);

        let t1 = Instant::now();
        let rayon_res = cpu::rayon_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params);
        let rayon_ms = t1.elapsed().as_secs_f64() * 1000.0;
        std::hint::black_box(&rayon_res);

        let mut best_ms = cpu_ms.min(rayon_ms);

        let vk_str = if let Some(vk_ctx) = vk {
            let t_vk = Instant::now();
            if let Some(r) = vk_ctx.compute_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params) {
                let ms = t_vk.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let simt_str = if let Some(cu_ctx) = cu {
            let t_simt = Instant::now();
            if let Some(r) = cu_ctx.simt_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params) {
                let ms = t_simt.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let sm86_str = if let Some(cu_ctx) = cu {
            let t_sm86 = Instant::now();
            if let Some(r) = cu_ctx.ptx_sm86_eval_bicubic_bezier_grid(&ctrl, &u_params, &u_params) {
                let ms = t_sm86.elapsed().as_secs_f64() * 1000.0;
                std::hint::black_box(&r);
                best_ms = best_ms.min(ms);
                format!("{ms:>11.3}")
            } else {
                "        N/A".to_string()
            }
        } else {
            "        N/A".to_string()
        };

        let speedup = cpu_ms / best_ms.max(0.0001);
        eprintln!(
            "| {total_pts:>8} | {cpu_ms:>11.3} | {rayon_ms:>11.3} | {vk_str} | {simt_str} | {sm86_str} | {speedup:>10.1}x |"
        );
    }
}
