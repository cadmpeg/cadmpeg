// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::scalar::PositiveLength;

use super::{unique_support_tangent_cylinder_frame, unique_tangent_axial_interval_corner_frame};
use crate::decode::analytic::equations::PlaneEquation;

const EPS_TEST_GEOMETRY: f64 = 1.0e-12;

#[test]
fn circular_sweep_feature_id_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo circular sweep feature ID nodes",
        |ctx| {
            super::transfer_circular_sweep_cylinders(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo circular sweep feature ID nodes")
    );
}

#[test]
fn hole_cylinder_feature_id_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    let row = crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Hole),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    };
    scan.features.rows.extend([row.clone(), row]);
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo hole cylinder feature ID nodes",
        |ctx| {
            super::transfer_hole_cylinders(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo hole cylinder feature ID nodes")
    );

    let run = |cap| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("root");
        super::transfer_hole_cylinders(
            &ctx,
            &scan,
            &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    assert_eq!(
        run(1).expect("two identical hole rows consume one feature ID node"),
        0
    );

    assert_eq!(
        run(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            None,
            run
        ))
        .expect("duplicate ID reuses its node"),
        0
    );
}

fn slot_fillet_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 913,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4, 5, 6],
            offset: 0,
        });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 913,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 7,
    });
    scan.planes.positional_frames.extend([
        crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 1,
        },
        crate::surface::OutlinePlane {
            surface_id: 2,
            origin: [1.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 2,
        },
        crate::surface::OutlinePlane {
            surface_id: 3,
            origin: [0.0, -1.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 3,
        },
        crate::surface::OutlinePlane {
            surface_id: 4,
            origin: [0.0, 1.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 4,
        },
        crate::surface::OutlinePlane {
            surface_id: 5,
            origin: [0.0, 0.0, -1.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 5,
        },
        crate::surface::OutlinePlane {
            surface_id: 6,
            origin: [0.0, 0.0, 1.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 6,
        },
    ]);
    scan
}

fn model_plane(id: u32, origin: [f64; 3], normal: [f64; 3]) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
            .expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                origin.into(),
                normal.into(),
                if normal[0].abs() > 0.5 {
                    [0.0, 1.0, 0.0].into()
                } else {
                    [1.0, 0.0, 0.0].into()
                },
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    }
}

fn model_cylinder(id: u32, radius: f64) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
            .expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                [0.0, 0.0, 0.0].into(),
                [0.0, 0.0, 1.0].into(),
                [1.0, 0.0, 0.0].into(),
                radius,
            )
            .expect("valid CylinderSurface fixture"),
        )),
        source_object: None,
    }
}

fn split_outline_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 1,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 10,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 1,
        },
        crate::surface::SurfaceRow {
            id: 2,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 10,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 2,
        },
        crate::surface::SurfaceRow {
            id: 3,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 10,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 3,
        },
    ]);
    scan.curves.topology_rows.extend([
        crate::curve::CurveTopologyRow {
            id: 11,
            type_byte: 0,
            feature_id: 10,
            directions: [1, 1],
            faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(2)],
            next_edges: [11, 11],
            offset: 11,
        },
        crate::curve::CurveTopologyRow {
            id: 12,
            type_byte: 0,
            feature_id: 10,
            directions: [1, 1],
            faces: [std::num::NonZeroU32::new(1), std::num::NonZeroU32::new(3)],
            next_edges: [12, 12],
            offset: 12,
        },
    ]);
    let parameter = |surface_id, bounds| crate::surface::SurfaceParameterRecord {
        surface_id,
        body: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::CylinderBounds(bounds),
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: usize::try_from(surface_id).expect("fixture index fits usize"),
        body_offset: usize::try_from(surface_id).expect("fixture index fits usize"),
    };
    scan.surfaces.parameters.extend([
        parameter(2, [[-0.3125, 1.3125], [0.3125, 1.625]]),
        parameter(3, [[-0.3125, 1.625], [0.3125, 1.9375]]),
    ]);
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, -1.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 1,
        });
    scan
}

