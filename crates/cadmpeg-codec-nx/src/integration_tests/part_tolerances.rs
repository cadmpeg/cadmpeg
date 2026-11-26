// SPDX-License-Identifier: Apache-2.0
//! The document tolerances an NX part states: the Parasolid modeller
//! precision of its bodies.

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::test_prt::{single_part_prt, topology_part_prt};
use crate::NxCodec;

const MODELLER_DEFAULT_LINEAR_MM: f64 = 1.0e-5;
const MODELLER_ANGULAR_RADIANS: f64 = 1.0e-11;
const COMPARISON_SLACK: f64 = 1.0e-25;

fn tolerances(bytes: Vec<u8>) -> (usize, f64, f64) {
    let decoded = NxCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("synthetic part decodes");
    let ir = decoded.ir();
    (
        ir.geometry.bodies.len(),
        ir.tolerances.linear.get(),
        ir.tolerances.angular.get(),
    )
}

#[test]
fn bodies_without_a_stored_resolution_state_the_modeller_default() {
    let (bodies, linear, angular) = tolerances(topology_part_prt());
    assert!(bodies > 0);
    assert!((linear - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK);
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
}

#[test]
fn a_part_without_bodies_states_the_modeller_precision() {
    let (bodies, linear, angular) = tolerances(single_part_prt());
    assert_eq!(bodies, 0);
    assert!((linear - MODELLER_DEFAULT_LINEAR_MM).abs() <= COMPARISON_SLACK);
    assert!((angular - MODELLER_ANGULAR_RADIANS).abs() <= COMPARISON_SLACK);
}
