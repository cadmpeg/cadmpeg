// SPDX-License-Identifier: Apache-2.0
use super::{fixture, parameter_companion, EPS_NATIVE_FALLBACK_LINEAR};
use crate::design::dimensions::project_spatial_dimension_constraints;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn spatial_refusal(operation: &'static str, dimension: ResourceDimension) {
    let fixture = fixture();
    let companion = parameter_companion();
    let entity = fixture.spatial_entity();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        project_spatial_dimension_constraints(
            ctx,
            &inputs,
            std::slice::from_ref(&fixture.spatial),
            std::slice::from_ref(&entity),
            EPS_NATIVE_FALLBACK_LINEAR,
        )
    });
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == dimension && limit.operation == operation));
}

#[test]
fn spatial_sketch_source_refuses_work_limit() {
    spatial_refusal("f3d spatial sketch source", ResourceDimension::WorkUnits);
}

#[test]
fn spatial_placement_source_refuses_work_limit() {
    spatial_refusal("f3d spatial placement source", ResourceDimension::WorkUnits);
}

#[test]
fn spatial_sketch_lookup_refuses_work_limit() {
    spatial_refusal("f3d spatial sketch lookup", ResourceDimension::WorkUnits);
}

#[test]
fn spatial_planar_sketch_lookup_refuses_work_limit() {
    spatial_refusal(
        "f3d spatial planar sketch lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_native_curve_source_refuses_work_limit() {
    spatial_refusal(
        "f3d spatial native curve source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_projected_entity_source_refuses_work_limit() {
    spatial_refusal(
        "f3d spatial projected entity source",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_native_record_lookup_refuses_work_limit() {
    spatial_refusal(
        "f3d spatial native record lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_projected_sketch_lookup_refuses_work_limit() {
    spatial_refusal(
        "f3d spatial projected sketch lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_owner_source_refuses_work_limit() {
    spatial_refusal("f3d spatial owner source", ResourceDimension::WorkUnits);
}

#[test]
fn spatial_companion_source_refuses_work_limit() {
    spatial_refusal("f3d spatial companion source", ResourceDimension::WorkUnits);
}

#[test]
fn spatial_owner_record_lookup_refuses_work_limit() {
    spatial_refusal(
        "f3d spatial owner record lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_companion_record_lookup_refuses_work_limit() {
    spatial_refusal(
        "f3d spatial companion record lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_scope_sketch_lookup_refuses_work_limit() {
    spatial_refusal(
        "f3d spatial scope sketch lookup",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn spatial_sketch_identity_index_refuses_materialized_limit() {
    spatial_refusal(
        "f3d spatial sketch identity index",
        ResourceDimension::MaterializedBytes,
    );
}

#[test]
fn spatial_planar_sketch_index_refuses_materialized_limit() {
    spatial_refusal(
        "f3d spatial planar sketch index",
        ResourceDimension::MaterializedBytes,
    );
}

#[test]
fn spatial_scope_sketch_index_refuses_materialized_limit() {
    spatial_refusal(
        "f3d spatial scope sketch index",
        ResourceDimension::MaterializedBytes,
    );
}

#[test]
fn spatial_native_record_index_refuses_materialized_limit() {
    spatial_refusal(
        "f3d spatial native record index",
        ResourceDimension::MaterializedBytes,
    );
}

#[test]
fn spatial_projected_record_index_refuses_materialized_limit() {
    spatial_refusal(
        "f3d spatial projected record index",
        ResourceDimension::MaterializedBytes,
    );
}

#[test]
fn spatial_owner_record_index_refuses_collection_limit() {
    spatial_refusal(
        "f3d spatial owner record index",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn spatial_companion_record_index_refuses_collection_limit() {
    spatial_refusal(
        "f3d spatial companion record index",
        ResourceDimension::CollectionItems,
    );
}

fn placement_storage_is_released(admitted: bool) {
    const SCRATCH_LIMIT: u64 = 4096;
    let fixture = fixture();
    let placements = vec![fixture.placement.clone(); 128];
    let mut inputs = fixture.inputs();
    inputs.placements = &placements;
    inputs.parameters = &[];
    inputs.owners = &[];
    inputs.curves = &[];
    inputs.entities = &[];
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_LIMIT;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let sketches = if admitted {
            std::slice::from_ref(&fixture.spatial)
        } else {
            &[]
        };
        let output = project_spatial_dimension_constraints(
            ctx,
            &inputs,
            sketches,
            &[],
            EPS_NATIVE_FALLBACK_LINEAR,
        )
        .unwrap();
        assert!(output.is_empty());
        drop(output);
        let _released = ctx
            .reserve_scoped(SCRATCH_LIMIT, "released spatial index storage")
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn admitted_duplicate_spatial_placements_release_scratch_storage() {
    placement_storage_is_released(true);
}

#[test]
fn rejected_duplicate_spatial_placements_release_scratch_storage() {
    placement_storage_is_released(false);
}

#[test]
fn spatial_owner_and_companion_indexes_release_all_storage() {
    const SCRATCH_LIMIT: u64 = 4096;
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.placements = &[];
    inputs.parameters = &[];
    inputs.curves = &[];
    inputs.entities = &[];
    inputs.companions = std::slice::from_ref(&companion);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_LIMIT;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let output = project_spatial_dimension_constraints(
            ctx,
            &inputs,
            &[],
            &[],
            EPS_NATIVE_FALLBACK_LINEAR,
        )
        .unwrap();
        assert!(output.is_empty());
        drop(output);
        let _released = ctx
            .reserve_scoped(SCRATCH_LIMIT, "released spatial owner index storage")
            .unwrap();
        assert!(ctx.resource_refusal().is_none());
    });
}

#[test]
fn spatial_native_point_source_refuses_before_filtering() {
    use crate::records::sketch_geometry::{
        SketchPoint, SketchPointClosure, SketchPointCompanion, SketchPointDraft,
        SketchPointRecordForm,
    };
    let fixture = fixture();
    let point = SketchPoint::try_from(SketchPointDraft {
        id: "unscoped-point#40".into(),
        record_index: 40,
        owner_reference: None,
        class_tag: fixture.curve.class_tag.clone(),
        byte_offset: 0,
        coordinate_offset: 0,
        record_form: SketchPointRecordForm::version11(
            40,
            SketchPointClosure::Selector0State0,
            None,
            0.0,
        ),
        companion: SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        paired_reference: 0,
        coordinates: cadmpeg_ir::math::Point2::new(0.0, 0.0),
    })
    .unwrap();
    assert!(crate::ids::native_stream(&point.id).is_none());
    let mut inputs = fixture.inputs();
    inputs.points = std::slice::from_ref(&point);
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d spatial native point source",
        0,
        |ctx| {
            project_spatial_dimension_constraints(
                ctx,
                &inputs,
                std::slice::from_ref(&fixture.spatial),
                &[],
                EPS_NATIVE_FALLBACK_LINEAR,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits && limit.operation == "f3d spatial native point source"));
}
