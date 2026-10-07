// SPDX-License-Identifier: Apache-2.0
//! Parameter alias, equation, and configuration-index tests.
#![allow(clippy::unwrap_used)]

use crate::history::configuration::configuration_lane_assignments;
use crate::history::parameters::expression_identifiers;
use crate::history::parameters::parameter_aliases;
use crate::history::parameters::parameters_with_unevaluable_expressions;
use crate::history::parameters::project_parameters;
use crate::history::parameters::ParameterAliases;
use crate::history::project::incomplete_history_reference_features;
use crate::history::project::project_features;
use crate::history::tests::design_configuration;
use crate::history::tests::feature;
use crate::history::tests::feature_input_lane;
use crate::history::tests::native_configuration;
use crate::history::tests::native_with_configuration_lanes;
use crate::history::tests::with_configuration_id;
use crate::history::write::configurations::sync_neutral_configurations;
use crate::history::write::parameters::{
    rewrite_parameter_expression, unquoted_expression_identifier,
};
use crate::records::FeatureContent;
use crate::records::FeatureHistory;
use crate::records::FeatureSource;
use cadmpeg_ir::features::DesignParameter;
use cadmpeg_ir::features::FeatureId;
use cadmpeg_ir::features::ParameterId;
use cadmpeg_ir::features::ParameterValue;
use cadmpeg_ir::scalar::Length;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;

#[test]
fn repeated_aliases_from_one_parameter_remain_unambiguous() {
    let mut owner = feature("owner", Some("1"), 0);
    owner
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Width"), "4mm".into());
    owner.dimension_properties.insert(
        "Width".into(),
        BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("EquationId"),
            "Width".into(),
        )]),
    );
    let parameters = project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }],
    )
    .unwrap();

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
        (
            cadmpeg_core::nonblank_literal!("TXD1"),
            "4X <MOD-DIAM> 12 <HOLE-DEPTH> 40".into(),
        ),
        (
            cadmpeg_core::nonblank_literal!("TXD2"),
            "<MOD-DIAM>4".into(),
        ),
        (cadmpeg_core::nonblank_literal!("D1"), "1 +".into()),
    ]);
    let parameters = project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }],
    )
    .unwrap();
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
        Some(ParameterValue::Length(Length::new(4.0).unwrap()))
    );
    assert_eq!(by_name["D1"].value, None);
    assert_eq!(
        {
            let ctx = cadmpeg_test_support::service_decode_context();
            let (aliases, _storage) =
                ParameterAliases::scoped(&ctx, &parameters, &HashMap::new(), &HashSet::new())
                    .unwrap();
            parameters_with_unevaluable_expressions(&ctx, &parameters, &aliases, &[]).unwrap()
        },
        1
    );
}

