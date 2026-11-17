// SPDX-License-Identifier: Apache-2.0
//! Design parameters survive a write as `CadParameter` prims (spec §18.6).

use std::collections::BTreeMap;

use cadmpeg_ir::features::{
    DesignParameter, DimensionDisplay, DistinctMembers, ParameterId, ParameterValue,
};
use cadmpeg_ir::scalar::{Angle, FiniteReal, Length};

use crate::test_support::{decode, encode};

/// Agreement of a parameter value after a write, per unit of its magnitude.
const VALUE_TOLERANCE: f64 = 1e-12;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/{name}.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

fn parameter(key: &str, value: Option<ParameterValue>) -> DesignParameter {
    DesignParameter {
        product_definition: None,
        id: ParameterId::mint(format!("test:model:parameter#{key}")).expect("an id"),
        owner: None,
        ordinal: 0,
        name: key.to_owned(),
        expression: String::new(),
        display: None,
        value,
        dependencies: DistinctMembers::default(),
        properties: BTreeMap::new(),
        pmi: None,
        native_ref: None,
    }
}

fn close(left: &ParameterValue, right: &ParameterValue) -> bool {
    let near = |a: f64, b: f64| (a - b).abs() <= VALUE_TOLERANCE * a.abs().max(1.0);
    match (left, right) {
        (ParameterValue::Length(a), ParameterValue::Length(b)) => near(a.get(), b.get()),
        (ParameterValue::Angle(a), ParameterValue::Angle(b)) => near(a.get(), b.get()),
        _ => left == right,
    }
}

#[test]
fn design_parameters_survive_a_write() {
    let source = decode(&fixture("brep_array_box"));
    let mut ir = source.ir().clone();
    let definition = ir.model.product_definitions[0].id.clone();
    let mut width = parameter(
        "width",
        Some(ParameterValue::Length(Length::new(12.5).expect("a length"))),
    );
    width.expression = "= 12.5mm".to_owned();
    width.display = Some(DimensionDisplay::Diameter);
    width.product_definition = Some(definition);
    width.properties.insert(
        cadmpeg_core::text::NonBlankString::new("unit").expect("a name"),
        "mm".to_owned(),
    );
    let mut height = parameter(
        "height",
        Some(ParameterValue::Length(Length::new(25.0).expect("a length"))),
    );
    height.expression = "= width * 2".to_owned();
    height.ordinal = 1;
    height.dependencies.insert(width.id.clone());
    ir.model.parameters = vec![
        height,
        width,
        parameter(
            "draft",
            Some(ParameterValue::Angle(Angle::new(0.1).expect("an angle"))),
        ),
        parameter(
            "ratio",
            Some(ParameterValue::Real(FiniteReal::new(0.75).expect("a real"))),
        ),
        parameter("count", Some(ParameterValue::Integer(6))),
        parameter("enabled", Some(ParameterValue::Boolean(true))),
        parameter(
            "finish",
            Some(ParameterValue::String("anodized".to_owned())),
        ),
        parameter("unset", None),
    ];
    let (text, _) = encode(&ir);
    assert!(text.contains("def CadParameter"), "{text}");
    let read = decode(&text);
    let after = &read.ir().model.parameters;
    assert_eq!(after.len(), ir.model.parameters.len());
    for expected in &ir.model.parameters {
        let found = after
            .iter()
            .find(|candidate| candidate.id == expected.id)
            .unwrap_or_else(|| panic!("{}", expected.id));
        assert_eq!(found.name, expected.name);
        assert_eq!(found.expression, expected.expression);
        assert_eq!(found.ordinal, expected.ordinal);
        assert_eq!(found.display, expected.display);
        assert_eq!(found.dependencies, expected.dependencies);
        assert_eq!(found.properties, expected.properties);
        assert_eq!(found.product_definition, expected.product_definition);
        match (&found.value, &expected.value) {
            (Some(found), Some(expected)) => assert!(close(found, expected), "{found:?}"),
            (found, expected) => assert_eq!(found, expected),
        }
    }
}

#[test]
fn a_sketch_dimension_keeps_its_parameter() {
    let first = decode(&fixture("sketch_curved"));
    let (written, _) = encode(first.ir());
    assert!(
        written.contains("rel sketch:constraint:parameter"),
        "{written}"
    );
    let second = decode(&written);
    let ids = |model: &cadmpeg_ir::document::Model| {
        let mut out: Vec<String> = model
            .parameters
            .iter()
            .map(|parameter| parameter.id.to_string())
            .collect();
        out.sort();
        out
    };
    assert!(!first.ir().model.parameters.is_empty());
    assert_eq!(ids(&first.ir().model), ids(&second.ir().model));
}
