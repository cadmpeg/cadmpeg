// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args
)]

mod linear;
mod atomic_limits;
mod frame_relations;
mod limits;
mod offset;
mod offset_binding;
mod owner_scoped;
mod recipe;
mod relations;

fn project_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
) -> Vec<cadmpeg_ir::sketches::SketchConstraint> {
    crate::design::dimensions::project_dimension_constraints(None, inputs, spatial_sketches, 1.0e-6)
        .expect("resource allocation did not fail")
}

fn project_spatial_dimension_constraints(
    inputs: &crate::design::dimensions::DimensionConstraintInputs<'_>,
    spatial_sketches: &[cadmpeg_ir::sketches::SpatialSketch],
    spatial_entities: &[cadmpeg_ir::sketches::SpatialSketchEntity],
) -> Vec<cadmpeg_ir::sketches::SpatialSketchConstraint> {
    crate::design::dimensions::project_spatial_dimension_constraints(
        None,
        inputs,
        spatial_sketches,
        spatial_entities,
        1.0e-6,
    ).expect("resource allocation did not fail")
}

mod numerical_ranges;
mod parameter_iteration;

fn assert_dimension_refusal(
    operation: &'static str,
    dimension: cadmpeg_core::decode::ResourceDimension,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<(), cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let invoke = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = limit,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = limit,
            _ => panic!("unsupported dimension refusal limit"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        run(&ctx)
    };
    for limit in 0..4096 {
        match invoke(limit) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation && failure.dimension == dimension => {
                let below = failure.used.checked_add(failure.additional).unwrap() - 1;
                assert!(matches!(invoke(below), Err(CodecError::ResourceLimit(failure))
                    if failure.operation == operation && failure.dimension == dimension));
                return;
            }
            Err(CodecError::ResourceLimit(_)) => {},
            result => panic!("missing {operation} refusal: {result:?}"),
        }
    }
    panic!("no {operation} refusal");
}

mod spatial_parallel_limits;

mod spatial_carrier_limits;

mod spatial_repeated_limits;

mod spatial_reflection_limits;

mod null_locus_limits;

mod two_locus_limits;

mod nurbs_containment_limits;
