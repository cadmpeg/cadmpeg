// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn rowless_round_cylinder_rejects_duplicate_sibling_model_surfaces() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 23,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows = crate::surface::unique_rows::UniqueIdRows::from_rows(vec![
        row(10, crate::surface::SurfaceKind::Plane),
        row(11, crate::surface::SurfaceKind::Plane),
        row(13, crate::surface::SurfaceKind::Cylinder),
    ]);
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            23,
            80,
            vec![
                crate::feature::entity::dummy_table_entry(10),
                crate::feature::entity::dummy_table_entry(11),
                crate::feature::entity::dummy_table_entry(12),
                crate::feature::entity::dummy_table_entry(13),
            ],
            &std::collections::BTreeSet::new(),
            47,
        )
        .with_surface_ids([10, 11, 13]),
    );
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .extend([model_cylinder(13, 2.0), model_cylinder(13, 3.0)]);

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::transfer_rowless_round_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity"),
        0
    );
    assert_eq!(ir.model.surfaces.len(), 2);
}

fn rowless_round_identity_input() -> (
    crate::container::ContainerScan<'static>,
    cadmpeg_ir::document::CadIr,
) {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 23,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows = [
        (10, crate::surface::SurfaceKind::Plane),
        (11, crate::surface::SurfaceKind::Plane),
        (13, crate::surface::SurfaceKind::Cylinder),
    ]
    .into_iter()
    .map(|(id, kind)| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    })
    .collect();
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            23,
            80,
            [10, 11, 12, 13]
                .into_iter()
                .map(crate::feature::entity::dummy_table_entry)
                .collect(),
            &std::collections::BTreeSet::new(),
            47,
        )
        .with_surface_ids([10, 11, 13]),
    );
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_cylinder(13, 2.0));
    (scan, ir)
}

fn rowless_round_retained_refusal(limit: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let (scan, mut ir) = rowless_round_identity_input();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    super::super::transfer_rowless_round_cylinders(
        &ctx,
        &scan,
        &mut ir,
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("rowless cylinder identity exceeds retained limit")
}

#[test]
fn rowless_round_cylinder_identity_refuses_retained_limit() {
    let error = rowless_round_retained_refusal(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo rowless round cylinder identity"),
        |cap| Err::<(), _>(rowless_round_retained_refusal(cap)),
    ));
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo rowless round cylinder identity")
    );
}

#[test]
fn rowless_round_cylinder_source_id_refuses_retained_limit() {
    let run = |limit| Err::<(), _>(rowless_round_retained_refusal(limit));
    let error = run(crate::test_support::allocation_limit_at(cadmpeg_core::decode::ResourceDimension::RetainedBytes, Some("creo rowless round cylinder source IDs"), run)).expect_err("named resource boundary");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo rowless round cylinder source IDs")
    );
}

#[test]
fn rowless_round_cylinder_identity_preserves_service_geometry() {
    let (scan, mut ir) = rowless_round_identity_input();
    let count = crate::decode::with_test_decode_ctx(|ctx| {
        super::super::transfer_rowless_round_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("rowless round cylinder admitted");
    assert_eq!(count, 1);
    assert_eq!(
        ir.model.surfaces[1].id.as_str(),
        "creo:visibgeom:surface#12"
    );
    assert_eq!(
        ir.model.surfaces[1]
            .source_object
            .as_ref()
            .expect("source object")
            .object_id
            .as_str(),
        "AllFeatur:12"
    );
}

#[test]
fn rowless_round_cylinder_rejects_duplicate_materialized_source_rows() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let table = crate::feature::entity::FeatureEntityTable::new(
        23,
        80,
        vec![
            crate::feature::entity::dummy_table_entry(10),
            crate::feature::entity::dummy_table_entry(11),
            crate::feature::entity::dummy_table_entry(12),
            crate::feature::entity::dummy_table_entry(13),
        ],
        &std::collections::BTreeSet::new(),
        47,
    )
    .with_surface_ids([10, 11, 13]);
    let rows = vec![
        row(10, crate::surface::SurfaceKind::Plane),
        row(11, crate::surface::SurfaceKind::Plane),
        row(13, crate::surface::SurfaceKind::Cylinder),
        row(13, crate::surface::SurfaceKind::Cylinder),
    ];

    assert!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::rowless_round_cylinder_pairs(
            ctx,
            &std::collections::BTreeSet::from([23]),
            &[table],
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
        ))
        .expect("service duplicate-row pair admitted")
        .is_empty()
    );
}
