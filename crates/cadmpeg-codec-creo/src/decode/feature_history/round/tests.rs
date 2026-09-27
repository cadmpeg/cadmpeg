mod chamfer;

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_ir::scalar::PositiveLength;
// SPDX-License-Identifier: Apache-2.0

fn chamfer_distance_with_service_ctx(
    scan: &crate::container::ContainerScan<'_>,
    ir: &cadmpeg_ir::document::CadIr,
    source_carriers: &crate::decode::source_carriers::SourceUnitCarriers,
    feature_id: u32,
) -> Option<f64> {
    crate::decode::with_test_decode_ctx(|ctx| {
        super::chamfer_constant_distance(ctx, scan, ir, source_carriers, feature_id)
    })
    .expect("service profile admits chamfer witnesses")
}

fn round_sample_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 7,
    });
    let token = crate::surface::SurfaceParameterScalar {
        value: Some(0.5),
        raw: vec![0x53, 0, 0, 0, 0, 0, 0],
        offset: 0,
    };
    scan.surfaces.parameters.push(crate::surface::SurfaceParameterRecord {
        surface_id: 7,
        body: token.raw.clone(),
        scalar_tokens: vec![token],
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::Cylinder,
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 7,
        body_offset: 7,
    });
    scan
}

fn round_sample_ir() -> cadmpeg_ir::document::CadIr {
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#7")
            .expect("identity grammar"),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
            SolvedSurfaceGeometry::Cylinder(
                cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    0.5,
                )
                .expect("cylinder fixture"),
            ),
        ),
        source_object: None,
    });
    ir
}

fn mixed_round_sample_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = round_sample_scan();
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 8,
        kind: crate::surface::SurfaceKind::TorusOrSphere,
        feature_id: 5,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 8,
    });
    let body = vec![
        0x18, 0x0d, 0x41, 0xcf, 0xff, 0xff, 0xff, 0xe5, 0x79, 0x7b, 0x0e, 0x29, 0xdf, 0xff,
    ];
    scan.surfaces.parameters.push(crate::surface::SurfaceParameterRecord {
        surface_id: 8,
        body,
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
            crate::surface::SurfaceKind::TorusOrSphere,
        ),
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: 8,
        body_offset: 8,
    });
    scan
}

fn mixed_round_sample_limit_error(limit: u64, operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = mixed_round_sample_scan();
    let rows = scan.surfaces.rows.iter().collect::<Vec<_>>();
    let ir = cadmpeg_ir::document::CadIr::empty();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::mixed_round_radius_samples(
        &ctx,
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        &rows,
    )
    .expect_err("mixed round sample growth exceeds the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn generated_round_rows_refuse_collection_limit() {
    let mut scan = round_sample_scan();
    scan.surfaces.parameters.clear();
    let ir = cadmpeg_ir::document::CadIr::empty();
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = super::round_constant_radius(
        &ctx,
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        5,
    )
    .expect_err("generated row exceeds the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo generated round rows"), "{error:?}");
}

#[test]
fn mixed_torus_rows_refuse_collection_limit() {
    mixed_round_sample_limit_error(0, "creo mixed torus rows");
}

#[test]
fn mixed_cylinder_radii_refuse_collection_limit() {
    mixed_round_sample_limit_error(1, "creo mixed cylinder radii");
}

#[test]
fn mixed_torus_override_samples_refuse_collection_limit() {
    mixed_round_sample_limit_error(2, "creo torus override samples");
}

#[test]
fn mixed_combined_samples_refuse_collection_limit() {
    mixed_round_sample_limit_error(3, "creo mixed round samples");
}

#[test]
fn mixed_round_samples_keep_family_order_under_service_policy() {
    let scan = mixed_round_sample_scan();
    let rows = scan.surfaces.rows.iter().collect::<Vec<_>>();
    let samples = crate::decode::with_test_decode_ctx(|ctx| {
        super::mixed_round_radius_samples(
            ctx,
            &scan,
            &cadmpeg_ir::document::CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            &rows,
        )
    })
    .expect("service profile admits mixed round samples")
    .expect("both families resolve");
    assert_eq!(samples.len(), 2);
    assert_eq!(samples[0], 0.5);
    assert_eq!(samples[1], 0.249_999_999_951_747_04);
}

