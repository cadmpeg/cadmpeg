// SPDX-License-Identifier: Apache-2.0
//! The document tolerances a part states: the Parasolid modeller precision of
//! its bodies.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::container::sldprt_with_body;
use crate::test_support::parasolid::{
    entity51, fixture_vertex_owner_class, owned_triangle_with_resolution, untyped_triangle,
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
    let mut body = untyped_triangle(0.0);
    let typed_start = body
        .windows(3)
        .position(|window| window == [0x00, 0x0c, 0xff])
        .expect("typed ownership suffix");
    body.truncate(typed_start);
    body.extend(fixture_vertex_owner_class(900));
    body.extend(entity51(2, 500, 0x0017, &[510, 700, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 510, 0x001b, &[520, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 520, 0x001f, &[530, 0, 0, 0, 0, 0]));
    body.extend(entity51(1, 530, 0x0021, &[10, 0, 0, 0, 0, 0]));
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
