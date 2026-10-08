// SPDX-License-Identifier: Apache-2.0

use super::generated_cap_plane_extent;
use crate::decode::holes::sweep::extrusion_extent_and_direction;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

fn service_generated_cap_plane_extent(
    scan: &crate::container::ContainerScan<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    crate::decode::with_test_decode_ctx(|ctx| {
        generated_cap_plane_extent(ctx, scan, ir, source_carriers, feature_id)
    })
    .expect("service cap planes admitted")
}

fn service_feature_plane_equations(
    scan: &crate::container::ContainerScan<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Option<Vec<([f64; 3], [f64; 3])>> {
    crate::decode::with_test_decode_ctx(|ctx| {
        super::feature_plane_equations(ctx, scan, ir, source_carriers, feature_id).map(|result| {
            result.map(|(planes, _plane_storage)| {
                planes
                    .into_iter()
                    .map(|plane| (plane.origin, plane.normal))
                    .collect::<Vec<_>>()
            })
        })
    })
    .expect("service resources")
}

fn service_generated_arc_cylinder_extent(
    scan: &crate::container::ContainerScan<'_>,
    ir: &CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    definition: &crate::feature::definitions::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    crate::decode::with_test_decode_ctx(|ctx| {
        super::generated_arc_cylinder_extent(ctx, scan, ir, source_carriers, definition, transform)
    })
    .expect("service resources")
}

fn expected_linear_plane_extent() -> (ExtrudeExtent, [f64; 3]) {
    (
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(8.0)
                        .expect("nonzero length fixture"),
                },
                draft: None,
            },
        },
        [0.0, 0.0, 1.0],
    )
}

fn plane_row(id: u32) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 917,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture index fits usize"),
    }
}

fn plane_surface(id: u32, z: f64) -> Surface {
    Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    }
}

fn plane_outline(id: u32, z: f64) -> crate::surface::OutlinePlane {
    crate::surface::OutlinePlane {
        surface_id: id,
        origin: [0.0, 0.0, z],
        normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: usize::try_from(id).expect("fixture index fits usize"),
    }
}

fn feature_plane_limit_error(operation: &'static str) -> cadmpeg_core::CodecError {
    let run = |limit| {
        let mut scan = crate::test_support::empty_container_scan();
        scan.surfaces.rows.push(plane_row(31));
        scan.planes.outlines.push(plane_outline(31, 2.0));
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = limit;
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
            .expect("empty root admitted");
        super::feature_plane_equations(
            &ctx,
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            917,
        )
        .map(|result| {
            result.map(|(planes, _plane_storage)| {
                planes
                    .into_iter()
                    .map(|plane| (plane.origin, plane.normal))
                    .collect::<Vec<_>>()
            })
        })
    };
    let limit = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some(operation),
        &run,
    );
    run(limit).expect_err("named collection boundary")
}

#[test]
fn feature_outline_planes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(plane_row(31));
    scan.planes.outlines.push(plane_outline(31, 2.0));
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo feature outline planes"),
        |limit| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            super::feature_outline_planes(&ctx, &scan, 917)
                .map(|result| result.map(|(planes, _storage)| planes))
        },
    );
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error =
        super::feature_outline_planes(&ctx, &scan, 917).expect_err("outline plane exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo feature outline planes")
    );
}

