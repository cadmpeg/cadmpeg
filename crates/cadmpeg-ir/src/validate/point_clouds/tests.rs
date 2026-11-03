// SPDX-License-Identifier: Apache-2.0

use super::check_point_clouds;
use crate::document::CadIr;

fn cloud(value: serde_json::Value) -> crate::point_cloud::PointCloud {
    serde_json::from_value(value).expect("a point cloud")
}

fn findings(ir: &CadIr) -> Vec<String> {
    let mut findings = Vec::new();
    check_point_clouds(ir, &mut findings);
    findings
        .into_iter()
        .map(|finding| finding.message)
        .collect()
}

#[test]
fn a_cloud_with_one_value_per_point_is_valid() {
    let mut ir = CadIr::empty();
    ir.model.point_clouds.push(cloud(serde_json::json!({
        "id": "test:model:point-cloud#scan",
        "positions": [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 1.0, "y": 0.0, "z": 0.0}],
        "channels": [{"name": "intensity", "values": [0.25, 0.75]}]
    })));
    assert!(findings(&ir).is_empty(), "{:?}", findings(&ir));
}

#[test]
fn per_point_values_must_match_the_points() {
    let mut ir = CadIr::empty();
    ir.model.point_clouds.push(cloud(serde_json::json!({
        "id": "test:model:point-cloud#scan",
        "product_definition": "test:model:definition#missing",
        "positions": [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 1.0, "y": 0.0, "z": 0.0}],
        "channels": [
            {"name": "intensity", "values": [0.25]},
            {"name": "intensity", "values": [0.25, 0.75]}
        ]
    })));
    let found = findings(&ir);
    assert_eq!(found.len(), 3, "{found:?}");
}