fn round_sample_limit_error(
    limit: u64,
    operation: &'static str,
    run: impl FnOnce(
        &cadmpeg_core::decode::DecodeContext<'_>,
        &crate::container::ContainerScan<'_>,
        &cadmpeg_ir::document::CadIr,
    ) -> Result<(), cadmpeg_core::CodecError>,
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = round_sample_scan();
    let ir = round_sample_ir();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = run(&ctx, &scan, &ir).expect_err("round samples exceed the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn observed_round_radii_refuse_collection_limit() {
    round_sample_limit_error(0, "creo observed round radii", |ctx, scan, _| {
        super::round_observed_radii(ctx, scan, 5).map(|_| ())
    });
}

#[test]
fn placed_round_radii_refuse_collection_limit() {
    round_sample_limit_error(0, "creo placed round radii", |ctx, scan, ir| {
        super::round_placed_cylinder_radii(
            ctx,
            scan,
            ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
        )
        .map(|_| ())
    });
}

#[test]
fn replay_round_combined_samples_refuse_collection_limit() {
    round_sample_limit_error(2, "creo round replay samples", |ctx, scan, ir| {
        super::round_replay_radius(
            ctx,
            scan,
            ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
        )
        .map(|_| ())
    });
}

#[test]
fn legacy_round_combined_samples_refuse_collection_limit() {
    round_sample_limit_error(2, "creo legacy round samples", |ctx, scan, ir| {
        super::legacy_round_radius_agrees(
            ctx,
            scan,
            ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            cadmpeg_ir::scalar::PositiveReal::new(0.5).expect("positive radius"),
        )
        .map(|_| ())
    });
}

#[test]
fn feature_round_combined_samples_refuse_collection_limit() {
    round_sample_limit_error(2, "creo feature round samples", |ctx, scan, ir| {
        crate::decode::feature_history::draft::schema_feature_definition(
            ctx,
            scan,
            ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            5,
            Some(crate::feature::schema::SchemaClass::Round),
            "Round",
        )
        .map(|_| ())
    });
}

#[test]
fn round_sample_fixture_keeps_constant_radius_under_service_policy() {
    let scan = round_sample_scan();
    let ir = round_sample_ir();
    let carriers = crate::decode::source_carriers::SourceUnitCarriers::default();
    let (observed, placed, agrees, radius) = crate::decode::with_test_decode_ctx(|ctx| {
        Ok::<_, cadmpeg_core::CodecError>((
            super::round_observed_radii(ctx, &scan, 5)?,
            super::round_placed_cylinder_radii(ctx, &scan, &ir, &carriers, 5)?,
            super::legacy_round_radius_agrees(
                ctx,
                &scan,
                &ir,
                &carriers,
                5,
                cadmpeg_ir::scalar::PositiveReal::new(0.5).expect("positive radius"),
            )?,
            super::round_constant_radius(ctx, &scan, &ir, &carriers, 5)?,
        ))
    })
    .expect("service profile admits round samples");
    assert_eq!(observed, [0.5]);
    assert_eq!(placed, [0.5]);
    assert!(agrees);
    assert_eq!(radius, Some(0.5));
}

#[test]
fn chamfer_does_not_use_a_cone_prototype_as_model_space_placement() {
    let body = [
        197, 251, 126, 24, 209, 212, 112, 107, 81, 235, 133, 30, 184, 70, 125, 251, 126, 24, 209,
        212, 112, 123, 0, 68, 204, 99, 17, 228, 72, 66, 64, 192, 170, 175, 125, 232, 45, 177, 195,
        0, 68, 204, 99, 17, 220, 70, 66, 1, 69, 135, 177, 98, 82, 120, 170, 175, 125, 232, 45, 187,
        65, 200, 122, 225, 71, 174, 20, 128, 227, 24, 228, 15, 24, 15, 24, 16, 24, 228, 70, 66,
        129, 71, 174, 20, 122, 225, 25, 194, 145, 29, 33, 143, 32, 210, 52, 233, 0, 116, 33, 251,
        84, 68, 45, 5,
    ];
    let (local_system, half_angle) = body.split_at(body.len() - 7);
    let mut payload = b"srf_array\0\xf8\x01".to_vec();
    payload.extend_from_slice(&[7, 0x25, 4, 0x01, 0, 0]);
    payload.extend_from_slice(b"srf_prim_ptr(cone)\0\xe0\x02local_sys\0\xf9\x04\x03");
    payload.extend_from_slice(local_system);
    payload.extend_from_slice(b"\xe0\x01half_angle\0");
    payload.extend_from_slice(half_angle);
    payload.extend_from_slice(b"\xe0\x00parent_feats\0\xf8\x01\x04");
    payload.extend_from_slice(b"crv_array\0\xf3\xf8\0");

    let mut scan = crate::container::scan_bytes_ok(crate::test_support::build_prt(
        "cone-template",
        &[("VisibGeom", payload)],
    ));
    let [prototype] = scan.surfaces.prototype_records.as_slice() else {
        panic!("complete cone prototype");
    };
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            crate::decode::surfaces::prototypes::unique_surface_prototype_associations(ctx, &scan)
        })
        .expect("prototype associations")
        .len(),
        1
    );
    let frame = crate::surface::prototype_cone_frame(prototype).expect("prototype frame");
    assert!(scan
        .surfaces
        .parameters
        .iter()
        .find(|record| record.surface_id == 7)
        .is_some_and(|record| record.positional_cone_frame().is_none()));

    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 31,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 31,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 31,
            origin: std::array::from_fn(|index| {
                frame.frame().origin()[index] + frame.frame().axis()[index]
            }),
            normal: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::from(
                frame.frame().axis(),
            ))
            .expect("unit axis"),
            u_axis: cadmpeg_ir::units::UnitVector3::new(cadmpeg_ir::math::Vector3::from(
                frame.frame().ref_direction(),
            ))
            .expect("unit reference direction"),
            offset: 31,
        });
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 4,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    assert_eq!(
        chamfer_distance_with_service_ctx(
            &scan,
            &cadmpeg_ir::document::CadIr::empty(),
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            4
        ),
        None
    );
}

