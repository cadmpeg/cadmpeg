// SPDX-License-Identifier: Apache-2.0
//! Allocation admission for tessellation coordinate and index lanes.

use super::{
    assert_tessellation_collection_refusal, decode_tessellation_under_policy, ONE_TRIANGLE,
    ONE_TRIANGLE_IN_CONTAINER, ONE_TRIANGLE_WITH_PNINDEX,
};
use crate::parse::Value;
use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodePolicy, ResourceDimension,
};
use cadmpeg_core::CodecError;

#[test]
fn tessellation_coordinate_rows_charge_before_collection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, "step_tessellation_coordinate_rows");
}

#[test]
fn tessellation_coordinate_lists_charge_before_insertion() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, "step_tessellation_coordinate_lists");
}

#[test]
fn tessellation_coordinate_rows_reserve_temporary_bytes_before_collection() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service)
        .expect("service admits coordinate rows");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_coordinate_rows",
        |cap| {
            let mut limited = service;
            limited.limits.max_materialized_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE, limited)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_coordinate_rows")
    );
}

#[test]
fn tessellation_triangle_rows_reserve_temporary_bytes_before_collection() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits triangle rows");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_triangle_rows",
        |cap| {
            let mut policy = service;
            policy.limits.max_materialized_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_triangle_rows")
    );
}

#[test]
fn tessellation_container_items_reserve_temporary_bytes_before_collection() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE_IN_CONTAINER, service)
        .expect("service admits one container item");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_container_items",
        |cap| {
            let mut policy = service;
            policy.limits.max_materialized_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE_IN_CONTAINER, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_container_items")
    );
}

#[test]
fn tessellation_pnindex_charges_before_collection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE_WITH_PNINDEX, "step_tessellation_pnindex");
}

#[test]
fn tessellation_pn_vertices_charge_before_projection() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_WITH_PNINDEX,
        "step_tessellation_pn_vertices",
    );
}

#[test]
fn tessellation_pn_triangles_charge_before_projection() {
    assert_tessellation_collection_refusal(
        ONE_TRIANGLE_WITH_PNINDEX,
        "step_tessellation_pn_triangles",
    );
}

#[test]
fn tessellation_coordinate_indices_charge_before_insertion() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, "step_tessellation_coordinate_indices");
}

#[test]
fn tessellation_local_index_charges_before_projection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, "step_tessellation_local_index");
}

#[test]
fn tessellation_local_vertices_charge_before_projection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, "step_tessellation_local_vertices");
}

#[test]
fn tessellation_local_triangles_charge_before_projection() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, "step_tessellation_local_triangles");
}

#[test]
fn tessellation_normal_rows_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.)),$,((1,2,3)));";
    assert_tessellation_collection_refusal(records, "step_tessellation_normal_rows");
}

#[test]
fn tessellation_projected_normals_charge_before_collection() {
    let records = "#1=COORDINATES_LIST('',4,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.),(1.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,4,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.),(0.,0.,-1.)),$,((1,2,3)));";
    assert_tessellation_collection_refusal(records, "step_tessellation_projected_normals");
}

#[test]
fn tessellation_shaded_rows_charge_before_pairing() {
    let records = "#1=COORDINATES_LIST('',3,((0.,0.,0.),(1.,0.,0.),(0.,1.,0.)));
#2=TRIANGULATED_SURFACE_SET('',#1,3,((1.,0.,0.),(0.,1.,0.),(0.,0.,1.)),$,((1,2,3)));";
    assert_tessellation_collection_refusal(records, "step_tessellation_shaded_rows");
}

#[test]
fn tessellation_mesh_list_charges_before_push() {
    assert_tessellation_collection_refusal(ONE_TRIANGLE, "step_tessellation_mesh_list");
}

#[test]
fn tessellation_mesh_entity_is_admitted_before_retention() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits mesh entity");
    let mut limited = service;
    limited.limits.max_entities = 0;
    let error = decode_tessellation_under_policy(ONE_TRIANGLE, limited)
        .expect_err("one mesh entity exceeds zero admitted entities");
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::Entities && limit.operation == "step_tessellation_mesh_entity")
    );
}

#[test]
fn tessellation_ir_mesh_charges_retained_bytes_before_creation() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits mesh bytes");
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_ir_mesh",
        |cap| {
            let mut policy = service;
            policy.limits.max_retained_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_ir_mesh")
    );
}

