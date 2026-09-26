// SPDX-License-Identifier: Apache-2.0

use crate::families::e5::decode::{
    fit_e5_plane_axes, fit_rank_one_e5_plane_axes, EPS_E5_DECODE_EXACT_GEOMETRY,
};
use crate::families::e5::graph::{E5Edge, E5Face, E5Loop, E5Pcurve, E5Topology};
use crate::families::e5::tests::e5_loop_members;
use crate::test_support::test_b5::{finite_pair, point};
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::units::FiniteVector;
use std::collections::BTreeMap;

fn uv(values: [f64; 2]) -> FiniteVector<2> {
    FiniteVector::new(values).expect("finite fixture pair")
}

#[test]
fn plane_axis_fit_is_uv_scale_independent() {
    for scale in [2.0, 1e-200] {
        let pairs = [
            (uv([scale, 0.0]), point([scale, 0.0, 0.0])),
            (uv([0.0, scale]), point([0.0, scale, 0.0])),
            (uv([scale, scale]), point([scale, scale, 0.0])),
        ];
        let (u_axis, v_axis, residual) =
            fit_e5_plane_axes(point([0.0; 3]), &pairs).expect("full-rank frame");
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
            (uv([0.0, -scale]), point([-scale, 0.0, 0.0])),
            (uv([0.0, scale]), point([scale, 0.0, 0.0])),
        ];
        let (u_axis, v_axis, residual) =
            fit_rank_one_e5_plane_axes(point([0.0; 3]), &pairs, Vector3::new(0.0, 1.0, 0.0))
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
        (uv([tiny, -20.0]), point([-20.0, 0.0, 0.0])),
        (uv([tiny, 20.0]), point([20.0, 0.0, 0.0])),
        (uv([-tiny, -7.5]), point([-7.5, 0.0, 0.0])),
        (uv([tiny, 7.5]), point([7.5, 0.0, 0.0])),
    ];
    assert!(fit_e5_plane_axes(point([0.0; 3]), &pairs).is_none());
    let (_, _, residual) =
        fit_rank_one_e5_plane_axes(point([0.0; 3]), &pairs, Vector3::new(0.0, 1.0, 0.0))
            .expect("rank-one frame");
    assert!(residual < EPS_E5_DECODE_EXACT_GEOMETRY);
}

#[test]
fn e5_uv_rank_detection_ignores_roundoff_transverse_components() {
    assert!(!super::super::e5_uv_vectors_are_independent(
        uv([1e-15, -20.0]),
        uv([-1e-15, 20.0]),
    ));
    assert!(super::super::e5_uv_vectors_are_independent(
        uv([1.0, 0.0]),
        uv([0.0, 1.0])
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
                origin: finite_pair(start),
                direction: finite_pair([end[0] - start[0], end[1] - start[1]]),
                range: finite_pair([0.0, 1.0]),
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
        point([20.0, 0.0, 0.0]),
        point([-20.0, 0.0, 0.0]),
        point([7.5, 0.0, 0.0]),
        point([-7.5, 0.0, 0.0]),
    ];
    let (normal, u_axis, uv_scale) = super::super::solve_e5_plane_frame(
        100,
        point([0.0, 0.0, 0.0]),
        &topology,
        &points,
        Some(Vector3::new(0.0, 1.0, 0.0)),
    )
    .expect("rank-one plane frame");
    assert!(normal.as_raw().dot(Vector3::new(0.0, 1.0, 0.0)) > 1.0 - EPS_E5_DECODE_EXACT_GEOMETRY);
    assert!(u_axis.as_raw().dot(Vector3::new(0.0, 0.0, 1.0)) > 1.0 - EPS_E5_DECODE_EXACT_GEOMETRY);
    assert_eq!(uv_scale, finite_pair([-1.0, -1.0]));
}

#[test]
fn e5_plane_solver_rechecks_the_returned_unit_frame() {
    let sites = [[1.0, 1.0], [3.0, 1.0], [3.0, 4.0], [1.0, 4.0]];
    let mut edges = BTreeMap::new();
    let mut pcurves = BTreeMap::new();
    for index in 0..4 {
        let next = (index + 1) % 4;
        let key = index as u32;
        edges.insert(
            key,
            E5Edge {
                support: 0,
                start_vertex: key,
                end_vertex: next as u32,
                parameter_start: 0,
                parameter_end: 0,
                tail: Vec::new(),
            },
        );
        pcurves.insert(
            key,
            E5Pcurve::Line {
                surface: 100,
                origin: finite_pair(sites[index]),
                direction: finite_pair([
                    sites[next][0] - sites[index][0],
                    sites[next][1] - sites[index][1],
                ]),
                range: finite_pair([0.0, 1.0]),
            },
        );
    }
    let topology = E5Topology {
        bodies: Vec::new(),
        faces: vec![E5Face {
            record_id: 1,
            surface: 100,
            trailer_sign: crate::families::e5::graph::Sign::Positive,
            loops: vec![E5Loop {
                record_id: 2,
                surface: 100,
                members: e5_loop_members(&[0, 1, 2, 3], &[0, 1, 2, 3], &[false; 4]),
                oriented_members: None,
                outer: Some(true),
                orientation_hint: None,
            }],
        }],
        edges,
        pcurves,
        bounds: BTreeMap::new(),
        curve_supports: BTreeMap::new(),
        vertex_refs: vec![0, 1, 2, 3],
    };
    for scale in [1.0, 2.0] {
        let points = sites.map(|[u, v]| point([u * scale, v * scale, 0.0]));
        let result = super::super::solve_e5_plane_frame(
            100,
            point([0.0; 3]),
            &topology,
            &points,
            Some(Vector3::new(0.0, 0.0, 1.0)),
        );
        if scale == 1.0 {
            let (normal, u_axis, uv_scale) = result.expect("unit plane chart");
            let v_axis = normal.as_raw().cross(*u_axis.as_raw());
            for ([u, v], expected) in sites.into_iter().zip(points) {
                let mapped = u_axis.as_raw().scale(u * uv_scale[0].get())
                    + v_axis.scale(v * uv_scale[1].get());
                assert!(
                    crate::math::distance(<[f64; 3]>::from(mapped), expected.get().into())
                        < EPS_E5_DECODE_EXACT_GEOMETRY
                );
            }
        } else {
            assert!(
                result.is_none(),
                "normalizing scale-two axes would move the endpoints"
            );
        }
    }
}

#[test]
fn plane_frame_residual_rejects_nonfinite_predictions() {
    let residual = super::super::plane_frame_residual(
        [0.0; 3],
        &[(uv([f64::MAX, f64::MAX]), point([0.0, 0.0, 0.0]))],
        Vector3::new(f64::MAX, 0.0, 0.0),
        Vector3::new(-f64::MAX, 0.0, 0.0),
    );
    assert_eq!(residual, f64::INFINITY);
}

#[test]
fn e5_native_uv_endpoints_hand_back_admitted_pairs() {
    let line = |direction: [f64; 2]| E5Pcurve::Line {
        surface: 100,
        origin: finite_pair([1.0, 2.0]),
        direction: finite_pair(direction),
        range: finite_pair([0.0, 2.0]),
    };
    assert_eq!(
        super::super::e5_native_uv_endpoints(&line([3.0, -1.0])),
        Some([uv([1.0, 2.0]), uv([7.0, 0.0])])
    );
    assert_eq!(
        super::super::e5_native_uv_endpoints(&line([f64::MAX, 0.0])),
        None
    );
}
