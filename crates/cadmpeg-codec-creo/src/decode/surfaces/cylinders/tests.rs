// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::geometry::SurfaceGeometry;

use super::unique_tangent_axial_interval_corner_frame;
use crate::decode::analytic::PlaneEquation;

const EPS_TEST_GEOMETRY: f64 = 1.0e-12;

fn axial_interval_candidate(origin: [f64; 3]) -> crate::surface::PositionalCylinderFrame {
    crate::surface::PositionalCylinderFrame {
        origin,
        axis: [1.0, 0.0, 0.0],
        ref_direction: [0.0, 1.0, 0.0],
        radius: 4.0,
        length: Some(6.0),
    }
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

fn slot_fillet_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 913,
        header: [0, 0],
        root_schema_class: Some(913),
        stream_offset: 0,
        body: Vec::new(),
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
        type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 913,
        reversed: false,
        boundary_type: 0,
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
        id: cadmpeg_ir::ids::SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin: origin.into(),
            normal: normal.into(),
            u_axis: [1.0, 0.0, 0.0].into(),
        },
        source_object: None,
    }
}

fn model_cylinder(id: u32, radius: f64) -> cadmpeg_ir::geometry::Surface {
    cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Cylinder {
            origin: [0.0, 0.0, 0.0].into(),
            axis: [0.0, 0.0, 1.0].into(),
            ref_direction: [1.0, 0.0, 0.0].into(),
            radius,
        },
        source_object: None,
    }
}

fn split_outline_scan() -> crate::container::ContainerScan<'static> {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 1,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 10,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 1,
        },
        crate::surface::SurfaceRow {
            id: 2,
            type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 10,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 2,
        },
        crate::surface::SurfaceRow {
            id: 3,
            type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 10,
            reversed: false,
            boundary_type: 0,
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
            faces: [1, 2],
            next_edges: [11, 11],
            offset: 11,
        },
        crate::curve::CurveTopologyRow {
            id: 12,
            type_byte: 0,
            feature_id: 10,
            directions: [1, 1],
            faces: [1, 3],
            next_edges: [12, 12],
            offset: 12,
        },
    ]);
    let parameter = |surface_id, bounds| crate::surface::SurfaceParameterRecord {
        surface_id,
        body: Vec::new(),
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: None,
        split_cylinder_outline_bounds: Some(bounds),
        positional_cone_frame: None,
        positional_torus_frame: None,
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
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    let transferred = super::transfer_constrained_slot_fillet_cylinders(
        &scan,
        &mut ir,
        &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
    );

    assert_eq!(transferred, 1);
    let [surface] = ir.model.surfaces.as_slice() else {
        panic!("one generated cylinder");
    };
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        radius,
        ..
    } = surface.geometry
    else {
        panic!("generated cylinder: {:?}", surface.geometry);
    };
    assert_eq!(origin, [0.0, 0.0, 0.0].into());
    assert_eq!(axis, [1.0, 0.0, 0.0].into());
    assert_eq!(radius, 1.0);
}

#[test]
fn split_outline_uses_native_plane_carrier_when_model_plane_is_absent() {
    let scan = split_outline_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());

    assert_eq!(
        super::transfer_split_outline_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        ),
        2
    );
    assert!(ir.model.surfaces.iter().all(|surface| {
        matches!(
            surface.geometry,
            SurfaceGeometry::Cylinder {
                radius,
                origin,
                axis,
                ..
            } if radius == 0.3125
                && origin == [0.0, 1.625, -1.0].into()
                && axis == [0.0, 0.0, 1.0].into()
        )
    }));
}

#[test]
fn split_outline_rejects_duplicate_surface_rows() {
    let mut scan = split_outline_scan();
    let duplicate = scan.surfaces.rows[1].clone();
    scan.surfaces.rows.push(duplicate);
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());

    assert_eq!(
        super::transfer_split_outline_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        ),
        0
    );
    assert!(ir.model.surfaces.is_empty());
}

