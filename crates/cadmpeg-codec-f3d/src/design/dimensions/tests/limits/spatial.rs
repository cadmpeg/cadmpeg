// SPDX-License-Identifier: Apache-2.0
use super::{fixture, parameter_companion};
use crate::design::dimensions::project_spatial_dimension_constraints;
use crate::records::parameters::DesignCompanionPayload;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

const EPS_SPATIAL_DIMENSION_LINEAR: f64 = 1.0e-6;

fn assert_spatial_index_refusal(operation: &'static str) {
    let fixture = fixture();
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        match project_spatial_dimension_constraints(
            &ctx,
            &fixture.inputs(),
            std::slice::from_ref(&fixture.spatial),
            &[],
            EPS_SPATIAL_DIMENSION_LINEAR,
        ) {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::CollectionItems =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal, got success"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn spatial_planar_sketch_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial planar sketch index");
}

#[test]
fn spatial_scope_sketch_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial scope sketch index");
}

#[test]
fn spatial_native_record_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial native record index");
}

#[test]
fn spatial_projected_record_index_refuses_collection_limit() {
    let fixture = fixture();
    let entity = fixture.spatial_entity();
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(
            &ctx,
            &fixture.inputs(),
            std::slice::from_ref(&fixture.spatial),
            std::slice::from_ref(&entity),
            EPS_SPATIAL_DIMENSION_LINEAR,
        );
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d spatial projected record index"
                    && failure.dimension == ResourceDimension::CollectionItems =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected spatial projected record index refusal"),
            Err(error) => panic!("expected spatial projected record index refusal: {error}"),
        }
    }
    panic!("no spatial projected record index refusal");
}

#[test]
fn spatial_parameter_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial parameter index");
}

#[test]
fn spatial_parameter_count_index_refuses_collection_limit() {
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    for limit in 0..64 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(
            &ctx,
            &inputs,
            std::slice::from_ref(&fixture.spatial),
            &[],
            EPS_SPATIAL_DIMENSION_LINEAR,
        );
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == "f3d spatial parameter count index"
                    && failure.dimension == ResourceDimension::CollectionItems =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected spatial parameter count index refusal"),
            Err(error) => panic!("expected spatial parameter count index refusal: {error}"),
        }
    }
    panic!("no spatial parameter count index refusal");
}

#[test]
fn spatial_scope_sketch_index_refuses_work_limit() {
    let fixture = fixture();
    let inputs = fixture.inputs();
    let error = crate::test_support::resource_refusal_at(
        ResourceDimension::WorkUnits,
        "f3d spatial scope sketch index",
        0,
        |ctx| {
            project_spatial_dimension_constraints(
                ctx,
                &inputs,
                std::slice::from_ref(&fixture.spatial),
                &[],
                EPS_SPATIAL_DIMENSION_LINEAR,
            )
        },
    );
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == "f3d spatial scope sketch index"
            && failure.dimension == ResourceDimension::WorkUnits));
}

#[test]
fn spatial_owner_record_index_refuses_collection_limit() {
    assert_spatial_index_refusal("f3d spatial owner record index");
}

fn assert_spatial_companion_collection_refusal(operation: &'static str) {
    let fixture = fixture();
    let entity = fixture.spatial_entity();
    let companion = parameter_companion().bound(DesignCompanionPayload::new(58, 1, Vec::new()));
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    inputs.entities = &[];
    for limit in 0..128 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::default();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = project_spatial_dimension_constraints(
            &ctx,
            &inputs,
            std::slice::from_ref(&fixture.spatial),
            std::slice::from_ref(&entity),
            EPS_SPATIAL_DIMENSION_LINEAR,
        );
        match result {
            Err(CodecError::ResourceLimit(failure))
                if failure.operation == operation
                    && failure.dimension == ResourceDimension::CollectionItems =>
            {
                return
            }
            Err(CodecError::ResourceLimit(_)) => {}
            Ok(_) => panic!("expected {operation} refusal"),
            Err(error) => panic!("expected {operation} refusal: {error}"),
        }
    }
    panic!("no {operation} refusal");
}

