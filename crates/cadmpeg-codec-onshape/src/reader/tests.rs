// SPDX-License-Identifier: Apache-2.0
use super::{part_studio_bodies, Model};
use crate::test_support::{fixture, Report};
use serde_json::json;

/// The revolve fixture's model with its body owned by one of two root products, each placed
/// once; `x` moves the second placement.
fn two_part_studio(x: f64) -> Model {
    let mut report = Report::default();
    let model = crate::test_support::read_bytes(
        std::path::Path::new("synthetic.onshape-bundle"),
        &fixture("revolve"),
        &mut report,
    )
    .unwrap();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    // As in a Part Studio's export, the part's name is on its product, not its body.
    wire["geometry"]["bodies"][0]
        .as_object_mut()
        .unwrap()
        .remove("name");
    wire["model"]["product_definitions"] = json!([
        {"id": "synthetic:test:component#first", "kind": "part", "label": "Part 1", "bodies": [body], "bom_properties": {}, "sketches": [], "storage":{"kind":"embedded"}, "tessellations":[], "occurrences":[], "instance_bodies":[], "representations":[]},
        {"id": "synthetic:test:component#second", "kind": "part", "label": "Part 2", "bodies": [], "bom_properties": {}, "sketches": [], "storage":{"kind":"embedded"}, "tessellations":[], "occurrences":[], "instance_bodies":[], "representations":[]},
    ]);
    let placed = |key: u32, definition: &str, x: f64| {
        json!({"id": format!("synthetic:test:occurrence#{key}"),
            "prototype": format!("synthetic:test:component#{definition}"),
            "overrides": [],
            "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]]})
    };
    wire["model"]["occurrences"] = json!([placed(0, "first", 0.0), placed(1, "second", x)]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn part_studio_parts_are_one_design_but_moved_parts_stay_placed() {
    let mut model = two_part_studio(0.0);
    part_studio_bodies(&mut model);
    assert!(model.ir.model.occurrences.is_empty());
    assert_eq!(model.ir.model.product_definitions.len(), 2);
    assert!(model.ir.model.occurrences.is_empty());
    assert_eq!(model.ir.geometry.bodies[0].name.as_deref(), Some("Part 1"));

    let mut moved = two_part_studio(30.0);
    part_studio_bodies(&mut moved);
    assert_eq!(moved.ir.model.occurrences.len(), 2);
    assert!(!moved.ir.model.occurrences.is_empty());
}
