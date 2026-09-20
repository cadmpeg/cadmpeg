// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod linear;
mod offset;
mod owner_scoped;
mod recipe;
mod relations;

fn project_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    crate::design::dimensions::project_dimension_constraints(inputs, spatial_sketches, 1.0e-6)
}

fn project_spatial_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    crate::design::dimensions::project_spatial_dimension_constraints(
        inputs,
        spatial_sketches,
        spatial_entities,
        1.0e-6,
    )
}

mod numerical_ranges;
