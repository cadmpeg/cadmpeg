// SPDX-License-Identifier: Apache-2.0

#[test]
fn compact_simple_hole_rejects_duplicate_materialized_roster_id() {
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
    let table = crate::feature::FeatureEntityTable {
        feature_id: Some(107),
        table_class_id: 29,
        entry_ids: vec![109, 112, 115, 117],
        entries: vec![
            entry(109, 204, None),
            entry(112, 203, None),
            entry(115, 200, Some(0)),
            entry(117, 200, None),
        ],
        surface_ids: vec![117],
        non_surface_entity_ids: Vec::new(),
        offset: 0,
    };
    let row = crate::surface::SurfaceRow {
        id: 117,
        type_byte: crate::surface::SurfaceKind::Cylinder.canonical_type_byte(),
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 107,
        reversed: true,
        boundary_type: 0,
        next_surface: 0,
        offset: 0,
    };

    assert_eq!(
        super::compact_simple_hole_cylinder_id(
            107,
            std::slice::from_ref(&table),
            std::slice::from_ref(&row),
        ),
        Some(117)
    );

    let mut duplicate = table;
    duplicate.surface_ids.push(117);
    assert_eq!(
        super::compact_simple_hole_cylinder_id(
            107,
            std::slice::from_ref(&duplicate),
            std::slice::from_ref(&row),
        ),
        None
    );
}