#[test]
fn layered_parameter_aliases_match_materialized_precedence() {
    let global_owner = FeatureId::mint("synthetic:test:id#global").expect("identity grammar");
    let local_owner = FeatureId::mint("synthetic:test:id#local").expect("identity grammar");
    let parameters = [
        DesignParameter {
            id: ParameterId::mint("synthetic:test:id#global-id").expect("identity grammar"),
            owner: Some(global_owner.clone()),
            ordinal: 0,
            name: "Width".into(),
            expression: "1".into(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        },
        DesignParameter {
            id: ParameterId::mint("synthetic:test:id#local-id").expect("identity grammar"),
            owner: Some(local_owner.clone()),
            ordinal: 0,
            name: "Width".into(),
            expression: "2".into(),
            display: None,
            value: None,
            dependencies: cadmpeg_ir::features::DistinctMembers::default(),
            properties: BTreeMap::new(),
            pmi: None,
            native_ref: None,
        },
    ];
    let aliases = ParameterAliases::new(
        &cadmpeg_test_support::service_decode_context(),
        &parameters,
        &HashMap::new(),
        &HashSet::from([global_owner]),
    )
    .unwrap();

    for owner in [
        Some(local_owner),
        Some(FeatureId::mint("synthetic:test:id#unrelated").expect("identity grammar")),
        None,
    ] {
        let materialized = aliases.materialize(owner.as_ref());
        let layered = aliases.for_owner(owner.as_ref());
        for alias in ["Width", "global-id", "local-id", "missing"] {
            assert_eq!(
                layered
                    .get(&cadmpeg_test_support::service_decode_context(), alias)
                    .unwrap(),
                materialized.get(alias)
            );
        }
    }
}

#[test]
fn an_empty_quoted_run_is_not_a_parameter_reference() {
    use crate::history::parameters::{
        definite_parameter_reference, expression_identifier_tokens, ExpressionIdentifier,
    };
    let ctx = cadmpeg_test_support::service_decode_context();
    let tokens = expression_identifier_tokens(&ctx, "\"\" + Width")
        .unwrap()
        .0
        .expect("closed quotes");
    assert_eq!(
        tokens
            .iter()
            .map(ExpressionIdentifier::value)
            .collect::<Vec<_>>(),
        ["Width"]
    );
    assert!(!tokens
        .iter()
        .any(|identifier| definite_parameter_reference(&ctx, identifier).unwrap()));

    let named = expression_identifier_tokens(&ctx, "\"D1@Sketch1\"")
        .unwrap()
        .0
        .expect("closed quotes");
    assert_eq!(
        named
            .iter()
            .map(ExpressionIdentifier::value)
            .collect::<Vec<_>>(),
        ["D1@Sketch1"]
    );
    assert!(named
        .iter()
        .all(|identifier| definite_parameter_reference(&ctx, identifier).unwrap()));
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
        (cadmpeg_core::nonblank_literal!("4"), "3mm".into()),
        (cadmpeg_core::nonblank_literal!("Literal"), "4".into()),
        (
            cadmpeg_core::nonblank_literal!("Reference"),
            "\"4\" * 2".into(),
        ),
    ]);
    let parameters = project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }],
    )
    .unwrap();
    let by_name = parameters
        .iter()
        .map(|parameter| (parameter.name.as_str(), parameter))
        .collect::<HashMap<_, _>>();

    assert!(by_name["Literal"].dependencies.is_empty());
    assert_eq!(
        by_name["Reference"].dependencies.as_slice(),
        [by_name["4"].id.clone()]
    );
    assert_eq!(
        by_name["Reference"].value,
        Some(ParameterValue::Length(Length::new(6.0).unwrap()))
    );
    assert!(!unquoted_expression_identifier("4"));
    assert_eq!(
        rewrite_parameter_expression(
            &cadmpeg_test_support::service_decode_context(),
            "Width * 2",
            &HashMap::from([("Width".into(), "4".into())]),
        )
        .unwrap()
        .as_deref(),
        Some("\"4\" * 2")
    );
}

#[test]
fn subtraction_projects_both_parameter_dependencies() {
    let mut owner = feature("owner", Some("1"), 0);
    owner.parameters = BTreeMap::from([
        (cadmpeg_core::nonblank_literal!("A"), "7".into()),
        (cadmpeg_core::nonblank_literal!("B"), "2".into()),
        (cadmpeg_core::nonblank_literal!("C"), "A-B".into()),
    ]);
    let parameters = project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }],
    )
    .unwrap();

    assert_eq!(
        parameters[2].dependencies.as_slice(),
        [parameters[0].id.clone(), parameters[1].id.clone()]
    );
    assert_eq!(parameters[2].value, Some(ParameterValue::Integer(5)));
}

#[test]
fn unqualified_aliases_are_local_to_the_expression_owner() {
    let mut first = feature("first", Some("1"), 0);
    first
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Width"), "4mm".into());
    let mut second = feature("second", Some("2"), 1);
    second
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Width"), "5mm".into());
    let parameters = project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![first, second],
        }],
    )
    .unwrap();

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
        Some(&FeatureId::mint("synthetic:test:id#unrelated").expect("identity grammar")),
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
    equations
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Width"), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer.parameters.insert(
        cadmpeg_core::nonblank_literal!("Result"),
        "Width * 2".into(),
    );

    let parameters = project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![equations, consumer],
        }],
    )
    .unwrap();

    assert_eq!(
        parameters[1].dependencies.as_slice(),
        [parameters[0].id.clone()]
    );
    assert_eq!(
        parameters[1].value,
        Some(ParameterValue::Length(Length::new(8.0).unwrap()))
    );
}

#[test]
fn ordinary_feature_parameters_do_not_leak_globally() {
    let mut source = feature("source", Some("1"), 0);
    source
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Width"), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer.parameters.insert(
        cadmpeg_core::nonblank_literal!("Result"),
        "Width * 2".into(),
    );

    let parameters = project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![source, consumer],
        }],
    )
    .unwrap();

    assert!(parameters[1].dependencies.is_empty());
    assert_eq!(parameters[1].value, None);
}

#[test]
fn local_parameter_precedes_same_named_global() {
    let mut equations = feature("equations", Some("1"), 0);
    equations.kind = "EquationDriven".into();
    equations
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Width"), "4mm".into());
    let mut consumer = feature("consumer", Some("2"), 1);
    consumer.parameters = BTreeMap::from([
        (cadmpeg_core::nonblank_literal!("Width"), "5mm".into()),
        (
            cadmpeg_core::nonblank_literal!("Result"),
            "Width * 2".into(),
        ),
    ]);

    let parameters = project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![equations, consumer],
        }],
    )
    .unwrap();

    assert_eq!(
        parameters[1].dependencies.as_slice(),
        [parameters[2].id.clone()]
    );
    assert_eq!(
        parameters[1].value,
        Some(ParameterValue::Length(Length::new(10.0).unwrap()))
    );
}

