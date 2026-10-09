// SPDX-License-Identifier: Apache-2.0
use crate::decode::records::surface_parameter_records;
use crate::surface::{
    BoundaryType, SurfaceBodyBoundary, SurfaceKind, SurfaceParameterCarrier,
    SurfaceParameterOpaqueSpan, SurfaceParameterRecord, SurfaceParameterScalar,
    SurfaceParameterScalarFrame, SurfaceRow,
};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

fn scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(SurfaceRow {
        id: 7,
        kind: SurfaceKind::Plane,
        feature_id: 2,
        reversed: false,
        boundary_type: BoundaryType::Code01,
        next_surface: 0,
        offset: 3,
    });
    let token = SurfaceParameterScalar {
        value: Some(1.0),
        raw: vec![0xf9, 0],
        offset: 0,
    };
    scan.surfaces.parameters.push(SurfaceParameterRecord {
        surface_id: 7,
        body: vec![0xf9, 0],
        scalar_tokens: vec![token.clone()],
        opaque_spans: vec![SurfaceParameterOpaqueSpan {
            raw: vec![0xe3],
            offset: 2,
        }],
        scalar_frames: vec![SurfaceParameterScalarFrame {
            offset: 0,
            slots: vec![token],
        }],
        carrier: SurfaceParameterCarrier::Unresolved(SurfaceKind::Plane),
        boundary: SurfaceBodyBoundary::CompoundClose,
        offset: 3,
        body_offset: 4,
    });
    scan
}

#[test]
fn surface_parameter_record_refuses_collection_limit() {
    let scan = scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        ResourceDimension::CollectionItems,
        Some("creo native surface parameter records"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_collection_items = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            surface_parameter_records(
                &trial_ctx,
                &scan,
                &scan.surfaces.rows,
                &scan.surfaces.parameters,
                "visibgeom",
            )
            .map(|_| ())
        },
    );

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(error) = surface_parameter_records(
        &ctx,
        &scan,
        &scan.surfaces.rows,
        &scan.surfaces.parameters,
        "visibgeom",
    ) else {
        panic!("one parameter record exceeds the collection limit");
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == "creo native surface parameter records"),
        "{error:?}"
    );
}

#[test]
fn surface_parameter_record_id_refuses_retained_limit() {
    let scan = scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = crate::test_support::allocation_limit_at(
        ResourceDimension::MaterializedBytes,
        Some("creo native surface parameter record id"),
        |cap| {
            let trial_arena = DecodeArena::new();
            let mut trial_policy = DecodePolicy::service();
            trial_policy.limits.max_materialized_bytes = cap;
            let (trial_ctx, _) =
                DecodeContext::from_root_bytes(&[], &trial_arena, &trial_policy).expect("root");
            surface_parameter_records(
                &trial_ctx,
                &scan,
                &scan.surfaces.rows,
                &scan.surfaces.parameters,
                "visibgeom",
            )
            .map(|_| ())
        },
    );

    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let Err(error) = surface_parameter_records(
        &ctx,
        &scan,
        &scan.surfaces.rows,
        &scan.surfaces.parameters,
        "visibgeom",
    ) else {
        panic!("one parameter ID exceeds the retained limit");
    };
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
            if resource.dimension == ResourceDimension::MaterializedBytes
                && resource.operation == "creo native surface parameter record id"),
        "{error:?}"
    );
}

#[test]
fn borrowed_surface_parameter_preserves_nested_json() {
    let scan = scan();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    let records_parts = surface_parameter_records(
        &ctx,
        &scan,
        &scan.surfaces.rows,
        &scan.surfaces.parameters,
        "visibgeom",
    )
    .expect("parameter record is admitted");
    let _records_storage = records_parts.1;
    let records = records_parts.0;
    let value = serde_json::to_value(&records[0]).expect("record serializes");
    assert_eq!(value["body"], serde_json::json!([249, 0]));
    assert_eq!(
        value["slots"],
        serde_json::json!([{"value":1.0,"raw":[249,0],"offset":0,"length":2}])
    );
    assert_eq!(
        value["opaque_spans"],
        serde_json::json!([{"raw":[227],"offset":2,"length":1}])
    );
}
