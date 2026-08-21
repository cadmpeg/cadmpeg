// SPDX-License-Identifier: Apache-2.0
//! Parameter alias, equation, and configuration-index tests.
#![allow(clippy::unwrap_used)]

use super::super::*;
use super::*;

#[test]
fn repeated_aliases_from_one_parameter_remain_unambiguous() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters.insert("Width".into(), "4mm".into());
    owner.dimension_properties.insert(
        "Width".into(),
        BTreeMap::from([("EquationId".into(), "Width".into())]),
    );
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }]);

    let aliases = parameter_aliases(
        &parameters,
        &HashMap::new(),
        &HashSet::new(),
        parameters[0].owner.as_ref(),
    );

    assert_eq!(aliases.get("Width"), Some(&Some(parameters[0].id.clone())));
}

#[test]
fn project_parameters_preserves_composite_txd_text_without_hiding_bad_equations() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters = BTreeMap::from([
        ("TXD1".into(), "4X <MOD-DIAM> 12 <HOLE-DEPTH> 40".into()),
        ("TXD2".into(), "<MOD-DIAM>4".into()),
        ("D1".into(), "1 +".into()),
    ]);
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }]);
    let by_name = parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter))
        .collect::<HashMap<_, _>>();

    assert_eq!(
        by_name["TXD1"].value,
        Some(ParameterValue::String(
            "4X <MOD-DIAM> 12 <HOLE-DEPTH> 40".into()
        ))
    );
    assert_eq!(
        by_name["TXD2"].value,
        Some(ParameterValue::Length(Length(4.0)))
    );
    assert_eq!(by_name["D1"].value, None);
    assert_eq!(
        parameters_with_unevaluable_expressions(&parameters, &HashMap::new(), &HashSet::new(), &[],),
        1
    );
}

#[test]
fn layered_parameter_aliases_match_materialized_precedence() {
    let global_owner = FeatureId("global".into());
    let local_owner = FeatureId("local".into());
    let parameters = [
        DesignParameter {
            id: ParameterId("global-id".into()),
            owner: Some(global_owner.clone()),
            ordinal: 0,
            name: "Width".into(),
            expression: "1".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        },
        DesignParameter {
            id: ParameterId("local-id".into()),
            owner: Some(local_owner.clone()),
            ordinal: 0,
            name: "Width".into(),
            expression: "2".into(),
            display: None,
            value: None,
            dependencies: Vec::new(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        },
    ];
    let aliases =
        ParameterAliases::new(&parameters, &HashMap::new(), &HashSet::from([global_owner]));

    for owner in [Some(local_owner), Some(FeatureId("unrelated".into())), None] {
        let materialized = aliases.materialize(owner.as_ref());
        let layered = aliases.for_owner(owner.as_ref());
        for alias in ["Width", "global-id", "local-id", "missing"] {
            assert_eq!(layered.get(alias), materialized.get(alias));
        }
    }
}

#[test]
fn subtraction_separates_unquoted_parameter_references() {
    assert_eq!(
        expression_identifiers("D1@Sketch1-D2@Sketch1").collect::<Vec<_>>(),
        ["D1@Sketch1", "D2@Sketch1"]
    );
}

#[test]
fn numeric_literals_do_not_bind_numeric_parameter_names() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters = BTreeMap::from([
        ("4".into(), "3mm".into()),
        ("Literal".into(), "4".into()),
        ("Reference".into(), "\"4\" * 2".into()),
    ]);
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }]);
    let by_name = parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter))
        .collect::<HashMap<_, _>>();

    assert!(by_name["Literal"].dependencies.is_empty());
    assert_eq!(by_name["Reference"].dependencies, [by_name["4"].id.clone()]);
    assert_eq!(
        by_name["Reference"].value,
        Some(ParameterValue::Length(Length(6.0)))
    );
    assert!(!unquoted_expression_identifier("4"));
    assert_eq!(
        rewrite_parameter_expression("Width * 2", &HashMap::from([("Width".into(), "4".into())]),)
            .as_deref(),
        Some("\"4\" * 2")
    );
}

#[test]
fn subtraction_projects_both_parameter_dependencies() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters = BTreeMap::from([
        ("A".into(), "7".into()),
        ("B".into(), "2".into()),
        ("C".into(), "A-B".into()),
    ]);
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }]);

    assert_eq!(
        parameters[2].dependencies,
        [parameters[0].id.clone(), parameters[1].id.clone()]
    );
    assert_eq!(parameters[2].value, Some(ParameterValue::Integer(5)));
}

