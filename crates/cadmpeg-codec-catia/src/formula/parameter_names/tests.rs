// SPDX-License-Identifier: Apache-2.0
//! Document parameter name collision guards.
#![allow(clippy::unwrap_used)]
use super::normalize;
use cadmpeg_ir::{document::CadIr, features::ParameterId};
use std::collections::BTreeMap;

fn parameter(id: &str, native_ref: &str) -> cadmpeg_ir::features::DesignParameter {
    cadmpeg_ir::features::DesignParameter {
        id: ParameterId::mint(format!("synthetic:test:id#{id}")).expect("identity grammar"),
        owner: None,
        ordinal: 99,
        name: id.to_string(),
        expression: "1 mm".to_string(),
        display: None,
        value: Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(1.0).unwrap(),
        )),
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: Some(native_ref.to_string()),
    }
}

#[test]
fn parameter_name_indexes_refuse_collection_limit() {
    let mut ir = CadIr::empty();
    let mut value = parameter("one", "native");
    value.name = "Length".to_string();
    ir.model.parameters.push(value);
    let refused = crate::test_support::with_collection_limit(0, |ctx| {
        normalize(ctx, &mut ir.model.parameters)
    });
    assert!(
        matches!(refused, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
        if limit.operation == "catia_parameter_reserved_names")
    );
    crate::test_support::with_service_context(|ctx| normalize(ctx, &mut ir.model.parameters))
        .expect("service profile admits name indexes");
    assert_eq!(ir.model.parameters[0].name, "Length");
}
#[test]
fn disambiguates_parameter_names_without_hiding_a_later_source_name() {
    let mut ir = CadIr::empty();
    let mut first = parameter("first", "first-native");
    first.name = "Angle".to_string();
    let mut second = parameter("second", "second-native");
    second.name = "Angle".to_string();
    let mut later_source_name = parameter("later", "later-native");
    later_source_name.name = "Angle#1".to_string();
    ir.model
        .parameters
        .extend([first, second, later_source_name]);

    crate::test_support::with_service_context(|ctx| normalize(ctx, &mut ir.model.parameters))
        .unwrap();

    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|parameter| parameter.name.as_str())
            .collect::<Vec<_>>(),
        ["Angle", "Angle#2", "Angle#1"]
    );
    assert_eq!(
        ir.model.parameters[1].properties.get("source_name"),
        Some(&"Angle".to_string())
    );
    assert!(!ir.model.parameters[2]
        .properties
        .contains_key("source_name"));
}
#[test]
fn normalizes_unnamed_document_parameters() {
    let mut ir = CadIr::empty();
    for id in ["first", "second"] {
        let mut value = parameter(id, "native");
        value.name.clear();
        ir.model.parameters.push(value);
    }
    crate::test_support::with_service_context(|ctx| normalize(ctx, &mut ir.model.parameters))
        .unwrap();
    assert_eq!(
        ir.model
            .parameters
            .iter()
            .map(|value| value.name.as_str())
            .collect::<Vec<_>>(),
        ["Parameter#1", "Parameter#2"]
    );
    assert!(ir.model.parameters.iter().all(|value| value
        .properties
        .get("source_name")
        .map(String::as_str)
        == Some("")));
}