fn split_outline_refusal_at_collection_limit(
    limit: u64,
) -> Result<usize, cadmpeg_core::CodecError> {
    let scan = split_outline_scan();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    super::transfer_split_outline_cylinders(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
}

fn positional_map_refusal_at_collection_limit(
    limit: u64,
) -> Result<super::PositionalCylinderTransferSummary, cadmpeg_core::CodecError> {
    let scan = split_outline_scan();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    super::transfer_positional_cylinders(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
}

fn reference_bound_refusal_at_collection_limit(
    limit: u64,
) -> Result<super::PositionalCylinderTransferSummary, cadmpeg_core::CodecError> {
    let mut scan = split_outline_scan();
    scan.features
        .entity_tables
        .push(crate::feature::entity::FeatureEntityTable::new(
            10,
            29,
            vec![crate::feature::entity::dummy_table_entry(99)],
            &std::collections::BTreeSet::new(),
            0,
        ));
    scan.references.circles.push(
        crate::reference::ReferenceCircle::try_new(
            99,
            crate::reference::ReferenceCircleCenter::Stored(
                cadmpeg_ir::features::FinitePoint3::ZERO,
            ),
            cadmpeg_ir::scalar::PositiveLength::new(1.0).expect("positive radius"),
            cadmpeg_ir::units::UnitVector3::Z_AXIS,
            [
                cadmpeg_ir::features::FinitePoint3::new([1.0, 0.0, 0.0].into())
                    .expect("finite start"),
                cadmpeg_ir::features::FinitePoint3::new([0.0, 1.0, 0.0].into())
                    .expect("finite end"),
            ],
            0,
        )
        .expect("checked reference geometry"),
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    super::transfer_positional_cylinders(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
}

#[test]
fn reference_bound_entity_id_nodes_refuse_collection_limit() {
    let error =
        reference_bound_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo reference cylinder entity ID nodes"),
            reference_bound_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo reference cylinder entity ID nodes")
    );
}

#[test]
fn reference_bound_circles_refuse_collection_limit() {
    let error =
        reference_bound_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo reference cylinder circles"),
            reference_bound_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo reference cylinder circles")
    );
}

#[test]
fn positional_topology_unique_row_count_refuses_collection_limit() {
    let error =
        positional_map_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo unique-row count nodes"),
            positional_map_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row count nodes")
    );
}

#[test]
fn positional_topology_unique_row_projection_refuses_collection_limit() {
    let error =
        positional_map_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo unique-row projection"),
            positional_map_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row projection")
    );
}

#[test]
fn positional_adjacent_cylinder_nodes_refuse_collection_limit() {
    let error =
        positional_map_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo positional adjacent cylinder nodes"),
            positional_map_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo positional adjacent cylinder nodes")
    );
}

#[test]
fn positional_adjacent_plane_id_nodes_refuse_collection_limit() {
    let error =
        positional_map_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo positional adjacent plane ID nodes"),
            positional_map_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo positional adjacent plane ID nodes")
    );
}

#[test]
fn positional_support_plane_vector_refuses_collection_limit() {
    let error =
        positional_map_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo positional support planes"),
            positional_map_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo positional support planes")
    );
}

#[test]
fn positional_support_plane_nodes_refuse_collection_limit() {
    let error =
        positional_map_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo positional support plane nodes"),
            positional_map_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo positional support plane nodes")
    );
}

#[test]
fn constant_round_radius_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 913,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 913,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 7,
    });
    scan.features
        .legacy_rounds
        .push(crate::legacy_feature::LegacyRoundFeature {
            feature_id: 913,
            radius: crate::legacy_feature::LegacyRoundRadius::Constant(
                cadmpeg_ir::scalar::PositiveReal::new(2.0).expect("positive radius"),
            ),
            edge_ids: None,
            offset: 0,
        });
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo constant round radius nodes",
        |ctx| {
            super::transfer_positional_cylinders(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo constant round radius nodes")
    );

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::transfer_positional_cylinders(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("service round radius admitted")
        .transferred,
        0
    );
}

#[test]
fn split_outline_topology_row_count_nodes_refuse_collection_limit() {
    let error =
        split_outline_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo unique-row count nodes"),
            split_outline_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row count nodes")
    );
}

#[test]
fn split_outline_topology_row_projection_refuses_collection_limit() {
    let error =
        split_outline_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo unique-row projection"),
            split_outline_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo unique-row projection")
    );
}

#[test]
fn split_outline_plane_nodes_refuse_collection_limit() {
    let error =
        split_outline_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo split cylinder plane nodes"),
            split_outline_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo split cylinder plane nodes")
    );
}

#[test]
fn split_outline_cylinder_id_nodes_refuse_collection_limit() {
    let error =
        split_outline_refusal_at_collection_limit(crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::CollectionItems,
            Some("creo split cylinder ID nodes"),
            split_outline_refusal_at_collection_limit,
        ))
        .expect_err("named allocation refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo split cylinder ID nodes")
    );
}