#[test]
fn unqualified_aliases_are_local_to_the_expression_owner() {
    let mut first = feature("first", Some("1"), 0);
    first.parameters.insert("Width".into(), "4mm".into());
    let mut second = feature("second", Some("2"), 1);
    second.parameters.insert("Width".into(), "5mm".into());
    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second],
    }]);

    let first_aliases = parameter_aliases(
        &parameters,
        &HashMap::new(),
        &HashSet::new(),
        parameters[0].owner.as_ref(),
    );
    let second_aliases = parameter_aliases(
        &parameters,
        &HashMap::new(),
        &HashSet::new(),
        parameters[1].owner.as_ref(),
    );
    let unrelated_aliases = parameter_aliases(
        &parameters,
        &HashMap::new(),
        &HashSet::new(),
        Some(&FeatureId("unrelated".into())),
    );

    assert_eq!(
        first_aliases.get("Width"),
        Some(&Some(parameters[0].id.clone()))
    );
    assert_eq!(
        second_aliases.get("Width"),
        Some(&Some(parameters[1].id.clone()))
    );
    assert_eq!(unrelated_aliases.get("Width"), None);
}

#[test]
fn equation_driven_parameters_are_global() {
    let mut equations = feature("equations", Some("1"), 0);
    equations.kind = "EquationDriven".into();
    equations.parameters.insert("Width".into(), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer
        .parameters
        .insert("Result".into(), "Width * 2".into());

    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![equations, consumer],
    }]);

    assert_eq!(parameters[1].dependencies, [parameters[0].id.clone()]);
    assert_eq!(
        parameters[1].value,
        Some(ParameterValue::Length(Length(8.0)))
    );
}

#[test]
fn ordinary_feature_parameters_do_not_leak_globally() {
    let mut source = feature("source", Some("1"), 0);
    source.parameters.insert("Width".into(), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer
        .parameters
        .insert("Result".into(), "Width * 2".into());

    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![source, consumer],
    }]);

    assert!(parameters[1].dependencies.is_empty());
    assert_eq!(parameters[1].value, None);
}

#[test]
fn local_parameter_precedes_same_named_global() {
    let mut equations = feature("equations", Some("1"), 0);
    equations.kind = "EquationDriven".into();
    equations.parameters.insert("Width".into(), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer.parameters = BTreeMap::from([
        ("Width".into(), "5mm".into()),
        ("Result".into(), "Width * 2".into()),
    ]);

    let parameters = project_parameters(&[FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![equations, consumer],
    }]);

    assert_eq!(parameters[1].dependencies, [parameters[2].id.clone()]);
    assert_eq!(
        parameters[1].value,
        Some(ParameterValue::Length(Length(10.0)))
    );
}

#[test]
fn ambiguous_and_missing_history_references_do_not_bind_arbitrarily() {
    let first = feature("first", Some("1"), 0);
    let second = feature("second", Some("1"), 1);
    let mut dependent = feature("dependent", Some("2"), 2);
    dependent.properties.insert("Dependency".into(), "1".into());
    let mut malformed = feature("malformed", Some("3"), 3);
    malformed.parent_source_id = Some("missing".into());
    malformed
        .content
        .push(FeatureContent::Feature("missing-child".into()));
    malformed
        .content
        .push(FeatureContent::Dimension("D1".into()));
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![first, second, dependent, malformed],
    };

    let projected = project_features(std::slice::from_ref(&history));

    assert!(projected[2].dependencies.is_empty());
    assert_eq!(incomplete_history_reference_features(&[history]), 4);
}

#[test]
fn assigning_configuration_index_does_not_capture_global_input_lane() {
    let mut native = native_with_configuration_lanes(
        vec![native_configuration("native-configuration", 0, None)],
        vec![feature_input_lane("global-lane", None)],
    )
    .into();
    let mut configuration =
        design_configuration("configuration", 0, Some(0), Some("native-configuration"));
    configuration.active = true.into();
    sync_neutral_configurations(&[configuration], &mut native);

    let native = native.expect("required invariant");
    assert_eq!(
        native.feature_histories[0].configurations[0].source_index,
        Some(0)
    );
    assert_eq!(native.feature_input_lanes[0].configuration, None);
}

#[test]
fn stored_configuration_id_precedes_ordinal_fallback() {
    let configurations = [
        with_configuration_id(design_configuration("explicit", 0, Some(7), None), 1),
        design_configuration("fallback", 1, None, None),
    ];
    let lanes = [feature_input_lane("lane", Some("1"))];

    assert_eq!(
        configuration_lane_assignments(&configurations, &lanes),
        [(0, 0)]
    );
}
