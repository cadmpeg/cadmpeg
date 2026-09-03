// SPDX-License-Identifier: Apache-2.0
//! Tests: rectilinear sweep extent carrier reconciliation.

use crate::decode::sweep::{
    generated_rectilinear_plane_extent, rectilinear_extent_from_section_plane,
    RectilinearPlaneFamily, RectilinearPlaneStation,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, Length, Termination};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

const STATION_TOLERANCE: f64 = 1e-9;

fn expected_extent() -> (ExtrudeExtent, [f64; 3]) {
    (
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(42.0),
                },
                draft: None,
                offset: None,
            },
        },
        [0.0, -1.0, 0.0],
    )
}

fn section() -> crate::feature::FeatureSection3d {
    crate::feature::FeatureSection3d {
        sketch_plane_entity_id: Some(30),
        sketch_plane_flip: Some(crate::feature::BinaryFlag::Clear),
        reference_plane_entity_ids: vec![29],
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: crate::feature::FeatureSectionOrientation {
            section_flip: Some(crate::feature::BinaryFlag::Set),
            ..Default::default()
        },
        dimension_ids: Vec::new(),
        offset: 0,
    }
}

fn blind(length: f64) -> ExtrudeSide {
    ExtrudeSide {
        termination: Termination::Blind {
            length: Length(length),
        },
        draft: None,
        offset: None,
    }
}

fn rectilinear_family(stations: &[(f64, bool)]) -> RectilinearPlaneFamily {
    RectilinearPlaneFamily {
        normal: [0.0, 1.0, 0.0],
        stations: stations
            .iter()
            .map(|(coordinate, reversed)| RectilinearPlaneStation {
                coordinate: *coordinate,
                reversed: *reversed,
            })
            .collect(),
    }
}

fn generated_fixture(
    axial_stations: &[(u32, f64, bool)],
    section_origin: f64,
) -> (
    crate::container::ContainerScan<'static>,
    CadIr,
    crate::feature::FeatureSection3d,
) {
    let row = |id, reversed| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, origin, normal| Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    let mut ir = CadIr::empty();
    for (id, coordinate, reversed) in axial_stations {
        scan.surfaces.rows.push(row(*id, *reversed));
        ir.model.surfaces.push(plane(
            *id,
            Point3::new(0.0, *coordinate, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ));
    }
    scan.surfaces.rows.extend([row(33, true), row(34, true)]);
    ir.model.surfaces.extend([
        plane(33, Point3::new(-4.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        plane(34, Point3::new(4.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
    ]);
    scan.planes
        .local_systems
        .push(crate::surface::PlaneLocalSystem {
            surface_id: 30,
            body: Vec::new(),
            slots: Vec::new(),
            origin: Some([0.0, section_origin, 0.0]),
            u_axis: Some([1.0, 0.0, 0.0]),
            normal: Some([0.0, 1.0, 0.0]),
            classification: crate::surface::LocalSystemClassification::Simple,
            row_offset: 0,
            offset: 0,
        });
    let section = crate::feature::FeatureSection3d {
        sketch_plane_entity_id: Some(30),
        sketch_plane_flip: Some(crate::feature::BinaryFlag::Clear),
        reference_plane_entity_ids: vec![29],
        reference_plane_rows: Vec::new(),
        reference_plane_datum_geometry_id: None,
        orientation: crate::feature::FeatureSectionOrientation {
            section_flip: Some(crate::feature::BinaryFlag::Clear),
            ..Default::default()
        },
        dimension_ids: Vec::new(),
        offset: 0,
    };
    (scan, ir, section)
}

#[test]
fn rectilinear_section_offsets_select_all_extent_forms() {
    assert_eq!(
        rectilinear_extent_from_section_plane(
            &rectilinear_family(&[(0.0, false), (8.0, true)]),
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            false,
            STATION_TOLERANCE,
        ),
        Some((
            ExtrudeExtent::OneSided { side: blind(8.0) },
            [0.0, 1.0, 0.0],
        ))
    );
    assert_eq!(
        rectilinear_extent_from_section_plane(
            &rectilinear_family(&[(-6.0, false), (8.0, true)]),
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            false,
            STATION_TOLERANCE,
        ),
        Some((
            ExtrudeExtent::TwoSided {
                first: blind(8.0),
                second: blind(6.0),
            },
            [0.0, 1.0, 0.0],
        ))
    );
    assert_eq!(
        rectilinear_extent_from_section_plane(
            &rectilinear_family(&[(-7.0, false), (7.0, true)]),
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            false,
            STATION_TOLERANCE,
        ),
        Some((
            ExtrudeExtent::Symmetric { side: blind(14.0) },
            [0.0, 1.0, 0.0],
        ))
    );
    assert_eq!(
        rectilinear_extent_from_section_plane(
            &rectilinear_family(&[(-6.0, false), (3.0, false), (8.0, true)]),
            [0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            false,
            STATION_TOLERANCE,
        ),
        Some((
            ExtrudeExtent::TwoSided {
                first: blind(8.0),
                second: blind(6.0),
            },
            [0.0, 1.0, 0.0],
        ))
    );
}

#[test]
fn generated_rectilinear_extent_uses_unique_section_origin() {
    let (scan, ir, section) = generated_fixture(&[(31, -6.0, false), (32, 8.0, true)], 0.0);
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)),
        Some((
            ExtrudeExtent::TwoSided {
                first: blind(8.0),
                second: blind(6.0),
            },
            [0.0, 1.0, 0.0],
        ))
    );

    let (scan, ir, section) = generated_fixture(&[(31, -7.0, false), (32, 7.0, true)], 0.0);
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)),
        Some((
            ExtrudeExtent::Symmetric { side: blind(14.0) },
            [0.0, 1.0, 0.0],
        ))
    );
}

