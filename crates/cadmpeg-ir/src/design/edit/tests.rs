// SPDX-License-Identifier: Apache-2.0
//! The resolvers and edit helpers on small documents.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use cadmpeg_core::text::NonBlankString;
use serde_json::json;

use super::eval::{check_types, Type};
use super::wire::{exchange_constructions, retarget};
use super::{evaluate, EditError, EvalError, Literal, ParameterDrivers, Value};
use crate::design::configuration::{
    Arrangement, ArrangementKind, ArrangementRef, DesignConfiguration, FeatureStateOverride,
    Overrides, ParameterOverride,
};
use crate::design::construction::{Construction, ConstructionInput, InputRole, Relation, Subject};
use crate::design::container::{Container, Folder, OrderedKind, Rollback, UnorderedKind};
use crate::design::dimension::{Dimension, ParameterRef, RefTarget};
use crate::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use crate::design::expr::{
    parse, Expression, ExpressionDialect, ExpressionToken, Measure, MeasureKind, MeasureTarget,
    Neutral, Node,
};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation, OperationFamily};
use crate::design::names::NameEntry;
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::options::{NativeOption, NativeOptions};
use crate::design::parameter::{
    DesignParameter, Deviations, FitDeviations, ParameterDefinition, ParameterOwner,
    ParameterValue, Tolerance,
};
use crate::design::refs::{ConfigurationRef, EntityRef, Evaluated, Member, Position};
use crate::design::scope::{DesignScope, OccurrenceDesign};
use crate::design::selection::{Anchor, Picks, Selection};
use crate::design::units::{Quantity, Unit, UnitFactor, UnitSymbol, ValueKind};
use crate::features::{ConfigurationId, FeatureId, FinitePoint3, ParameterId};
use crate::ids::{BodyId, OccurrenceId, ProductDefinitionId};
use crate::pmi::LimitsAndFits;
use crate::scalar::{FiniteReal, Length};
use crate::topology::{Body, BodyKind, Region};
use crate::CadIr;

fn id<T>(kind: &str, key: u32) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:edit:{kind}#{key}")).unwrap()
}

fn feature(key: u32) -> FeatureId {
    id("feature", key)
}

fn parameter_id(key: u32) -> ParameterId {
    id("parameter", key)
}

fn configuration(key: u32) -> ConfigurationId {
    id("configuration", key)
}

fn definition() -> ProductDefinitionId {
    id("product-definition", 1)
}

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

fn mm() -> Unit {
    Unit(vec![UnitFactor {
        symbol: UnitSymbol::Mm,
        power: std::num::NonZeroI8::new(1).unwrap(),
    }])
}

fn reference(key: u32) -> ParameterRef {
    ParameterRef {
        target: RefTarget::Parameter(parameter_id(key)),
        through: None,
    }
}

fn neutral(text: &str, refs: &[ParameterRef], measures: &[Measure]) -> Expression {
    Expression::Neutral(Neutral(parse(text, refs, measures).unwrap()))
}

fn value_of(text: &str) -> Result<Value, EvalError> {
    let node: Node<ParameterRef> = parse(text, &[], &[]).unwrap();
    evaluate(&node, &mut |_| None)
}

fn number(text: &str) -> (f64, Quantity) {
    match value_of(text).unwrap() {
        Value::Number { value, quantity } => (value, quantity),
        other => panic!("{text} is {other:?}"),
    }
}

