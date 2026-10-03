// SPDX-License-Identifier: Apache-2.0

use crate::native::attach::expressions::attach_block_dimension_parameter_consumers;
use crate::native::attach::expressions::attach_expression_parameters;
use crate::native::attach::expressions::expression_parameter_id;
use crate::native::attach::parameter_owner_dependencies;
use cadmpeg_ir::features::{FeatureId, ParameterId};
use std::collections::BTreeMap;

#[test]
fn nx_block_dimension_parameters_name_the_block_as_consumer() {
    let expression = |key: u32| crate::native::om::ParameterFormula {
        id: format!("nx:test:expression#{key}"),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new(format!("p{key}")),
        unit: crate::native::om::ExpressionUnit::Millimeter,
        expression: key.to_string(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(f64::from(key)).unwrap()),
        source_entry: "part".into(),
        source_table: cadmpeg_core::text::NonBlankString::try_from("nx:test:expression-table#table")
            .unwrap(),
        source_offset: u64::from(key),
    };
    let expressions = [expression(20), expression(21), expression(22)];
    let dimensions = crate::native::features::FeatureBlockDimensions {
        id: "dimensions".into(),
        operation_label: "nx:feature-history:operation-label#1-4".into(),
        construction: "construction".into(),
        anchor_bindings: vec!["binding".into()],
        dimensions: std::array::from_fn(|slot| crate::native::features::FeatureBlockDimension {
            declaration: ["d20", "d21", "d22"][slot].into(),
            expression: expressions[slot].id.clone(),
            value: cadmpeg_ir::scalar::FiniteReal::new([20.0, 21.0, 22.0][slot])
                .expect("finite dimension"),
        }),
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
    crate::test_support::with_decode_context(|ctx| {
        attach_expression_parameters(ctx, &mut ir, &expressions, &[], &[], &mut annotations)
    })
    .expect("valid exactness fields");
    let parameter_owners = ir
        .model
        .parameters
        .iter()
        .map(|parameter| (parameter.id.clone(), parameter.owner.clone()))
        .collect();
    let parameter_references = dimensions
        .dimensions
        .iter()
        .filter_map(|dimension| expression_parameter_id(&dimension.expression))
        .collect::<Vec<_>>();
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| parameter_owner_dependencies(
            ctx,
            &parameter_owners,
            &parameter_references
        ))
        .unwrap(),
        [ir.model.features[0].id.clone()]
    );
    assert_eq!(
        (&*ir.model.features[0].source_content),
        ir.model
            .parameters
            .iter()
            .map(|parameter| {
                cadmpeg_ir::features::FeatureSourceContent::Parameter(parameter.id.clone())
            })
            .collect::<Vec<_>>()
    );
    crate::test_support::with_decode_context(|ctx| {
        attach_block_dimension_parameter_consumers(ctx, &mut ir, &[dimensions], &mut annotations)
    })
    .expect("valid exactness fields");
    assert_eq!(ir.model.parameters.len(), 3);
    for (ordinal, parameter) in ir.model.parameters.iter().enumerate() {
        assert_eq!(
            parameter.properties[format!("block_dimension.{ordinal}").as_str()],
            "dimensions"
        );
        assert_eq!(
            parameter.properties["consumer.0"],
            "nx:feature-history:feature#1-4"
        );
    }
}

fn parameter_owner_with_limit(
    dimension: cadmpeg_core::decode::ResourceDimension,
) -> Result<(), cadmpeg_core::CodecError> {
    let adjust: fn(&mut cadmpeg_core::decode::DecodePolicy) = match dimension {
        cadmpeg_core::decode::ResourceDimension::CollectionItems => |policy| {
            policy.limits.max_collection_items = 0;
        },
        cadmpeg_core::decode::ResourceDimension::RetainedBytes => |policy| {
            policy.limits.max_retained_bytes = 0;
        },
        cadmpeg_core::decode::ResourceDimension::WorkUnits => |policy| {
            policy.limits.max_work_units = 0;
        },
        _ => {
            return Err(cadmpeg_core::CodecError::InvalidInput(
                "unsupported parameter owner test limit".to_string(),
            ))
        }
    };
    crate::test_support::with_decode_context_over(&[], adjust, |ctx| {
        let parameter = ParameterId::mint("nx:test:parameter#20").unwrap();
        let owner = FeatureId::mint("nx:test:feature#1").unwrap();
        let owners = BTreeMap::from([(parameter.clone(), Some(owner.clone()))]);
        let dependencies = parameter_owner_dependencies(ctx, &owners, &[parameter])?;
        assert_eq!(dependencies, [owner]);
        Ok(())
    })
}

