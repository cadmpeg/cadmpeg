// SPDX-License-Identifier: Apache-2.0
use crate::features::{
    DesignParameter, Expression, ExpressionToken, Feature, FeatureDefinition, FeatureEvaluation,
    FeatureId, FeatureOperation, ParameterId, ParameterValue,
};
use crate::report::{check::Check, Severity};
use crate::scalar::{Angle, Length};
use crate::CadIr;

fn parameter(id: &str, ordinal: u32) -> DesignParameter {
    DesignParameter {
        id: ParameterId::mint(id).unwrap(),
        owner: None,
        ordinal,
        name: format!("p{ordinal}"),
        expression: ParameterValue::Length(Length::new(2.0).unwrap()).literal_expression(),
        driving: true,
        display: None,
        value: Some(ParameterValue::Length(Length::new(2.0).unwrap())),
        properties: Default::default(),
        pmi: None,
        native_ref: None,
    }
}
fn feature() -> Feature {
    Feature {
        id: FeatureId::mint("test:model:feature#owner").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        needs_recompute: false,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: None,
        source_text: None,
        source_content: Default::default(),
        native_ref: None,
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::StoredGeometry {},
        )),
    }
}

#[test]
fn literal_cache_mismatch_requires_explicit_owner_staleness() {
    let mut ir = CadIr::empty();
    let owner = feature();
    let mut parameter = parameter("test:model:parameter#width", 0);
    parameter.owner = Some(owner.id.clone());
    parameter.value = Some(ParameterValue::Length(Length::new(9.0).unwrap()));
    ir.model.features.push(owner);
    ir.model.parameters.push(parameter);
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.check == Check::GeometricConsistency
            && finding.severity == Severity::Error
            && finding.message.contains("canonical literal expression")));
    ir.model.features[0].needs_recompute = true;
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.message.contains("canonical literal expression")));
}

#[test]
fn token_dependencies_keep_forward_references_and_report_cycles_as_warnings() {
    let mut ir = CadIr::empty();
    let mut first = parameter("test:model:parameter#first", 0);
    let mut second = parameter("test:model:parameter#second", 1);
    first.expression = Expression {
        tokens: vec![ExpressionToken::Parameter(second.id.clone())],
    };
    ir.model.parameters = vec![first, second.clone()];
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report.is_ok(), "{:#?}", report.findings);
    second.expression = Expression {
        tokens: vec![ExpressionToken::Parameter(
            ir.model.parameters[0].id.clone(),
        )],
    };
    ir.model.parameters[1] = second;
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Warning
            && finding.message.contains("dependency cycle")));
    assert!(!report
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Error));
}

#[test]
fn missing_expression_reference_is_an_error_without_erasing_the_token() {
    let mut ir = CadIr::empty();
    let mut value = parameter("test:model:parameter#first", 0);
    let missing = ParameterId::mint("test:model:parameter#missing").unwrap();
    value.expression = Expression {
        tokens: vec![ExpressionToken::Parameter(missing.clone())],
    };
    ir.model.parameters.push(value);
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Error
            && finding.message.contains("parameter dependency")));
    assert_eq!(
        ir.model.parameters[0]
            .expression
            .dependencies()
            .collect::<Vec<_>>(),
        vec![&missing]
    );
}

#[test]
fn canonical_quantity_literals_compare_in_canonical_units() {
    let length = ParameterValue::Length(Length::new(20.0).unwrap());
    assert_eq!(
        Expression::text("2 cm").literal_value_as(&length),
        Some(length)
    );
    let angle = ParameterValue::Angle(Angle::new(std::f64::consts::FRAC_PI_2).unwrap());
    assert_eq!(
        Expression::text("90 deg").literal_value_as(&angle),
        Some(angle)
    );
}

