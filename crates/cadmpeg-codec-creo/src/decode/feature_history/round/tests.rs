// SPDX-License-Identifier: Apache-2.0

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

    let mut scan = crate::container::scan_bytes(crate::test_support::build_prt(
        "cone-template",
        &[("VisibGeom", payload)],
    ));
    let [prototype] = scan.surfaces.prototype_records.as_slice() else {
        panic!("complete cone prototype");
    };
    assert_eq!(
        crate::decode::surfaces::unique_surface_prototype_associations(&scan).len(),
        1
    );
    let frame = crate::surface::prototype_cone_frame(prototype).expect("prototype frame");
    assert!(scan
        .surfaces
        .parameters
        .iter()
        .find(|record| record.surface_id == 7)
        .is_some_and(|record| record.positional_cone_frame.is_none()));

    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 31,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 3,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 31,
    });
    scan.planes
        .positional_frames
        .push(crate::surface::OutlinePlane {
            surface_id: 31,
            origin: std::array::from_fn(|index| frame.apex[index] + frame.axis[index]),
            normal: frame.axis,
            u_axis: frame.ref_direction,
            offset: 31,
        });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 4,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    assert_eq!(
        super::chamfer_constant_distance(
            &scan,
            &cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default()),
            4
        ),
        None
    );
}

#[test]
fn chamfer_uses_transferred_model_plane_carrier() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 10,
            type_byte: crate::surface::SurfaceKind::Cone.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Cone,
            feature_id: 914,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 10,
        },
        crate::surface::SurfaceRow {
            id: 31,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 3,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 31,
        },
    ]);
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 10,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: None,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: Some(crate::surface::PositionalConeFrame {
                apex: [0.5, 0.0, 0.0],
                axis: [-1.0, 0.0, 0.0],
                ref_direction: [0.0, 1.0, 0.0],
                half_angle: std::f64::consts::FRAC_PI_4,
            }),
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 10,
            body_offset: 11,
        });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 914,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
        id: cadmpeg_ir::ids::SurfaceId("creo:visibgeom:surface#31".to_string()),
        geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
            origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
            u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
        },
        source_object: None,
    });

    assert_eq!(super::chamfer_constant_distance(&scan, &ir, 914), Some(0.5));

    let transferred_plane_row = scan.surfaces.rows.pop().expect("support plane row");
    assert_eq!(super::chamfer_constant_distance(&scan, &ir, 914), Some(0.5));
    scan.surfaces.rows.push(transferred_plane_row);

    scan.planes.outlines.push(crate::surface::OutlinePlane {
        surface_id: 31,
        origin: [0.0, 0.0, 0.0],
        normal: [1.0, 0.0, 0.0],
        u_axis: [0.0, 1.0, 0.0],
        offset: 31,
    });
    assert_eq!(super::chamfer_constant_distance(&scan, &ir, 914), Some(0.5));

    let mut conflicting_ir = ir.clone();
    match &mut conflicting_ir.model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, .. } => origin.x = 0.25,
        _ => panic!("transferred plane geometry"),
    }
    assert_eq!(
        super::chamfer_constant_distance(&scan, &conflicting_ir, 914),
        None
    );
}

