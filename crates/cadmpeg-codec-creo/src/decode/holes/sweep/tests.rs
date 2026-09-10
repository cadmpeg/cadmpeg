// SPDX-License-Identifier: Apache-2.0

#[test]
fn compact_simple_hole_rejects_duplicate_materialized_roster_id() {
    let entry = |entity_id, class_id, source_entity_id| crate::feature::FeatureEntityTableEntry {
        payload: crate::feature::entry_payload(class_id, source_entity_id, None, None),

        entity_id,
        prefixed: false,
        offset: 0,
        end_offset: 0,
    };
    let table = crate::feature::FeatureEntityTable::new(
        107,
        29,
        vec![
            entry(109, 204, None),
            entry(112, 203, None),
            entry(115, 200, Some(0)),
            entry(117, 200, None),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([117]);
    let row = crate::surface::SurfaceRow {
        id: 117,
        kind: crate::surface::SurfaceKind::Cylinder,
        feature_id: 107,
        reversed: true,
        boundary_type: crate::surface::BoundaryType::Code00,
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
    duplicate
        .entries
        .push(crate::feature::dummy_table_entry(117));
    assert_eq!(
        super::compact_simple_hole_cylinder_id(
            107,
            std::slice::from_ref(&duplicate),
            std::slice::from_ref(&row),
        ),
        None
    );
}

#[test]
fn circular_sweep_requires_an_exact_materialized_surface_roster() {
    let table = crate::feature::FeatureEntityTable::new(
        40,
        29,
        vec![
            crate::feature::dummy_table_entry(46),
            crate::feature::dummy_table_entry(51),
        ],
        &std::collections::BTreeSet::new(),
        0,
    )
    .with_surface_ids([46, 51]);

    assert!(super::has_exact_materialized_surface_roster(
        &table,
        [46, 51]
    ));

    let mut duplicate = table.clone();
    duplicate
        .entries
        .push(crate::feature::dummy_table_entry(51));
    assert!(!super::has_exact_materialized_surface_roster(
        &duplicate,
        [46, 51]
    ));

    let mut extra = table;
    extra.entries.push(crate::feature::dummy_table_entry(54));
    extra.mark_surface_id(54);
    assert!(!super::has_exact_materialized_surface_roster(
        &extra,
        [46, 51]
    ));
}
