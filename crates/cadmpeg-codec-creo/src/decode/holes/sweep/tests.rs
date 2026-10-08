// SPDX-License-Identifier: Apache-2.0

#[test]
fn compact_simple_hole_rejects_duplicate_materialized_roster_id() {
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),

            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
    let table = crate::feature::entity::FeatureEntityTable::new(
        107,
        29,
        vec![
            entry(109, 204, None),
            entry(112, 203, None),
            entry(115, 200, Some(0)),
            entry(117, 200, None),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([117]);
    let row = crate::surface::SurfaceRow {
        id: 117,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 107,
        reversed: true,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::compact_simple_hole_cylinder_id(ctx, 107, std::slice::from_ref(&table), &crate::surface::SurfaceRows::from_rows((std::slice::from_ref(&row)).to_vec())))
        .expect("admitted surface roster"),
        Some(117)
    );

    let mut duplicate = table;
    duplicate
        .entries
        .push(crate::feature::entity::dummy_table_entry(117));
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::compact_simple_hole_cylinder_id(ctx, 107, std::slice::from_ref(&duplicate), &crate::surface::SurfaceRows::from_rows((std::slice::from_ref(&row)).to_vec())))
        .expect("admitted surface roster"),
        None
    );
}

#[test]
fn circular_sweep_requires_an_exact_materialized_surface_roster() {
    let table = crate::feature::entity::FeatureEntityTable::new(
        40,
        29,
        vec![
            crate::feature::entity::dummy_table_entry(46),
            crate::feature::entity::dummy_table_entry(51),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([46, 51]);

    assert!(crate::decode::with_test_decode_ctx(|ctx| {
        super::has_exact_materialized_surface_roster(ctx, &table, &[46, 51])
    })
    .expect("admitted surface roster"));
    assert!(!crate::decode::with_test_decode_ctx(|ctx| {
        super::has_exact_materialized_surface_roster(ctx, &table, &[46, 46])
    })
    .expect("admitted surface roster"));

    let mut duplicate = table.clone();
    duplicate
        .entries
        .push(crate::feature::entity::dummy_table_entry(51));
    assert!(!crate::decode::with_test_decode_ctx(|ctx| {
        super::has_exact_materialized_surface_roster(ctx, &duplicate, &[46, 51])
    })
    .expect("admitted surface roster"));

    let mut extra = table;
    extra
        .entries
        .push(crate::feature::entity::dummy_table_entry(54));
    extra.mark_surface_id(54);
    assert!(!crate::decode::with_test_decode_ctx(|ctx| {
        super::has_exact_materialized_surface_roster(ctx, &extra, &[46, 51])
    })
    .expect("admitted surface roster"));
}

#[test]
fn extrusion_span_refuses_offsets_whose_length_overflows() {
    let planes = [
        ([0.0, 0.0, f64::MAX], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, -f64::MAX], [0.0, 0.0, 1.0]),
    ];
    assert!(crate::decode::with_test_decode_ctx(|ctx| super::extrusion_span(ctx, [0.0; 3], [0.0, 0.0, 1.0], planes)).expect("service resources").is_none());
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::extrusion_span(ctx, [0.0; 3], [0.0, 0.0, 1.0], [
                ([0.0, 0.0, 2.0], [0.0, 0.0, 1.0]),
                ([0.0, 0.0, -1.0], [0.0, 0.0, -1.0]),
            ])).expect("service resources"),
        Some(super::ExtrusionSpan::new(-1.0, 2.0).expect("valid span fixture"))
    );
}

#[test]
fn extrusion_span_keeps_the_first_offset_in_a_near_duplicate_pair() {
    let plane = |z| ([0.0, 0.0, z], [0.0, 0.0, 1.0]);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::extrusion_span(ctx, [0.0; 3], [0.0, 0.0, 1.0], [plane(1.0), plane(1.0 + 5.0e-10)])).expect("service resources"),
        super::ExtrusionSpan::new(0.0, 1.0)
    );
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::extrusion_span(ctx, [0.0; 3], [0.0, 0.0, 1.0], [plane(5.0e-10), plane(-5.0e-10)])).expect("service resources"),
        super::ExtrusionSpan::new(0.0, 5.0e-10)
    );
}

fn test_decode_ctx_with_collection_limit<'a>(
    arena: &'a cadmpeg_core::decode::DecodeArena,
    policy: &'a cadmpeg_core::decode::DecodePolicy,
) -> cadmpeg_core::decode::DecodeContext<'a> {
    cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], arena, policy)
        .expect("empty root admitted")
        .0
}