#[test]
fn generated_rectilinear_extent_rejects_ambiguous_or_missing_section_flags() {
    let (mut scan, ir, section) = generated_fixture(&[(31, -6.0, false), (32, 8.0, true)], 0.0);
    scan.planes
        .local_systems
        .push(crate::surface::PlaneLocalSystem {
            surface_id: 30,
            body: Vec::new(),
            slots: Vec::new(),
            origin: Some([0.0, 0.0, 0.0]),
            u_axis: Some([1.0, 0.0, 0.0]),
            normal: Some([0.0, 1.0, 0.0]),
            classification: crate::surface::LocalSystemClassification::Simple,
            row_offset: 1,
            offset: 1,
        });
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)).is_none());

    let (scan, ir, mut section) = generated_fixture(&[(31, -6.0, false), (32, 8.0, true)], 0.0);
    section.orientation.section_flip = None;
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)).is_none());
}

#[test]
fn rectilinear_extent_reconciles_native_and_transferred_planes() {
    let row = |id, reversed| crate::surface::SurfaceRow {
        id,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed,
        boundary_type: 0,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, origin, normal| Surface {
        id: SurfaceId(format!("creo:visibgeom:surface#{id}")),
        geometry: SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis: Vector3::new(0.0, 0.0, 1.0),
        },
        source_object: None,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.surfaces.rows.extend([
        row(37, false),
        row(31, false),
        row(32, true),
        row(33, true),
        row(34, true),
        row(36, false),
        row(35, true),
    ]);
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId("creo:visibgeom:surface#37".to_string()),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        },
        plane(31, Point3::new(0.0, 6.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(32, Point3::new(0.0, 48.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(33, Point3::new(4.0, 48.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
        plane(
            34,
            Point3::new(-4.0, 48.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
        ),
        plane(36, Point3::new(0.0, 30.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
        plane(35, Point3::new(0.0, 48.0, 0.0), Vector3::new(0.0, 1.0, 0.0)),
    ]);
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section())),
        Some(expected_extent())
    );

    scan.planes
        .local_systems
        .push(crate::surface::PlaneLocalSystem {
            surface_id: 32,
            body: Vec::new(),
            slots: Vec::new(),
            origin: Some([0.0, 48.0, 0.0]),
            u_axis: Some([0.0, 0.0, 1.0]),
            normal: Some([0.0, 1.0, 0.0]),
            classification: crate::surface::LocalSystemClassification::Simple,
            row_offset: 0,
            offset: 0,
        });
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section())),
        Some(expected_extent())
    );

    let mut local_only = ir.clone();
    local_only
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == SurfaceId("creo:visibgeom:surface#32".to_string()))
        .expect("plane surface")
        .geometry = SurfaceGeometry::Unknown { record: None };
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &local_only, 7, Some(&section())),
        Some(expected_extent())
    );

    scan.planes.local_systems[0].origin = Some([0.0, 49.0, 0.0]);
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section())).is_none());
}