#[test]
fn chamfer_uses_transferred_model_plane_carrier() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 10,
            kind: crate::surface::SurfaceKind::Cone,
            feature_id: 914,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 10,
        },
        crate::surface::SurfaceRow {
            id: 31,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 3,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 31,
        },
    ]);
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 10,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Cone(
                    crate::surface::PositionalConeFrame::new(
                        [0.5, 0.0, 0.0],
                        [-1.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0],
                        crate::surface::ApexConeHalfAngle::new(std::f64::consts::FRAC_PI_4)
                            .expect("apex cone half angle"),
                    )
                    .expect("valid positional cone frame"),
                ),
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 10,
            body_offset: 11,
        });
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 914,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#31".to_string())
            .expect("identity grammar"),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        )),
        source_object: None,
    });

    assert_eq!(
        chamfer_distance_with_service_ctx(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        Some(0.5)
    );

    let transferred_plane_row = scan.surfaces.rows.pop().expect("support plane row");
    assert_eq!(
        chamfer_distance_with_service_ctx(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        Some(0.5)
    );
    scan.surfaces.rows.push(transferred_plane_row);

    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 31,
        origin: [0.0, 0.0, 0.0],
        normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
        u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
        offset: 31,
    });
    assert_eq!(
        chamfer_distance_with_service_ctx(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        Some(0.5)
    );

    let mut conflicting_ir = ir.clone();
    match &mut conflicting_ir.model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            plane_surface,
        )) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let mut origin = *origin;
            origin.x = 0.25;
            *plane_surface =
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin, *normal, *u_axis)
                    .expect("valid PlaneSurface fixture");
        }
        _ => panic!("transferred plane geometry"),
    }
    assert_eq!(
        chamfer_distance_with_service_ctx(
            &scan,
            &conflicting_ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        None
    );
}

