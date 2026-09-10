// SPDX-License-Identifier: Apache-2.0

use super::{feature_plane_equations, generated_arc_cylinder_extent, generated_cap_plane_extent};
use crate::decode::holes::extrusion_extent_and_direction;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

fn expected_linear_plane_extent() -> (ExtrudeExtent, [f64; 3]) {
    (
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(8.0)
                        .expect("nonzero length fixture"),
                },
                draft: None,
            },
        },
        [0.0, 0.0, 1.0],
    )
}

fn plane_row(id: u32) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 917,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
    }
}

fn plane_surface(id: u32, z: f64) -> Surface {
    Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ),
        source_object: None,
    }
}

fn plane_outline(id: u32, z: f64) -> crate::surface::OutlinePlane {
    crate::surface::OutlinePlane {
        surface_id: id,
        origin: [0.0, 0.0, z],
        normal: [0.0, 0.0, 1.0],
        u_axis: [1.0, 0.0, 0.0],
        offset: id as usize,
    }
}

fn cylinder_surface(id: u32, origin: Point3, axis: Vector3) -> Surface {
    Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::CylinderSurface::try_new(
                origin,
                axis,
                Vector3::new(1.0, 0.0, 0.0),
                0.75,
            )
            .expect("valid CylinderSurface fixture"),
        ),
        source_object: None,
    }
}

#[test]
fn generated_table_cap_classes_use_placed_cap_planes() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.entity_tables.push(
        crate::feature::FeatureEntityTable {
            surface_ids: std::collections::BTreeSet::new(),
            feature_id: 7,
            table_class_id: 29,
            entries: vec![
                entry(31, 204, None),
                entry(32, 203, None),
                entry(33, 200, Some(11)),
            ],
            offset: 0,
        }
        .with_surface_ids([31, 32, 33]),
    );
    for id in [31, 32, 33] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 7,
            reversed: id == 31,
            boundary_type: crate::surface::BoundaryType::Code00,
            next_surface: 0,
            offset: id as usize,
        });
    }
    scan.planes.positional_frames.extend([
        crate::surface::OutlinePlane {
            surface_id: 31,
            origin: [4.0, -2.0, 2.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 31,
        },
        crate::surface::OutlinePlane {
            surface_id: 32,
            origin: [4.0, -2.0, 8.0],
            normal: [0.0, 0.0, 1.0],
            u_axis: [1.0, 0.0, 0.0],
            offset: 32,
        },
    ]);

    assert_eq!(
        generated_cap_plane_extent(&scan, &CadIr::empty(), 7,),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(6.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, 0.0, 1.0],
        ))
    );
}

#[test]
fn feature_plane_extent_reconciles_native_and_transferred_carriers() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([plane_row(31), plane_row(32)]);
    scan.planes
        .outlines
        .extend([plane_outline(31, 2.0), plane_outline(32, 8.0)]);
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend([plane_surface(31, 2.0), plane_surface(32, 8.0)]);

    assert_eq!(
        feature_plane_equations(&scan, &ir, 917).and_then(|planes| {
            extrusion_extent_and_direction([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], planes)
        }),
        Some(expected_linear_plane_extent())
    );

    ir.model.surfaces[1] = plane_surface(32, 9.0);
    assert!(feature_plane_equations(&scan, &ir, 917).is_none());

    ir.model.surfaces[1] = Surface {
        id: SurfaceId::mint("creo:visibgeom:surface#32".to_string()).expect("identity grammar"),
        geometry: SurfaceGeometry::Unknown { record: None },
        source_object: None,
    };
    assert_eq!(
        feature_plane_equations(&scan, &ir, 917).and_then(|planes| {
            extrusion_extent_and_direction([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], planes)
        }),
        Some(expected_linear_plane_extent())
    );
}

#[test]
fn feature_plane_extent_accepts_complete_transferred_carriers_without_local_frames() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([plane_row(31), plane_row(32)]);
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend([plane_surface(31, 2.0), plane_surface(32, 8.0)]);

    assert_eq!(
        feature_plane_equations(&scan, &ir, 917).and_then(|planes| {
            extrusion_extent_and_direction([0.0, 0.0, 0.0], [0.0, 0.0, 1.0], planes)
        }),
        Some(expected_linear_plane_extent())
    );
}

#[test]
fn feature_plane_extent_rejects_ambiguous_or_non_plane_carriers() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([plane_row(31), plane_row(32)]);
    scan.planes.outlines.extend([
        plane_outline(31, 2.0),
        plane_outline(31, 2.0),
        plane_outline(32, 8.0),
    ]);
    let mut ir = CadIr::empty();
    ir.model
        .surfaces
        .extend([plane_surface(31, 2.0), plane_surface(32, 8.0)]);
    assert!(feature_plane_equations(&scan, &ir, 917).is_none());

    scan.planes.outlines.remove(1);
    ir.model.surfaces[0].geometry = SurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            Point3::new(0.0, 0.0, 2.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            1.0,
        )
        .expect("valid CylinderSurface fixture"),
    );
    assert!(feature_plane_equations(&scan, &ir, 917).is_none());
}

