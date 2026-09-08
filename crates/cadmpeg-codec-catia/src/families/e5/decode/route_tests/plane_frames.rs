// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn plane_axis_fit_is_uv_scale_independent() {
    for scale in [2.0, 1e-200] {
        let pairs = [
            ([scale, 0.0], Point3::new(scale, 0.0, 0.0)),
            ([0.0, scale], Point3::new(0.0, scale, 0.0)),
            ([scale, scale], Point3::new(scale, scale, 0.0)),
        ];
        let (u_axis, v_axis, residual) =
            fit_e5_plane_axes([0.0; 3], &pairs).expect("full-rank frame");
        assert!(residual <= scale * EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!((u_axis.x - 1.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!(u_axis.y.abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!(u_axis.z.abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!(v_axis.x.abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!((v_axis.y - 1.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!(v_axis.z.abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    }
}

#[test]
fn rank_one_plane_endpoints_complete_with_known_normal() {
    for scale in [2.0, 1e-200] {
        let pairs = [
            ([0.0, -scale], Point3::new(-scale, 0.0, 0.0)),
            ([0.0, scale], Point3::new(scale, 0.0, 0.0)),
        ];
        let (u_axis, v_axis, residual) =
            fit_rank_one_e5_plane_axes([0.0; 3], &pairs, Vector3::new(0.0, 1.0, 0.0))
                .expect("rank-one frame");
        assert!(residual <= scale * EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!((v_axis.x - 1.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        assert!((u_axis.z - 1.0).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
    }
}

#[test]
fn plane_axis_fit_rejects_numerically_rank_one_uv_data() {
    let tiny = 1e-15;
    let pairs = [
        ([tiny, -20.0], Point3::new(-20.0, 0.0, 0.0)),
        ([tiny, 20.0], Point3::new(20.0, 0.0, 0.0)),
        ([-tiny, -7.5], Point3::new(-7.5, 0.0, 0.0)),
        ([tiny, 7.5], Point3::new(7.5, 0.0, 0.0)),
    ];
    assert!(fit_e5_plane_axes([0.0; 3], &pairs).is_none());
    let (_, _, residual) =
        fit_rank_one_e5_plane_axes([0.0; 3], &pairs, Vector3::new(0.0, 1.0, 0.0))
            .expect("rank-one frame");
    assert!(residual < EPS_E5_DECODE_EXACT_GEOMETRY);
}

#[test]
fn e5_uv_rank_detection_ignores_roundoff_transverse_components() {
    assert!(!super::super::e5_uv_vectors_are_independent(
        [1e-15, -20.0],
        [-1e-15, 20.0],
    ));
    assert!(super::super::e5_uv_vectors_are_independent(
        [1.0, 0.0],
        [0.0, 1.0]
    ));
}

#[test]
fn e5_plane_solver_uses_known_normal_and_canonical_sign_for_rank_one_uv() {
    let tiny = 1e-15;
    let mut edges = BTreeMap::new();
    let mut pcurves = BTreeMap::new();
    let mut add_segment = |edge_ref: u32,
                           pcurve_ref: u32,
                           start_vertex: u32,
                           end_vertex: u32,
                           start: [f64; 2],
                           end: [f64; 2]| {
        edges.insert(
            edge_ref,
            E5Edge {
                support: 0,
                start_vertex,
                end_vertex,
                parameter_start: 0,
                parameter_end: 0,
                tail: Vec::new(),
            },
        );
        pcurves.insert(
            pcurve_ref,
            E5Pcurve::Line {
                surface: 100,
                origin: start,
                direction: [end[0] - start[0], end[1] - start[1]],
                range: [0.0, 1.0],
            },
        );
    };
    add_segment(3, 1, 10, 11, [tiny, -20.0], [tiny, 20.0]);
    add_segment(4, 2, 12, 13, [-tiny, -7.5], [-tiny, 7.5]);
    let topology = E5Topology {
        bodies: Vec::new(),
        faces: vec![E5Face {
            record_id: 1,
            surface: 100,
            trailer_sign: crate::families::e5::graph::Sign::Positive,
            loops: vec![E5Loop {
                record_id: 2,
                surface: 100,
                members: e5_loop_members(&[1, 2], &[3, 4], &[false, false]),
                oriented_members: None,
                outer: Some(true),
                orientation_hint: None,
            }],
        }],
        edges,
        pcurves,
        bounds: BTreeMap::new(),
        curve_supports: BTreeMap::new(),
        vertex_refs: vec![10, 11, 12, 13],
    };
    let points = vec![
        Point3::new(20.0, 0.0, 0.0),
        Point3::new(-20.0, 0.0, 0.0),
        Point3::new(7.5, 0.0, 0.0),
        Point3::new(-7.5, 0.0, 0.0),
    ];
    let (normal, u_axis, uv_scale) = super::super::solve_e5_plane_frame(
        100,
        [0.0, 0.0, 0.0],
        &topology,
        &points,
        Some(Vector3::new(0.0, 1.0, 0.0)),
    )
    .expect("rank-one plane frame");
    assert!(normal.dot(Vector3::new(0.0, 1.0, 0.0)) > 1.0 - EPS_E5_DECODE_EXACT_GEOMETRY);
    assert!(u_axis.dot(Vector3::new(0.0, 0.0, 1.0)) > 1.0 - EPS_E5_DECODE_EXACT_GEOMETRY);
    assert_eq!(uv_scale, [-1.0, -1.0]);
}