#[test]
fn slot_fillet_cylinder_skips_parallel_midplane_candidates() {
    let cylinder = crate::decode::with_test_decode_ctx(|ctx| super::slot_fillet_cylinder(
        ctx,
        [
            crate::decode::analytic::equations::PlaneEquation {
                origin: [0.0, -2.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
            crate::decode::analytic::equations::PlaneEquation {
                origin: [0.0, 3.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
        ],
        &[
            crate::decode::analytic::equations::PlaneEquation {
                origin: [-9.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            crate::decode::analytic::equations::PlaneEquation {
                origin: [-8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            crate::decode::analytic::equations::PlaneEquation {
                origin: [-9.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            crate::decode::analytic::equations::PlaneEquation {
                origin: [-8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            crate::decode::analytic::equations::PlaneEquation {
                origin: [0.0, 0.0, -7.0],
                normal: [0.0, 0.0, 1.0],
            },
            crate::decode::analytic::equations::PlaneEquation {
                origin: [0.0, 0.0, -6.0],
                normal: [0.0, 0.0, 1.0],
            },
        ],
    ))
    .expect("service profile admits slot midplanes")
    .expect("later independent support pair");

    assert_eq!(cylinder.origin, [-8.5, -2.0, -6.5]);
    assert_eq!(cylinder.radius, 0.5);
}

#[test]
fn slot_fillet_midplanes_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let plane = |origin, normal| crate::decode::analytic::equations::PlaneEquation { origin, normal };
    let caps = [
        plane([0.0, -2.0, 0.0], [0.0, 1.0, 0.0]),
        plane([0.0, 3.0, 0.0], [0.0, 1.0, 0.0]),
    ];
    let supports = [
        plane([-9.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        plane([-8.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        plane([0.0, 0.0, -7.0], [0.0, 0.0, 1.0]),
        plane([0.0, 0.0, -6.0], [0.0, 0.0, 1.0]),
    ];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let Err(error) = super::slot_fillet_cylinder(&ctx, caps, &supports) else {
        panic!("one slot midplane exceeds the collection limit");
    };
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo slot fillet midplanes"), "{error:?}");
}

#[test]
fn chamfer_uses_transferred_model_cone_when_row_parameters_are_opaque() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 10,
            kind: crate::surface::SurfaceKind::Cone,
            feature_id: 914,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 10,
        },
        crate::surface::SurfaceRow {
            id: 31,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 3,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 31,
        },
    ]);
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 10,
            body: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
                crate::surface::SurfaceKind::Cone,
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 10,
            body_offset: 11,
        });
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 914,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.extend([
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#10".to_string())
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(
                cadmpeg_ir::geometry::analytic::ConeSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                    0.0,
                    1.0,
                    std::f64::consts::FRAC_PI_4,
                )
                .expect("valid ConeSurface fixture"),
            )),
            source_object: None,
        },
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#31".to_string())
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        },
    ]);

    assert_eq!(
        chamfer_distance_with_service_ctx(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        Some(0.5)
    );

    let duplicate = scan.surfaces.parameters[0].clone();
    scan.surfaces.parameters.push(duplicate);
    assert_eq!(
        chamfer_distance_with_service_ctx(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        None
    );
}

#[test]
fn round_support_radius_reconciles_placed_and_transferred_planes() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });
    scan.planes.positional_frames.extend([
        crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 1,
        },
        crate::surface::OutlinePlane {
            surface_id: 2,
            origin: [0.0, 2.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 2,
        },
        crate::surface::OutlinePlane {
            surface_id: 3,
            origin: [-9.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 3,
        },
        crate::surface::OutlinePlane {
            surface_id: 4,
            origin: [-8.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 4,
        },
    ]);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    assert_eq!(
        super::round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ),
        Some(0.5)
    );

    for (id, x) in [(3, -9.0), (4, -8.0)] {
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    cadmpeg_ir::math::Point3::new(x, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                    cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            source_object: None,
        });
    }
    assert_eq!(
        super::round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ),
        Some(0.5)
    );

    match &mut ir.model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            plane_surface,
        )) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let mut origin = *origin;
            origin.x = -8.5;
            *plane_surface =
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin, *normal, *u_axis)
                    .expect("valid PlaneSurface fixture");
        }
        _ => panic!("transferred support plane"),
    }
    assert_eq!(
        super::round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ),
        None
    );

    match &mut ir.model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            plane_surface,
        )) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            let mut origin = *origin;
            origin.x = -9.0;
            *plane_surface =
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(origin, *normal, *u_axis)
                    .expect("valid PlaneSurface fixture");
        }
        _ => panic!("transferred support plane"),
    }
    scan.features.affected_ids[0].ids.insert(3, 99);
    assert_eq!(
        super::round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ),
        None
    );
    let frame = super::round_support_envelope_cylinder(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        913,
        crate::surface::Type24RoundEnvelope {
            diameter: 2.0,
            extent_endpoints: [[-9.0, 0.0, -3.0], [-8.0, 2.0, -2.5]],
        },
    )
    .expect("resolved support and envelope cylinder");
    assert_eq!(frame.frame().origin(), [-8.5, 0.0, -3.0]);
    assert_eq!(frame.frame().axis(), [0.0, 1.0, 0.0]);
    assert_eq!(frame.frame().ref_direction(), [1.0, 0.0, 0.0]);
    assert_eq!(frame.radius().get(), 0.5);
    assert_eq!(frame.length().map(PositiveLength::get), Some(2.0));
    assert!(super::round_support_envelope_cylinder(
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        913,
        crate::surface::Type24RoundEnvelope {
            diameter: 2.0,
            extent_endpoints: [[-9.0, 0.0, -3.0], [-7.0, 2.0, -2.5]],
        },
    )
    .is_none());
}