#[test]
fn retained_spatial_parameter_index_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d retained spatial parameter index");
}

#[test]
fn spatial_companion_record_index_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d spatial companion record index");
}

#[test]
fn spatial_source_parameter_index_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d spatial source parameter index");
}

#[test]
fn projected_spatial_dimension_output_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d projected spatial dimension output");
}

#[test]
fn spatial_line_length_match_refuses_collection_limit() {
    assert_spatial_companion_collection_refusal("f3d spatial line length match");
}

fn assert_spatial_companion_retained_refusal(operation: &'static str) {
    let fixture = fixture();
    let entity = fixture.spatial_entity();
    let companion = parameter_companion().bound(DesignCompanionPayload::new(58, 1, Vec::new()));
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    inputs.entities = &[];
    {
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::RetainedBytes,
            operation,
            |cap| {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = cap;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = project_spatial_dimension_constraints(
                    &ctx,
                    &inputs,
                    std::slice::from_ref(&fixture.spatial),
                    std::slice::from_ref(&entity),
                    EPS_SPATIAL_DIMENSION_LINEAR,
                );
                result
            },
        );
        assert!(matches!(error, CodecError::ResourceLimit(failure)
            if failure.operation == operation && failure.dimension == (ResourceDimension::RetainedBytes)));
    }
}

#[test]
fn projected_spatial_sketch_id_refuses_retained_limit() {
    assert_spatial_companion_retained_refusal("f3d projected spatial sketch id");
}

#[test]
fn spatial_line_length_entity_id_refuses_retained_limit() {
    assert_spatial_companion_retained_refusal("f3d spatial line length entity id");
}

#[test]
fn spatial_line_length_parameter_id_refuses_retained_limit() {
    assert_spatial_companion_retained_refusal("f3d spatial line length parameter id");
}

fn assert_missing_spatial_refusal(operation: &'static str, dimension: ResourceDimension) {
    let fixture = fixture();
    let companion = parameter_companion();
    let mut inputs = fixture.inputs();
    inputs.companions = std::slice::from_ref(&companion);
    let error = crate::test_support::resource_refusal_at(dimension, operation, 0, |ctx| {
        project_spatial_dimension_constraints(
            ctx,
            &inputs,
            std::slice::from_ref(&fixture.spatial),
            &[],
            EPS_SPATIAL_DIMENSION_LINEAR,
        )
    });
    assert!(matches!(error, CodecError::ResourceLimit(failure)
        if failure.operation == operation && failure.dimension == dimension));
}

#[test]
fn missing_spatial_parameter_scan_refuses_work_limit() {
    assert_missing_spatial_refusal(
        "f3d missing spatial parameter scan",
        ResourceDimension::WorkUnits,
    );
}

#[test]
fn missing_spatial_constraint_output_refuses_collection_limit() {
    assert_missing_spatial_refusal(
        "f3d missing spatial constraint output",
        ResourceDimension::CollectionItems,
    );
}

#[test]
fn missing_spatial_sketch_id_refuses_retained_limit() {
    assert_missing_spatial_refusal(
        "f3d missing spatial sketch id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn missing_spatial_operand_native_id_refuses_retained_limit() {
    assert_missing_spatial_refusal(
        "f3d missing spatial operand native id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn missing_spatial_constraint_native_id_refuses_retained_limit() {
    assert_missing_spatial_refusal(
        "f3d missing spatial constraint native id",
        ResourceDimension::RetainedBytes,
    );
}

#[test]
fn missing_spatial_output_parameter_id_refuses_retained_limit() {
    assert_missing_spatial_refusal(
        "f3d missing spatial output parameter id",
        ResourceDimension::RetainedBytes,
    );
}