#[test]
fn simple_hole_cylinder_rows_refuse_collection_limit() {
    let mut scan = crate::test_support::empty_container_scan();
    let entry = |entity_id| crate::feature::entity::dummy_table_entry(entity_id);
    scan.features.entity_tables.push(
        crate::feature::entity::FeatureEntityTable::new(
            7,
            29,
            vec![entry(11), entry(12), entry(13), entry(14)],
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let service_ctx = test_decode_ctx_with_collection_limit(&arena, &policy);
    assert!(super::simple_hole_geometry(&service_ctx, &scan, 7)
        .expect("service resources")
        .is_some());
    policy.limits.max_collection_items = 2;
    let ctx = test_decode_ctx_with_collection_limit(&arena, &policy);
    let error =
        super::simple_hole_geometry(&ctx, &scan, 7).expect_err("cylinder rows exceed limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo simple hole cylinder rows")
    );
}

#[test]
fn compact_hole_cylinder_rows_refuse_collection_limit() {
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
            107,
            29,
            vec![
                entry(109, 204, None),
                entry(112, 203, None),
                entry(115, 200, Some(0)),
                entry(117, 200, None),
            ],
            &std::collections::BTreeSet::new(),
            0,
        )
        .with_surface_ids([117]),
    );
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 117,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 107,
        reversed: true,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    });
    let frame = crate::surface::PositionalCylinderFrame::new(
        [0.0; 3],
        [0.0, 0.0, 1.0],
        [1.0, 0.0, 0.0],
        0.75,
        Some(2.0),
    )
    .expect("cylinder frame");
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 117,
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let service_ctx = test_decode_ctx_with_collection_limit(&arena, &policy);
    assert!(
        super::compact_simple_hole_geometry(&service_ctx, &scan, 107)
            .expect("service resources")
            .is_some()
    );
    policy.limits.max_collection_items = 0;
    let ctx = test_decode_ctx_with_collection_limit(&arena, &policy);
    let error = super::compact_simple_hole_geometry(&ctx, &scan, 107)
        .expect_err("cylinder row exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo compact hole cylinder rows")
    );
}

fn circular_sweep_limit_scan(two_cap: bool) -> crate::container::ContainerScan<'static> {
    let mut scan = crate::test_support::empty_container_scan();
    let entry =
        |entity_id, class_id, source_entity_id| crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),
            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        };
    let table = crate::feature::entity::FeatureEntityTable::new(
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
    );
    scan.features.entity_tables.push(if two_cap {
        table.with_surface_ids([43, 46, 51])
    } else {
        table.with_surface_ids([46, 51])
    });
    let row = |id, kind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 40,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: usize::try_from(id).expect("fixture index fits usize"),
    };
    if two_cap {
        scan.surfaces
            .rows
            .push(row(43, crate::surface::SurfaceKind::Plane));
        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: 43,
            origin: [0.0, 4.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 43,
        });
    } else {
        scan.features.section_transforms.push(
            crate::placement::FeatureSectionTransform::new(
                40,
                Some(40),
                [0.0; 3],
                [1.0, 0.0, 0.0],
                [0.0, 0.0, -1.0],
                0,
            )
            .expect("section transform"),
        );
    }
    scan.surfaces.rows.extend([
        row(46, crate::surface::SurfaceKind::Plane),
        row(51, crate::surface::SurfaceKind::Cylinder),
    ]);
    let y = if two_cap { -4.0 } else { 16.0 };
    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 46,
        origin: [0.0, y, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
        offset: 46,
    });
    scan.planes
        .envelopes
        .push(crate::surface::PlaneEnvelopeRecord {
            surface_id: 46,
            body: Vec::new(),
            envelope: crate::surface::PlaneEnvelope::Standard {
                bounds_2d: [[None; 2]; 2],
                corners_3d: [
                    [Some(-1.0), Some(y), Some(-1.0)],
                    [Some(1.0), Some(y), Some(1.0)],
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
fn single_cap_circular_cylinder_rows_refuse_collection_limit() {
    let scan = circular_sweep_limit_scan(false);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let service_ctx = test_decode_ctx_with_collection_limit(&arena, &policy);
    assert!(
        super::single_cap_circular_sweep_geometry(&service_ctx, &scan, 40)
            .expect("service resources")
            .is_some()
    );
    policy.limits.max_collection_items = 0;
    let ctx = test_decode_ctx_with_collection_limit(&arena, &policy);
    let error = super::single_cap_circular_sweep_geometry(&ctx, &scan, 40)
        .expect_err("cylinder row exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo single-cap circular cylinder rows")
    );
}

#[test]
fn two_cap_circular_cylinder_rows_refuse_collection_limit() {
    let scan = circular_sweep_limit_scan(true);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    let service_ctx = test_decode_ctx_with_collection_limit(&arena, &policy);
    assert!(
        super::two_cap_circular_sweep_geometry(&service_ctx, &scan, 40)
            .expect("service resources")
            .is_some()
    );
    policy.limits.max_collection_items = 0;
    let ctx = test_decode_ctx_with_collection_limit(&arena, &policy);
    let error = super::two_cap_circular_sweep_geometry(&ctx, &scan, 40)
        .expect_err("cylinder row exceeds limit");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.dimension == cadmpeg_core::decode::ResourceDimension::CollectionItems
            && resource.operation == "creo two-cap circular cylinder rows")
    );
}