#[test]
fn round_support_radius_requires_distinct_parallel_cap_planes() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });
    scan.planes.positional_frames.extend([
        crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 1,
        },
        crate::surface::OutlinePlane {
            surface_id: 2,
            origin: [0.0, 2.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::X_AXIS,
            offset: 2,
        },
        crate::surface::OutlinePlane {
            surface_id: 3,
            origin: [-9.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 3,
        },
        crate::surface::OutlinePlane {
            surface_id: 4,
            origin: [-8.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 4,
        },
    ]);
    let ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        super::round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ),
        Some(0.5)
    );

    scan.planes.positional_frames[2].normal = cadmpeg_ir::units::UnitVector3::Y_AXIS;
    scan.planes.positional_frames[3].normal = cadmpeg_ir::units::UnitVector3::Y_AXIS;
    assert_eq!(
        super::round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ),
        None
    );

    scan.features.affected_ids[0].ids[0] = 3;
    assert_eq!(
        super::round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ),
        None
    );

    scan.features.affected_ids[0].ids = vec![1, 1, 3, 4];
    assert_eq!(
        super::round_support_radius(
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ),
        None
    );
}

#[test]
fn round_placed_cylinder_radius_rejects_duplicate_model_surfaces() {
    let row = crate::surface::SurfaceRow {
        id: 7,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 913,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.extend([
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#7".to_string())
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                        2.0,
                    )
                    .expect("valid CylinderSurface fixture"),
                ),
            ),
            source_object: None,
        },
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint("creo:visibgeom:surface#7".to_string())
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                        3.0,
                    )
                    .expect("valid CylinderSurface fixture"),
                ),
            ),
            source_object: None,
        },
    ]);

    assert_eq!(
        super::round_placed_cylinder_radius(
            &ir,
            &row,
            &crate::decode::source_carriers::SourceUnitCarriers::default()
        ),
        None
    );
}

#[test]
fn round_uses_complete_placed_cylinders_with_cap_and_support_rows() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 1,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 1,
        },
        crate::surface::SurfaceRow {
            id: 2,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 2,
        },
        crate::surface::SurfaceRow {
            id: 3,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 3,
        },
        crate::surface::SurfaceRow {
            id: 4,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: 4,
        },
    ]);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    for id in [3, 4] {
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                        0.5,
                    )
                    .expect("valid CylinderSurface fixture"),
                ),
            ),
            source_object: None,
        });
    }

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        Some(0.5)
    );
}

