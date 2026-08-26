// SPDX-License-Identifier: Apache-2.0

use super::ordered_row_feature_ids;

#[test]
fn row_feature_ids_preserve_first_source_order() {
    let rows = [
        crate::feature::FeatureRow {
            feature_id: 40,
            header: [0; 2],
            root_schema_class: None,
            stream_offset: 0,
            body: Vec::new(),
            body_offset: 30,
            offset: 20,
        },
        crate::feature::FeatureRow {
            feature_id: 12,
            header: [0; 2],
            root_schema_class: None,
            stream_offset: 0,
            body: Vec::new(),
            body_offset: 50,
            offset: 40,
        },
        crate::feature::FeatureRow {
            feature_id: 40,
            header: [0; 2],
            root_schema_class: None,
            stream_offset: 0,
            body: Vec::new(),
            body_offset: 70,
            offset: 60,
        },
    ];

    assert_eq!(ordered_row_feature_ids(&rows), vec![40, 12]);
}