#[test]
fn inactive_configuration_maps_store_only_actual_differences() {
    use crate::features::{ConfigurationFeatureState, ConfigurationId, DesignConfiguration};
    let mut ir = CadIr::empty();
    ir.model.features.push(feature());
    ir.model
        .parameters
        .push(parameter("test:model:parameter#width", 0));
    let mut configuration = DesignConfiguration {
        id: ConfigurationId::mint("test:model:configuration#other").unwrap(),
        ordinal: 0,
        active: false,
        source_index: None,
        name: None,
        material: None,
        properties: Default::default(),
        parameter_overrides: Default::default(),
        bodies: None,
        parameter_values: Default::default(),
        feature_states: Default::default(),
        native_ref: None,
    };
    let parameter = &ir.model.parameters[0];
    configuration
        .parameter_overrides
        .insert(parameter.id.clone(), parameter.expression.clone());
    configuration.feature_states.insert(
        ir.model.features[0].id.clone(),
        ConfigurationFeatureState::Active {},
    );
    ir.model.configurations.push(configuration);
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report.findings.iter().any(|finding| finding
        .message
        .contains("repeats the base parameter expression")));
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.message.contains("repeats the base feature state")));
    ir.model.configurations[0].parameter_overrides.clear();
    ir.model.features[0].suppressed = Some(true);
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .is_ok());
    ir.model.configurations[0].parameter_values.insert(
        ir.model.parameters[0].id.clone(),
        ir.model.parameters[0].value.clone().unwrap(),
    );
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("repeats the base parameter cache")));
}

#[test]
fn feature_dependency_order_is_independent_of_source_ordinals_and_cycles_remain_warnings() {
    let mut ir = CadIr::empty();
    let mut first = feature();
    let mut second = feature();
    second.id = FeatureId::mint("test:model:feature#zlater").unwrap();
    second.ordinal = 1;
    first.dependencies = vec![second.id.clone()].try_into().unwrap();
    first
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::DerivedGeometry {
                source: second.id.clone(),
            },
        ));
    ir.model.features = vec![first, second];
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report.is_ok(), "{:#?}", report.findings);
    let first = ir.model.features[0].id.clone();
    ir.model.features[1].dependencies = vec![first.clone()].try_into().unwrap();
    ir.model.features[1]
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::DerivedGeometry { source: first },
        ));
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Warning
            && finding.message.contains("feature dependency cycle")));
    assert!(report.is_ok(), "{:#?}", report.findings);
    ir.model.features[0].dependencies =
        vec![FeatureId::mint("test:model:feature#missing").unwrap()]
            .try_into()
            .unwrap();
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Error
            && finding.message.contains("missing dependency feature")));
}

#[test]
fn tree_children_are_canonical_and_source_content_only_adds_interleaving_order() {
    use crate::features::{FeatureSourceContent, FeatureTreeNodeRole, TreeChildren};
    let mut ir = CadIr::empty();
    let mut first = feature();
    first.id = FeatureId::mint("test:model:feature#first").unwrap();
    let mut second = feature();
    second.id = FeatureId::mint("test:model:feature#second").unwrap();
    second.ordinal = 1;
    let mut parent = feature();
    parent.id = FeatureId::mint("test:model:feature#tree-parent").unwrap();
    parent.ordinal = 9;
    parent
        .evaluation
        .set_definition(FeatureDefinition::Operation(FeatureOperation::TreeNode {
            role: FeatureTreeNodeRole::FeatureFolder,
            children: TreeChildren::new(vec![first.id.clone(), second.id.clone()], None).unwrap(),
        }));
    let contents = vec![
        FeatureSourceContent::Text("header".into()),
        FeatureSourceContent::Feature(first.id.clone()),
        FeatureSourceContent::Feature(second.id.clone()),
    ];
    parent.source_content = contents.clone().try_into().unwrap();
    ir.model.features = vec![first, second, parent];
    let report = crate::validate::validate_neutral(&ir, vec![]).unwrap();
    assert!(report.is_ok(), "{:#?}", report.findings);
    ir.model.features[2].source_content = contents[1..].to_vec().try_into().unwrap();
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("canonical tree children")));
    let mut reversed = contents;
    reversed.swap(1, 2);
    ir.model.features[2].source_content = reversed.try_into().unwrap();
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("canonical tree children")));
}

