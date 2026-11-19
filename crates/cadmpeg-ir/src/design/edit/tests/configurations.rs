// SPDX-License-Identifier: Apache-2.0
//! A parameter's cache in a configuration, and the edits that recompute it
//! (§5.13).

use std::collections::{BTreeMap, BTreeSet};

use super::{
    after, cache, configuration, definition, feature, id, length, length_override, measure, mm,
    neutral, parameter_id, real, reference, timeline,
};
use crate::design::configuration::{
    DesignConfiguration, FeatureStateOverride, Overrides, ParameterOverride,
};
use crate::design::dimension::{Dimension, ParameterRef};
use crate::design::edit::Literal;
use crate::design::parameter::{ParameterDefinition, ParameterValue};
use crate::design::selection::{OccurrencePath, PathStep, StepRef};
use crate::features::{FeatureId, ParameterId};
use crate::scalar::Length;
use crate::CadIr;

fn configuration_with(
    key: u32,
    parameters: BTreeMap<ParameterId, ParameterOverride>,
    features: BTreeMap<FeatureId, FeatureStateOverride>,
) -> DesignConfiguration {
    DesignConfiguration {
        id: configuration(key),
        scope: definition(),
        name: format!("C{key}"),
        parent: None,
        comment: None,
        overrides: Overrides {
            parameters,
            features,
            ..Overrides::default()
        },
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    }
}

/// Base p1 = 10 mm, p2 = p1 + 1 mm, p3 = p2 * 2; configuration 1 overrides
/// p1 with 20 mm and p3 with p2 * 3, whose cache there is 63 mm. In
/// configuration 1, p2 has no cache: its base cache of 11 mm reads p1, which
/// the configuration overrides (§5.13).
#[test]
fn an_override_reads_no_base_cache_its_configuration_changes() {
    let mut document = timeline();
    document.design.parameters = vec![
        length(1, neutral("10[mm]", &[], &[]), Some(10.0)),
        length(2, neutral("$0 + 1[mm]", &[reference(1)], &[]), Some(11.0)),
        length(3, neutral("$0 * 2", &[reference(2)], &[]), Some(22.0)),
    ];
    document.design.configurations.push(configuration_with(
        1,
        BTreeMap::from([
            (parameter_id(1), length_override("20[mm]", &[], 20.0)),
            (
                parameter_id(3),
                length_override("$0 * 3", &[reference(2)], 63.0),
            ),
        ]),
        BTreeMap::new(),
    ));
    let in_one = |document: &CadIr, key: u32| {
        document
            .parameter_cache(&parameter_id(key), Some(&configuration(1)))
            .unwrap()
            .cloned()
    };
    assert_eq!(in_one(&document, 2), None);
    assert_eq!(
        in_one(&document, 3),
        Some(ParameterValue::Number(real(63.0)))
    );
    assert_eq!(
        document.parameter_cache(&parameter_id(2), None),
        Ok(Some(&ParameterValue::Number(real(11.0))))
    );

    document
        .set_dimension(
            &parameter_id(1),
            Literal::Number {
                value: real(12.0),
                unit: mm(),
            },
        )
        .unwrap();

    assert_eq!(
        cache(&document, 2),
        Some(ParameterValue::Number(real(13.0)))
    );
    assert_eq!(
        cache(&document, 3),
        Some(ParameterValue::Number(real(26.0)))
    );
    let overrides = &document.design.configurations[0].overrides.parameters;
    assert_eq!(
        overrides[&parameter_id(1)].value,
        Some(ParameterValue::Number(real(20.0)))
    );
    assert_eq!(
        overrides[&parameter_id(3)].value,
        None,
        "p2 has no cache in configuration 1, so p3's override is cleared, not 39 mm"
    );
    assert!(overrides[&parameter_id(3)].expression.is_some());
}

/// A parameter whose definition reads a measure keeps its base cache only in
/// the base and in a configuration that states nothing (§5.13).
#[test]
fn a_measured_cache_holds_only_where_nothing_differs() {
    let mut document = timeline();
    document.design.parameters.push(length(
        3,
        neutral("@0 * 2", &[], &[measure(after(2), 4.0)]),
        Some(8.0),
    ));
    document.design.parameters.push(length(
        4,
        neutral("$0 + 1[mm]", &[reference(3)], &[]),
        Some(9.0),
    ));
    document
        .design
        .configurations
        .push(configuration_with(1, BTreeMap::new(), BTreeMap::new()));
    document.design.configurations.push(configuration_with(
        2,
        BTreeMap::new(),
        BTreeMap::from([(
            feature(3),
            FeatureStateOverride {
                suppressed: Some(true),
                definition: None,
            },
        )]),
    ));
    for key in [3, 4] {
        let base = document.parameter_cache(&parameter_id(key), None).unwrap();
        assert!(base.is_some(), "p{key} in the base");
        assert_eq!(
            document.parameter_cache(&parameter_id(key), Some(&configuration(1))),
            Ok(base),
            "p{key} where nothing differs"
        );
        assert_eq!(
            document.parameter_cache(&parameter_id(key), Some(&configuration(2))),
            Ok(None),
            "p{key} where a feature is suppressed"
        );
    }
    document.design.scopes[0].active_configuration = Some(configuration(2));
    assert_eq!(
        document.dimension_value(&Dimension::<Length>::Parameter(parameter_id(4))),
        Ok(None)
    );
    document.design.parameters[1].definition = ParameterDefinition::Driven;
    assert_eq!(
        document.parameter_cache(&parameter_id(2), Some(&configuration(2))),
        Ok(None),
        "a driven parameter's cache follows the geometry"
    );
}

