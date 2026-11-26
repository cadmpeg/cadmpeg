// SPDX-License-Identifier: Apache-2.0
//! The document tolerances a part states: the Parasolid modeller precision of
//! its bodies.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::{
    bridge, coedge, edge_use, loop_head, owned_triangle_with_resolution, plane_carrier, vertex_use,
    world_point,
};
use crate::SldprtCodec;

const MODELLER_DEFAULT_LINEAR_MM: f64 = 1.0e-5;
const MODELLER_ANGULAR_RADIANS: f64 = 1.0e-11;
const COMPARISON_SLACK: f64 = 1.0e-18;

/// The body count and the stated linear and angular tolerances of `body`.
fn tolerances(body: &[u8]) -> (usize, f64, f64) {
    let decoded = SldprtCodec
        .decode(
            &mut Cursor::new(sldprt_with_body(body)),
            &DecodeOptions::default(),
        )
        .expect("synthetic part decodes");
    let ir = decoded.ir();
    (
        ir.geometry.bodies.len(),
        ir.tolerances.linear.get(),
        ir.tolerances.angular.get(),
    )
}

#[test]
fn bodies_with_two_stored_resolutions_state_the_larger() {
    let mut body = owned_triangle_with_resolution(0, 700, 0.0, 1.0e-8);
    body.extend(owned_triangle_with_resolution(200, 701, 2.0, 1.0e-6));
    let (bodies, linear, angular) = tolerances(&body);
    assert_eq!(bodies, 2);
    assert!((linear - 1.0e-3).abs() <= COMPARISON_SLACK, "{linear}");
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
}

#[test]
fn faces_without_a_body_node_state_the_modeller_default() {
    let mut body = plane_carrier(100, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    body.extend(bridge(10, 20, 100));
    body.extend(loop_head(20, 30, 10));
    body.extend(coedge(30, 20, 31, 50, 0, 40, false));
    body.extend(coedge(31, 20, 32, 51, 0, 41, false));
    body.extend(coedge(32, 20, 30, 52, 0, 42, false));
    for edge in [40, 41, 42] {
        body.extend(edge_use(edge, 0));
    }
    body.extend(vertex_use(50, 60));
    body.extend(vertex_use(51, 61));
    body.extend(vertex_use(52, 62));
    body.extend(world_point(60, [0.0, 0.0, 0.0]));
    body.extend(world_point(61, [1.0, 0.0, 0.0]));
    body.extend(world_point(62, [0.0, 1.0, 0.0]));
    let (bodies, linear, angular) = tolerances(&body);
    assert_eq!(bodies, 1);
    assert!(
        (linear - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK,
        "{linear}"
    );
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
}

#[test]
fn a_negative_stored_resolution_is_not_a_tolerance() {
    let (bodies, linear, angular) =
        tolerances(&owned_triangle_with_resolution(0, 700, 0.0, -1.0e-6));
    assert_eq!(bodies, 1);
    assert!(
        (linear - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK,
        "{linear}"
    );
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
}