#[test]
fn tessellation_local_triangles_copy_retained_bytes_before_push() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE, service).expect("service admits triangle bytes");
    // The final copy retains one [u32; 3] per triangle; the remapped lane is temporary.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_ir_triangles",
        |cap| {
            let mut policy = service;
            policy.limits.max_retained_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_ir_triangles")
    );
}

#[test]
fn tessellation_pn_triangles_copy_retained_bytes_before_push() {
    let service = DecodePolicy::service();
    decode_tessellation_under_policy(ONE_TRIANGLE_WITH_PNINDEX, service)
        .expect("service admits PN triangle bytes");
    // The final copy retains one [u32; 3] per triangle; the remapped lane is temporary.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        "step_tessellation_ir_triangles",
        |cap| {
            let mut policy = service;
            policy.limits.max_retained_bytes = cap;
            decode_tessellation_under_policy(ONE_TRIANGLE_WITH_PNINDEX, policy)
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::RetainedBytes && limit.operation == "step_tessellation_ir_triangles")
    );
}

#[test]
fn tessellation_pnindex_reserves_temporary_bytes_before_collection() {
    let values = Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ]);
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    assert!(super::super::index_list(Some(&values), &ctx)
        .expect("service admits PNINDEX")
        .is_some());
    // The probe uses the backing capacity allocated for three indices.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_tessellation_pnindex",
        |cap| {
            let arena = DecodeArena::new();
            let mut limited = service;
            limited.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &limited)
                .expect("limited root admission");
            super::super::index_list(Some(&values), &ctx).map(|_| ())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_tessellation_pnindex")
    );
}

fn one_complex_strip() -> Value {
    Value::List(vec![Value::List(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ])])
}

#[test]
fn complex_tessellation_rows_reserve_temporary_bytes_before_collection() {
    let strips = one_complex_strip();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    super::super::index_rows(
        Some(&strips),
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        "strip",
        &ctx,
    )
    .expect("service admits strip row");
    // The outer row buffer is allocated after the three-index child lane.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_complex_tessellation_rows",
        |cap| {
            let arena = DecodeArena::new();
            let mut limited = service;
            limited.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &limited)
                .expect("limited root admission");
            super::super::index_rows(
                Some(&strips),
                "COMPLEX_TRIANGULATED_SURFACE_SET",
                1,
                "strip",
                &ctx,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_complex_tessellation_rows")
    );
}

#[test]
fn complex_tessellation_indices_reserve_temporary_bytes_before_collection() {
    let strips = one_complex_strip();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    super::super::index_rows(
        Some(&strips),
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        "strip",
        &ctx,
    )
    .expect("service admits three strip indices");
    // The index buffer is allocated before the outer strip row.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_complex_tessellation_indices",
        |cap| {
            let arena = DecodeArena::new();
            let mut limited = service;
            limited.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &limited)
                .expect("limited root admission");
            super::super::index_rows(
                Some(&strips),
                "COMPLEX_TRIANGULATED_SURFACE_SET",
                1,
                "strip",
                &ctx,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_complex_tessellation_indices")
    );
}

#[test]
fn complex_tessellation_triangles_reserve_temporary_bytes_before_collection() {
    let strips = one_complex_strip();
    let arena = DecodeArena::new();
    let service = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(b"", &arena, &service).expect("service root admission");
    assert!(super::super::complex_triangles(
        Some(&strips),
        None,
        "COMPLEX_TRIANGULATED_SURFACE_SET",
        1,
        &ctx
    )
    .expect("service admits one triangle")
    .is_some());
    // The triangle buffer is allocated after the three-index strip and row.
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "step_complex_tessellation_triangles",
        |cap| {
            let arena = DecodeArena::new();
            let mut limited = service;
            limited.limits.max_materialized_bytes = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, &limited)
                .expect("limited root admission");
            super::super::complex_triangles(
                Some(&strips),
                None,
                "COMPLEX_TRIANGULATED_SURFACE_SET",
                1,
                &ctx,
            )
            .map(|_| ())
        },
    );
    assert!(
        matches!(error, CodecError::ResourceLimit(limit) if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "step_complex_tessellation_triangles")
    );
}