#[test]
fn slot_fillet_cylinder_skips_parallel_midplane_candidates() {
    let cylinder = super::slot_fillet_cylinder(
        [
            crate::decode::analytic::PlaneEquation {
                origin: [0.0, -2.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
            crate::decode::analytic::PlaneEquation {
                origin: [0.0, 3.0, 0.0],
                normal: [0.0, 1.0, 0.0],
            },
        ],
        &[
            crate::decode::analytic::PlaneEquation {
                origin: [-9.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            crate::decode::analytic::PlaneEquation {
                origin: [-8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            crate::decode::analytic::PlaneEquation {
                origin: [-9.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            crate::decode::analytic::PlaneEquation {
                origin: [-8.0, 0.0, 0.0],
                normal: [1.0, 0.0, 0.0],
            },
            crate::decode::analytic::PlaneEquation {
                origin: [0.0, 0.0, -7.0],
                normal: [0.0, 0.0, 1.0],
            },
            crate::decode::analytic::PlaneEquation {
                origin: [0.0, 0.0, -6.0],
                normal: [0.0, 0.0, 1.0],
            },
        ],
    )
    .expect("later independent support pair");

    assert_eq!(cylinder.origin, [-8.5, -2.0, -6.5]);
    assert_eq!(cylinder.radius, 0.5);
}

#[test]
fn chamfer_uses_transferred_model_cone_when_row_parameters_are_opaque() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 10,
            type_byte: crate::surface::SurfaceKind::Cone.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Cone,
            feature_id: 914,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 10,
        },
        crate::surface::SurfaceRow {
            id: 31,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 3,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 31,
        },
    ]);
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 10,
            body: Vec::new(),
            scalar_values: Vec::new(),
            scalar_tokens: Vec::new(),
            opaque_spans: Vec::new(),
            scalar_frames: Vec::new(),
            terminal_scalar_frame: None,
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: None,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
            boundary: crate::surface::SurfaceBodyBoundary::CompoundClose,
            offset: 10,
            body_offset: 11,
        });
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 914,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![31],
            offset: 0,
        });

    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.surfaces.extend([
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("creo:visibgeom:surface#10".to_string()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Cone {
                origin: cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(-1.0, 0.0, 0.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
                radius: 0.0,
                ratio: 1.0,
                half_angle: std::f64::consts::FRAC_PI_4,
            },
            source_object: None,
        },
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("creo:visibgeom:surface#31".to_string()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        },
    ]);

    assert_eq!(super::chamfer_constant_distance(&scan, &ir, 914), Some(0.5));

    let duplicate = scan.surfaces.parameters[0].clone();
    scan.surfaces.parameters.push(duplicate);
    assert_eq!(super::chamfer_constant_distance(&scan, &ir, 914), None);
}

#[test]
fn round_support_radius_reconciles_placed_and_transferred_planes() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });
    scan.planes.positional_frames.extend([
        crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 1,
        },
        crate::surface::OutlinePlane {
            surface_id: 2,
            origin: [0.0, 2.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 2,
        },
        crate::surface::OutlinePlane {
            surface_id: 3,
            origin: [-9.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 3,
        },
        crate::surface::OutlinePlane {
            surface_id: 4,
            origin: [-8.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 4,
        },
    ]);
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    assert_eq!(super::round_support_radius(&scan, &ir, 913), Some(0.5));

    for (id, x) in [(3, -9.0), (4, -8.0)] {
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Plane {
                origin: cadmpeg_ir::math::Point3::new(x, 0.0, 0.0),
                normal: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                u_axis: cadmpeg_ir::math::Vector3::new(0.0, 1.0, 0.0),
            },
            source_object: None,
        });
    }
    assert_eq!(super::round_support_radius(&scan, &ir, 913), Some(0.5));

    match &mut ir.model.surfaces[0].geometry {
        cadmpeg_ir::geometry::SurfaceGeometry::Plane { origin, .. } => origin.x = -8.5,
        _ => panic!("transferred support plane"),
    }
    assert_eq!(super::round_support_radius(&scan, &ir, 913), None);
}

#[test]
fn round_support_radius_requires_distinct_parallel_cap_planes() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .affected_ids
        .push(crate::feature::FeatureAffectedIds {
            feature_id: 913,
            kind: crate::feature::AffectedIdKind::Geometry,
            ids: vec![1, 2, 3, 4],
            offset: 0,
        });
    scan.planes.positional_frames.extend([
        crate::surface::OutlinePlane {
            surface_id: 1,
            origin: [0.0, 0.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 1,
        },
        crate::surface::OutlinePlane {
            surface_id: 2,
            origin: [0.0, 2.0, 0.0],
            normal: [0.0, 1.0, 0.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 2,
        },
        crate::surface::OutlinePlane {
            surface_id: 3,
            origin: [-9.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 3,
        },
        crate::surface::OutlinePlane {
            surface_id: 4,
            origin: [-8.0, 0.0, 0.0],
            normal: [1.0, 0.0, 0.0],
            u_axis: [0.0, 1.0, 0.0],
            offset: 4,
        },
    ]);
    let ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());

    assert_eq!(super::round_support_radius(&scan, &ir, 913), Some(0.5));

    scan.planes.positional_frames[2].normal = [0.0, 1.0, 0.0];
    scan.planes.positional_frames[3].normal = [0.0, 1.0, 0.0];
    assert_eq!(super::round_support_radius(&scan, &ir, 913), None);

    scan.features.affected_ids[0].ids[0] = 3;
    assert_eq!(super::round_support_radius(&scan, &ir, 913), None);

    scan.features.affected_ids[0].ids = vec![1, 1, 3, 4];
    assert_eq!(super::round_support_radius(&scan, &ir, 913), None);
}

#[test]
fn round_placed_cylinder_radius_rejects_duplicate_model_surfaces() {
    let row = crate::surface::SurfaceRow {
        id: 7,
        type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 913,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    ir.model.surfaces.extend([
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("creo:visibgeom:surface#7".to_string()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                radius: 2.0,
            },
            source_object: None,
        },
        cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId("creo:visibgeom:surface#7".to_string()),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                radius: 3.0,
            },
            source_object: None,
        },
    ]);

    assert_eq!(super::round_placed_cylinder_radius(&ir, &row), None);
}

