// SPDX-License-Identifier: Apache-2.0

#[test]
fn draft_neutral_plane_rejects_duplicate_materialized_roster_entry() {
    let mut scan = crate::container::scan_bytes(Vec::new());
    scan.features
        .entity_tables
        .push(crate::feature::FeatureEntityTable {
            feature_id: Some(225),
            table_class_id: 29,
            entry_ids: vec![226],
            entries: vec![crate::feature::FeatureEntityTableEntry {
                entity_id: 226,
                class_id: 209,
                source_entity_id: None,
                related_entity_id: None,
                related_entity_state: None,
                prefixed: true,
                offset: 0,
                end_offset: 0,
            }],
            surface_ids: vec![226],
            non_surface_entity_ids: Vec::new(),
            offset: 0,
        });
    scan.surfaces.rows.push(crate::surface::SurfaceRow {
        id: 226,
        type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Plane,
        feature_id: 225,
        reversed: false,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    });

    assert_eq!(
        super::draft_neutral_plane_selection(&scan, 225),
        cadmpeg_ir::features::FaceSelection::Native("creo:visibgeom:surface#226".to_string())
    );

    scan.features.entity_tables[0].surface_ids.push(226);
    assert_eq!(
        super::draft_neutral_plane_selection(&scan, 225),
        cadmpeg_ir::features::FaceSelection::Unresolved
    );
}

#[test]
fn feature_surface_transitions_reject_duplicate_output_roster_entry() {
    let entry = |entity_id, class_id, related_entity_id| crate::feature::FeatureEntityTableEntry {
        entity_id,
        class_id,
        source_entity_id: None,
        related_entity_id,
        related_entity_state: related_entity_id.map(|_| 0),
        prefixed: true,
        offset: entity_id as usize,
        end_offset: entity_id as usize,
    };
    let mut table = crate::feature::FeatureEntityTable {
        feature_id: Some(17),
        table_class_id: 80,
        entry_ids: vec![101, 201],
        entries: vec![entry(101, 214, Some(11)), entry(201, 210, Some(101))],
        surface_ids: vec![201],
        non_surface_entity_ids: vec![101],
        offset: 0,
    };
    let rows = vec![
        crate::surface::SurfaceRow {
            id: 11,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 3,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 11,
        },
        crate::surface::SurfaceRow {
            id: 201,
            type_byte: crate::surface::SurfaceKind::Plane.canonical_type_byte(),
            kind: crate::surface::SurfaceKind::Plane,
            feature_id: 17,
            reversed: false,
            boundary_type: 0,
            next_surface: 0,
            offset: 201,
        },
    ];

    assert_eq!(
        super::feature_surface_transitions(17, std::slice::from_ref(&table), &rows),
        Some(vec![(11, 201)])
    );

    table.surface_ids.push(201);
    assert_eq!(
        super::feature_surface_transitions(17, std::slice::from_ref(&table), &rows),
        None
    );
}