#[test]
fn feature_plane_id_nodes_refuse_collection_limit() {
    assert!(
        matches!(feature_plane_limit_error("creo feature plane ID nodes"),
        cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo feature plane ID nodes")
    );
}

#[test]
fn feature_local_plane_nodes_refuse_collection_limit() {
    assert!(
        matches!(feature_plane_limit_error("creo feature local plane nodes"),
        cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo feature local plane nodes")
    );
}

#[test]
fn feature_plane_equations_refuse_collection_limit() {
    assert!(
        matches!(feature_plane_limit_error("creo feature plane equations"),
        cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo feature plane equations")
    );
}

#[test]
fn generated_arc_cylinder_id_nodes_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            7,
            29,
            vec![crate::feature::entity::FeatureEntityTableEntry {
                entity_id: 33,
                payload: crate::feature::entity::entry_payload(200, Some(11), None, None),
                prefixed: false,
                offset: 0,
                end_offset: 0,
            }],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([33]),
    );
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 33,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 7,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 33,
    });
    let definition = crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: Some(7),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: vec![crate::feature::segment_rows::SegmentRow::Ordinary(
                crate::feature::definitions::FeatureSegment {
                    kind: crate::feature::definitions::FeatureSegmentKind::Arc([1, 2]),
                    directions: [None; 3],
                    center_id: Some(3),
                    arc_orientation: Some(0),
                    vertical_horizontal: None,
                    radius_ref: Some(4),
                    radius2_ref: None,
                    external_id: 11,
                    body: Vec::new(),
                    offset: 0,
                },
            )]
            .into_iter()
            .collect(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let transform = crate::placement::FeatureSectionTransform::new(
        7,
        Some(7),
        [0.0; 3],
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        0,
    )
    .expect("section transform");
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo generated arc cylinder ID nodes"),
        |limit| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            super::generated_arc_cylinder_extent(
                &ctx,
                &scan,
                &CadIr::empty(),
                &crate::decode::source_carriers::SourceUnitCarriers::default(),
                &definition,
                &transform,
            )
        },
    );
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = super::generated_arc_cylinder_extent(
        &ctx,
        &scan,
        &CadIr::empty(),
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        &definition,
        &transform,
    )
    .expect_err("source ID node exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo generated arc cylinder ID nodes")
    );
}

#[test]
fn available_positional_cylinder_frames_refuse_collection_limit() {
    let frame = crate::surface::PositionalCylinderFrame::new(
        [0.0; 3],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        0.75,
        Some(2.0),
    )
    .expect("cylinder frame");
    let parameters = [crate::surface::SurfaceParameterRecord {
        surface_id: 1,
        body: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Cylinder {
                frame,
                split_bounds: None,
            },
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    }];
    let ids = std::collections::BTreeSet::from([1]);
    let limit = crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        Some("creo available positional cylinder frames"),
        |limit| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                    .expect("root");
            super::unique_available_positional_cylinder_frame_records(
                &ctx,
                &ids,
                &crate::surface::SurfaceParameters::from_rows(parameters.to_vec()),
            )
        },
    );
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    let error = super::unique_available_positional_cylinder_frame_records(
        &ctx,
        &ids,
        &crate::surface::SurfaceParameters::from_rows(parameters.to_vec()),
    )
    .expect_err("frame item exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo available positional cylinder frames")
    );
}

fn cylinder_surface(id: u32, origin: Point3, axis: Vector3) -> Surface {
    Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                origin,
                axis,
                Vector3::new(1.0, 0.0, 0.0),
                0.75,
            )
            .expect("valid CylinderSurface fixture"),
        )),
        source_object: None,
    }
}

#[test]
fn generated_table_cap_classes_use_placed_cap_planes() {
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),

            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            7,
            29,
            vec![
                entry(31, 204, None),
                entry(32, 203, None),
                entry(33, 200, Some(11)),
            ],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([31, 32, 33]),
    );
    for id in [31, 32, 33] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 7,
            reversed: id == 31,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.planes.positional_frames.extend([
        crate::surface::OutlinePlane {
            surface_id: 31,
            origin: [4.0, -2.0, 2.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 31,
        },
        crate::surface::OutlinePlane {
            surface_id: 32,
            origin: [4.0, -2.0, 8.0],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 32,
        },
    ]);

    assert_eq!(
        service_generated_cap_plane_extent(
            &scan,
            &CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            7,
        ),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(6.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, 0.0, 1.0],
        ))
    );
}

#[test]
fn feature_plane_extent_reconciles_native_and_transferred_carriers() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.extend([plane_row(31), plane_row(32)]);
    scan.planes
        .outlines
        .extend([plane_outline(31, 2.0), plane_outline(32, 8.0)]);
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend([plane_surface(31, 2.0), plane_surface(32, 8.0)]);

    assert_eq!(
        service_feature_plane_equations(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            917
        )
        .and_then(|planes| {
            crate::decode::with_test_decode_ctx(|ctx| {
                extrusion_extent_and_direction(ctx, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], planes)
            })
            .expect("service resources")
        }),
        Some(expected_linear_plane_extent())
    );

    ir.model.surfaces[1] = plane_surface(32, 9.0);
    assert!(service_feature_plane_equations(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        917
    )
    .is_none());

    ir.model.surfaces[1] = Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#32".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
        source_object: None,
    };
    assert_eq!(
        service_feature_plane_equations(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            917
        )
        .and_then(|planes| {
            crate::decode::with_test_decode_ctx(|ctx| {
                extrusion_extent_and_direction(ctx, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], planes)
            })
            .expect("service resources")
        }),
        Some(expected_linear_plane_extent())
    );
}

