// SPDX-License-Identifier: Apache-2.0

use super::check_point_clouds;
use crate::document::CadIr;
use crate::ids::ProductDefinitionId;
use crate::products::{ExternalDocument, ProductDefinition, Storage};
use cadmpeg_core::serde_erase::json;

const DEFINITION: &str = "test:model:definition#part";

fn cloud(value: serde_json::Value) -> crate::point_cloud::PointCloud {
    json::from_value(value).expect("a point cloud")
}

fn scan(definition: &str) -> crate::point_cloud::PointCloud {
    cloud(cadmpeg_core::json!({
        "id": "test:model:point-cloud#scan",
        "definition": definition,
        "positions": [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 1.0, "y": 0.0, "z": 0.0}],
        "normals": [],
        "colors": [],
        "channels": [{"name": "intensity", "values": [0.25, 0.75]}]
    }))
}

fn with_definition(storage: Storage) -> CadIr {
    let mut ir = CadIr::empty();
    let id = ProductDefinitionId::mint(DEFINITION).unwrap();
    ir.model
        .product_definitions
        .push(ProductDefinition::new(id, storage));
    ir
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
    let mut ir = with_definition(Storage::Embedded);
    ir.model.point_clouds.push(scan(DEFINITION));
    assert!(findings(&ir).is_empty(), "{:?}", findings(&ir));
}

#[test]
fn a_cloud_states_its_definition() {
    let mut value = json::to_value(scan(DEFINITION)).unwrap();
    assert_eq!(value["definition"], DEFINITION);
    let fields = value.as_object_mut().unwrap();
    fields.remove("definition");
    let missing = json::from_value::<crate::point_cloud::PointCloud>(value.clone());
    assert!(missing.is_err(), "a cloud with no definition reads");
    value["product_definition"] = DEFINITION.into();
    let renamed = json::from_value::<crate::point_cloud::PointCloud>(value);
    assert!(renamed.is_err(), "the v10 field name reads");
}

#[test]
fn a_cloud_names_a_definition_of_the_document() {
    let mut ir = with_definition(Storage::Embedded);
    ir.model
        .point_clouds
        .push(scan("test:model:definition#missing"));
    let found = findings(&ir);
    assert_eq!(
        found,
        ["references missing product definition `test:model:definition#missing`"]
    );
}

#[test]
fn a_cloud_is_not_held_by_an_external_definition() {
    let mut ir = with_definition(Storage::External(ExternalDocument::path("scan.e57")));
    ir.model.point_clouds.push(scan(DEFINITION));
    let found = findings(&ir);
    assert_eq!(
        found,
        ["is held by external product definition `test:model:definition#part`"]
    );
}

#[test]
fn per_point_values_must_match_the_points() {
    let mut ir = with_definition(Storage::Embedded);
    ir.model.point_clouds.push(cloud(cadmpeg_core::json!({
        "id": "test:model:point-cloud#scan",
        "definition": DEFINITION,
        "positions": [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 1.0, "y": 0.0, "z": 0.0}],
        "normals": [],
        "colors": [],
        "channels": [
            {"name": "intensity", "values": [0.25]},
            {"name": "intensity", "values": [0.25, 0.75]}
        ]
    })));
    let found = findings(&ir);
    assert_eq!(found.len(), 2, "{found:?}");
}
