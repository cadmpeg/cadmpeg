// SPDX-License-Identifier: Apache-2.0
use crate::products::{AssemblyGraph, Occurrence, OccurrenceParent};
use crate::transform::Transform;

#[test]
fn affine_placement_carries_scale_without_a_duplicate_scale_field() {
    let mut occurrence = super::occurrence(
        "test:model:occurrence#affine",
        OccurrenceParent::Root {},
        0.0,
    );
    let rows = [
        [-2.0, 0.0, 0.0, 7.0],
        [0.0, 0.0, 0.0, 8.0],
        [0.0, 0.0, 3.0, 9.0],
    ];
    occurrence.transform = Transform::affine(rows).unwrap();
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert!(wire.get("scale").is_none());
    let restored: Occurrence = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(restored.transform.affine_rows(), rows);
    let occurrences = [restored];
    let graph = AssemblyGraph::new(&occurrences).unwrap();
    assert_eq!(
        graph
            .effective_transform(&occurrences[0].id)
            .unwrap()
            .affine_rows(),
        rows
    );
    let mut old_wire = wire;
    old_wire["scale"] = serde_json::json!([1.0, 1.0, 1.0]);
    assert!(serde_json::from_value::<Occurrence>(old_wire)
        .unwrap_err()
        .to_string()
        .contains("scale"));
}