#[test]
fn feature_plane_extent_accepts_complete_transferred_carriers_without_local_frames() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.extend([plane_row(31), plane_row(32)]);
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend([plane_surface(31, 2.0), plane_surface(32, 8.0)]);

    assert_eq!(
        service_feature_plane_equations(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            917
        )
        .and_then(|planes| {
            crate::decode::with_test_decode_ctx(|ctx| {
                extrusion_extent_and_direction(ctx, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0], planes)
            })
            .expect("service resources")
        }),
        Some(expected_linear_plane_extent())
    );
}

#[test]
fn feature_plane_extent_rejects_ambiguous_or_non_plane_carriers() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.extend([plane_row(31), plane_row(32)]);
    scan.planes.outlines.extend([
        plane_outline(31, 2.0),
        plane_outline(31, 2.0),
        plane_outline(32, 8.0),
    ]);
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend([plane_surface(31, 2.0), plane_surface(32, 8.0)]);
    assert!(service_feature_plane_equations(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        917
    )
    .is_none());

    scan.planes.outlines.remove(1);
    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    assert!(service_feature_plane_equations(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        917
    )
    .is_none());
}

#[test]
fn generated_arc_cylinder_extent_reconciles_transferred_carriers() {
    let entry = crate::feature::entity::FeatureEntityTableEntry {
        entity_id: 33,
        payload: crate::feature::entity::entry_payload(200, Some(11), None, None),
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let frame = crate::surface::PositionalCylinderFrame::new(
        [0.0, 4.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        0.75,
        Some(34.0),
    )
    .expect("valid positional cylinder frame");
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            7,
            29,
            vec![entry],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([33]),
    );
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 33,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 7,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 33,
    });
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 33,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Cylinder {
                    frame,
                    split_bounds: None,
                },
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 0,
            body_offset: 0,
        });
    let definition = crate::feature::definitions::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: Some(7),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::definitions::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![crate::feature::definitions::FeatureSegment {
                kind: crate::feature::definitions::FeatureSegmentKind::Arc([1, 2]),
                directions: [None; 3],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: Some(4),
                radius2_ref: None,
                external_id: 11,
                body: Vec::new(),
                offset: 0,
            }])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let transform = crate::placement::FeatureSectionTransform::new(
        7,
        Some(7),
        frame.frame().origin(),
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        0,
    )
    .expect("valid section frame");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(cylinder_surface(
        33,
        Point3::new(0.0, 4.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ));
    let expected = Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(34.0)
                        .expect("nonzero length fixture"),
                },
                draft: None,
            },
        },
        [0.0, 1.0, 0.0],
    ));
    assert_eq!(
        service_generated_arc_cylinder_extent(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &definition,
            &transform
        ),
        expected
    );

    ir.model.surfaces.clear();
    assert_eq!(
        service_generated_arc_cylinder_extent(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &definition,
            &transform
        ),
        expected
    );
    ir.model.surfaces.push(cylinder_surface(
        33,
        Point3::new(0.0, 7.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ));
    assert_eq!(
        service_generated_arc_cylinder_extent(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &definition,
            &transform
        ),
        expected
    );
    ir.model.surfaces[0] =
        cylinder_surface(33, Point3::new(1.0, 4.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
    assert!(service_generated_arc_cylinder_extent(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        &definition,
        &transform
    )
    .is_none());

    ir.model.surfaces[0] =
        cylinder_surface(33, Point3::new(0.0, 4.0, 0.0), Vector3::new(0.0, -1.0, 0.0));
    assert!(service_generated_arc_cylinder_extent(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        &definition,
        &transform
    )
    .is_none());

    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 4.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            0.75,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    assert!(service_generated_arc_cylinder_extent(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        &definition,
        &transform
    )
    .is_none());

    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            Point3::new(0.0, 4.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            0.8,
        )
        .expect("valid CylinderSurface fixture"),
    ));
    assert!(service_generated_arc_cylinder_extent(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        &definition,
        &transform
    )
    .is_none());

    ir.model.surfaces[0].geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 4.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    ));
    assert!(service_generated_arc_cylinder_extent(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        &definition,
        &transform
    )
    .is_none());

    ir.model.surfaces[0] =
        cylinder_surface(33, Point3::new(0.0, 4.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
    ir.model.surfaces.push(ir.model.surfaces[0].clone());
    assert!(service_generated_arc_cylinder_extent(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        &definition,
        &transform
    )
    .is_none());
    ir.model.surfaces.pop();

    ir.model.surfaces[0].geometry =
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None });
    assert_eq!(
        service_generated_arc_cylinder_extent(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &definition,
            &transform
        ),
        expected
    );
}

