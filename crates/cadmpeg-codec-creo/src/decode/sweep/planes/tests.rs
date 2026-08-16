// SPDX-License-Identifier: Apache-2.0

use super::{feature_plane_equations, generated_cap_plane_extent};
use crate::decode::holes::extrusion_extent_and_direction;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, Length, Termination};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::Units;

fn expected_linear_plane_extent() -> (ExtrudeExtent, [f64; 3]) {
    (
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(8.0),
                },
                draft: None,
                offset: None,
            },
        },
        [0.0, 0.0, 1.0],
    )
}

fn plane_row(id: u32) -> crate::surface::SurfaceRow {
    crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 917,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    }
}

fn plane_surface(id: u32, z: f64) -> Surface {
    Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, z),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        },
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

#[test]
fn generated_table_cap_classes_use_placed_cap_planes() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id,
        related_entity_id: None,
        related_entity_state: None,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(7),
            table_class_id: 29,
            entry_ids: vec![31, 32, 33],
            entries: vec![
                entry(31, 204, None),
                entry(32, 203, None),
                entry(33, 200, Some(11)),
            ],
            surface_ids: vec![31, 32, 33],
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        });
    for id in [31, 32, 33] {
        scan.surfaces.rows.push(crate::surface::SurfaceRow {
            id,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 7,
            reversed: id == 31,
            boundary_type: 0,
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
        generated_cap_plane_extent(&scan, &CadIr::empty(Units::default()), 7,),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: Termination::Blind {
                        length: Length(6.0),
                    },
                    draft: None,
                    offset: None,
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
    let mut ir = CadIr::empty(Units::default());
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
        id: SurfaceId("creo:visibgeom:surface#32".to_string()),
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
    let mut ir = CadIr::empty(Units::default());
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
    let mut ir = CadIr::empty(Units::default());
    ir.model
        .surfaces
        .extend([plane_surface(31, 2.0), plane_surface(32, 8.0)]);
    assert!(feature_plane_equations(&scan, &ir, 917).is_none());

    scan.planes.outlines.remove(1);
    ir.model.surfaces[0].geometry = SurfaceGeometry::Cylinder {
        origin: Point3::new(0.0, 0.0, 2.0),
        axis: Vector3::new(0.0, 0.0, 1.0),
        ref_direction: Vector3::new(1.0, 0.0, 0.0),
        radius: 1.0,
    };
    assert!(feature_plane_equations(&scan, &ir, 917).is_none());
}
