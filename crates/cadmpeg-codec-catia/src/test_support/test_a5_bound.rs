// SPDX-License-Identifier: Apache-2.0
//! a5 bound-edge and topology-run synthetic stream builders.

#![allow(clippy::unwrap_used)]
use super::{
    a5_native_edge_identity_stream, a5_pcurve_stream, a5_pcurve_stream_with_support_and_uv,
    a5_pcurve_stream_with_uv, a5_surface_stream_with_poles, b2_circle_stream, b2_cone_stream,
    b2_cylinder_stream, b2_edge_node_stream, b2_edge_parameter_stream_for, b2_sphere_stream,
    b2_torus_stream,
};

pub(crate) fn a5_circle_bound_edge_stream() -> Vec<u8> {
    let radius = 3.0;
    let arc = [0.0, 2.0 * std::f64::consts::PI * radius];
    let mut bytes = a5_pcurve_stream_with_uv(arc, [2.0, 2.0]);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(arc, [2.0, 2.0]));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&b2_circle_stream());
    bytes
}

pub(crate) fn a5_cone_bound_edge_stream() -> Vec<u8> {
    let u = [0.0f64, 1.0];
    let v = [2.0f64, 3.0];
    let mut bytes = a5_pcurve_stream_with_uv(u, v);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(u, v));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&b2_cone_stream());
    for (u, v) in u.into_iter().zip(v) {
        let phi = u / 3.0;
        let point = [
            1.0 + v * 0.25f64.sin() * phi.cos(),
            2.0 + v * 0.25f64.sin() * phi.sin(),
            3.0 + v * 0.25f64.cos(),
        ];
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&(value as f32).to_le_bytes());
        }
    }
    bytes
}

pub(crate) fn a5_torus_bound_edge_stream() -> Vec<u8> {
    let major_scale = 14.0;
    let u = [
        major_scale * std::f64::consts::FRAC_PI_2,
        major_scale * std::f64::consts::PI,
    ];
    let v = [0.0, 0.0];
    let mut bytes = a5_pcurve_stream_with_uv(u, v);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(u, v));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
    bytes.extend_from_slice(&b2_torus_stream());
    for point in [[1.0f32, 11.0, 3.0], [-8.0, 2.0, 3.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

pub(crate) fn a5_sphere_bound_edge_stream() -> Vec<u8> {
    let u = [0.0, std::f64::consts::FRAC_PI_2];
    let v = [0.0, 0.0];
    let mut bytes = a5_pcurve_stream_with_uv(u, v);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(u, v));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
    bytes.extend_from_slice(&b2_sphere_stream());
    for point in [[6.0f32, 2.0, 3.0], [1.0, 7.0, 3.0]] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

pub(crate) fn a5_edge_block_stream() -> Vec<u8> {
    let mut bytes = a5_pcurve_stream();
    bytes.extend_from_slice(&a5_pcurve_stream());
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes
}

pub(crate) fn a5_topology_edge_run_stream() -> Vec<u8> {
    let mut bytes = a5_edge_block_stream();
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 5, 9, 0x84]);
    bytes.extend_from_slice(&[0xb2, 0x03, 0x06, 0x04, 0x05, 0x82, 9, 13, 0x88]);
    bytes.extend_from_slice(&b2_edge_node_stream());
    bytes
}

pub(crate) fn a5_native_edge_run_stream(curve: u8, start: u8, end: u8) -> Vec<u8> {
    a5_native_edge_run_stream_with_support(curve, start, end, 0x1234)
}

pub(crate) fn a5_native_edge_run_stream_with_support(
    curve: u8,
    start: u8,
    end: u8,
    support_id: u32,
) -> Vec<u8> {
    assert!(curve >= 3);
    let mut bytes = a5_pcurve_stream_with_support_and_uv(support_id, [0.0, 1.0], [0.0, 1.0]);
    bytes.extend_from_slice(&a5_pcurve_stream_with_support_and_uv(
        support_id,
        [0.0, 1.0],
        [0.0, 1.0],
    ));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&a5_native_edge_identity_stream(curve, start, end));
    bytes
}

pub(crate) fn a5_cylinder_bound_edge_stream() -> Vec<u8> {
    let mut bytes = a5_edge_block_stream();
    bytes.extend_from_slice(&b2_cylinder_stream());
    let endpoints = [
        [1.0f32, 4.0, 3.0],
        [2.0, (2.0 + 2.0 * 0.5f32.cos()), (3.0 + 2.0 * 0.5f32.sin())],
    ];
    for point in endpoints {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes
}

pub(crate) fn a5_nurbs_bound_edge_stream(offset: f64) -> Vec<u8> {
    let cylinder_uv = ([0.0f64, 1.0], [0.0f64, 1.0]);
    let surface_uv = ([0.0f64, 1.0], [0.0f64, 0.0]);
    let p0 = [1.0, 4.0, 3.0];
    let p1 = [2.0, 2.0 + 2.0 * 0.5f64.cos(), 3.0 + 2.0 * 0.5f64.sin()];
    let normal = {
        let u = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        let v = [0.0f64, 0.0, 1.0];
        let cross = [u[1] * v[2] - u[2] * v[1], -u[0] * v[2], 0.0];
        let length = cross[0].hypot(cross[1]);
        [cross[0] / length, cross[1] / length, 0.0]
    };
    let shifted = |point: [f64; 3]| {
        [
            point[0] - offset * normal[0],
            point[1] - offset * normal[1],
            point[2],
        ]
    };
    let s0 = shifted(p0);
    let s1 = shifted(p1);
    let mut bytes = a5_pcurve_stream_with_uv(cylinder_uv.0, cylinder_uv.1);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv(surface_uv.0, surface_uv.1));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    bytes.extend_from_slice(&a5_native_edge_identity_stream(6, 139, 142));
    bytes.extend_from_slice(&b2_cylinder_stream());
    bytes.extend_from_slice(&a5_surface_stream_with_poles([
        s0,
        [s0[0], s0[1], s0[2] + 1.0],
        s1,
        [s1[0], s1[1], s1[2] + 1.0],
    ]));
    for point in [p0, p1] {
        bytes.extend_from_slice(&[0x05, 0x08, 0x01]);
        for value in point {
            bytes.extend_from_slice(&(value as f32).to_le_bytes());
        }
    }
    bytes
}

pub(crate) fn a5_nurbs_pair_bound_edge_stream(duplicate_first_surface: bool) -> Vec<u8> {
    let p0 = [1.0, 2.0, 3.0];
    let p1 = [4.0, 5.0, 6.0];
    let mut bytes = a5_pcurve_stream_with_uv([0.0, 1.0], [0.0, 0.0]);
    bytes.extend_from_slice(&a5_pcurve_stream_with_uv([0.0, 0.0], [0.0, 1.0]));
    bytes.extend_from_slice(&b2_edge_parameter_stream_for(0.0, 1.0));
    let first_surface = a5_surface_stream_with_poles([
        p0,
        [p0[0], p0[1], p0[2] + 1.0],
        p1,
        [p1[0], p1[1], p1[2] + 1.0],
    ]);
    bytes.extend_from_slice(&first_surface);
    bytes.extend_from_slice(&a5_surface_stream_with_poles([
        p0,
        p1,
        [p0[0], p0[1] + 1.0, p0[2]],
        [p1[0], p1[1] + 1.0, p1[2]],
    ]));
    if duplicate_first_surface {
        bytes.extend_from_slice(&first_surface);
    }
    bytes
}