#[test]
fn ambiguous_and_missing_history_references_do_not_bind_arbitrarily() {
    let first = feature("first", Some("1"), 0);
    let second = feature("second", Some("1"), 1);
    let mut dependent = feature("dependent", Some("2"), 2);
    dependent
        .properties
        .insert(cadmpeg_core::nonblank_literal!("Dependency"), "1".into());
    let mut malformed = feature("malformed", Some("3"), 3);
    malformed.tree_parent = Some(crate::records::TreeParent::Source(
        FeatureSource::from_value(9_999).expect("test feature source id"),
    ));
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

    let projected = project_features(
        &cadmpeg_test_support::service_decode_context(),
        std::slice::from_ref(&history),
    )
    .unwrap();

    assert!(projected[2].dependencies.is_empty());
    assert_eq!(
        incomplete_history_reference_features(
            &cadmpeg_test_support::service_decode_context(),
            &[history],
        )
        .unwrap(),
        4
    );
}

#[test]
fn incomplete_history_reference_index_refuses_collection_limit() {
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature("first", Some("1"), 0)],
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(b"history", &arena, &policy).unwrap();
    let error = incomplete_history_reference_features(&ctx, &[history])
        .err()
        .unwrap();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

#[test]
fn incomplete_history_reference_scan_refuses_work_limit() {
    let history = FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![feature("first", Some("1"), 0)],
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(b"history", &arena, &policy).unwrap();
    let error = incomplete_history_reference_features(&ctx, &[history])
        .err()
        .unwrap();
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
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
    configuration.active = true;
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
        configuration_lane_assignments(
            &cadmpeg_test_support::service_decode_context(),
            &configurations,
            &lanes
        )
        .unwrap(),
        [(0, 0)]
    );
}

#[test]
fn alias_lookup_refusal_does_not_become_a_missing_alias() {
    let aliases = ParameterAliases {
        global: HashMap::new(),
        exact: HashMap::new(),
        document_local: HashMap::new(),
        feature_local: HashMap::new(),
    };
    let view = aliases.for_owner(None);
    for work_limit in [0, 5] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // Five work units admit one hash lookup of the five-byte alias.
        policy.limits.max_work_units = work_limit;
        let (ctx, _) =
            cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error = view.get(&ctx, "Width").unwrap_err();
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("resource refusal");
        };
        assert_eq!(ctx.resource_refusal(), Some(limit));
    }
}

#[test]
fn alias_update_lookup_refusal_preserves_the_existing_binding() {
    let parameter = ParameterId::mint("test:test:parameter#width").unwrap();
    let mut aliases = HashMap::from([(String::from("Width"), Some(parameter.clone()))]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // One work unit admits the owner step; the five-byte lookup must refuse.
    policy.limits.max_work_units = 1;
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = crate::history::parameters::insert_parameter_alias(
        &ctx,
        &mut aliases,
        String::from("Width"),
        &parameter,
    )
    .unwrap_err();
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("resource refusal");
    };
    assert_eq!(limit.operation, "look up mutable SLDPRT hash key");
    assert_eq!(
        limit.dimension,
        cadmpeg_core::decode::ResourceDimension::WorkUnits
    );
    assert_eq!(limit.additional, 5);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert_eq!(aliases.get("Width"), Some(&Some(parameter)));
}

fn scratch_parameters() -> Vec<DesignParameter> {
    let mut owner = feature("owner", Some("1"), 0);
    owner
        .parameters
        .insert(cadmpeg_core::nonblank_literal!("Note"), "plain text".into());
    project_parameters(
        &cadmpeg_test_support::service_decode_context(),
        &[FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }],
    )
    .unwrap()
}

/// Alias tables are scratch: their copies count against the scoped limit.
#[test]
fn parameter_aliases_refuse_scoped_limit() {
    let parameters = scratch_parameters();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "retain SLDPRT parameter alias",
        |cap| {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_materialized_bytes = cap;
            let (ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            ParameterAliases::scoped(&ctx, &parameters, &HashMap::new(), &HashSet::new())
                .map(|_| ())
        },
    );
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(_)));
}