#[test]
fn round_rejects_conflicting_complete_direct_and_placed_cylinder_radii() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    for id in [3, 4] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: id as usize,
        });
        let token = crate::surface::SurfaceParameterScalar {
            value: Some(0.5),
            raw: vec![0x53, 0, 0, 0, 0, 0, 0],
            offset: 0,
        };
        scan.surfaces
            .parameters
            .push(crate::surface::SurfaceParameterRecord {
                surface_id: id,
                body: vec![0; 7],
                scalar_tokens: vec![token.clone()],
                opaque_spans: Vec::new(),
                scalar_frames: Vec::new(),
                carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
                    crate::surface::SurfaceKind::Cylinder,
                ),
                boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
                offset: id as usize,
                body_offset: id as usize + 1,
            });
    }

    let mut ir = cadmpeg_ir::document::CadIr::empty();
    for (id, radius) in [(3, 0.5), (4, 0.5)] {
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
                .expect("identity grammar"),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(
                    cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                        cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                        cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                        radius,
                    )
                    .expect("valid CylinderSurface fixture"),
                ),
            ),
            source_object: None,
        });
    }

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        Some(0.5)
    );
    if let cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
        cylinder_surface,
    )) = &mut ir.model.surfaces[1].geometry
    {
        let origin = cylinder_surface.origin();
        let axis = cylinder_surface.frame().axis().as_raw();
        let ref_direction = cylinder_surface.frame().reference().as_raw();

        let radius = 0.75;
        *cylinder_surface = cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
            *origin,
            *axis,
            *ref_direction,
            radius,
        )
        .expect("valid CylinderSurface fixture");
    }
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        None
    );
    ir.model.surfaces.pop();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        Some(0.5)
    );
}

#[test]
fn prototype_round_radius_rejects_multiple_associated_torus_prototypes() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.framing.layout = crate::container::Layout::Nd;
    scan.framing.sections.push(
        crate::container::Section::scan("VisibGeom#1".to_string(), 0, 20, None, &[0u8; 20])
            .expect("section extent")
            .section,
    );

    let scalar = |name: &str, value: f64| crate::surface::SurfaceNamedParameter {
        name: name.to_string(),
        value: crate::surface::SurfaceNamedValue::ScalarSequence(vec![value]),
        body: Vec::new(),
        offset: 0,
        value_offset: 0,
    };
    let prototype = |offset| crate::surface::SurfacePrototypeRecord {
        family: crate::surface::SurfacePrototypeFamily::Torus(crate::surface::TorusLabel::Torus),
        parameters: vec![scalar("radius1", 10.0), scalar("radius2", 0.5)],
        offset,
    };
    let row = |id, offset| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::TorusOrSphere,
        feature_id: 913,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset,
    };
    let parameter = |surface_id, offset| {
        let token = crate::surface::SurfaceParameterScalar {
            value: Some(0.5),
            raw: vec![0],
            offset: 0,
        };
        crate::surface::SurfaceParameterRecord {
            surface_id,
            body: vec![0],
            scalar_tokens: vec![token.clone()],
            opaque_spans: Vec::new(),
            scalar_frames: vec![crate::surface::SurfaceParameterScalarFrame {
                offset: 0,
                slots: vec![token],
            }],
            carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
                crate::surface::SurfaceKind::TorusOrSphere,
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset,
            body_offset: offset + 1,
        }
    };

    scan.surfaces.prototype_records.push(prototype(5));
    scan.surfaces.rows.push(row(1, 6));
    scan.surfaces.parameters.push(parameter(1, 6));
    let first_row = &scan.surfaces.rows[0];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::prototype_round_radius(ctx, &scan, &[first_row])
        })
        .expect("prototype round radius"),
        Some(0.5)
    );

    scan.framing.layout = crate::container::Layout::Depdb;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::prototype_round_radius(ctx, &scan, &[first_row])
        })
        .expect("prototype round radius"),
        Some(0.5)
    );

    scan.framing.sections.push(
        crate::container::Section::scan("VisibGeom#2".to_string(), 20, 40, None, &[0u8; 40])
            .expect("section extent")
            .section,
    );
    scan.surfaces.prototype_records.push(prototype(25));
    scan.surfaces.rows.push(row(2, 26));
    scan.surfaces.parameters.push(parameter(2, 26));
    let rows = scan.surfaces.rows.iter().collect::<Vec<_>>();

    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::prototype_round_radius(ctx, &scan, &rows)
        })
        .expect("prototype round radius"),
        None
    );
}

