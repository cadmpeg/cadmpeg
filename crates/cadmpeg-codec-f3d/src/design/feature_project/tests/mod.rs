// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod body_selection;
mod coil;
mod direct_face_limits;
mod dispatcher;
mod extrude;
mod extrude_limits;
mod hole_limits;
mod loft_limits;
mod mirror;
mod native_scope_definition_limits;
mod parameter_cycles;
mod parameters;
mod pattern;
mod pipe;
mod replace_face;
mod revolve_limits;
mod scope_properties_limits;
mod sheet_metal;
mod spatial_profiles;
mod split;
mod surface;
mod sweep_limits;
mod timeline;
mod treatments;
mod work_plane_limits;
mod work_point_binding_limits;

#[test]
fn audit_regression_near_half_turn_retains_negative_axis() {
    let theta = std::f64::consts::PI - 5.0e-9;
    let (sine, cosine) = theta.sin_cos();
    let matrix = [
        [cosine, sine, 0., 0.],
        [-sine, cosine, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    let rotation = super::matrix_axis_angle(&matrix).unwrap();
    assert!(rotation.direction.z < 0.0);
    assert!((rotation.angle.get() - theta).abs() <= 4.0 * f64::EPSILON);
}
