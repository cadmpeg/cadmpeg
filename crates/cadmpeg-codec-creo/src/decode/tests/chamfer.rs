// SPDX-License-Identifier: Apache-2.0

use crate::decode::analytic::equations::{ConeEquation, PlaneEquation};
use crate::decode::feature_history::{chamfer_constant_distance, equal_distance_chamfer_setback};
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

#[test]
fn chamfer_requires_every_affected_support_plane_to_be_placed() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    let empty_ir = CadIr::empty();
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
            terminal_scalar_frame: None,
            carrier: crate::surface::SurfaceParameterCarrier::Resolved(
                crate::surface::InlineSurfaceCarrier::Cone(
                    crate::surface::PositionalConeFrame::new(
                        [0.5, 0.0, 0.0],
                        [-1.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0],
                        std::f64::consts::FRAC_PI_4,
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
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 31,
        });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 914,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    assert_eq!(chamfer_constant_distance(&scan, &empty_ir, 914), Some(0.5));
    scan.features.affected_ids[0].ids.extend([98, 99]);
    assert_eq!(chamfer_constant_distance(&scan, &empty_ir, 914), Some(0.5));

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
    assert_eq!(chamfer_constant_distance(&scan, &empty_ir, 914), None);
}
