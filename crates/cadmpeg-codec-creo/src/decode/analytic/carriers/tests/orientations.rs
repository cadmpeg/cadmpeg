// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;

use super::super::{native_face_orientations, rowless_round_face_orientations};

fn limited_native(
    scan: &crate::container::ContainerScan<'_>,
    ir: &CadIr,
    limit: u64,
) -> CodecError {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    native_face_orientations(&ctx, scan, ir).expect_err("orientation index refused")
}

fn assert_refusal(error: &CodecError, operation: &'static str) {
    assert!(
        matches!(error, CodecError::ResourceLimit(resource)
        if resource.dimension == ResourceDimension::CollectionItems
            && resource.operation == operation),
        "{error:?}"
    );
}

fn one_surface_row() -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id: 17,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 1,
        reversed: true,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    }
}

fn one_round_feature() -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id: 23,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    }
}

#[test]
fn native_face_source_id_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(one_surface_row());
    assert_refusal(
        &limited_native(&scan, &CadIr::empty(), 0),
        "creo native face source ID nodes",
    );
}

#[test]
fn native_face_orientation_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(one_surface_row());
    assert_refusal(
        &limited_native(&scan, &CadIr::empty(), 1),
        "creo native face orientation nodes",
    );
}

#[test]
fn native_datum_orientation_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.planes
        .datum_cylinders
        .push(crate::datum::DatumCylinder {
            id: 17,
            feature_id: 1,
            reversed: true,
            frame: crate::surface::PositionalCylinderFrame::new(
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                2.0,
                None,
            )
            .expect("valid cylinder frame"),
            offset_in_payload: 0,
        });
    assert_refusal(
        &limited_native(&scan, &CadIr::empty(), 0),
        "creo native face orientation nodes",
    );
}

#[test]
fn native_round_feature_id_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(one_round_feature());
    assert_refusal(
        &limited_native(&scan, &CadIr::empty(), 0),
        "creo native round feature ID nodes",
    );
}

#[test]
fn rowless_transfer_feature_id_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(one_round_feature());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = crate::decode::surfaces::cylinders::transfer_rowless_round_cylinders(
        &ctx,
        &scan,
        &mut CadIr::empty(),
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("round feature ID node refused");
    assert_refusal(&error, "creo rowless round feature ID nodes");
}

#[test]
fn constrained_round_feature_id_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(one_round_feature());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = crate::decode::surfaces::cylinders::transfer_constrained_slot_fillet_cylinders(
        &ctx,
        &scan,
        &mut CadIr::empty(),
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("constrained round node refused");
    assert_refusal(&error, "creo constrained round feature ID nodes");
}

#[test]
fn positional_round_feature_id_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(one_round_feature());
    let mut row = one_surface_row();
    row.id = 13;
    row.feature_id = 23;
    row.kind = crate::surface::SurfaceKind::Cylinder;
    scan.surfaces.rows.push(row);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = crate::decode::surfaces::cylinders::transfer_positional_cylinders(
        &ctx,
        &scan,
        &mut CadIr::empty(),
        &mut cadmpeg_ir::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("positional round node refused");
    assert_refusal(&error, "creo positional round feature ID nodes");
}

#[test]
fn available_surface_id_nodes_refuse_collection_limit() {
    let scan = crate::test_support::empty_container_scan();
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(Surface {
        id: SurfaceId::compose(&crate::identity::VISIBGEOM_SURFACE, 17),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    });
    assert_refusal(
        &limited_native(&scan, &ir, 0),
        "creo available surface ID nodes",
    );
}

fn rowless_fixture() -> (
    crate::feature::entity::FeatureEntityTable,
    Vec<crate::surface::SurfaceRow>,
) {
    let table = crate::feature::entity::FeatureEntityTable::new(
        23,
        47,
        vec![10, 11, 12, 13]
            .into_iter()
            .map(crate::feature::entity::dummy_table_entry)
            .collect(),
        &BTreeSet::new(),
        47,
    )
    .with_surface_ids([10, 11, 13]);
    let rows = [
        (10, crate::surface::SurfaceKind::Plane),
        (11, crate::surface::SurfaceKind::Plane),
        (13, crate::surface::SurfaceKind::Cylinder),
    ]
    .into_iter()
    .map(|(id, kind)| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 23,
        reversed: true,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    })
    .collect();
    (table, rows)
}

#[test]
fn rowless_round_pairs_refuse_collection_limit() {
    let (table, rows) = rowless_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = crate::decode::surfaces::cylinders::rowless_round_cylinder_pairs(
        &ctx,
        &BTreeSet::from([23]),
        &[table],
        &rows,
    )
    .expect_err("pair refused");
    assert_refusal(&error, "creo rowless round cylinder pairs");
}

#[test]
fn rowless_face_orientation_nodes_refuse_collection_limit() {
    let (table, rows) = rowless_fixture();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root admitted");
    let error = rowless_round_face_orientations(
        &ctx,
        &BTreeSet::from([23]),
        &[table],
        &rows,
        &BTreeSet::from([12]),
    )
    .expect_err("orientation node refused");
    assert_refusal(&error, "creo rowless face orientation nodes");
}

#[test]
fn native_face_orientation_indexes_preserve_service_values() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(one_surface_row());
    scan.features.rows.push(one_round_feature());
    let orientations = crate::decode::with_test_decode_ctx(|ctx| {
        native_face_orientations(ctx, &scan, &CadIr::empty())
    })
    .expect("service native orientations admitted");
    assert_eq!(orientations.get(&17), Some(&true));
}