/// In a configuration that states something, a parameter's own value holds
/// unless what it rests on can differ there (§5.13): a `Source` expression
/// or program does not let it hold, an `Assigned` parameter reads what its
/// `Neutral` program reads, and an `External` reference stops it only where
/// an occurrence override is stated.
#[test]
fn an_own_value_holds_where_nothing_it_rests_on_differs() {
    use cadmpeg_core::text::NonBlankString;

    use crate::design::configuration::OccurrenceOverride;
    use crate::design::dimension::RefTarget;
    use crate::design::expr::{parse, Expression, ExpressionDialect, ExpressionToken, Neutral};
    use crate::design::program::{ParameterProgram, ProgramBody, Statement};

    let mut document = timeline();
    let program = |key: u32, body: ProgramBody| ParameterProgram {
        id: id("program", key),
        scope: definition(),
        owner: None,
        stage: None,
        name: None,
        body,
        native_ref: None,
    };
    let assigned = |key: u32, program: u32| crate::design::parameter::DesignParameter {
        definition: ParameterDefinition::Assigned(id("program", program)),
        ..length(key, neutral("0[mm]", &[], &[]), Some(10.0))
    };
    let neutral_of = |text: &str, refs: &[ParameterRef]| Neutral(parse(text, refs, &[]).unwrap());
    let assign = |key: u32, text: &str| Statement::Assign {
        target: reference(key),
        value: neutral_of(text, &[reference(1)]),
    };
    document.design.programs = vec![
        program(
            1,
            ProgramBody::Neutral(vec![Statement::If {
                condition: neutral_of("true", &[]),
                then: vec![assign(4, "$0")],
                otherwise: Vec::new(),
            }]),
        ),
        program(
            2,
            ProgramBody::Source {
                dialect: ExpressionDialect::Creo,
                tokens: vec![ExpressionToken::Text("p5 = 10".into())],
            },
        ),
    ];
    let external = ParameterRef {
        target: RefTarget::External {
            definition: id("product-definition", 2),
            name: NonBlankString::new("width").unwrap(),
        },
        through: None,
    };
    let source = Expression::Source {
        dialect: ExpressionDialect::Creo,
        tokens: vec![ExpressionToken::Text("5".into())],
    };
    document.design.parameters.extend([
        length(3, source, Some(5.0)),
        assigned(4, 1),
        assigned(5, 2),
        length(6, neutral("$0 * 2", &[external], &[]), Some(4.0)),
    ]);
    let suppressing = BTreeMap::from([(
        feature(3),
        FeatureStateOverride {
            suppressed: Some(true),
            definition: None,
        },
    )]);
    document
        .design
        .configurations
        .push(configuration_with(2, BTreeMap::new(), suppressing));
    let mut occurrence = configuration_with(3, BTreeMap::new(), BTreeMap::new());
    occurrence.overrides.occurrences.insert(
        id("occurrence", 1),
        OccurrenceOverride {
            suppressed: Some(true),
            ..OccurrenceOverride::default()
        },
    );
    document.design.configurations.push(occurrence);
    let holds = |key: u32, configuration_key: u32| {
        document
            .parameter_cache(&parameter_id(key), Some(&configuration(configuration_key)))
            .unwrap()
            .is_some()
    };
    assert!(!holds(3, 2), "a Source expression");
    assert!(
        holds(4, 2),
        "a Neutral program that reads nothing overridden"
    );
    assert!(!holds(5, 2), "a Source program");
    assert!(
        holds(6, 2),
        "an External reference with no occurrence override"
    );
    assert!(
        !holds(6, 3),
        "an External reference under an occurrence override"
    );
}

/// A reference `through` an occurrence path has no value in an edit, which
/// clears what reads it rather than reading the target's base cache.
#[test]
fn an_edit_clears_a_cache_that_reads_through_an_occurrence() {
    let mut document = timeline();
    let through = ParameterRef {
        through: Some(OccurrencePath(vec![PathStep {
            occurrence: StepRef::Occurrence(id("occurrence", 1)),
            configuration: None,
        }])),
        ..reference(1)
    };
    document
        .design
        .parameters
        .push(length(3, neutral("$0 * 3", &[through], &[]), Some(30.0)));
    document
        .set_dimension(
            &parameter_id(1),
            Literal::Number {
                value: real(12.0),
                unit: mm(),
            },
        )
        .unwrap();
    assert_eq!(
        cache(&document, 2),
        Some(ParameterValue::Number(real(24.0)))
    );
    assert_eq!(cache(&document, 3), None);
}
