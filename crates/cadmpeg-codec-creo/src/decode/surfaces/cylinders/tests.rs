// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::SurfaceGeometry;

use super::{unique_support_tangent_cylinder_frame, unique_tangent_axial_interval_corner_frame};
use crate::decode::analytic::equations::PlaneEquation;

const EPS_TEST_GEOMETRY: f64 = 1.0e-12;

fn axial_interval_candidate(origin: [f64; 3]) -> crate::surface::PositionalCylinderFrame {
    crate::surface::PositionalCylinderFrame::new(
        origin,
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        4.0,
        Some(6.0),
    )
    .expect("valid positional cylinder frame")
}

#[test]
fn axial_interval_corner_frame_requires_a_unique_tangent_maximum() {
    let candidates = [
        axial_interval_candidate([10.0, 7.0, 9.0]),
        axial_interval_candidate([10.0, 7.0, 5.0]),
        axial_interval_candidate([10.0, 3.0, 5.0]),
        axial_interval_candidate([10.0, 3.0, 9.0]),
    ];
    let y_support = PlaneEquation {
        origin: [0.0, 3.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let z_support = PlaneEquation {
        origin: [0.0, 0.0, 5.0],
        normal: [0.0, 0.0, 1.0],
    };
    let cap = PlaneEquation {
        origin: [10.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
    };

    assert_eq!(
        unique_tangent_axial_interval_corner_frame(&candidates, &[y_support, z_support, cap]),
        Some(candidates[0])
    );
    assert!(unique_tangent_axial_interval_corner_frame(&candidates, &[y_support]).is_none());
}

#[test]
fn support_tangent_frame_selects_the_uniquely_witnessed_origin_sign() {
    let stored = crate::surface::PositionalCylinderFrame::new(
        [-29.8, 5.25, 6.76],
        [1.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        0.25,
        None,
    )
    .expect("valid positional cylinder frame");
    let tangent = PlaneEquation {
        origin: [0.0, -5.5, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let unrelated_parallel = PlaneEquation {
        origin: [0.0, 6.51, 0.0],
        normal: [0.0, 1.0, 0.0],
    };

    let selected = unique_support_tangent_cylinder_frame(stored, &[tangent, unrelated_parallel])
        .expect("unique tangent origin");
    assert_eq!(selected.origin(), [-29.8, -5.25, 6.76]);
}

#[test]
fn support_tangent_frame_requires_a_matching_axis_aligned_support() {
    let stored = axial_interval_candidate([10.0, 7.0, 9.0]);
    let unmatched = PlaneEquation {
        origin: [0.0, 20.0, 0.0],
        normal: [0.0, 1.0, 0.0],
    };
    let oblique = PlaneEquation {
        origin: [0.0, 3.0, 0.0],
        normal: [0.0, 1.0, 1.0],
    };

    assert!(unique_support_tangent_cylinder_frame(stored, &[unmatched]).is_none());
    assert!(unique_support_tangent_cylinder_frame(stored, &[oblique]).is_none());
}

fn slot_fillet_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 913,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::AffectedIdKind::Geometry,
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
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 1,
        },
        crate::surface::OutlinePlane {
            surface_id: 2,
            origin: [1.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 2,
        },
        crate::surface::OutlinePlane {
            surface_id: 3,
            origin: [0.0, -1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 3,
        },
        crate::surface::OutlinePlane {
            surface_id: 4,
            origin: [0.0, 1.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 4,
        },
        crate::surface::OutlinePlane {
            surface_id: 5,
            origin: [0.0, 0.0, -1.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 5,
        },
        crate::surface::OutlinePlane {
            surface_id: 6,
            origin: [0.0, 0.0, 1.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 6,
        },
    ]);
    scan
}

fn model_plane(id: u32, origin: [f64; 3], normal: [f64; 3]) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
            .expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                origin.into(),
                normal.into(),
                if normal[0].abs() > 0.5 {
                    [0.0, 1.0, 0.0].into()
                } else {
                    [1.0, 0.0, 0.0].into()
                },
            )
            .expect("valid PlaneSurface fixture"),
        ),
        source_object: None,
    }
}

fn model_cylinder(id: u32, radius: f64) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId::mint(format!("creo:visibgeom:surface#{id}"))
            .expect("identity grammar"),
        geometry: SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                [0.0, 0.0, 0.0].into(),
                [0.0, 0.0, 1.0].into(),
                [1.0, 0.0, 0.0].into(),
                radius,
            )
            .expect("valid CylinderSurface fixture"),
        ),
        source_object: None,
    }
}

fn split_outline_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
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
        offset: surface_id as usize,
        body_offset: surface_id as usize,
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
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 1,
        });
    scan
}

