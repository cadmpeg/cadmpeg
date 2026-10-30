// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::compare::floats_agree;

use super::circular_cone_normal_curvature;

#[test]
fn a_circular_cone_generatrix_is_straight() {
    assert_eq!(circular_cone_normal_curvature(1, 1.0, 2.0, 0.5), Some(0.0));
}

#[test]
fn a_circular_cone_parallel_uses_the_section_radius() {
    let radius = 2.0_f64;
    let half_angle = 0.5_f64;
    let v = 1.5_f64;
    let rho = radius + v * half_angle.tan();
    let expected = half_angle.cos().abs() / rho.abs();
    assert!(floats_agree(
        circular_cone_normal_curvature(0, v, radius, half_angle).expect("finite"),
        expected
    ));
}

#[test]
fn a_circular_cone_apex_has_no_finite_parallel_curvature() {
    let radius = 2.0_f64;
    let half_angle = 0.5_f64;
    let apex = -radius / half_angle.tan();
    assert_eq!(
        circular_cone_normal_curvature(0, apex, radius, half_angle),
        None
    );
}
