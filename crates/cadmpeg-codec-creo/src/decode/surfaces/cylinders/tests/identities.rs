// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn hole_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
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
            offset: id as usize,
        });
        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: id,
            origin: [0.0, 0.0, z],
            normal: cadmpeg_ir::units::UnitVector3::Z_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: id as usize,
        });
        scan.planes.envelopes.push(crate::surface::PlaneEnvelopeRecord {
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
            offset: id as usize,
        });
    }
    scan
}

fn circular_sweep_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    let entry = |entity_id, class_id, source_entity_id| {
        crate::feature::entity::FeatureEntityTableEntry {
            payload: crate::feature::entity::entry_payload(class_id, source_entity_id, None, None),
            entity_id,
            prefixed: false,
            offset: 0,
            end_offset: 0,
        }
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
            offset: id as usize,
        });
        scan.planes.outlines.push(crate::surface::OutlinePlane {
            surface_id: id,
            origin: [0.0, y, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: id as usize,
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
    scan.planes.envelopes.push(crate::surface::PlaneEnvelopeRecord {
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

fn hole_retained_refusal(limit: u64) -> cadmpeg_core::CodecError {
    let scan = hole_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root admitted");
    super::super::transfer_hole_cylinders(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("hole cylinder identity exceeds retained limit")
}

fn circular_sweep_retained_refusal(limit: u64) -> cadmpeg_core::CodecError {
    let scan = circular_sweep_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    super::super::transfer_circular_sweep_cylinders(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("circular sweep cylinder identity exceeds retained limit")
}

#[test]
fn hole_cylinder_identity_refuses_retained_limit() {
    let error = hole_retained_refusal(0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo hole cylinder identity"), "{error:?}");
}

#[test]
fn hole_cylinder_source_id_refuses_retained_limit() {
    let limit = u64::try_from("creo:visibgeom:surface#13".len()).expect("identity length fits");
    let error = hole_retained_refusal(limit);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo hole cylinder source IDs"), "{error:?}");
}

#[test]
fn circular_sweep_cylinder_identity_refuses_retained_limit() {
    let error = circular_sweep_retained_refusal(0);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo circular sweep cylinder identity"), "{error:?}");
}

#[test]
fn circular_sweep_cylinder_source_id_refuses_retained_limit() {
    let limit = u64::try_from("creo:visibgeom:surface#51".len()).expect("identity length fits");
    let error = circular_sweep_retained_refusal(limit);
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo circular sweep cylinder source IDs"), "{error:?}");
}

#[test]
fn hole_and_circular_sweep_identities_preserve_service_geometry() {
    let hole = hole_scan();
    let mut hole_ir = cadmpeg_ir::document::CadIr::empty();
    let hole_count = crate::decode::with_test_decode_ctx(|ctx| {
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
    assert_eq!(hole_ir.model.surfaces[0].id.as_str(), "creo:visibgeom:surface#13");
    assert_eq!(hole_ir.model.surfaces[0].source_object.as_ref().expect("source").object_id, "VisibGeom:13");

    let sweep = circular_sweep_scan();
    let mut sweep_ir = cadmpeg_ir::document::CadIr::empty();
    let sweep_count = crate::decode::with_test_decode_ctx(|ctx| {
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
    assert_eq!(sweep_ir.model.surfaces[0].id.as_str(), "creo:visibgeom:surface#51");
    assert_eq!(sweep_ir.model.surfaces[0].source_object.as_ref().expect("source").object_id, "VisibGeom:51");
}

#[test]
fn constrained_slot_cylinder_identity_refuses_retained_limit() {
    let scan = super::slot_fillet_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::super::transfer_constrained_slot_fillet_cylinders(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("constrained slot identity exceeds retained limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo constrained slot cylinder identity"), "{error:?}");
}