#[test]
fn split_outline_identity_refuses_retained_limit() {
    let scan = split_outline_scan();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo split cylinder identities"),
        |cap| {
            let trial_arena = cadmpeg_core::decode::DecodeArena::new();
            let mut trial_policy = policy;
            trial_policy.limits.max_retained_bytes = cap;
            let (trial_ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
                &[],
                &trial_arena,
                &trial_policy,
            )
            .expect("root");
            super::transfer_split_outline_cylinders(
                &trial_ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = super::transfer_split_outline_cylinders(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("split outline ID exceeds retained limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && resource.operation == "creo split cylinder identities")
    );
}

#[test]
fn split_outline_source_object_id_refuses_retained_limit() {
    let scan = split_outline_scan();
    let run = |limit| {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("root");
        super::transfer_split_outline_cylinders(
            &ctx,
            &scan,
            &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    let error = run(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo split cylinder source object IDs"),
        run,
    ))
    .expect_err("named resource boundary");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
            && resource.operation == "creo split cylinder source object IDs")
    );
}

#[test]
fn constrained_slot_fillet_uses_native_plane_carriers_when_model_planes_are_absent() {
    let scan = slot_fillet_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let transferred = crate::decode::with_test_decode_ctx(|ctx| {
        super::transfer_constrained_slot_fillet_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("valid source object identity");

    assert_eq!(transferred, 1);
    let [surface] = ir.model.surfaces.as_slice() else {
        panic!("one generated cylinder");
    };
    let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved() else {
        panic!("generated cylinder: {:?}", surface.geometry);
    };
    let origin = cylinder_surface.origin().get();
    let axis = *cylinder_surface.frame().axis().as_raw();
    let radius = cylinder_surface.radius().get();
    assert_eq!(origin, [0.0, 0.0, 0.0].into());
    assert_eq!(axis, [1.0, 0.0, 0.0].into());
    assert_eq!(radius, 1.0);
}

#[test]
fn constrained_slot_fillet_propagates_midplane_collection_limit() {
    let scan = slot_fillet_scan();
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo slot fillet midplanes",
        |ctx| {
            super::transfer_constrained_slot_fillet_cylinders(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo slot fillet midplanes"),
        "{error:?}"
    );
}

#[test]
fn constrained_slot_fillet_plane_rows_refuse_collection_limit() {
    let scan = slot_fillet_scan();
    let error = crate::test_support::last_refusal_at(
        &[],
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "creo constrained slot plane rows",
        |ctx| {
            super::transfer_constrained_slot_fillet_cylinders(
                ctx,
                &scan,
                &mut cadmpeg_ir::document::CadIr::empty(),
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo constrained slot plane rows")
    );
}

#[test]
fn active_datum_cylinder_source_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let mut scan = crate::test_support::empty_container_scan();
    scan.planes
        .datum_cylinders
        .push(crate::datum::DatumCylinder {
            id: 8,
            feature_id: 1,
            reversed: false,
            frame: crate::surface::PositionalCylinderFrame::new(
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
                1.0,
                Some(2.0),
            )
            .expect("valid datum cylinder frame"),
            offset_in_payload: 0,
        });
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        super::transfer_active_datum_cylinders(
            &ctx,
            &scan,
            &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    let error = run(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo active datum cylinder source IDs"),
        run,
    ))
    .expect_err("named resource boundary");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo active datum cylinder source IDs")
    );
}

#[test]
fn constrained_slot_cylinder_source_id_refuses_retained_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = slot_fillet_scan();
    let run = |limit| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = limit;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        super::transfer_constrained_slot_fillet_cylinders(
            &ctx,
            &scan,
            &mut cadmpeg_ir::document::CadIr::empty(),
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    };
    let error = run(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo constrained slot cylinder source IDs"),
        run,
    ))
    .expect_err("named resource boundary");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo constrained slot cylinder source IDs")
    );
}

#[test]
fn constrained_slot_cylinder_radius_is_in_millimeters_at_ir_admission() {
    let mut scan = slot_fillet_scan();
    scan.framing.principal_unit = Some(crate::legacy::PrincipalUnitSystem::InchPoundMassSecond);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let mut source_carriers = crate::decode::source_carriers::SourceUnitCarriers::new(
        cadmpeg_ir::scalar::PositiveReal::new(25.4),
    );
    crate::decode::with_test_decode_ctx(|ctx| {
        super::transfer_constrained_slot_fillet_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut source_carriers,
        )
        .expect("constrained slot cylinder transfer");
    });
    let surface = ir.model.surfaces.first().expect("transferred cylinder");
    let Some(SolvedSurfaceGeometry::Cylinder(cylinder)) = surface.geometry.solved() else {
        panic!("transferred surface changed family");
    };
    assert_eq!(cylinder.radius().get(), 25.4);
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(source_cylinder)) = source_carriers
        .surface_geometry(surface)
        .expect("source carrier lookup")
    else {
        panic!("source carrier changed family");
    };
    assert_eq!(source_cylinder.radius().get(), 1.0);
}

