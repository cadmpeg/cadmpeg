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
    occurrence.transform = Some(Transform::affine(rows).unwrap());
    let wire = serde_json::to_value(&occurrence).unwrap();
    assert!(wire.get("scale").is_none());
    let restored: Occurrence = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(
        restored
            .transform
            .expect("source occurrence placement")
            .affine_rows(),
        rows
    );
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

#[test]
fn absent_occurrence_placement_is_unknown_on_wire_and_through_ancestors() {
    let root_id = "test:model:occurrence#unknown".try_into().unwrap();
    let mut root = super::occurrence(
        "test:model:occurrence#unknown",
        OccurrenceParent::Root {},
        7.0,
    );
    root.transform = None;
    root.linked_prototype = Some(Transform::identity());
    assert_eq!(root.effective_transform().unwrap(), None);
    let wire = serde_json::to_value(&root).unwrap();
    assert!(wire.get("transform").is_none());
    assert_eq!(
        serde_json::from_value::<Occurrence>(wire.clone())
            .unwrap()
            .transform,
        None
    );
    let mut null = wire;
    null["transform"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<Occurrence>(null).is_err());
    let child = super::occurrence(
        "test:model:occurrence#child",
        OccurrenceParent::Occurrence {
            occurrence: root_id,
        },
        11.0,
    );
    let mut sibling = super::occurrence(
        "test:model:occurrence#known",
        OccurrenceParent::Root {},
        13.0,
    );
    sibling.ordinal = 1;
    let known = sibling.effective_transform().unwrap();
    let rows = [root, child, sibling];
    let graph = AssemblyGraph::new(&rows).unwrap();
    assert_eq!(graph.effective_transform(&rows[0].id), None);
    assert_eq!(graph.effective_transform(&rows[1].id), None);
    assert_eq!(graph.effective_transform(&rows[2].id), known);
    let report = crate::validate::validate_neutral(
        &{
            let mut ir = crate::CadIr::empty();
            ir.model.occurrences = rows.to_vec();
            ir.model.occurrences.sort_by(|a, b| a.id.cmp(&b.id));
            ir
        },
        vec![],
    )
    .unwrap();
    assert!(report.is_ok(), "{:?}", report.findings);
}