#[test]
fn torus_radius_samples_refuse_collection_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.framing.layout = crate::container::Layout::Nd;
    scan.framing.sections.push(
        crate::container::Section::scan("VisibGeom#1".to_string(), 0, 20, None, &[0u8; 20])
            .expect("section extent")
            .section,
    );
    scan.surfaces
        .prototype_records
        .push(crate::surface::SurfacePrototypeRecord {
            family: crate::surface::SurfacePrototypeFamily::Torus(
                crate::surface::TorusLabel::Torus,
            ),
            parameters: vec![
                crate::surface::SurfaceNamedParameter {
                    name: "radius1".to_string(),
                    value: crate::surface::SurfaceNamedValue::ScalarSequence(vec![10.0]),
                    body: Vec::new(),
                    offset: 0,
                    value_offset: 0,
                },
                crate::surface::SurfaceNamedParameter {
                    name: "radius2".to_string(),
                    value: crate::surface::SurfaceNamedValue::ScalarSequence(vec![0.5]),
                    body: Vec::new(),
                    offset: 0,
                    value_offset: 0,
                },
            ],
            offset: 5,
        });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 1,
        kind: crate::surface::SurfaceKind::TorusOrSphere,
        feature_id: 913,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 6,
    });
    let token = crate::surface::SurfaceParameterScalar {
        value: Some(0.5),
        raw: vec![0],
        offset: 0,
    };
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 1,
            body: vec![0],
            scalar_tokens: vec![token.clone()],
            opaque_spans: Vec::new(),
            scalar_frames: vec![crate::surface::SurfaceParameterScalarFrame {
                offset: 0,
                slots: vec![token],
            }],
            carrier: crate::surface::SurfaceParameterCarrier::Unresolved(
                crate::surface::SurfaceKind::TorusOrSphere,
            ),
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 6,
            body_offset: 7,
        });
    let rows = [&scan.surfaces.rows[0]];
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| {
            super::mixed_torus_radius_samples(ctx, &scan, &rows)
        })
        .expect("torus radius samples"),
        Some(vec![0.5])
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::default();
    policy.limits.max_collection_items = 2;
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[0], &arena, &policy).expect("test decode context");
    let error = super::mixed_torus_radius_samples(&ctx, &scan, &rows)
        .expect_err("one torus sample exceeds the collection limit");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "creo_torus_radius_samples"
    ), "{error:?}");
}

#[test]
fn legacy_round_dimension_supplies_constant_radius() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
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
    let ir = cadmpeg_ir::document::CadIr::empty();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        Some(2.0)
    );
}

#[test]
fn legacy_variable_round_dimension_withholds_radius() {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.features
        .legacy_rounds
        .push(crate::legacy_feature::LegacyRoundFeature {
            feature_id: 913,
            radius: crate::legacy_feature::LegacyRoundRadius::Ambiguous,
            edge_ids: None,
            offset: 0,
        });
    let ir = cadmpeg_ir::document::CadIr::empty();
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| super::round_constant_radius(
            ctx,
            &scan,
            &ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            913
        ))
        .expect("round constant radius"),
        None
    );
}

#[test]
fn numerical_followup_slot_requires_one_tangent_radius() {
    use crate::decode::analytic::equations::PlaneEquation;
    let plane = |origin, normal| PlaneEquation { origin, normal };
    let caps = [
        plane([0., 0., 0.], [0., 0., 1.]),
        plane([0., 0., 1.], [0., 0., 1.]),
    ];
    for radius in [1e-8, 1.0] {
        for ratio in [1., 1.05] {
            let supports = [
                plane([-radius, 0., 0.], [1., 0., 0.]),
                plane([radius, 0., 0.], [1., 0., 0.]),
                plane([0., -ratio * radius, 0.], [0., 1., 0.]),
                plane([0., ratio * radius, 0.], [0., 1., 0.]),
            ];
            let result = crate::decode::with_test_decode_ctx(|ctx| {
                super::slot_fillet_cylinder(ctx, caps, &supports)
            })
            .expect("service profile admits slot midplanes");
            assert_eq!(result.is_some(), ratio == 1.);
        }
    }
}