#[test]
fn parameter_owner_dependency_refuses_collection_limit() {
    let dimension = cadmpeg_core::decode::ResourceDimension::CollectionItems;
    assert!(
        matches!(parameter_owner_with_limit(dimension), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
    );
}

#[test]
fn parameter_owner_dependency_refuses_retained_limit() {
    let dimension = cadmpeg_core::decode::ResourceDimension::RetainedBytes;
    assert!(
        matches!(parameter_owner_with_limit(dimension), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
    );
}

#[test]
fn parameter_owner_dependency_refuses_work_limit() {
    let dimension = cadmpeg_core::decode::ResourceDimension::WorkUnits;
    assert!(
        matches!(parameter_owner_with_limit(dimension), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.dimension == dimension)
    );
}

#[test]
fn nx_inch_expression_values_are_attached_in_millimeters() {
    let expression = |key: u32, name: &str, formula: &str, value: Option<f64>| {
        crate::native::om::ParameterFormula {
            id: format!("nx:test:expression#{key}"),
            owner: None,
            declaration: None,
            name: crate::om::parameter_name::ParameterName::new(name.to_string()),
            unit: crate::native::om::ExpressionUnit::Inch,
            expression: formula.into(),
            value: value.map(|value| cadmpeg_ir::scalar::FiniteReal::try_from(value).unwrap()),
            source_entry: "/Root/UG_PART/UG_PART".into(),
            source_table: cadmpeg_core::text::NonBlankString::try_from("nx:test:expression-table#table")
                .unwrap(),
            source_offset: u64::from(key),
        }
    };
    let expressions = [
        expression(1, "p1", "2", Some(2.0)),
        expression(2, "p2", "p1 * 3", Some(6.0)),
    ];
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::test_support::with_decode_context(|ctx| {
        attach_expression_parameters(ctx, &mut ir, &expressions, &[], &[], &mut annotations)
    })
    .expect("valid exactness fields");

    assert_eq!(
        ir.model.parameters[0].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(2.0 * 25.4).unwrap()
        ))
    );
    assert_eq!(
        ir.model.parameters[1].value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(6.0 * 25.4).unwrap()
        ))
    );
    assert_eq!(
        ir.model.parameters[0]
            .properties
            .get("unit")
            .map(String::as_str),
        Some("inch")
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::decode::feature_completeness::incomplete_expression_parameters(ctx, &ir)
    })
    .unwrap()
    .is_empty());
}

#[test]
fn nx_native_expression_units_remain_outside_neutral_values() {
    let expression = crate::native::om::ParameterFormula {
        id: "nx:test:expression#native".into(),
        owner: None,
        declaration: None,
        name: crate::om::parameter_name::ParameterName::new("p1".to_string()),
        unit: crate::native::om::ExpressionUnit::Native("custom/unit".into()),
        expression: "4".into(),
        value: Some(cadmpeg_ir::scalar::FiniteReal::try_from(4.0).unwrap()),
        source_entry: "part".into(),
        source_table: cadmpeg_core::text::NonBlankString::try_from("nx:test:expression-table#table")
            .unwrap(),
        source_offset: 1,
    };
    let mut ir = cadmpeg_ir::CadIr::empty();
    let mut annotations = cadmpeg_ir::AnnotationBuilder::new();

    crate::test_support::with_decode_context(|ctx| {
        attach_expression_parameters(ctx, &mut ir, &[expression], &[], &[], &mut annotations)
    })
    .expect("valid exactness fields");

    assert_eq!(ir.model.parameters[0].value, None);
    assert_eq!(
        ir.model.parameters[0]
            .properties
            .get("unit")
            .map(String::as_str),
        Some("custom/unit")
    );
    assert_eq!(
        crate::test_support::with_decode_context(|ctx| {
            crate::decode::feature_completeness::incomplete_expression_parameters(ctx, &ir)
        })
        .unwrap(),
        [ir.model.parameters[0].id.clone()].into()
    );
}