#[test]
fn duplicate_parameter_lookup_does_not_visit_unrelated_tail() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let frame = crate::surface::PositionalCylinderFrame::new(
        [0.0; 3],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        0.75,
        Some(2.0),
    )
    .expect("frame");
    let record = crate::surface::SurfaceParameterRecord {
        surface_id: 1,
        body: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: crate::surface::SurfaceParameterCarrier::Resolved(
            crate::surface::InlineSurfaceCarrier::Cylinder {
                frame,
                split_bounds: None,
            },
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 0,
        body_offset: 0,
    };
    let ids = std::collections::BTreeSet::from([1]);
    let short = vec![record.clone(), record.clone()];
    let mut long = short.clone();
    let mut unrelated = record;
    unrelated.surface_id = 2;
    long.extend(std::iter::repeat_n(unrelated, 128));
    let short = crate::surface::SurfaceParameters::from_rows(short);
    let long = crate::surface::SurfaceParameters::from_rows(long);
    let work_limit = |parameters: &crate::surface::SurfaceParameters| {
        crate::test_support::allocation_limit_at(ResourceDimension::WorkUnits, None, |limit| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
            super::unique_available_positional_cylinder_frame_records(&ctx, &ids, parameters)
        })
    };
    assert_eq!(work_limit(&short), work_limit(&long));
    let result = crate::test_support::assert_work_boundaries(
        &["creo positional cylinder frame ID scan"],
        |ctx| super::unique_available_positional_cylinder_frame_records(ctx, &ids, &long),
    );
    assert!(result.is_none());
}

#[test]
fn feature_plane_storage_stays_live_until_its_collection_is_dropped() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(plane_row(31));
    scan.planes.outlines.push(plane_outline(31, 2.0));
    let ir = CadIr::empty();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let limit = |hold_first: bool| {
        crate::test_support::allocation_limit_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            None,
            |cap| {
                let arena = cadmpeg_core::decode::DecodeArena::new();
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_materialized_bytes = cap;
                let (ctx, _) =
                    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
                        .expect("root");
                let first = super::feature_plane_equations(&ctx, &scan, &ir, &carriers, 917)?
                    .expect("complete planes");
                assert_eq!(first.0.len(), 1);
                let held = hold_first.then_some(first);
                let second = super::feature_plane_equations(&ctx, &scan, &ir, &carriers, 917)?
                    .expect("complete planes");
                assert_eq!(second.0.len(), 1);
                drop(held);
                Ok(())
            },
        )
    };
    assert!(limit(true) > limit(false));
}

#[test]
fn plane_carrier_index_borrows_rows_and_keeps_unrelated_ambiguity() {
    let records = [
        plane_outline(7, 2.0),
        plane_outline(8, 3.0),
        plane_outline(8, 4.0),
    ];
    let index = crate::test_support::assert_work_boundaries(
        &["creo plane carrier index scan", "creo plane carrier index"],
        |ctx| super::plane_carrier_index(ctx, &records, |plane| plane.surface_id),
    );
    assert!(std::ptr::eq(
        index.get(&7).copied().flatten().expect("unique carrier"),
        &records[0]
    ));
    assert!(index.get(&8).expect("ambiguous carrier").is_none());
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(plane_row(7));
    scan.planes.outlines.extend(records);
    let planes = crate::decode::with_test_decode_ctx(|ctx| {
        super::feature_outline_planes(ctx, &scan, 917)
            .map(|result| result.map(|(planes, _storage)| planes))
    })
    .expect("service resources")
    .expect("complete plane");
    assert_eq!(planes, [(7, [0.0, 0.0, 2.0], [0.0, 0.0, 1.0])]);
}
