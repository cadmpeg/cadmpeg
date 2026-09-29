// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod body_selection;
mod coil;
mod dispatcher;
mod extrude;
mod form;
mod mirror;
mod parameter_cycles;
mod parameters;
mod pattern;
mod pipe;
mod replace_face;
mod sheet_metal;
mod spatial_profiles;
mod sketch_binding_limits;
mod split;
mod surface;
mod timeline;
mod treatments;
mod work_point_binding_limits;

fn project_single_scope_with_context(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Result<(Vec<cadmpeg_ir::features::Feature>, Vec<cadmpeg_ir::features::DesignParameter>), cadmpeg_core::CodecError> {
    let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
    let timeline = crate::records::entity_header::DesignFeatureTimeline::try_new(
        crate::ids::native_design_feature_timeline_id_in_stream(stream, 0),
        crate::records::entity_header::DesignTimelineFrame::test_items(
            0, vec![crate::records::identity::Located {
                value: u64::from(scope.record_index), offset: 0,
            }],
        ),
        "256".to_owned().try_into().unwrap(),
        std::num::NonZeroU64::new(1).unwrap(),
        0,
        std::num::NonZeroU64::new(1).unwrap(),
    ).unwrap();
    crate::design::feature_project::project_parameter_design_with_edge_identities(
        Some(ctx), &crate::design::feature_project::ProjectInputs {
            native: &[], owners: &[], scopes: std::slice::from_ref(scope),
            timelines: std::slice::from_ref(&timeline), construction_groups: &[],
            fillet_radius_groups: &[], edge_operands: &[], edge_identity_operands: &[],
            edge_treatment_vertex_operands: &[], entity_selection_operands: &[],
            curve_identities: &[], face_operands: &[], body_recipe_operands: &[],
            legacy_loft_body_carriers: &[], placements: &[], body_bindings: &[],
            component_naming_spaces: &[], histories: &[],
        },
    )
}

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
