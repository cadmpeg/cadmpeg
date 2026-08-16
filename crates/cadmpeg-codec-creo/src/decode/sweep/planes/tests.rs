// SPDX-License-Identifier: Apache-2.0

use super::generated_cap_plane_extent;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, Length, Termination};
use cadmpeg_ir::units::Units;

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
