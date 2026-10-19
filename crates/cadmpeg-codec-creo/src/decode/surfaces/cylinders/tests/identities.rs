// SPDX-License-Identifier: Apache-2.0

fn hole_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 7,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Hole),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            7,
            29,
            [11, 12, 13, 14]
                .into_iter()
                .map(crate::feature::entity::dummy_table_entry)
                .collect(),
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([11, 12, 13, 14]),
    );
    for (id, z) in [(11, 0.0), (12, 2.0)] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 7,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: id,
            origin: [0.0, 0.0, z],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
        scan.planes
            .envelopes
            .push(crate::surface::PlaneEnvelopeRecord {
                surface_id: id,
                body: Vec::new(),
                envelope: crate::surface::PlaneEnvelope::Standard {
                    bounds_2d: [[None; 2]; 2],
                    corners_3d: [
                        [Some(-1.0), Some(-1.0), Some(z)],
                        [Some(1.0), Some(1.0), Some(z)],
                    ],
                },
                corner_coordinate_equal: [Some(false), Some(false), Some(true)],
                scalar_tokens: Vec::new(),
                row_offset: 0,
                offset: 0,
            });
    }
    for id in [13, 14] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 7,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan
}

fn circular_sweep_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),
            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            40,
            29,
            vec![
                entry(43, 204, None),
                entry(46, 203, None),
                entry(49, 200, Some(4)),
                entry(51, 200, None),
            ],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([43, 46, 51]),
    );
    for (id, y) in [(43, 4.0), (46, -4.0)] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 40,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: id,
            origin: [0.0, y, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: usize::try_from(id).expect("fixture index fits usize"),
        });
    }
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 51,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 40,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 51,
    });
    scan.planes
        .envelopes
        .push(crate::surface::PlaneEnvelopeRecord {
            surface_id: 46,
            body: Vec::new(),
            envelope: crate::surface::PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [Some(-1.0), Some(-4.0), Some(-1.0)],
                    [Some(1.0), Some(-4.0), Some(1.0)],
                ],
            },
            corner_coordinate_equal: [Some(false), Some(true), Some(false)],
            scalar_tokens: Vec::new(),
            row_offset: 0,
            offset: 0,
        });
    scan
}

#[test]
fn hole_and_circular_sweep_identities_preserve_service_geometry() {
    let hole = hole_scan();
    let mut hole_ir = cadmpeg_ir::document::CadIr::empty();
    let hole_count = crate::decode::with_service_context(|ctx| {
        super::super::transfer_hole_cylinders(
            ctx,
            &hole,
            &mut hole_ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("hole cylinders admitted");
    assert_eq!(hole_count, 2);
    assert_eq!(
        hole_ir.model.surfaces[0].id.as_str(),
        "creo:visibgeom:surface#13"
    );
    assert_eq!(
        hole_ir.model.surfaces[0]
            .source_object
            .as_ref()
            .expect("source")
            .object_id,
        "VisibGeom:13"
    );

    let sweep = circular_sweep_scan();
    let mut sweep_ir = cadmpeg_ir::document::CadIr::empty();
    let sweep_count = crate::decode::with_service_context(|ctx| {
        super::super::transfer_circular_sweep_cylinders(
            ctx,
            &sweep,
            &mut sweep_ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("circular sweep cylinder admitted");
    assert_eq!(sweep_count, 1);
    assert_eq!(
        sweep_ir.model.surfaces[0].id.as_str(),
        "creo:visibgeom:surface#51"
    );
    assert_eq!(
        sweep_ir.model.surfaces[0]
            .source_object
            .as_ref()
            .expect("source")
            .object_id,
        "VisibGeom:51"
    );
}

fn cross_section_plane_scan(local_system: bool) -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let diagonal = std::f64::consts::FRAC_1_SQRT_2;
    if local_system {
        scan.planes
            .cross_section_local_systems
            .push(crate::surface::PlaneLocalSystem {
                surface_id: 7,
                body: Vec::new(),
                slots: [
                    0.0, 0.0, 1.0, 0.0, 0.0, 0.0, diagonal, diagonal, 0.0, 0.0, 0.0, 0.0,
                ]
                .map(Some),
                layout: Some(crate::scalar::PlaneSupportFrameLayout::DirectNormalTriples),
                classification: crate::surface::LocalSystemClassification::Simple,
                row_offset: 0,
                offset: 0,
            });
    } else {
        scan.planes
            .cross_section_outlines
            .push(crate::surface::OutlinePlane {
                surface_id: 7,
                origin: [0.0, 0.0, 0.0],
                normal: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::from([
                    diagonal, diagonal, 0.0,
                ]))
                .expect("unit diagonal normal"),
                u_axis: cadmpeg_ir::units::UnitVector3::Z_AXIS,
                offset: 0,
            });
    }
    scan
}

#[test]
fn cross_section_plane_identities_preserve_service_geometry() {
    for local_system in [true, false] {
        let scan = cross_section_plane_scan(local_system);
        let mut ir = cadmpeg_ir::document::CadIr::empty();
        let count = crate::decode::with_service_context(|ctx| {
            super::super::transfer_cross_section_planes(
                ctx,
                &scan,
                &mut ir,
                &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
                &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
            )
        })
        .expect("cross-section plane admitted");
        assert_eq!(count, 1);
        assert_eq!(
            ir.model.surfaces[0].id.as_str(),
            "creo:cross_section_geometry:surface#7"
        );
        assert_eq!(
            ir.model.surfaces[0]
                .source_object
                .as_ref()
                .expect("source object")
                .object_id,
            "Xsections:7"
        );
    }
}

fn positional_cone_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Cone,
        feature_id: 1,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 7,
    });
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 7,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Cone(
                    crate::surface::PositionalConeFrame::new(
                        [0.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0],
                        crate::surface::ApexConeHalfAngle::new(std::f64::consts::FRAC_PI_4)
                            .expect("valid cone half angle"),
                    )
                    .expect("valid positional cone frame"),
                ),
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 7,
            body_offset: 7,
        });
    scan
}

#[test]
fn positional_cone_identity_preserves_service_geometry() {
    let scan = positional_cone_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let count = crate::decode::with_service_context(|ctx| {
        super::super::transfer_positional_cones(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        )
    })
    .expect("positional cone admitted");
    assert_eq!(count, 1);
    assert_eq!(ir.model.surfaces[0].id.as_str(), "creo:visibgeom:surface#7");
    assert_eq!(
        ir.model.surfaces[0]
            .source_object
            .as_ref()
            .expect("source object")
            .object_id,
        "VisibGeom:7"
    );
}