#[test]
fn split_outline_uses_native_plane_carrier_when_model_plane_is_absent() {
    let scan = split_outline_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::transfer_split_outline_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity"),
        2
    );
    assert!(ir.model.surfaces.iter().all(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface))
                if {
                    let origin = cylinder_surface.origin();
        let axis = cylinder_surface.frame().axis().as_raw();
        let radius = cylinder_surface.radius().get();
                    radius == 0.3125
                        && *origin == [0.0, 1.625, -1.0].into()
                        && *axis == [0.0, 0.0, 1.0].into()
                })
    }));
}

#[test]
fn split_outline_rejects_duplicate_surface_rows() {
    let mut scan = split_outline_scan();
    let duplicate = scan.surfaces.rows[1].clone();
    scan.surfaces.rows.push(duplicate);
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::transfer_split_outline_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity"),
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

fn counterbore_dimension_gate_scan(radius: f64) -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 42,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Hole),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .definitions
        .push(crate::feature::definitions::FeatureDefinition {
            identity: crate::feature::definitions::DefinitionIdentity::Parsed {
                schema_id: std::num::NonZeroU32::new(911),
                owner_feature_id: None,
            },
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: None,
            trim_entities: None,
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: Some(crate::feature::definitions::FeatureDimensionTable {
                declared_count: 4,
                entity_ref: Some(88),
                rows: vec![(2, 20.0, 0), (2, 1.0, 1), (1, 8.0, 2), (2, 60.0, 3)]
                    .into_iter()
                    .map(|(dimension_type, value, external_id)| {
                        crate::feature::definitions::FeatureDimension {
                            dimension_type,
                            value: crate::feature::definitions::DimensionValue::Resolved(value),
                            value_body: Vec::new(),
                            direction_byte: 0,
                            auxiliary_value: Some(0.0),
                            auxiliary_body: Vec::new(),
                            external_id,
                            references: None,
                            offset: 0,
                        }
                    })
                    .collect(),
                offset: 0,
            }),
            relations: None,
            saved_section: None,
            offset: 0,
        });
    scan.surfaces
        .rows
        .extend((1..=4).map(|id| crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 42,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        }));
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 3,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Cylinder {
                    frame: crate::surface::PositionalCylinderFrame::new(
                        [0.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0],
                        radius,
                        Some(2.0),
                    )
                    .expect("valid positional cylinder frame"),
                    split_bounds: None,
                },
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 3,
            body_offset: 3,
        });
    let entry = |entity_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
        entity_id,
        payload: crate::feature::entity::entry_payload(200, Some(source_entity_id), None, None),
        prefixed: false,
        offset: usize::try_from(entity_id).expect("fixture index fits usize"),
        end_offset: usize::try_from(entity_id).expect("fixture index fits usize") + 1,
    };
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            42,
            29,
            vec![entry(1, 100), entry(2, 100), entry(3, 101), entry(4, 101)],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([1, 2, 3, 4]),
    );
    scan
}

#[test]
fn constrained_slot_fillet_uses_transferred_plane_carriers_when_native_planes_are_absent() {
    let mut scan = slot_fillet_scan();
    scan.planes.positional_frames.clear();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.extend([
        model_plane(1, [0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        model_plane(2, [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        model_plane(3, [0.0, -1.0, 0.0], [0.0, 1.0, 0.0]),
        model_plane(4, [0.0, 1.0, 0.0], [0.0, 1.0, 0.0]),
        model_plane(5, [0.0, 0.0, -1.0], [0.0, 0.0, 1.0]),
        model_plane(6, [0.0, 0.0, 1.0], [0.0, 0.0, 1.0]),
    ]);

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::transfer_constrained_slot_fillet_cylinders(
                ctx,
                &scan,
                &mut ir,
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("valid source object identity"),
        1
    );
}

#[test]
fn constrained_slot_fillet_rejects_conflicting_model_plane_carriers() {
    let scan = slot_fillet_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .push(model_plane(3, [0.0, -0.5, 0.0], [0.0, 1.0, 0.0]));

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::transfer_constrained_slot_fillet_cylinders(
                ctx,
                &scan,
                &mut ir,
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("valid source object identity"),
        0
    );
    assert!(ir
        .model
        .surfaces
        .iter()
        .all(|surface| { surface.id.as_str() != "creo:visibgeom:surface#7" }));
}

#[test]
fn split_outline_rejects_conflicting_model_plane_carrier() {
    let scan = split_outline_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .push(model_plane(1, [0.0, 0.0, -0.5], [0.0, 0.0, 1.0]));

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::transfer_split_outline_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity"),
        0
    );
}
mod identities;

mod frames;
mod positional;
mod rowless_round;