fn operation(key: u32, operation: FeatureOperation) -> Feature {
    Feature {
        id: feature(key),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation,
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

fn unresolved(key: u32) -> Feature {
    operation(
        key,
        FeatureOperation::Unresolved {
            family: OperationFamily::Extrude,
        },
    )
}

/// A native operation bound to a parameter.
fn bound(key: u32, parameter: u32) -> Feature {
    operation(
        key,
        FeatureOperation::Native {
            kind: NonBlankString::new("test:op").unwrap(),
            native_options: NativeOptions::from([(
                NonBlankString::new("test:depth").unwrap(),
                NativeOption::Length(Dimension::Parameter(parameter_id(parameter))),
            )]),
            source_settings: NativeOptions::new(),
        },
    )
}

fn parameter(
    key: u32,
    kind: ValueKind,
    definition: ParameterDefinition,
    value: Option<ParameterValue>,
) -> DesignParameter {
    DesignParameter {
        id: parameter_id(key),
        scope: definition_id(),
        owner: None,
        name: Some(NonBlankString::new(format!("p{key}")).unwrap()),
        kind,
        unit: None,
        definition,
        value,
        display: None,
        tolerance: None,
        decimals: None,
        comment: None,
        native_options: NativeOptions::new(),
        source_settings: NativeOptions::new(),
        native_ref: None,
    }
}

fn definition_id() -> ProductDefinitionId {
    definition()
}

fn length(key: u32, expression: Expression, value: Option<f64>) -> DesignParameter {
    parameter(
        key,
        ValueKind::Quantity(Quantity::LENGTH),
        ParameterDefinition::Expression(expression),
        value.map(|value| ParameterValue::Number(real(value))),
    )
}

fn measure(at: Position, cached: f64) -> Measure {
    Measure {
        kind: MeasureKind::Length,
        of: MeasureTarget::Selection(Selection {
            at: Anchor::Pinned(at),
            picks: Picks::Unread,
            native: None,
        }),
        cached: Some(real(cached)),
    }
}

fn after(key: u32) -> Position {
    Position::After(Member::Feature(feature(key)))
}

fn evaluation(
    key: u32,
    of: Evaluated,
    configuration: Option<ConfigurationId>,
    content: Option<StateContent>,
) -> Evaluation {
    Evaluation {
        id: id("evaluation", key),
        scope: definition(),
        configuration,
        of,
        status: EvaluationStatus::Current,
        content,
        native_ref: None,
    }
}

fn body(key: u32) -> Body {
    Body {
        id: id("body", key),
        kind: BodyKind::Solid,
        transform: None,
        name: None,
        color: None,
        visible: None,
        native_ref: None,
    }
}

fn with_bodies(keys: &[u32]) -> StateContent {
    let mut delta = StateDelta::default();
    delta.records.bodies = keys.iter().map(|key| body(*key)).collect();
    StateContent {
        base: StateBase::Empty,
        delta,
    }
}

fn product_definition(bodies: &[u32]) -> crate::products::ProductDefinition {
    serde_json::from_value(json!({
        "id": definition().as_str(),
        "kind": "part",
        "bom_properties": {},
        "bodies": bodies.iter().map(|key| format!("test:edit:body#{key}")).collect::<Vec<_>>(),
        "sketches": [],
        "tessellations": [],
    }))
    .unwrap()
}

/// One scope whose timeline holds features 1 to 3: 1 a sketch-like stateless
/// datum stand-in, 2 bound to parameter 2, 3 unresolved. Parameter 1 is a
/// literal 10 mm and parameter 2 is twice parameter 1.
fn timeline() -> CadIr {
    let mut document = CadIr::empty();
    let container: crate::design::ids::ContainerId = id("container", 1);
    document.design.containers.push(Container::Ordered {
        id: container.clone(),
        kind: OrderedKind::Timeline,
        members: (1..=3).map(|key| Member::Feature(feature(key))).collect(),
        folders: vec![Folder {
            name: None,
            first: 1,
            last: 2,
        }],
        rollback: Rollback::Before(Member::Feature(feature(2))),
        placement: None,
    });
    document.design.scopes.push(DesignScope {
        definition: definition(),
        roots: vec![container],
        active_configuration: None,
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: after(3),
            bodies: Vec::new(),
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
    });
    document.design.features = vec![
        operation(1, FeatureOperation::Spreadsheet(id("spreadsheet", 1))),
        bound(2, 2),
        unresolved(3),
    ];
    document.design.parameters = vec![
        length(1, neutral("10[mm]", &[], &[]), Some(10.0)),
        length(2, neutral("$0 * 2", &[reference(1)], &[]), Some(20.0)),
    ];
    document.design.evaluations = vec![
        evaluation(1, Evaluated::State(after(1)), None, None),
        evaluation(2, Evaluated::State(after(2)), None, Some(with_bodies(&[2]))),
        evaluation(3, Evaluated::Tool(feature(2)), None, None),
    ];
    for evaluation in &mut document.design.evaluations {
        if evaluation.content.is_none() {
            evaluation.status = EvaluationStatus::Failed;
        }
    }
    document
}

fn status(document: &CadIr, key: u32) -> EvaluationStatus {
    document
        .design
        .evaluations
        .iter()
        .find(|evaluation| evaluation.id == id("evaluation", key))
        .unwrap()
        .status
}

fn cache(document: &CadIr, key: u32) -> Option<ParameterValue> {
    document
        .design_parameter(&parameter_id(key))
        .unwrap()
        .value
        .clone()
}

#[test]
fn literals_convert_to_canonical_units() {
    assert_eq!(number("10[in]"), (254.0, Quantity::LENGTH));
    assert_eq!(number("2 * 3[mm] + 1[cm]"), (16.0, Quantity::LENGTH));
    let (area, quantity) = number("(2[mm])^2");
    assert_eq!((area, quantity), (4.0, Quantity::AREA));
    let (speed, quantity) = number("60[rev/min]");
    assert!((speed - 2.0 * std::f64::consts::PI).abs() < f64::EPSILON * 8.0);
    assert_eq!(
        quantity,
        Quantity {
            length: 0,
            mass: 0,
            angle: 1,
            time: -1
        }
    );
    let (half, quantity) = number("sin(30[deg])");
    assert!((half - 0.5).abs() < f64::EPSILON * 4.0);
    assert_eq!(quantity, Quantity::REAL);
}

#[test]
fn operators_check_their_operands() {
    assert!(matches!(value_of("sin(1)"), Err(EvalError::Type(_))));
    assert!(matches!(
        value_of("1[mm] + 1[deg]"),
        Err(EvalError::Type(_))
    ));
    assert!(matches!(
        value_of("(2[mm])^(1 / 3)"),
        Err(EvalError::Type(_))
    ));
    assert!(matches!(value_of("1 / 0"), Err(EvalError::Domain(_))));
    assert!(matches!(value_of("sqrt(-1)"), Err(EvalError::Domain(_))));
    assert!(matches!(value_of("mod(1, 0)"), Err(EvalError::Domain(_))));
    assert!(matches!(value_of("t"), Err(EvalError::Unbound(_))));
    assert_eq!(
        value_of("if(true, 1, 1 / 0)"),
        Ok(Value::Number {
            value: 1.0,
            quantity: Quantity::REAL
        })
    );
    assert_eq!(number("mod(-7, 3)").0, -1.0);
    assert_eq!(number("-2^2").0, -4.0);
    assert_eq!(number("2^3^2").0, 512.0);
    assert_eq!(number("round(-2.5)").0, -3.0);
    assert_eq!(value_of("\"a\" == \"a\""), Ok(Value::Boolean(true)));
    assert!(matches!(value_of("\"a\" < \"b\""), Err(EvalError::Type(_))));
}

#[test]
fn a_deep_tree_evaluates_on_a_small_thread() {
    let mut node: Node<ParameterRef> = parse("1[mm]", &[], &[]).unwrap();
    for _ in 0..100_000 {
        node = Node::Unary {
            op: crate::design::expr::UnaryOp::Negate,
            operand: Box::new(node),
        };
    }
    let handle = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || {
            let checked = check_types(&node, &|_| Ok(Type::Unknown));
            let result = evaluate(&node, &mut |_| None);
            drop(node);
            (checked, result)
        })
        .unwrap();
    assert_eq!(
        handle.join().unwrap(),
        (
            Ok(Type::Number(Quantity::LENGTH)),
            Ok(Value::Number {
                value: 1.0,
                quantity: Quantity::LENGTH
            })
        )
    );
}

#[test]
fn a_dimension_resolves_through_the_active_configuration() {
    let mut document = timeline();
    let literal = Dimension::<Length>::Literal(Length::new(3.0).unwrap());
    assert_eq!(
        document.dimension_value(&literal),
        Ok(Some(Length::new(3.0).unwrap()))
    );
    let bound = Dimension::<Length>::Parameter(parameter_id(2));
    assert_eq!(
        document.dimension_value(&bound),
        Ok(Some(Length::new(20.0).unwrap()))
    );
    let negated = Dimension::<Length>::Negated(parameter_id(2));
    assert_eq!(
        document.dimension_value(&negated),
        Ok(Some(Length::new(-20.0).unwrap()))
    );

    document.design.configurations.push(DesignConfiguration {
        id: configuration(1),
        scope: definition(),
        name: "Long".to_owned(),
        parent: None,
        comment: None,
        overrides: Overrides {
            parameters: BTreeMap::from([(
                parameter_id(2),
                ParameterOverride {
                    expression: None,
                    value: Some(ParameterValue::Number(real(50.0))),
                },
            )]),
            ..Overrides::default()
        },
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    });
    document.design.scopes[0].active_configuration = Some(configuration(1));
    assert_eq!(
        document.dimension_value(&bound),
        Ok(Some(Length::new(50.0).unwrap()))
    );

    document.design.parameters[0].value = None;
    let unbound = Dimension::<Length>::Parameter(parameter_id(1));
    assert_eq!(document.dimension_value(&unbound), Ok(None));
}

#[test]
fn an_option_enum_binds_by_its_wire_name() {
    let mut document = timeline();
    document.design.parameters.push(parameter(
        3,
        ValueKind::Text,
        ParameterDefinition::Expression(neutral("\"maximum\"", &[], &[])),
        Some(ParameterValue::Text("maximum".to_owned())),
    ));
    document.design.parameters.push(parameter(
        4,
        ValueKind::Boolean,
        ParameterDefinition::Expression(neutral("true", &[], &[])),
        Some(ParameterValue::Boolean(true)),
    ));
    let side = Dimension::<crate::design::parameter::LimitSide>::Parameter(parameter_id(3));
    assert_eq!(
        document.dimension_value(&side),
        Ok(Some(crate::design::parameter::LimitSide::Maximum))
    );
    let flag = Dimension::<bool>::Negated(parameter_id(4));
    assert_eq!(document.dimension_value(&flag), Ok(Some(false)));
    let wrong = Dimension::<bool>::Parameter(parameter_id(3));
    assert!(matches!(
        document.dimension_value(&wrong),
        Err(EditError::DimensionValue { .. })
    ));
}

#[test]
fn stateless_features_follow_their_configuration() {
    let mut document = timeline();
    assert_eq!(document.is_stateless(&feature(1), None), Ok(true));
    assert_eq!(document.is_stateless(&feature(3), None), Ok(false));
    document.design.parameters.push(parameter(
        4,
        ValueKind::Boolean,
        ParameterDefinition::Expression(neutral("false", &[], &[])),
        Some(ParameterValue::Boolean(false)),
    ));
    document.design.features[1].suppressed = Some(Dimension::Negated(parameter_id(4)));
    assert_eq!(document.is_stateless(&feature(2), None), Ok(true));
    document.design.configurations.push(DesignConfiguration {
        id: configuration(1),
        scope: definition(),
        name: "Lean".to_owned(),
        parent: None,
        comment: None,
        overrides: Overrides {
            features: BTreeMap::from([(
                feature(3),
                FeatureStateOverride {
                    suppressed: Some(true),
                    definition: None,
                },
            )]),
            ..Overrides::default()
        },
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    });
    assert_eq!(
        document.is_stateless(&feature(3), Some(&configuration(1))),
        Ok(true)
    );
    assert_eq!(document.is_stateless(&feature(3), None), Ok(false));
    assert_eq!(
        document.is_stateless(&feature(9), None),
        Err(EditError::UnknownFeature(feature(9)))
    );
}

#[test]
fn set_dimension_refuses_a_driven_parameter() {
    let mut document = timeline();
    let refused = document.set_dimension(
        &parameter_id(2),
        Literal::Number {
            value: real(5.0),
            unit: mm(),
        },
    );
    assert_eq!(
        refused,
        Err(EditError::DrivenParameter {
            parameter: parameter_id(2),
            drivers: ParameterDrivers::References(vec![reference(1)]),
        })
    );
    document.design.parameters[1].definition = ParameterDefinition::Driven;
    assert!(matches!(
        document.set_dimension(&parameter_id(2), Literal::Boolean(true)),
        Err(EditError::DrivenParameter {
            drivers: ParameterDrivers::Geometry,
            ..
        })
    ));
    assert!(matches!(
        document.set_dimension(&parameter_id(1), Literal::Boolean(true)),
        Err(EditError::KindMismatch { .. })
    ));
}

#[test]
fn set_dimension_marks_dependents_and_recomputes_caches() {
    let mut document = timeline();
    document.design.parameters.push(DesignParameter {
        owner: Some(ParameterOwner::Feature(feature(3))),
        ..length(
            3,
            neutral("@0 * 2", &[], &[measure(after(2), 4.0)]),
            Some(8.0),
        )
    });
    document.design.parameters.push(length(
        4,
        neutral("@0 + $0", &[reference(1)], &[measure(after(1), 1.0)]),
        Some(11.0),
    ));
    document.design.parameters[1].tolerance = Some(Tolerance::Fit {
        fit: serde_json::from_value::<LimitsAndFits>(json!({
            "form_variance": "H", "zone": "shaft", "grade": "7", "source": {"kind": "iso286"}
        }))
        .unwrap(),
        mate: None,
        deviations: Some(FitDeviations::Cell(Deviations {
            lower: real(-0.01),
            upper: real(0.0),
        })),
    });
    document.design.configurations.push(DesignConfiguration {
        id: configuration(1),
        scope: definition(),
        name: "Other".to_owned(),
        parent: None,
        comment: None,
        overrides: Overrides {
            parameters: BTreeMap::from([
                (
                    parameter_id(1),
                    ParameterOverride {
                        expression: None,
                        value: Some(ParameterValue::Number(real(12.0))),
                    },
                ),
                (
                    parameter_id(2),
                    ParameterOverride {
                        expression: Some(neutral("$0 * 3", &[reference(1)], &[])),
                        value: Some(ParameterValue::Number(real(36.0))),
                    },
                ),
            ]),
            ..Overrides::default()
        },
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    });

    document
        .set_dimension(
            &parameter_id(1),
            Literal::Number {
                value: real(2.0),
                unit: Unit(vec![UnitFactor {
                    symbol: UnitSymbol::Cm,
                    power: std::num::NonZeroI8::new(1).unwrap(),
                }]),
            },
        )
        .unwrap();

    assert_eq!(
        cache(&document, 1),
        Some(ParameterValue::Number(real(20.0)))
    );
    assert_eq!(
        cache(&document, 2),
        Some(ParameterValue::Number(real(40.0)))
    );
    assert_eq!(
        cache(&document, 3),
        None,
        "its measure reads a state after the edit"
    );
    assert_eq!(
        cache(&document, 4),
        Some(ParameterValue::Number(real(21.0)))
    );
    let measured = |key: u32| match &document
        .design_parameter(&parameter_id(key))
        .unwrap()
        .definition
    {
        ParameterDefinition::Expression(Expression::Neutral(Neutral(node))) => {
            crate::design::expr::print(node).unwrap().measures[0].cached
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(measured(3), None);
    assert_eq!(measured(4), Some(real(1.0)));
    assert!(matches!(
        &document.design.parameters[1].tolerance,
        Some(Tolerance::Fit {
            deviations: None,
            ..
        })
    ));
    let overrides = &document.design.configurations[0].overrides.parameters;
    assert!(
        !overrides.contains_key(&parameter_id(1)),
        "a value-only override of the edit states nothing"
    );
    assert_eq!(
        overrides[&parameter_id(2)].value,
        Some(ParameterValue::Number(real(60.0)))
    );

    assert_eq!(status(&document, 1), EvaluationStatus::Failed);
    assert_eq!(status(&document, 2), EvaluationStatus::Stale);
    assert_eq!(status(&document, 3), EvaluationStatus::Stale);
    assert_eq!(
        document.design.scopes[0].current[0].status,
        EvaluationStatus::Stale
    );
}

#[test]
fn set_feature_definition_marks_the_states_after_the_feature() {
    let mut document = timeline();
    for evaluation in &mut document.design.evaluations {
        evaluation.status = EvaluationStatus::Current;
        evaluation.content.get_or_insert_with(|| with_bodies(&[]));
    }
    let replacement = unresolved(2).definition;
    document
        .set_feature_definition(&feature(3), replacement)
        .unwrap();
    assert_eq!(status(&document, 2), EvaluationStatus::Current);
    assert_eq!(status(&document, 3), EvaluationStatus::Current);
    assert_eq!(
        document.design.scopes[0].current[0].status,
        EvaluationStatus::Stale
    );
    document
        .set_feature_definition(&feature(2), unresolved(2).definition)
        .unwrap();
    assert_eq!(status(&document, 1), EvaluationStatus::Current);
    assert_eq!(status(&document, 2), EvaluationStatus::Stale);
    assert_eq!(status(&document, 3), EvaluationStatus::Stale);
    assert_eq!(
        cache(&document, 2),
        Some(ParameterValue::Number(real(20.0)))
    );
}

fn arrangement(key: u32, parameters: BTreeMap<ParameterId, ParameterOverride>) -> Arrangement {
    Arrangement {
        id: id("arrangement", key),
        scope: definition(),
        name: NonBlankString::new(format!("A{key}")).unwrap(),
        kind: ArrangementKind::Positional,
        configuration: None,
        overrides: Overrides {
            parameters,
            ..Overrides::default()
        },
        appearances: Vec::new(),
        moves: Vec::new(),
        nested: Vec::new(),
        native_ref: None,
    }
}

fn length_override(text: &str, refs: &[ParameterRef], value: f64) -> ParameterOverride {
    ParameterOverride {
        expression: Some(neutral(text, refs, &[])),
        value: Some(ParameterValue::Number(real(value))),
    }
}

#[test]
fn an_occurrence_value_resolves_through_the_arrangement_in_force() {
    let mut document = timeline();
    document.design.arrangements = vec![
        arrangement(
            1,
            BTreeMap::from([(parameter_id(1), length_override("5[mm]", &[], 5.0))]),
        ),
        arrangement(2, BTreeMap::new()),
    ];
    document.design.scopes[0].active_arrangements = BTreeSet::from([id("arrangement", 1)]);
    document.design.configurations.push(DesignConfiguration {
        id: configuration(1),
        scope: definition(),
        name: "Seven".to_owned(),
        parent: None,
        comment: None,
        overrides: Overrides {
            parameters: BTreeMap::from([(parameter_id(1), length_override("7[mm]", &[], 7.0))]),
            ..Overrides::default()
        },
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    });
    let doubled = BTreeMap::from([(
        parameter_id(2),
        length_override("$0 * 2", &[reference(1)], 1.0),
    )]);
    let occurrence = |key: u32| -> OccurrenceId { id("occurrence", key) };
    document.design.scopes[0].occurrences = BTreeMap::from([
        (
            occurrence(1),
            OccurrenceDesign {
                configuration: None,
                arrangements: Vec::new(),
                grounded: None,
                flexible: None,
                parameters: doubled.clone(),
            },
        ),
        (
            occurrence(2),
            OccurrenceDesign {
                configuration: Some(ConfigurationRef::Local(configuration(1))),
                arrangements: vec![ArrangementRef::Local(id("arrangement", 2))],
                grounded: None,
                flexible: None,
                parameters: doubled,
            },
        ),
    ]);

    document
        .set_parameter_expression(&parameter_id(1), neutral("10[mm]", &[], &[]))
        .unwrap();

    let value = |key: u32| {
        document.design.scopes[0].occurrences[&occurrence(key)].parameters[&parameter_id(2)]
            .value
            .clone()
    };
    assert_eq!(
        value(1),
        Some(ParameterValue::Number(real(10.0))),
        "the base's positional arrangement sets p1 to 5"
    );
    assert_eq!(
        value(2),
        Some(ParameterValue::Number(real(14.0))),
        "the occurrence's configuration sets p1 to 7, and its own arrangement overrides nothing"
    );
}

#[test]
fn set_parameter_expression_refuses_an_ill_typed_tree() {
    let mut document = timeline();
    document.design.parameters[1].value = None;
    let before = serde_json::to_value(&document.design).unwrap();
    for (text, refs) in [
        ("if(true, 2[mm], 1[deg])", &[][..]),
        ("if(false, 1[deg], 2[mm])", &[]),
        ("if(1, 2[mm], 3[mm])", &[]),
        ("min(1[mm])", &[]),
        ("max(1[mm])", &[]),
        ("atan2(1[mm]) * 1[mm] / 1[rad]", &[]),
        ("mod(1[mm])", &[]),
        ("$0 + 1[deg]", &[reference(2)][..]),
        ("t * 1[mm]", &[]),
        ("bendradius", &[]),
        ("\"a\"", &[]),
        ("$0", &[reference(9)][..]),
    ] {
        let refused = document.set_parameter_expression(&parameter_id(1), neutral(text, refs, &[]));
        assert!(refused.is_err(), "{text} is refused");
        assert_eq!(
            serde_json::to_value(&document.design).unwrap(),
            before,
            "refusing {text} changes nothing"
        );
    }
    for text in [
        "if(true, 2[mm], 3[mm])",
        "min(1[mm], 2[cm])",
        "max(1[mm], 2[mm], 3[mm])",
    ] {
        document
            .set_parameter_expression(&parameter_id(1), neutral(text, &[], &[]))
            .unwrap();
    }
    assert_eq!(cache(&document, 1), Some(ParameterValue::Number(real(3.0))));
}

#[test]
fn set_parameter_expression_refuses_a_neutral_reference_to_a_native_kind() {
    let source = |tokens| Expression::Source {
        dialect: ExpressionDialect::Nx,
        tokens,
    };
    let mut document = timeline();
    document.design.parameters.push(parameter(
        5,
        ValueKind::Native(NonBlankString::new("nx:list").unwrap()),
        ParameterDefinition::Expression(source(vec![ExpressionToken::Text("{1, 2}".into())])),
        None,
    ));
    let before = document.clone();
    for text in ["$0", "$0 + 1[mm]", "if(true, $0, 1[mm])"] {
        let refused = document
            .set_parameter_expression(&parameter_id(1), neutral(text, &[reference(5)], &[]));
        assert!(
            matches!(refused, Err(EditError::KindMismatch { .. })),
            "{text} is refused: {refused:?}"
        );
        assert_eq!(document, before, "refusing {text} changes nothing");
    }
    document
        .set_parameter_expression(
            &parameter_id(5),
            source(vec![ExpressionToken::Text("{3, 4}".into())]),
        )
        .unwrap();
    document
        .set_parameter_expression(
            &parameter_id(1),
            source(vec![
                ExpressionToken::Text("size(".into()),
                ExpressionToken::Reference(reference(5)),
                ExpressionToken::Text(") * 1".into()),
            ]),
        )
        .unwrap();
}

#[test]
fn inserting_a_member_marks_later_states_and_moves_folders() {
    let mut document = timeline();
    for evaluation in &mut document.design.evaluations {
        evaluation.status = EvaluationStatus::Current;
        evaluation.content.get_or_insert_with(|| with_bodies(&[]));
    }
    document.design.features.push(unresolved(4));
    document
        .insert_member(&id("container", 1), 1, Member::Feature(feature(4)))
        .unwrap();
    let Container::Ordered {
        members, folders, ..
    } = &document.design.containers[0]
    else {
        panic!("ordered");
    };
    assert_eq!(members[1], Member::Feature(feature(4)));
    assert_eq!((folders[0].first, folders[0].last), (2, 3));
    assert_eq!(status(&document, 1), EvaluationStatus::Current);
    assert_eq!(status(&document, 2), EvaluationStatus::Stale);
    assert_eq!(
        document.insert_member(&id("container", 1), 0, Member::Feature(feature(4))),
        Err(EditError::AlreadyPlaced(Member::Feature(feature(4))))
    );
    document.design.features.push(unresolved(5));
    assert!(matches!(
        document.insert_member(&id("container", 1), 9, Member::Feature(feature(5))),
        Err(EditError::IndexOutOfRange { index: 9, .. })
    ));
}

#[test]
fn removing_a_member_rebases_and_retargets() {
    let mut document = timeline();
    let mut later = with_bodies(&[3]);
    later.base = StateBase::Evaluation(id("evaluation", 2));
    later.delta.removed = vec![crate::design::refs::RecordRef::Body(id("body", 2))];
    document
        .design
        .evaluations
        .push(evaluation(4, Evaluated::State(after(3)), None, Some(later)));
    document.design.parameters.push(DesignParameter {
        owner: Some(ParameterOwner::Feature(feature(3))),
        ..length(3, neutral("@0", &[], &[measure(after(2), 4.0)]), Some(4.0))
    });
    let before = document.resolved(&id("evaluation", 4)).unwrap();

    document
        .remove_member(&Member::Feature(feature(2)))
        .unwrap();

    let keys: Vec<_> = document
        .design
        .evaluations
        .iter()
        .map(|evaluation| evaluation.id.clone())
        .collect();
    assert_eq!(keys, vec![id("evaluation", 1), id("evaluation", 4)]);
    assert_eq!(
        document.design.evaluations[1]
            .content
            .as_ref()
            .unwrap()
            .base,
        StateBase::Empty
    );
    assert_eq!(document.resolved(&id("evaluation", 4)).unwrap(), before);
    assert_eq!(status(&document, 4), EvaluationStatus::Stale);
    let Container::Ordered {
        members,
        folders,
        rollback,
        ..
    } = &document.design.containers[0]
    else {
        panic!("ordered");
    };
    assert_eq!(
        members,
        &vec![Member::Feature(feature(1)), Member::Feature(feature(3))]
    );
    assert_eq!(rollback, &Rollback::Before(Member::Feature(feature(3))));
    assert_eq!((folders[0].first, folders[0].last), (1, 1));
    let anchor = serde_json::to_value(&document.design.parameters[2].definition).unwrap();
    let retargeted = serde_json::to_value(Anchor::Pinned(after(1))).unwrap();
    assert!(
        anchor.to_string().contains(&retargeted.to_string()),
        "{anchor}"
    );
    assert_eq!(
        document.remove_member(&Member::Feature(feature(2))),
        Err(EditError::NotAMember(Member::Feature(feature(2))))
    );
}

#[test]
fn removing_from_an_unordered_container_refuses_a_named_position() {
    let mut document = timeline();
    document.design.containers[0] = Container::Unordered {
        id: id("container", 1),
        kind: UnorderedKind::Group,
        members: (1..=3).map(|key| Member::Feature(feature(key))).collect(),
    };
    assert_eq!(
        document.remove_member(&Member::Feature(feature(3))),
        Err(EditError::NoPositionBefore(Member::Feature(feature(3))))
    );
    document.design.scopes[0].current.clear();
    document
        .remove_member(&Member::Feature(feature(3)))
        .unwrap();
}

/// A scope with configurations 1 (active) and 2. The current geometry holds
/// body 1 and its region; configuration 2's evaluation at the current
/// state's position holds body 2.
fn configured() -> CadIr {
    let mut document = timeline();
    document.design.evaluations.clear();
    for key in [1, 2] {
        document.design.configurations.push(DesignConfiguration {
            id: configuration(key),
            scope: definition(),
            name: format!("C{key}"),
            parent: None,
            comment: None,
            overrides: Overrides::default(),
            active_arrangements: BTreeSet::new(),
            materials: Vec::new(),
            native_ref: None,
        });
    }
    let scope = &mut document.design.scopes[0];
    scope.active_configuration = Some(configuration(1));
    scope.current[0].bodies = vec![id("body", 1)];
    document.geometry.bodies.push(body(1));
    document.geometry.bodies.push(body(9));
    document.geometry.regions.push(Region {
        id: id("region", 1),
        body: id("body", 1),
        shells: Vec::new(),
        native_ref: None,
    });
    document
        .model
        .product_definitions
        .push(product_definition(&[1, 9]));
    let name = |text: &str| -> NameEntry {
        serde_json::from_value(
            json!({"native": {"codec": "test", "value": {"kind": "text", "value": text}}}),
        )
        .unwrap()
    };
    document.design.names = BTreeMap::from([(EntityRef::Body(id("body", 1)), name("one"))]).into();
    let mut incoming = with_bodies(&[2]);
    incoming.delta.names = BTreeMap::from([(EntityRef::Body(id("body", 2)), Some(name("two")))]);
    document.design.evaluations.push(evaluation(
        5,
        Evaluated::State(after(3)),
        Some(configuration(2)),
        Some(incoming),
    ));
    let mut chained = StateContent {
        base: StateBase::Current(after(3)),
        delta: StateDelta::default(),
    };
    chained.delta.removed = vec![crate::design::refs::RecordRef::Region(id("region", 1))];
    document.design.evaluations.push(evaluation(
        6,
        Evaluated::State(after(2)),
        Some(configuration(1)),
        Some(chained),
    ));
    document
}

#[test]
fn switching_the_active_configuration_exchanges_each_extent() {
    let mut document = configured();
    let chained = document.resolved(&id("evaluation", 6)).unwrap();
    document
        .set_active_configuration(&definition(), &configuration(2))
        .unwrap();

    let bodies: Vec<BodyId> = document
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    assert_eq!(bodies, vec![id("body", 2), id("body", 9)]);
    assert!(document.geometry.regions.is_empty());
    let scope = &document.design.scopes[0];
    assert_eq!(scope.active_configuration, Some(configuration(2)));
    assert_eq!(scope.current[0].bodies, vec![id("body", 2)]);
    assert_eq!(
        document
            .design
            .names
            .entries()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec![EntityRef::Body(id("body", 2))]
    );
    let listed = &document.model.product_definitions[0].bodies;
    assert_eq!(listed, &vec![id::<BodyId>("body", 9), id("body", 2)]);
    assert!(document
        .design
        .evaluations
        .iter()
        .all(|evaluation| evaluation.id != id("evaluation", 5)));
    let written = document
        .design
        .evaluations
        .iter()
        .find(|evaluation| {
            evaluation.configuration == Some(configuration(1))
                && evaluation.of == Evaluated::State(after(3))
        })
        .unwrap();
    let content = written.content.as_ref().unwrap();
    assert_eq!(content.base, StateBase::Empty);
    assert_eq!(content.delta.records.bodies, vec![body(1)]);
    assert_eq!(content.delta.records.regions.len(), 1);
    assert!(content
        .delta
        .names
        .contains_key(&EntityRef::Body(id("body", 1))));
    let rebased = document
        .design
        .evaluations
        .iter()
        .find(|evaluation| evaluation.id == id("evaluation", 6))
        .unwrap();
    assert_eq!(
        rebased.content.as_ref().unwrap().base,
        StateBase::Evaluation(written.id.clone())
    );
    assert_eq!(document.resolved(&id("evaluation", 6)).unwrap(), chained);

    document
        .set_active_configuration(&definition(), &configuration(1))
        .unwrap();
    let bodies: Vec<BodyId> = document
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    assert_eq!(bodies, vec![id("body", 1), id("body", 9)]);
    assert_eq!(document.geometry.regions.len(), 1);
    assert_eq!(document.resolved(&id("evaluation", 6)).unwrap(), chained);
}

#[test]
fn switching_exchanges_every_current_state_of_the_scope() {
    let mut document = configured();
    document
        .design
        .evaluations
        .retain(|evaluation| evaluation.id != id("evaluation", 6));
    document.geometry.bodies.push(body(3));
    document.model.product_definitions[0]
        .bodies
        .push(id("body", 3));
    document.design.scopes[0].current.push(CurrentState {
        at: after(1),
        bodies: vec![id("body", 3)],
        poses: BTreeMap::new(),
        status: EvaluationStatus::Current,
        native_ref: None,
    });
    document.design.evaluations.push(evaluation(
        7,
        Evaluated::State(after(1)),
        Some(configuration(2)),
        Some(with_bodies(&[4])),
    ));
    document
        .set_active_configuration(&definition(), &configuration(2))
        .unwrap();
    let bodies: Vec<BodyId> = document
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    assert_eq!(bodies, vec![id("body", 2), id("body", 4), id("body", 9)]);
    let current = &document.design.scopes[0].current;
    assert_eq!(current[0].bodies, vec![id("body", 2)]);
    assert_eq!(current[1].bodies, vec![id("body", 4)]);
    let written: Vec<_> = document
        .design
        .evaluations
        .iter()
        .filter(|evaluation| evaluation.configuration == Some(configuration(1)))
        .map(|evaluation| evaluation.of.clone())
        .collect();
    assert_eq!(written.len(), 2);
    assert!(written.contains(&Evaluated::State(after(1))));
    assert!(written.contains(&Evaluated::State(after(3))));
}

#[test]
fn switching_to_a_configuration_with_no_state_keeps_the_geometry() {
    let mut document = configured();
    document
        .design
        .evaluations
        .retain(|evaluation| evaluation.id != id("evaluation", 5));
    document
        .set_active_configuration(&definition(), &configuration(2))
        .unwrap();
    assert_eq!(document.geometry.bodies.len(), 2);
    let scope = &document.design.scopes[0];
    assert_eq!(scope.current[0].status, EvaluationStatus::Stale);
    assert_eq!(scope.current[0].bodies, vec![id("body", 1)]);
    assert!(document
        .design
        .evaluations
        .iter()
        .any(
            |evaluation| evaluation.configuration == Some(configuration(1))
                && evaluation.of == Evaluated::State(after(3))
        ));
    assert_eq!(
        document.set_active_configuration(&definition(), &configuration(7)),
        Err(EditError::UnknownConfiguration(configuration(7)))
    );
}

/// A point datum on one coincident input, at (1, 0, 0) in the active
/// configuration and at (2, 0, 0) in configuration 2.
fn point_datum(key: u32) -> Feature {
    let point = |x: f64| FinitePoint3::from_coordinates(real(x), real(0.0), real(0.0));
    operation(
        key,
        FeatureOperation::Datum(Box::new(Datum {
            element: DatumElement::Points(vec![Construction {
                inputs: vec![ConstructionInput {
                    role: Some(InputRole {
                        subject: Subject::Whole,
                        relation: Relation::Coincident,
                    }),
                    reference: None,
                    value: None,
                    reversed: None,
                }],
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved: Some(point(1.0)),
                resolved_in: BTreeMap::from([(configuration(2), point(2.0))]),
            }]),
            link: None,
            native_options: NativeOptions::new(),
            source_settings: NativeOptions::new(),
        })),
    )
}

#[test]
fn switching_exchanges_the_constructions_of_feature_overrides() {
    let mut document = configured();
    document.design.features[0] = point_datum(1);
    let state = FeatureStateOverride {
        suppressed: None,
        definition: Some(document.design.features[0].definition.clone()),
    };
    document.design.configurations[1].overrides.features =
        BTreeMap::from([(feature(1), state.clone())]);
    let mut arranged = arrangement(1, BTreeMap::new());
    arranged.overrides.features = BTreeMap::from([(feature(1), state)]);
    document.design.arrangements.push(arranged);

    document
        .set_active_configuration(&definition(), &configuration(2))
        .unwrap();

    let construction = |definition: &FeatureDefinition| {
        let FeatureOperation::Datum(datum) = &definition.operation else {
            panic!("{definition:?} is a datum");
        };
        let DatumElement::Points(points) = &datum.element else {
            panic!("{datum:?} holds points");
        };
        (
            points[0].resolved.map(|point| point.get().x),
            points[0].resolved_in.keys().cloned().collect::<Vec<_>>(),
        )
    };
    let expected = (Some(2.0), vec![configuration(1)]);
    assert_eq!(
        construction(&document.design.features[0].definition),
        expected
    );
    let overridden = |overrides: &Overrides| {
        construction(overrides.features[&feature(1)].definition.as_ref().unwrap())
    };
    assert_eq!(
        overridden(&document.design.configurations[1].overrides),
        expected,
        "a configuration's feature override"
    );
    assert_eq!(
        overridden(&document.design.arrangements[0].overrides),
        expected,
        "an arrangement's feature override"
    );
}

#[test]
fn positions_and_constructions_rewrite_in_the_wire_form() {
    let removed = Member::Feature(feature(2));
    let mut value = json!({"at": {"kind": "pinned", "value": {"kind": "after", "value": {"kind": "feature", "value": feature(2).as_str()}}}, "picks": {"kind": "unread"}});
    assert!(retarget(&mut value, &removed, &after(1)).unwrap());
    assert_eq!(
        value["at"]["value"]["value"]["value"],
        json!(feature(1).as_str())
    );

    let mut construction = json!({
        "inputs": [{"kind": "x"}], "after": [],
        "resolved": [1.0], "resolved_in": {"c2": [2.0]},
        "nested": {"inputs": [], "after": [], "resolved": [3.0], "resolved_in": {}},
    });
    assert!(exchange_constructions(&mut construction, "c1", "c2"));
    assert_eq!(construction["resolved"], json!([2.0]));
    assert_eq!(construction["resolved_in"], json!({"c1": [1.0]}));
    assert_eq!(
        construction["nested"]["resolved"],
        json!([3.0]),
        "a construction with no inputs or steps keeps its value"
    );
}