#[test]
fn section_feature_type24_frame_is_not_admitted_as_round_cylinder() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 916,
        header: [0, 0],
        root_schema_class: Some(916),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        type_byte: 0x24,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 916,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 7,
    });
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 7,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: Some(crate::surface::PositionalCylinderFrame {
                origin: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 1.0,
                length: Some(2.0),
            }),
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 7,
            body_offset: 7,
        });
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());

    assert_eq!(
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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
        header: [0, 0],
        root_schema_class: Some(913),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 7,
            type_byte: 0x24,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 7,
        },
        crate::surface::SurfaceRow {
            id: 8,
            type_byte: 0x24,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 8,
        },
    ]);
    let parameter = |surface_id, radius| crate::surface::SurfaceParameterRecord {
        surface_id,
        body: Vec::new(),
        scalar_values: Vec::new(),
        scalar_tokens: Vec::new(),
        opaque_spans: Vec::new(),
        scalar_frames: Vec::new(),
        terminal_scalar_frame: None,
        tabulated_cylinder_frame: None,
        positional_cylinder_frame: Some(crate::surface::PositionalCylinderFrame {
            origin: [0.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            ref_direction: [1.0, 0.0, 0.0],
            radius,
            length: Some(2.0),
        }),
        split_cylinder_outline_bounds: None,
        positional_cone_frame: None,
        positional_torus_frame: None,
        boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
        offset: surface_id as usize,
        body_offset: surface_id as usize,
    };
    scan.surfaces
        .parameters
        .extend([parameter(7, 1.0), parameter(8, 2.0)]);
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());

    assert_eq!(
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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
        header: [0, 0],
        root_schema_class: Some(913),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 7,
        type_byte: 0x24,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 913,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 7,
    });
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 7,
            body: vec![0x0f, 0x12, 0xe3, 0x0f],
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: Some(crate::surface::PositionalCylinderFrame {
                origin: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius: 1.0,
                length: Some(2.0),
            }),
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 7,
            body_offset: 7,
        });
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());

    assert_eq!(
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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

    assert_eq!(frame.origin, [1.2, 0.2, 0.0]);
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert_eq!(frame.ref_direction, [-1.0, 0.0, 0.0]);
    assert_eq!(frame.radius, 0.2);
    assert_eq!(frame.length, Some(5.0));
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
        .origin
        .into_iter()
        .zip([1.2, 0.2, 0.0])
        .all(|(actual, expected)| (actual - expected).abs() < EPS_TEST_GEOMETRY));
    assert_eq!(frame.axis, [0.0, 0.0, 1.0]);
    assert!((frame.radius - 0.2).abs() < EPS_TEST_GEOMETRY);
    assert_eq!(frame.length, Some(5.0));
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
        header: [0, 0],
        root_schema_class: Some(911),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 0,
        offset: 0,
    });
    scan.features
        .definitions
        .push(crate::feature::FeatureDefinition {
            id: 911,
            owner_feature_id: None,
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
                            value: Some(value),
                            value_body: Vec::new(),
                            unresolved_value_token: None,
                            value_unit: crate::feature::DimensionUnit::Millimeters,
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
            type_byte: 0x24,
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 42,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: id as usize,
        }));
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 3,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: Some(crate::surface::PositionalCylinderFrame {
                origin: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                ref_direction: [1.0, 0.0, 0.0],
                radius,
                length: Some(2.0),
            }),
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 3,
            body_offset: 3,
        });
    let entry = |entity_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id: 200,
        source_entity_id: Some(source_entity_id),
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset: entity_id as usize,
        end_offset: entity_id as usize + 1,
    };
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(42),
            table_class_id: 29,
            entry_ids: vec![1, 2, 3, 4],
            entries: vec![entry(1, 100), entry(2, 100), entry(3, 101), entry(4, 101)],
            surface_ids: vec![1, 2, 3, 4],
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        });
    scan
}

#[test]
fn counterbore_positional_radius_gate_rejects_unrelated_frame() {
    let scan = counterbore_dimension_gate_scan(24.5);
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.surfaces.push(model_cylinder(1, 60.0));

    assert_eq!(
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.surfaces.push(model_cylinder(1, 60.0));

    assert_eq!(
        super::transfer_positional_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        )
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
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
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
        ),
        1
    );
}

#[test]
fn constrained_slot_fillet_rejects_conflicting_model_plane_carriers() {
    let scan = slot_fillet_scan();
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model
        .surfaces
        .push(model_plane(3, [0.0, -0.5, 0.0], [0.0, 1.0, 0.0]));

    assert_eq!(
        super::transfer_constrained_slot_fillet_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        ),
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
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.rows.push(crate::feature::FeatureRow {
        feature_id: 23,
        header: [0, 0],
        root_schema_class: Some(913),
        stream_offset: 0,
        body: Vec::new(),
        body_offset: 0,
        offset: 0,
    });
    scan.surfaces.rows = vec![
        row(10, crate::surface::SurfaceKind::Plane),
        row(11, crate::surface::SurfaceKind::Plane),
        row(13, crate::surface::SurfaceKind::Cylinder),
    ];
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(23),
            table_class_id: 80,
            entry_ids: vec![10, 11, 12, 13],
            entries: Vec::new(),
            surface_ids: vec![10, 11, 13],
            non_surface_entity_ids: vec![12],
            offset: 47,
        });
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model
        .surfaces
        .extend([model_cylinder(13, 2.0), model_cylinder(13, 3.0)]);

    assert_eq!(
        super::transfer_rowless_round_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        ),
        0
    );
    assert_eq!(ir.model.surfaces.len(), 2);
}

#[test]
fn rowless_round_cylinder_rejects_duplicate_materialized_source_rows() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        type_byte: kind.canonical_type_byte(),
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(23),
        table_class_id: 80,
        entry_ids: vec![10, 11, 12, 13],
        entries: Vec::new(),
        surface_ids: vec![10, 11, 13],
        non_surface_entity_ids: vec![12],
        offset: 47,
    };
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
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model
        .surfaces
        .push(model_plane(1, [0.0, 0.0, -0.5], [0.0, 0.0, 1.0]));

    assert_eq!(
        super::transfer_split_outline_cylinders(
            &scan,
            &mut ir,
            &mut cadmpeg_ir::annotations::AnnotationBuilder::new(),
        ),
        0
    );
}
