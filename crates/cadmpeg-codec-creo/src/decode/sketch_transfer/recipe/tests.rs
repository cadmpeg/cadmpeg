// SPDX-License-Identifier: Apache-2.0

use super::feature_row_schema_classes;

fn row(schema_class: crate::feature::schema::SchemaClass) -> crate::feature::rows::FeatureRow {
    crate::feature::rows::FeatureRow {
        feature_id: 40,
        root_schema_class: Some(schema_class),
        stream_offset: 0,
        body: vec![0; 2].try_into().expect("row body"),
        body_offset: 0,
        offset: 0,
    }
}

#[test]
fn feature_schema_classes_keep_distinct_sorted_values() {
    let mut scan = crate::test_support::empty_container_scan();
    scan.features
        .rows
        .push(row(crate::feature::schema::SchemaClass::Round));
    scan.features
        .depdb_recipe_rows
        .push(row(crate::feature::schema::SchemaClass::Round));
    scan.features
        .depdb_recipe_rows
        .push(row(crate::feature::schema::SchemaClass::Chamfer));
    let classes = feature_row_schema_classes(&scan, 40);
    assert_eq!(classes.len(), 2);
    assert!(classes.contains(&crate::feature::schema::SchemaClass::Round));
    assert!(classes.contains(&crate::feature::schema::SchemaClass::Chamfer));
}