#[test]
fn generated_arc_cylinder_extent_reconciles_transferred_carriers() {
    let entry = crate::feature::FeatureEntityTableEntry {
        entity_id: 33,
        payload: crate::feature::entry_payload(200, Some(11), None, None),
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let frame = crate::surface::PositionalCylinderFrame::new(
        [0.0, 4.0, 0.0],
        [0.0, 1.0, 0.0],
        [1.0, 0.0, 0.0],
        0.75,
        Some(34.0),
    )
    .expect("valid positional cylinder frame");
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.entity_tables.push(
        crate::feature::FeatureEntityTable {
            surface_ids: std::collections::BTreeSet::new(),
            feature_id: 7,
            table_class_id: 29,
            entries: vec![entry],
            offset: 0,
        }
        .with_surface_ids([33]),
    );
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 33,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 7,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 33,
    });
    scan.surfaces
        .parameters
        .push(crate::surface::SurfaceParameterRecord {
            surface_id: 33,
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
    let definition = crate::feature::FeatureDefinition {
        identity: crate::feature::definitions::DefinitionIdentity::Parsed {
            schema_id: std::num::NonZeroU32::new(7),
            owner_feature_id: Some(7),
        },
        body: Vec::new(),
        parameter_frames: Vec::new(),
        outlines: Vec::new(),
        variables: None,
        segments: Some(crate::feature::FeatureSegmentTable {
            declared_count: 1,
            has_elided_prototype: false,
            entity_ref: None,
            rows: (vec![crate::feature::FeatureSegment {
                kind: crate::feature::FeatureSegmentKind::Arc([1, 2]),
                directions: [None; 3],
                center_id: Some(3),
                arc_orientation: Some(0),
                vertical_horizontal: None,
                radius_ref: Some(4),
                radius2_ref: None,
                external_id: 11,
                body: Vec::new(),
                offset: 0,
            }])
            .into_iter()
            .map(crate::feature::segment_rows::SegmentRow::Ordinary)
            .collect(),
            offset: 0,
        }),
        trim_entities: None,
        trim_vertices: None,
        order_table: None,
        section_3d: None,
        dimensions: None,
        relations: None,
        saved_section: None,
        offset: 0,
    };
    let transform = crate::placement::FeatureSectionTransform::new(
        7,
        Some(7),
        frame.origin(),
        [1.0, 0.0, 0.0],
        [0.0, 0.0, -1.0],
        0,
    )
    .expect("valid section frame");
    let mut ir = CadIr::empty();
    ir.model.surfaces.push(cylinder_surface(
        33,
        Point3::new(0.0, 4.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ));
    let expected = Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: LinearTermination::Blind {
                    length: cadmpeg_ir::scalar::NonZeroLength::new(34.0)
                        .expect("nonzero length fixture"),
                },
                draft: None,
            },
        },
        [0.0, 1.0, 0.0],
    ));
    assert_eq!(
        generated_arc_cylinder_extent(&scan, &ir, &definition, &transform),
        expected
    );

    ir.model.surfaces.clear();
    assert_eq!(
        generated_arc_cylinder_extent(&scan, &ir, &definition, &transform),
        expected
    );
    ir.model.surfaces.push(cylinder_surface(
        33,
        Point3::new(0.0, 7.0, 0.0),
        Vector3::new(0.0, 1.0, 0.0),
    ));
    assert_eq!(
        generated_arc_cylinder_extent(&scan, &ir, &definition, &transform),
        expected
    );
    ir.model.surfaces[0] =
        cylinder_surface(33, Point3::new(1.0, 4.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
    assert!(generated_arc_cylinder_extent(&scan, &ir, &definition, &transform).is_none());

    ir.model.surfaces[0] =
        cylinder_surface(33, Point3::new(0.0, 4.0, 0.0), Vector3::new(0.0, -1.0, 0.0));
    assert!(generated_arc_cylinder_extent(&scan, &ir, &definition, &transform).is_none());

    ir.model.surfaces[0].geometry = SurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            Point3::new(0.0, 4.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            0.75,
        )
        .expect("valid CylinderSurface fixture"),
    );
    assert!(generated_arc_cylinder_extent(&scan, &ir, &definition, &transform).is_none());

    ir.model.surfaces[0].geometry = SurfaceGeometry::Cylinder(
        cadmpeg_ir::geometry::CylinderSurface::try_new(
            Point3::new(0.0, 4.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            0.8,
        )
        .expect("valid CylinderSurface fixture"),
    );
    assert!(generated_arc_cylinder_extent(&scan, &ir, &definition, &transform).is_none());

    ir.model.surfaces[0].geometry = SurfaceGeometry::Plane(
        cadmpeg_ir::geometry::PlaneSurface::try_new(
            Point3::new(0.0, 4.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .expect("valid PlaneSurface fixture"),
    );
    assert!(generated_arc_cylinder_extent(&scan, &ir, &definition, &transform).is_none());

    ir.model.surfaces[0] =
        cylinder_surface(33, Point3::new(0.0, 4.0, 0.0), Vector3::new(0.0, 1.0, 0.0));
    ir.model.surfaces.push(ir.model.surfaces[0].clone());
    assert!(generated_arc_cylinder_extent(&scan, &ir, &definition, &transform).is_none());
    ir.model.surfaces.pop();

    ir.model.surfaces[0].geometry = SurfaceGeometry::Unknown { record: None };
    assert_eq!(
        generated_arc_cylinder_extent(&scan, &ir, &definition, &transform),
        expected
    );
}