#[test]
fn round_uses_complete_placed_cylinders_with_cap_and_support_rows() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        crate::surface::SurfaceRow {
            id: 1,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 913,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 1,
        },
        crate::surface::SurfaceRow {
            id: 2,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 913,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 2,
        },
        crate::surface::SurfaceRow {
            id: 3,
            type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 3,
        },
        crate::surface::SurfaceRow {
            id: 4,
            type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Cylinder,
            feature_id: 913,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 4,
        },
    ]);
    let mut ir = cadmpeg_ir::document::CadIr::empty(cadmpeg_ir::units::Units::default());
    for id in [3, 4] {
        ir.model.surfaces.push(cadmpeg_ir::geometry::Surface {
            id: cadmpeg_ir::ids::SurfaceId(format!("creo:visibgeom:surface#{id}")),
            geometry: cadmpeg_ir::geometry::SurfaceGeometry::Cylinder {
                origin: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
                ref_direction: cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                radius: 0.5,
            },
            source_object: None,
        });
    }

    assert_eq!(super::round_constant_radius(&scan, &ir, 913), Some(0.5));
}

#[test]
fn prototype_round_radius_rejects_multiple_associated_torus_prototypes() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.framing.layout = crate::container::Layout::Nd;
    scan.framing.sections.push(crate::container::Section {
        name: "first".to_string(),
        raw_name: "first".to_string(),
        offset: 0,
        length: 20,
        expanded_length: None,
        role: crate::container::role::GEOMETRY,
    });

    let scalar = |name: &str, value: f64| crate::surface::SurfaceNamedParameter {
        name: name.to_string(),
        value: crate::surface::SurfaceNamedValue::ScalarSequence(vec![value]),
        body: Vec::new(),
        offset: 0,
        value_offset: 0,
    };
    let prototype = |offset| crate::surface::SurfacePrototypeRecord {
        declared_family: "torus".to_string(),
        family: crate::surface::SurfacePrototypeFamily::Torus,
        parameters: vec![scalar("radius1", 10.0), scalar("radius2", 0.5)],
        offset,
    };
    let row = |id, offset| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::TorusOrSphere.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::TorusOrSphere,
        feature_id: 913,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset,
    };
    let parameter = |surface_id, offset| {
        let token = crate::surface::SurfaceParameterScalar {
            value: Some(0.5),
            raw: vec![0],
            offset: 0,
            length: 1,
        };
        crate::surface::SurfaceParameterRecord {
            surface_id,
            body: vec![0],
            scalar_values: vec![0.5],
            scalar_tokens: vec![token.clone()],
            opaque_spans: Vec::new(),
            scalar_frames: vec![crate::surface::SurfaceParameterScalarFrame {
                offset: 0,
                slots: vec![token.clone()],
            }],
            terminal_scalar_frame: Some(crate::surface::SurfaceParameterScalarFrame {
                offset: 0,
                slots: vec![token],
            }),
            tabulated_cylinder_frame: None,
            positional_cylinder_frame: None,
            split_cylinder_outline_bounds: None,
            positional_cone_frame: None,
            positional_torus_frame: None,
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
        super::prototype_round_radius(&scan, &[first_row]),
        Some(0.5)
    );

    scan.framing.layout = crate::container::Layout::Depdb;
    assert_eq!(
        super::prototype_round_radius(&scan, &[first_row]),
        Some(0.5)
    );

    scan.framing.sections.push(crate::container::Section {
        name: "second".to_string(),
        raw_name: "second".to_string(),
        offset: 20,
        length: 20,
        expanded_length: None,
        role: crate::container::role::GEOMETRY,
    });
    scan.surfaces.prototype_records.push(prototype(25));
    scan.surfaces.rows.push(row(2, 26));
    scan.surfaces.parameters.push(parameter(2, 26));
    let rows = scan.surfaces.rows.iter().collect::<Vec<_>>();

    assert_eq!(super::prototype_round_radius(&scan, &rows), None);
}
