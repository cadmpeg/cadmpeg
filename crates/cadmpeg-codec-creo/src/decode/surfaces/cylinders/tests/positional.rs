// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn section_feature_type24_frame_is_not_admitted_as_round_cylinder() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 916,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Cut),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 916,
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
                crate::surface::InlineSurfaceCarrier::Cylinder {
                    frame: crate::surface::PositionalCylinderFrame::new(
                        [0.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0],
                        1.0,
                        Some(2.0),
                    )
                    .expect("valid positional cylinder frame"),
                    split_bounds: None,
                },
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 7,
            body_offset: 7,
        });
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::transfer_positional_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity")
        .transferred,
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn unresolved_round_type24_frame_is_not_admitted_as_constant_cylinder() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 913,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 7,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 7,
        },
        crate::surface::SurfaceRow {
            id: 8,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 8,
        },
    ]);
    let parameter = |surface_id, radius| crate::surface::SurfaceParameterRecord {
        surface_id,
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
        offset: usize::try_from(surface_id).expect("fixture index fits usize"),
        body_offset: usize::try_from(surface_id).expect("fixture index fits usize"),
    };
    scan.surfaces
        .parameters
        .extend([parameter(7, 1.0), parameter(8, 2.0)]);
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::transfer_positional_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity")
        .transferred,
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

fn inline_type24_scan() -> crate::container::ContainerScan<'static> {
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
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 7,
            body: vec![0x0f, 0x12, 0xe3, 0x0f],
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Cylinder {
                    frame: crate::surface::PositionalCylinderFrame::new(
                        [0.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0],
                        [1.0, 0.0, 0.0],
                        1.0,
                        Some(2.0),
                    )
                    .expect("valid positional cylinder frame"),
                    split_bounds: None,
                },
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 7,
            body_offset: 7,
        });
    scan
}

#[test]
fn inline_type24_frame_is_admitted_in_a_round_feature() {
    let scan = inline_type24_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::transfer_positional_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity")
        .transferred,
        1
    );
    assert!(ir
        .model
        .surfaces
        .iter()
        .any(|surface| { surface.id.as_str() == "creo:visibgeom:surface#7" }));
}

fn inline_type24_retained_refusal(limit: u64) -> cadmpeg_core::CodecError {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = inline_type24_scan();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = limit;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &policy).expect("empty root is admitted");
    super::super::transfer_positional_cylinders(
        &ctx,
        &scan,
        &mut cadmpeg_ir::document::CadIr::empty(),
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
    )
    .expect_err("positional cylinder identity exceeds retained limit")
}

#[test]
fn positional_cylinder_identity_refuses_retained_limit() {
    let error = inline_type24_retained_refusal(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo positional cylinder identity"),
        |cap| Err::<(), _>(inline_type24_retained_refusal(cap)),
    ));
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo positional cylinder identity")
    );
}

#[test]
fn positional_cylinder_source_id_refuses_retained_limit() {
    let run = |limit| Err::<(), _>(inline_type24_retained_refusal(limit));
    let error = run(crate::test_support::allocation_limit_at(
        cadmpeg_core::decode::ResourceDimension::RetainedBytes,
        Some("creo positional cylinder source IDs"),
        run,
    ))
    .expect_err("named resource boundary");
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo positional cylinder source IDs")
    );
}

#[test]
fn positional_frame_reconciles_an_existing_model_cylinder() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features.rows.push(crate::feature::rows::FeatureRow {
        feature_id: 917,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Protrusion),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 917,
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
                crate::surface::InlineSurfaceCarrier::Cylinder {
                    frame: crate::surface::PositionalCylinderFrame::new(
                        [-12.5, 4.0, 0.0],
                        [0.0, 1.0, 0.0],
                        [1.0, 0.0, 0.0],
                        0.75,
                        Some(34.0),
                    )
                    .expect("valid positional cylinder frame"),
                    split_bounds: None,
                },
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 7,
            body_offset: 7,
        });
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_cylinder(7, 0.75));

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::transfer_positional_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity")
        .transferred,
        0
    );
    let [surface] = ir.model.surfaces.as_slice() else {
        panic!("one reconciled cylinder");
    };
    let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved() else {
        panic!("reconciled cylinder: {:?}", surface.geometry);
    };
    let origin = cylinder_surface.origin().get();
    let axis = *cylinder_surface.frame().axis().as_raw();
    let ref_direction = *cylinder_surface.frame().reference().as_raw();
    let radius = cylinder_surface.radius().get();
    assert_eq!(origin, [-12.5, 4.0, 0.0].into());
    assert_eq!(axis, [0.0, 1.0, 0.0].into());
    assert_eq!(ref_direction, [1.0, 0.0, 0.0].into());
    assert_eq!(radius, 0.75);
}

#[test]
fn counterbore_positional_radius_gate_rejects_unrelated_frame() {
    let scan = counterbore_dimension_gate_scan(24.5);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_cylinder(1, 60.0));

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::transfer_positional_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity")
        .transferred,
        0
    );
    assert!(ir
        .model
        .surfaces
        .iter()
        .all(|surface| surface.id.as_str() != "creo:visibgeom:surface#3"));
}

#[test]
fn counterbore_positional_radius_gate_accepts_declared_source_radius() {
    let scan = counterbore_dimension_gate_scan(20.0);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_cylinder(1, 60.0));

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::super::transfer_positional_cylinders(
            ctx,
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
            &mut crate::decode::source_carriers::SourceUnitCarriers::default(),
        ))
        .expect("valid source object identity")
        .transferred,
        1
    );
    assert!(ir
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id.as_str() == "creo:visibgeom:surface#3"));
}
