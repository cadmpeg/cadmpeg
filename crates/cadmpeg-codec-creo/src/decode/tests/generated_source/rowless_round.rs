// SPDX-License-Identifier: Apache-2.0

use crate::decode::analytic::carriers::rowless_round_face_orientations;
use crate::decode::surfaces::cylinders::rowless_round_cylinder_pairs;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn rowless_round_cylinder_requires_the_four_entry_sibling_layout() {
    let row = |id, kind: crate::surface::SurfaceKind| crate::surface::SurfaceRow {
        id,
        kind,
        feature_id: 23,
        reversed: false,
        boundary_type: crate::surface::BoundaryType::Code00,
        next_surface: 0,
        offset: 0,
    };
    let mut rows = vec![
        row(10, crate::surface::SurfaceKind::Plane),
        row(11, crate::surface::SurfaceKind::Plane),
        row(13, crate::surface::SurfaceKind::Cylinder),
    ];
    let table = crate::feature::entity::FeatureEntityTable::new(
        23,
        80,
        vec![
            crate::feature::entity::dummy_table_entry(10),
            crate::feature::entity::dummy_table_entry(11),
            crate::feature::entity::dummy_table_entry(12),
            crate::feature::entity::dummy_table_entry(13),
        ],
        &std::collections::BTreeSet::new(),
        47,
    )
    .with_surface_ids([10, 11, 13]);
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| rowless_round_cylinder_pairs(
            ctx,
            &BTreeSet::from([23]),
            std::slice::from_ref(&table),
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
        ))
        .expect("service pair admitted"),
        vec![(12, 13, 47)]
    );
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| rowless_round_cylinder_pairs(
            ctx,
            &BTreeSet::new(),
            std::slice::from_ref(&table),
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
        ))
        .expect("service empty pair admitted")
        .is_empty()
    );
    rows[2].reversed = true;
    assert_eq!(
        crate::decode::with_test_decode_ctx(|ctx| rowless_round_face_orientations(
            ctx,
            &BTreeSet::from([23]),
            std::slice::from_ref(&table),
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
            &BTreeSet::from([12]),
        ))
        .expect("service rowless orientation admitted"),
        BTreeMap::from([(12, true)])
    );
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| rowless_round_face_orientations(
            ctx,
            &BTreeSet::from([23]),
            std::slice::from_ref(&table),
            &crate::surface::unique_rows::UniqueIdRows::from_rows(rows.clone()),
            &BTreeSet::new(),
        ))
        .expect("service absent orientation admitted")
        .is_empty()
    );
    let mut materialized_rowless = rows;
    materialized_rowless.push(row(12, crate::surface::SurfaceKind::Cylinder));
    assert!(
        crate::decode::with_test_decode_ctx(|ctx| rowless_round_cylinder_pairs(
            ctx,
            &BTreeSet::from([23]),
            &[table],
            &crate::surface::unique_rows::UniqueIdRows::from_rows(materialized_rowless.clone()),
        ))
        .expect("service materialized row admitted")
        .is_empty()
    );
}
