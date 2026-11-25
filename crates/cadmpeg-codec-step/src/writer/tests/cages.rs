// SPDX-License-Identifier: Apache-2.0
//! A cage body is left out with its cage, and the loss that names the cage
//! names the body (§5.6).

use cadmpeg_ir::examples::unit_cube;
use cadmpeg_test_support::cages::{assert_cage_body_dropped, push_cage_body};

use crate::export::write_step;
use crate::{StepSchema, StepWriteOptions};

#[test]
fn a_cage_body_is_left_out_with_its_cage() {
    let mut ir = unit_cube().expect("unit cube fixture is admitted");
    let cage = push_cage_body(&mut ir, "step");
    let mut bytes = Vec::new();
    let report = write_step(
        &ir,
        &mut bytes,
        StepSchema::Ap214,
        &StepWriteOptions::default(),
    )
    .unwrap();
    assert_cage_body_dropped(&report, &cage, "MANIFOLD_SOLID_BREP", 1);
    let text = String::from_utf8(bytes).unwrap();
    assert_eq!(text.matches("MANIFOLD_SOLID_BREP(").count(), 1, "{text}");
}