#[test]
fn constrained_slot_fillet_uses_native_plane_carriers_when_model_planes_are_absent() {
    let scan = slot_fillet_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    let transferred = super::transfer_constrained_slot_fillet_cylinders(
        &scan,
        &mut ir,
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
    )
    .expect("valid source object identity");

    assert_eq!(transferred, 1);
    let [surface] = ir.model.surfaces.as_slice() else {
        panic!("one generated cylinder");
    };
    let SurfaceGeometry::Cylinder(cylinder_surface) = surface.geometry else {
        panic!("generated cylinder: {:?}", surface.geometry);
    };
    let origin = *cylinder_surface.origin();
    let axis = *cylinder_surface.axis();
    let radius = cylinder_surface.radius();
    assert_eq!(origin, [0.0, 0.0, 0.0].into());
    assert_eq!(axis, [1.0, 0.0, 0.0].into());
    assert_eq!(radius, 1.0);
}

#[test]
fn split_outline_uses_native_plane_carrier_when_model_plane_is_absent() {
    let scan = split_outline_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        super::transfer_split_outline_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity"),
        2
    );
    assert!(ir.model.surfaces.iter().all(|surface| {
        matches!(surface.geometry, SurfaceGeometry::Cylinder(cylinder_surface)
                if {
                    let origin = cylinder_surface.origin();
        let axis = cylinder_surface.axis();
        let radius = cylinder_surface.radius();
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
        super::transfer_split_outline_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity"),
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn section_feature_type24_frame_is_not_admitted_as_round_cylinder() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
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
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity")
        .transferred,
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn unresolved_round_type24_frame_is_not_admitted_as_constant_cylinder() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
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
        offset: surface_id as usize,
        body_offset: surface_id as usize,
    };
    scan.surfaces
        .parameters
        .extend([parameter(7, 1.0), parameter(8, 2.0)]);
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity")
        .transferred,
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn inline_type24_frame_is_admitted_in_a_round_feature() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
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
    let mut ir = cadmpeg_ir::document::CadIr::empty();

    assert_eq!(
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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

#[test]
fn positional_frame_reconciles_an_existing_model_cylinder() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
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
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity")
        .transferred,
        0
    );
    let [surface] = ir.model.surfaces.as_slice() else {
        panic!("one reconciled cylinder");
    };
    let SurfaceGeometry::Cylinder(cylinder_surface) = surface.geometry else {
        panic!("reconciled cylinder: {:?}", surface.geometry);
    };
    let origin = *cylinder_surface.origin();
    let axis = *cylinder_surface.axis();
    let ref_direction = *cylinder_surface.ref_direction();
    let radius = cylinder_surface.radius();
    assert_eq!(origin, [-12.5, 4.0, 0.0].into());
    assert_eq!(axis, [0.0, 1.0, 0.0].into());
    assert_eq!(ref_direction, [1.0, 0.0, 0.0].into());
    assert_eq!(radius, 0.75);
}

#[test]
fn round_edge_support_frame_selects_one_offset_line() {
    let frame = super::round_edge_cylinder_frame(
        crate::surface::Type24RoundEdgeEnvelope {
            parameter_interval: [0.25, 5.25],
            vertices: [[1.0, 0.2, 3.0], [1.2, 0.0, 8.0]],
            generated_entity_reference: None,
        },
        0.2,
        &[
            PlaneEquation {
                origin: [1.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
        ],
    )
    .expect("one offset round-edge cylinder");

    assert_eq!(frame.origin(), [1.2, 0.2, 0.0]);
    assert_eq!(frame.axis(), [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction(), [-1.0, 0.0, 0.0]);
    assert_eq!(frame.radius(), 0.2);
    assert_eq!(frame.length(), Some(5.0));
}

#[test]
fn perpendicular_round_edge_supports_solve_their_radius() {
    let frame = super::perpendicular_round_edge_cylinder_frame(
        crate::surface::Type24RoundEdgeEnvelope {
            parameter_interval: [0.25, 5.25],
            vertices: [[1.0, 0.2, 3.0], [1.2, 0.0, 8.0]],
            generated_entity_reference: None,
        },
        &[
            PlaneEquation {
                origin: [1.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [0.0, 0.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
        ],
    )
    .expect("one endpoint-solved perpendicular round cylinder");

    assert!(frame
        .origin()
        .into_iter()
        .zip([1.2, 0.2, 0.0])
        .all(|(actual, expected)| (actual - expected).abs() < EPS_TEST_GEOMETRY));
    assert_eq!(frame.axis(), [0.0, 0.0, 1.0]);
    assert!((frame.radius() - 0.2).abs() < EPS_TEST_GEOMETRY);
    assert_eq!(frame.length(), Some(5.0));
}

#[test]
fn round_edge_support_frame_rejects_parallel_supports() {
    assert!(super::round_edge_cylinder_frame(
        crate::surface::Type24RoundEdgeEnvelope {
            parameter_interval: [0.0, 1.0],
            vertices: [[1.0, 0.2, 0.0], [1.0, 0.0, 1.0]],
            generated_entity_reference: Some(17),
        },
        0.2,
        &[
            PlaneEquation {
                origin: [1.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            PlaneEquation {
                origin: [2.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
        ],
    )
    .is_none());
}

fn counterbore_dimension_gate_scan(radius: f64) -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 42,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Hole),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
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
            dimensions: Some(crate::feature::FeatureDimensionTable {
                declared_count: 4,
                entity_ref: Some(88),
                rows: vec![(2, 20.0, 0), (2, 1.0, 1), (1, 8.0, 2), (2, 60.0, 3)]
                    .into_iter()
                    .map(
                        |(dimension_type, value, external_id)| crate::feature::FeatureDimension {
                            dimension_type,
                            value: crate::feature::definitions::DimensionValue::Resolved(value),
                            value_body: Vec::new(),
                            direction_byte: 0,
                            auxiliary_value: Some(0.0),
                            auxiliary_body: Vec::new(),
                            external_id,
                            references: None,
                            offset: 0,
                        },
                    )
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
            offset: id as usize,
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
    let entry = |entity_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        payload: crate::feature::entry_payload(200, Some(source_entity_id), None, None),
        prefixed: false,
        offset: entity_id as usize,
        end_offset: entity_id as usize + 1,
    };
    scan.features.entity_tables.push(
        crate::feature::FeatureEntityTable {
            surface_ids: std::collections::BTreeSet::new(),
            feature_id: 42,
            table_class_id: 29,
            entries: vec![entry(1, 100), entry(2, 100), entry(3, 101), entry(4, 101)],
            offset: 0,
        }
        .with_surface_ids([1, 2, 3, 4]),
    );
    scan
}

#[test]
fn counterbore_positional_radius_gate_rejects_unrelated_frame() {
    let scan = counterbore_dimension_gate_scan(24.5);
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model.surfaces.push(model_cylinder(1, 60.0));

    assert_eq!(
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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
        super::transfer_constrained_slot_fillet_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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
        super::transfer_constrained_slot_fillet_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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
fn rowless_round_cylinder_rejects_duplicate_sibling_model_surfaces() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 23,
        root_schema_class: Some(crate::feature::schema::SchemaClass::Round),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows = vec![
        row(10, crate::surface::SurfaceKind::Plane),
        row(11, crate::surface::SurfaceKind::Plane),
        row(13, crate::surface::SurfaceKind::Cylinder),
    ];
    scan.features.entity_tables.push(
        crate::feature::FeatureEntityTable {
            surface_ids: std::collections::BTreeSet::new(),
            feature_id: 23,
            table_class_id: 80,
            entries: vec![
                crate::feature::dummy_table_entry(10),
                crate::feature::dummy_table_entry(11),
                crate::feature::dummy_table_entry(12),
                crate::feature::dummy_table_entry(13),
            ],
            offset: 47,
        }
        .with_surface_ids([10, 11, 13]),
    );
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .extend([model_cylinder(13, 2.0), model_cylinder(13, 3.0)]);

    assert_eq!(
        super::transfer_rowless_round_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity"),
        0
    );
    assert_eq!(ir.model.surfaces.len(), 2);
}

#[test]
fn rowless_round_cylinder_rejects_duplicate_materialized_source_rows() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let table = crate::feature::FeatureEntityTable {
        surface_ids: std::collections::BTreeSet::new(),
        feature_id: 23,
        table_class_id: 80,
        entries: vec![
            crate::feature::dummy_table_entry(10),
            crate::feature::dummy_table_entry(11),
            crate::feature::dummy_table_entry(12),
            crate::feature::dummy_table_entry(13),
        ],
        offset: 47,
    }
    .with_surface_ids([10, 11, 13]);
    let rows = vec![
        row(10, crate::surface::SurfaceKind::Plane),
        row(11, crate::surface::SurfaceKind::Plane),
        row(13, crate::surface::SurfaceKind::Cylinder),
        row(13, crate::surface::SurfaceKind::Cylinder),
    ];

    assert!(super::rowless_round_cylinder_pairs(
        &std::collections::BTreeSet::from([23]),
        &[table],
        &rows,
    )
    .is_empty());
}

#[test]
fn round_envelope_rejects_an_extra_reference_circle() {
    let circle = |entity_id, axis, start, end| crate::reference::ReferenceCircle {
        entity_id,
        center: [0.0; 3],
        center_stored: true,
        radius: 2.0,
        axis,
        start,
        end,
        offset: 0,
    };
    let envelope = crate::surface::Type24RoundEnvelope {
        diameter: 2.0,
        extent_endpoints: [[3.5, 8.0, -6.0], [5.5, 10.0, -4.0]],
    };
    let first = circle(367, [0.0, 0.0, 1.0], [3.5, 8.0, -6.0], [5.5, 10.0, -6.0]);
    let second = circle(368, [0.0, 0.0, -1.0], [5.5, 10.0, -4.0], [3.5, 8.0, -4.0]);
    let duplicate_first = circle(369, [0.0, 0.0, 1.0], [3.5, 8.0, -6.0], [5.5, 10.0, -6.0]);

    assert!(
        super::reference_cap_bound_round_frame(envelope, &[&first, &second, &duplicate_first],)
            .is_none()
    );
}

#[test]
fn split_outline_rejects_conflicting_model_plane_carrier() {
    let scan = split_outline_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty();
    ir.model
        .surfaces
        .push(model_plane(1, [0.0, 0.0, -0.5], [0.0, 0.0, 1.0]));

    assert_eq!(
        super::transfer_split_outline_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
        .expect("valid source object identity"),
        0
    );
}
