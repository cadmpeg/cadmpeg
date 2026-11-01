// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod body_selection;
mod coil;
mod direct_face;
mod dispatcher;
mod extrude_extents;
mod extrude_fallbacks;
mod fixed_extrude;
mod hole;
mod loft;
mod mirror;
mod native_scope_definition;
mod parameter_cycles;
mod parameters;
mod pattern;
mod pipe;
mod primitives;
mod replace_face;
mod revolve;
mod scope_properties;
mod sheet_metal;
mod sketch_dependencies;
mod spatial_profiles;
mod split;
mod surface;
mod sweep;
mod timeline;
mod treatments;
mod work_plane;
mod work_point_binding;

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
    assert!((rotation.angle.literal().unwrap().get() - theta).abs() <= 4.0 * f64::EPSILON);
}

fn parameter_document(native: &[crate::records::parameters::DesignParameter]) -> cadmpeg_ir::CadIr {
    let projection = crate::test_support::with_decode_context(|ctx| {
        super::project_parameter_design_with_edge_identities(
            ctx,
            &super::ProjectInputs {
                native,
                ..Default::default()
            },
        )
    })
    .unwrap();
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model.parameters = projection.parameters;
    ir
}

fn dimension_value<Q: cadmpeg_ir::features::DimensionValue>(
    dimension: &cadmpeg_ir::Dimension<Q>,
    native: &[crate::records::parameters::DesignParameter],
) -> Q {
    parameter_document(native)
        .dimension_value(dimension)
        .expect("canonical test parameter or literal")
}