#[test]
fn assembly_joint_suppression_has_only_the_joint_as_authority() {
    use crate::products::{AssemblyJoint, JointConnector, JointOperand, PairedJointKind};
    let mut ir = CadIr::empty();
    let id = "test:model:joint#fixed".try_into().unwrap();
    let connector = |object| JointConnector {
        operand: JointOperand::root(object, Vec::new()),
        frame: crate::transform::Transform::identity(),
        detached: false,
    };
    ir.model.assembly_joints.push(AssemblyJoint::paired(
        id,
        PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        [connector("first"), connector("second")],
        None,
    ));
    let mut joint_feature = feature();
    joint_feature
        .evaluation
        .set_definition(FeatureDefinition::Operation(
            FeatureOperation::AssemblyJoint {
                joint: ir.model.assembly_joints[0].id.clone(),
            },
        ));
    joint_feature.suppressed = Some(false);
    ir.model.features.push(joint_feature);
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .findings
        .iter()
        .any(|finding| finding.message.contains("suppression belongs only")));
    ir.model.features[0].suppressed = None;
    ir.model.assembly_joints[0].suppressed = true;
    assert!(crate::validate::validate_neutral(&ir, vec![])
        .unwrap()
        .is_ok());
}

#[test]
fn structural_configuration_overrides_keep_bound_dimensions_in_the_parameter_overlay() {
    use crate::features::{
        ConfigurationFeatureState, DesignConfiguration, Dimension, RadialDimension,
    };
    use crate::scalar::PositiveLength;
    let mut ir = CadIr::empty();
    let parameter = parameter("test:model:parameter#radius", 0);
    let radius = parameter.id.clone();
    ir.model.parameters.push(parameter);
    let mut sphere = feature();
    let definition = |center, radius| {
        FeatureDefinition::Operation(FeatureOperation::Sphere {
            center: crate::features::FinitePoint3::new(crate::math::Point3::new(center, 0.0, 0.0))
                .unwrap(),
            radius,
            op: crate::features::BooleanOp::NewBody,
        })
    };
    sphere.evaluation.set_definition(definition(
        0.0,
        RadialDimension::Radius(Dimension::Parameter(radius.clone())),
    ));
    ir.model.features.push(sphere);
    let mut configuration: DesignConfiguration = serde_json::from_value(serde_json::json!({
        "id": "test:model:configuration#other"
    }))
    .unwrap();
    configuration.feature_states.insert(
        ir.model.features[0].id.clone(),
        ConfigurationFeatureState::Definition {
            definition: Box::new(definition(
                3.0,
                RadialDimension::Radius(Dimension::Literal(PositiveLength::new(5.0).unwrap())),
            )),
        },
    );
    ir.model.configurations.push(configuration);
    let check = |ir: &CadIr| crate::validate::validate_neutral(ir, vec![]).unwrap();
    assert!(check(&ir)
        .findings
        .iter()
        .any(|finding| finding.severity == Severity::Error
            && finding
                .message
                .contains("changes canonical parameter binding")));

    // The same structural change is valid when it retains the binding and
    // supplies the dimensional difference only in the parameter overlay.
    ir.model.configurations[0].feature_states.insert(
        ir.model.features[0].id.clone(),
        ConfigurationFeatureState::Definition {
            definition: Box::new(definition(
                3.0,
                RadialDimension::Radius(Dimension::Parameter(radius.clone())),
            )),
        },
    );
    let value = ParameterValue::Length(Length::new(5.0).unwrap());
    ir.model.configurations[0]
        .parameter_overrides
        .insert(radius.clone(), value.literal_expression());
    ir.model.configurations[0]
        .parameter_values
        .insert(radius, value);
    let report = check(&ir);
    assert!(report.is_ok(), "{:#?}", report.findings);

    // A different operation genuinely has different controls, even where its
    // serialized field names coincide with the original operation.
    ir.model.configurations[0].feature_states.insert(
        ir.model.features[0].id.clone(),
        ConfigurationFeatureState::Definition {
            definition: Box::new(FeatureDefinition::Operation(
                FeatureOperation::StoredGeometry {},
            )),
        },
    );
    let report = check(&ir);
    assert!(report.is_ok(), "{:#?}", report.findings);
}
