// SPDX-License-Identifier: Apache-2.0

use crate::decode::analytic::equations::{ConeEquation, PlaneEquation};
use crate::decode::feature_history::round::{
    chamfer_constant_distance, equal_distance_chamfer_setback,
};
use cadmpeg_ir::document::CadIr;

#[test]
fn equal_distance_chamfer_setback_uses_nearest_forward_parallel_support() {
    let cone = |origin, axis| {
        ConeEquation::new(
            origin,
            axis,
            [0.0, 0.0, 1.0],
            0.0,
            1.0,
            std::f64::consts::FRAC_PI_4,
        )
        .expect("valid test cone")
    };
    let cones = [
        cone([10.5, 0.0, 0.0], [-1.0, 0.0, 0.0]),
        cone([-10.5, 0.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    let supports = [
        PlaneEquation {
            origin: [10.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
        PlaneEquation {
            origin: [-10.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
        },
        PlaneEquation {
            origin: [0.0, 2.0, 0.0],
            normal: [0.0, 1.0, 0.0],
        },
    ];

    assert_eq!(equal_distance_chamfer_setback(&cones, &supports), Some(0.5));

    let mut non_equal = cones;
    let mut origin = non_equal[1].origin();
    origin[0] = -10.25;
    non_equal[1] = ConeEquation::new(
        origin,
        non_equal[1].axis(),
        non_equal[1].ref_direction(),
        non_equal[1].radius(),
        non_equal[1].ratio(),
        non_equal[1].half_angle(),
    )
    .expect("valid test cone");
    assert_eq!(equal_distance_chamfer_setback(&non_equal, &supports), None);
}

fn chamfer_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes_ok(Vec::new());
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 10,
        kind: crate::surface::SurfaceKind::Cone,
        feature_id: 914,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 10,
    });
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
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 31,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 31,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 98,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 3,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 98,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 31,
            origin: [0.0, 0.0, 0.0],
            normal: cadmpeg_ir::units::UnitVector3::X_AXIS,
            u_axis: cadmpeg_ir::units::UnitVector3::Y_AXIS,
            offset: 31,
        });
    scan.features
        .affected_ids
        .push(crate::feature::rows::FeatureAffectedIds {
            feature_id: 914,
            kind: crate::feature::rows::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });
    scan
}

#[test]
fn chamfer_requires_every_affected_support_plane_to_be_placed() {
    let mut scan = chamfer_scan();
    let empty_ir = CadIr::empty();

    assert_eq!(
        super::chamfer_distance_with_service_ctx(
            &scan,
            &empty_ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        Some(0.5)
    );
    scan.features.affected_ids[0].ids.extend([98, 99]);
    assert_eq!(
        super::chamfer_distance_with_service_ctx(
            &scan,
            &empty_ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        Some(0.5)
    );

    scan.features.affected_ids[0].ids.push(32);
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 32,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 32,
    });
    assert_eq!(
        super::chamfer_distance_with_service_ctx(
            &scan,
            &empty_ir,
            &crate::decode::source_carriers::SourceUnitCarriers::default(),
            914
        ),
        None
    );
}

fn chamfer_limit_error(limit: u64, operation: &'static str) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = chamfer_scan();
    let ir = CadIr::empty();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = limit;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = chamfer_constant_distance(
        &ctx,
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        914,
    )
    .expect_err("chamfer witnesses exceed the collection limit");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == operation), "{error:?}");
}

#[test]
fn chamfer_cone_witnesses_refuse_collection_limit() {
    chamfer_limit_error(0, "creo chamfer cone witnesses");
}

#[test]
fn chamfer_support_plane_ids_refuse_collection_limit() {
    chamfer_limit_error(1, "creo chamfer support plane IDs");
}

#[test]
fn chamfer_support_planes_refuse_collection_limit() {
    chamfer_limit_error(2, "creo chamfer support planes");
}

#[test]
fn chamfer_feature_definition_propagates_cone_limit() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let scan = chamfer_scan();
    let ir = CadIr::empty();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root is admitted");
    let error = crate::decode::feature_history::draft::schema_feature_definition(
        &ctx,
        &scan,
        &ir,
        &crate::decode::source_carriers::SourceUnitCarriers::default(),
        914,
        Some(crate::feature::schema::SchemaClass::Chamfer),
        "Chamfer",
    )
    .expect_err("chamfer feature keeps the resource refusal");
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(resource)
        if resource.operation == "creo chamfer cone witnesses"), "{error:?}");
}
