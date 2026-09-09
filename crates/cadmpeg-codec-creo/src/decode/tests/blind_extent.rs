// SPDX-License-Identifier: Apache-2.0

use crate::decode::analytic::equations::PlaneEquation;
use crate::decode::sweep::{
    blind_extrusion_from_carriers, generated_cap_plane_extent, generated_rectilinear_plane_extent,
    ordered_parallel_cap_extent, ExtrusionCarrierSpan,
};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, LinearTermination};
use cadmpeg_ir::geometry::{Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;
use cadmpeg_ir::math::{Point3, Vector3};

#[test]
fn terminal_plane_orients_oppositely_parameterized_extrusion_carriers() {
    let carriers = [
        ExtrusionCarrierSpan {
            starts: vec![[0.0, 5.5, 0.0]],
            vector: [0.0, 2.0, 0.0],
        },
        ExtrusionCarrierSpan {
            starts: vec![[4.0, 7.5, 0.0]],
            vector: [0.0, -2.0, 0.0],
        },
    ];
    let terminal_plane = [([0.0, 7.5, 0.0], [0.0, 1.0, 0.0])];
    assert_eq!(
        blind_extrusion_from_carriers(&carriers, &terminal_plane, None),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(2.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, 1.0, 0.0],
        ))
    );

    let reversed = [
        ExtrusionCarrierSpan {
            starts: vec![[4.0, 7.5, 0.0]],
            vector: [0.0, -2.0, 0.0],
        },
        ExtrusionCarrierSpan {
            starts: vec![[0.0, 5.5, 0.0]],
            vector: [0.0, 2.0, 0.0],
        },
    ];
    assert_eq!(
        blind_extrusion_from_carriers(&reversed, &terminal_plane, None),
        blind_extrusion_from_carriers(&carriers, &terminal_plane, None)
    );
    assert!(blind_extrusion_from_carriers(&carriers, &[], None).is_none());
}

#[test]
fn ordered_parallel_caps_define_blind_direction_and_depth() {
    let start = PlaneEquation {
        origin: [2.0, 7.0, 3.0],
        normal: [0.0, 0.0, -2.0],
    };
    let end = PlaneEquation {
        origin: [-4.0, 11.0, 13.0],
        normal: [0.0, 0.0, 5.0],
    };

    assert_eq!(
        ordered_parallel_cap_extent(start, end),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(10.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, 0.0, 1.0],
        ))
    );
    assert_eq!(
        ordered_parallel_cap_extent(end, start),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(10.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, 0.0, -1.0],
        ))
    );

    let tilted = PlaneEquation {
        origin: end.origin,
        normal: [0.0, 1.0, 1.0],
    };
    assert!(ordered_parallel_cap_extent(start, tilted).is_none());
    assert!(ordered_parallel_cap_extent(start, start).is_none());
}

#[test]
fn generated_table_cap_classes_bind_the_ordered_cap_planes() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        class_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
        is_surface: false,
    };
    let table = crate::feature::FeatureEntityTable {
        feature_id: 7,
        table_class_id: 29,
        entries: vec![
            entry(31, 204, None),
            entry(32, 203, None),
            entry(33, 200, Some(11)),
        ],
        offset: 0,
    }
    .with_surface_ids([31, 32, 33]);
    let row = |id| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed: id == 31,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
    };
    let plane = |id, z| Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                Point3::new(4.0, -2.0, z),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ),
        source_object: None,
    };
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features.entity_tables.push(table.clone());
    scan.surfaces.rows.extend([row(31), row(32), row(33)]);
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([plane(31, 2.0), plane(32, 8.0)]);

    assert_eq!(
        generated_cap_plane_extent(&scan, &ir, 7),
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

    scan.features.entity_tables[0].entries[2].payload =
        crate::feature::EntryPayload::Source { entity: None };
    assert!(generated_cap_plane_extent(&scan, &ir, 7).is_none());
    scan.features.entity_tables[0] = table.clone();
    scan.features.entity_tables.push(table);
    assert!(generated_cap_plane_extent(&scan, &ir, 7).is_none());
}

#[test]
fn rectilinear_generated_planes_define_one_axial_extrusion_family() {
    let row = |id, reversed| crate::surface::SurfaceRow {
        id,
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 7,
        reversed,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: id as usize,
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
    let plane = |id, origin, normal| Surface {
        id: SurfaceId::mint(format!("creo:visibgeom:surface#{id}")).expect("identity grammar"),
        geometry: SurfaceGeometry::Plane(
            cadmpeg_ir::geometry::PlaneSurface::try_new(
                origin,
                normal,
                Vector3::new(0.0, 0.0, 1.0),
            )
            .expect("valid PlaneSurface fixture"),
        ),
        source_object: None,
    };
    let mut ir = CadIr::empty();
    ir.model.surfaces.extend([
        Surface {
            id: SurfaceId::mint("creo:visibgeom:surface#37".to_string()).expect("identity grammar"),
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
    let mut section = crate::feature::FeatureSection3d {
        sketch_plane_entity_id: Some(30),
        sketch_plane_flip: Some(crate::feature::BinaryFlag::Clear),
        reference_planes: crate::feature::definitions::ReferencePlanes::Named(vec![29]),
        reference_plane_datum_geometry_id: None,
        orientation: crate::feature::FeatureSectionOrientation {
            section_flip: Some(crate::feature::BinaryFlag::Set),
            ..Default::default()
        },
        dimension_ids: Vec::new(),
        offset: 0,
    };

    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(42.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, -1.0, 0.0],
        ))
    );
    section.sketch_plane_flip = Some(crate::feature::BinaryFlag::Set);
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(42.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, 1.0, 0.0],
        ))
    );
    section.orientation.section_flip = Some(crate::feature::BinaryFlag::Clear);
    assert_eq!(
        generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)),
        Some((
            ExtrudeExtent::OneSided {
                side: ExtrudeSide {
                    termination: LinearTermination::Blind {
                        length: cadmpeg_ir::scalar::NonZeroLength::new(42.0)
                            .expect("nonzero length fixture"),
                    },
                    draft: None,
                },
            },
            [0.0, -1.0, 0.0],
        ))
    );
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, None).is_none());
    let mut incomplete_section = section.clone();
    incomplete_section.sketch_plane_entity_id = None;
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&incomplete_section)).is_none());

    scan.surfaces.rows[3].reversed = false;
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)).is_none());
    scan.surfaces.rows[3].reversed = true;
    ir.model.surfaces.pop();
    assert!(generated_rectilinear_plane_extent(&scan, &ir, 7, Some(&section)).is_none());
}