/// Read-only validation borrows stated values instead of copying their text.
#[test]
fn parameter_value_states_borrow_stated_values() {
    let parameters = scratch_parameters();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (aliases, _storage) = ParameterAliases::scoped(&ctx, &parameters, &HashMap::new(), &HashSet::new()).unwrap();
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        "retain SLDPRT parameter value text", None,
    );
    assert_eq!(parameters_with_unevaluable_expressions(&ctx, &parameters, &aliases, &[]).unwrap(), 0);
    assert!(ctx.resource_refusal().is_none());
}

#[test]
fn reverse_parameter_dependency_chain_evaluates_all_values() {
    let mut owner = feature("chain", Some("1"), 0);
    for index in 0..40 {
        let name = cadmpeg_core::text::NonBlankString::try_from(format!("D{index:02}")).unwrap();
        let expression = if index == 39 {
            "1".to_owned()
        } else {
            format!("D{:02}+1", index + 1)
        };
        owner.parameters.insert(name, expression);
    }
    let histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }];
    let parameters =
        project_parameters(&cadmpeg_test_support::service_decode_context(), &histories).unwrap();
    for (index, parameter) in parameters.iter().enumerate() {
        assert_eq!(
            parameter.value,
            Some(ParameterValue::Integer(40 - i64::try_from(index).unwrap()))
        );
        assert_eq!(parameter.ordinal, 39 - u32::try_from(index).unwrap());
    }
}

#[test]
fn repeated_parameter_ids_keep_source_pass_replacements() {
    let mut first = scratch_parameters().remove(0);
    first.name = "First".into();
    first.expression = "Known + 1".into();
    first.value = None;
    let mut known = first.clone();
    known.name = "Known".into();
    known.expression = "1".into();
    known.value = Some(ParameterValue::Integer(1));
    let mut consumer = first.clone();
    consumer.id = ParameterId::mint("synthetic:test:parameter#consumer").unwrap();
    consumer.name = "Consumer".into();
    consumer.expression = "Known + 1".into();
    let mut parameters = [first, known, consumer];
    super::evaluate_parameter_expressions(
        &cadmpeg_test_support::service_decode_context(),
        &mut parameters,
        &HashMap::new(),
        &HashSet::new(),
    )
    .unwrap();
    assert_eq!(parameters[0].value, Some(ParameterValue::Integer(2)));
    assert_eq!(parameters[1].value, Some(ParameterValue::Integer(1)));
    assert_eq!(parameters[2].value, Some(ParameterValue::Integer(3)));
}

#[test]
fn parameter_retries_follow_numeric_and_non_ascii_aliases() {
    for (consumer, producer) in [("0Consumer", "9Alias"), ("D01", "ä")] {
        let mut owner = feature("aliases", Some("1"), 0);
        for (name, expression) in [
            (consumer, format!("{producer} + 1")),
            (producer, "D00 + 1".to_owned()),
            ("D00", "1".to_owned()),
        ] {
            owner.parameters.insert(
                cadmpeg_core::text::NonBlankString::try_from(name).unwrap(),
                expression,
            );
        }
        let histories = [FeatureHistory {
            id: "history".into(),
            part_name: None,
            properties: BTreeMap::new(),
            content: Vec::new(),
            configurations: Vec::new(),
            features: vec![owner],
        }];
        let parameters =
            project_parameters(&cadmpeg_test_support::service_decode_context(), &histories)
                .unwrap();
        let value = |name| {
            parameters
                .iter()
                .find(|parameter| parameter.name == name)
                .unwrap()
                .value
                .as_ref()
                .unwrap()
        };
        assert_eq!(value(consumer), &ParameterValue::Integer(3));
        assert_eq!(value(producer), &ParameterValue::Integer(2));
        assert_eq!(value("D00"), &ParameterValue::Integer(1));
    }
}

#[test]
fn display_modifier_aliases_do_not_block_expression_evaluation() {
    let mut owner = feature("modifiers", Some("1"), 0);
    owner.parameters = BTreeMap::from([
        (
            cadmpeg_core::nonblank_literal!("D1"),
            "(<MOD-DIAM>12mm) + 1mm".into(),
        ),
        (cadmpeg_core::nonblank_literal!("MOD"), "MOD + 1".into()),
    ]);
    let histories = [FeatureHistory {
        id: "history".into(),
        part_name: None,
        properties: BTreeMap::new(),
        content: Vec::new(),
        configurations: Vec::new(),
        features: vec![owner],
    }];
    let parameters =
        project_parameters(&cadmpeg_test_support::service_decode_context(), &histories).unwrap();
    assert_eq!(
        parameters[0].value,
        Some(ParameterValue::Length(Length::new(13.0).unwrap()))
    );
    assert_eq!(parameters[1].value, None);
}
