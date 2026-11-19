// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use cadmpeg_core::decode::ContentScope;

use super::normalize;
use super::selections;
use crate::design::construction::{
    kinds, AnyConstruction, Construction, ConstructionInput, FrameAxis, InputReference, InputRole,
    PostStep, Relation, ScalarDimension, Subject,
};
use crate::design::container::{Container, OrderedKind, Rollback};
use crate::design::dimension::Dimension;
use crate::design::evaluation::{
    CurrentState, Evaluation, EvaluationStatus, StateBase, StateContent, StateDelta,
};
use crate::design::feature::{Feature, FeatureDefinition, FeatureOperation};
use crate::design::names::NameKind;
use crate::design::ops::datum::{Datum, DatumElement};
use crate::design::parameter::{DesignParameter, ParameterDefinition, Tolerance};
use crate::design::refs::{Evaluated, Member, Position, StateRef};
use crate::design::scope::DesignScope;
use crate::design::selection::{
    Anchor, MemberOrder, Members, Ordered, PickTarget, Picked, Picks, PrincipalEntity, Propagation,
    RuleTerm, Selection, SelectionRule, Unordered,
};
use crate::design::units::ValueKind;
use crate::document::CadIr;
use crate::features::FeatureId;
use crate::pmi::zones::{ToleranceZone, ToleranceZoneForm};
use crate::pmi::{
    DimensionModifier, FitZone, GeometricToleranceKind, LimitsAndFits, PmiAnnotation,
    PmiDefinition, PmiDimension, PmiDimensionKind, PmiMagnitude, PmiQuantity, PmiTarget, PmiValue,
};
use crate::references::InstanceTarget;
use crate::scalar::{FiniteReal, Length};

fn id<T>(kind: &str, key: &str) -> T
where
    T: TryFrom<String>,
    T::Error: Debug,
{
    T::try_from(format!("test:normalize:{kind}#{key}")).unwrap()
}

fn feature(key: &str) -> FeatureId {
    id("feature", key)
}

fn after_feature(key: &str) -> Position {
    Position::After(Member::Feature(feature(key)))
}

fn after_body() -> Position {
    Position::After(Member::Container(id("container", "body")))
}

fn real(value: f64) -> FiniteReal {
    FiniteReal::new(value).unwrap()
}

fn length(value: f64) -> Dimension<Length> {
    Dimension::Literal(Length::new(value).unwrap())
}

fn pick(target: PickTarget) -> Picked {
    Picked {
        target,
        context: None,
        native: None,
        domain: None,
        witness: None,
        resolved: None,
    }
}

fn xy_plane() -> Picked {
    pick(PickTarget::Principal {
        entity: PrincipalEntity::XyPlane,
        container: None,
    })
}

fn picks_feature(key: &str) -> Picked {
    pick(PickTarget::Feature(feature(key)))
}

fn term<O: MemberOrder>(
    rule: SelectionRule,
    seeds: Vec<Picked>,
    propagation: Option<Propagation>,
) -> RuleTerm<O> {
    RuleTerm {
        rule,
        seeds: Members::new(seeds).unwrap(),
        propagation,
    }
}

fn explicit<O: MemberOrder>(seeds: Vec<Picked>) -> RuleTerm<O> {
    term(SelectionRule::Explicit, seeds, None)
}

fn rule<O: MemberOrder>(terms: Vec<RuleTerm<O>>) -> Picks<O> {
    Picks::Rule {
        terms,
        excluded: Vec::new(),
        resolved: None,
    }
}

fn named<O: MemberOrder>(picks: Vec<Picked>) -> Picks<O> {
    Picks::Named(Members::new(picks).unwrap())
}

fn selection(picks: Picks<Ordered>) -> InputReference {
    InputReference::Picks(Selection {
        at: Anchor::Input,
        picks,
        native: None,
    })
}

fn input(
    relation: Relation,
    reference: InputReference,
    value: Option<ScalarDimension>,
) -> ConstructionInput {
    ConstructionInput {
        role: Some(InputRole {
            subject: Subject::Whole,
            relation,
        }),
        reference: Some(reference),
        value,
        reversed: None,
    }
}

fn translate_z(distance: f64) -> PostStep {
    PostStep::Translate {
        axis: FrameAxis::Z,
        distance: length(distance),
    }
}

/// A plane on the XY plane, offset along its normal by a leading step, whose
/// reference is a one-term explicit rule.
fn offset_plane() -> Construction<kinds::Plane> {
    Construction {
        inputs: vec![input(
            Relation::Coincident,
            selection(rule(vec![explicit(vec![xy_plane()])])),
            None,
        )],
        after: vec![translate_z(5.0), translate_z(2.0)],
        ..Construction::default()
    }
}

fn point_at(reference: InputReference, ratio: f64) -> Construction<kinds::Point> {
    Construction {
        inputs: vec![input(
            Relation::AtParameter,
            reference,
            Some(ScalarDimension::ParameterRatio(Dimension::Literal(real(
                ratio,
            )))),
        )],
        ..Construction::default()
    }
}

/// Points: at the end of a picked feature's curve, inside it, along a built
/// curve, and on the XY plane moved along Z, which is no plane's offset.
fn points() -> Vec<Construction<kinds::Point>> {
    vec![
        point_at(selection(named(vec![picks_feature("fillet")])), 1.0),
        point_at(selection(named(vec![picks_feature("fillet")])), 0.25),
        point_at(
            InputReference::Built(Box::new(AnyConstruction::Curve(Construction::default()))),
            0.25,
        ),
        Construction {
            inputs: vec![input(
                Relation::Coincident,
                selection(named(vec![xy_plane()])),
                None,
            )],
            after: vec![translate_z(5.0)],
            ..Construction::default()
        },
    ]
}

fn datum(key: &str, element: DatumElement) -> Feature {
    Feature {
        id: feature(key),
        name: None,
        suppressed: None,
        definition: FeatureDefinition {
            operation: FeatureOperation::Datum(Box::new(Datum {
                element,
                link: None,
                native_options: BTreeMap::new(),
                source_settings: BTreeMap::new(),
            })),
            refine: None,
            fuzzy_tolerance: None,
        },
        placement: None,
        component: None,
        driven_by: None,
        native_ref: None,
    }
}

fn designation(form_variance: &str, zone: FitZone, grade: &str) -> LimitsAndFits {
    LimitsAndFits {
        form_variance: form_variance.to_owned(),
        zone,
        grade: grade.to_owned(),
        source: None,
    }
}

fn fit(fit: LimitsAndFits, mate: Option<LimitsAndFits>) -> Tolerance {
    Tolerance::Fit {
        fit,
        mate,
        deviations: None,
    }
}

fn target(state: Option<StateRef>) -> InstanceTarget<PmiTarget> {
    InstanceTarget {
        state,
        ..InstanceTarget::new(PmiTarget::Body {
            body: id("body", "cube"),
        })
    }
}

fn state(configuration: Option<&str>, at: Position) -> StateRef {
    StateRef {
        configuration: configuration.map(|key| id("configuration", key)),
        of: Evaluated::State(at),
    }
}

/// A part whose timeline holds a datum plane and an ordered body container
/// that holds a datum of points and a fillet, every spelling rule stated in
/// its other form: the current state, an evaluation, and its base at
/// `After(Container(body))`; selections as one-term explicit rules; a plane
/// offset by a leading step; `ParameterRatio` values that are arc-length
/// ratios; combined fit designations; and `Other` texts that name arms.
fn document() -> CadIr {
    let mut ir = crate::examples::unit_cube().unwrap();
    ir.set_scope(ContentScope::full()).unwrap();
    let design = &mut ir.design;
    design.scopes.push(DesignScope {
        definition: id("definition", "part"),
        roots: vec![id("container", "timeline")],
        active_configuration: Some(id("configuration", "base")),
        active_arrangements: BTreeSet::new(),
        occurrences: BTreeMap::new(),
        current: vec![CurrentState {
            at: after_body(),
            bodies: Vec::new(),
            poses: BTreeMap::new(),
            status: EvaluationStatus::Current,
            native_ref: None,
        }],
    });
    design.containers.push(Container::Ordered {
        id: id("container", "timeline"),
        kind: OrderedKind::Timeline,
        members: vec![
            Member::Feature(feature("plane")),
            Member::Container(id("container", "body")),
        ],
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    design.containers.push(Container::Ordered {
        id: id("container", "body"),
        kind: OrderedKind::Body,
        members: vec![
            Member::Feature(feature("points")),
            Member::Feature(feature("fillet")),
        ],
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    design
        .features
        .push(datum("plane", DatumElement::Plane(offset_plane())));
    design
        .features
        .push(datum("points", DatumElement::Points(points())));
    design.evaluations.push(Evaluation {
        id: id("evaluation", "large"),
        scope: id("definition", "part"),
        configuration: Some(id("configuration", "large")),
        of: Evaluated::State(after_body()),
        status: EvaluationStatus::Current,
        content: Some(StateContent {
            base: StateBase::Current(after_body()),
            delta: StateDelta::default(),
        }),
        native_ref: None,
    });
    design.parameters.push(DesignParameter {
        id: id("parameter", "bore"),
        scope: id("definition", "part"),
        owner: None,
        name: Some(cadmpeg_core::text::NonBlankString::new("bore").unwrap()),
        kind: ValueKind::Integer,
        unit: None,
        definition: ParameterDefinition::Driven,
        value: None,
        display: None,
        tolerance: Some(fit(designation("JS10", FitZone::Hole, ""), None)),
        decimals: None,
        comment: None,
        native_options: BTreeMap::new(),
        source_settings: BTreeMap::new(),
        native_ref: None,
    });
    let dimension = PmiDimension {
        tolerance: Some(fit(
            designation("G6", FitZone::Hole, ""),
            Some(designation("h7", FitZone::Shaft, "")),
        )),
        modifiers: vec![
            DimensionModifier::Other("Auxiliary".to_owned()),
            DimensionModifier::Reference,
            DimensionModifier::Other("CONTROLLED RADIUS".to_owned()),
            DimensionModifier::Other("flat".to_owned()),
        ],
        ..PmiDimension::new(PmiDimensionKind::Size)
    };
    ir.model.pmi.push(PmiAnnotation::new(
        id("pmi", "size"),
        vec![
            target(Some(state(None, after_body()))),
            target(Some(state(Some("base"), after_body()))),
            target(Some(state(Some("large"), after_body()))),
            target(Some(state(None, after_feature("points")))),
        ],
        PmiDefinition::Dimension(dimension),
    ));
    ir.model.pmi.push(PmiAnnotation::new(
        id("pmi", "position"),
        Vec::new(),
        PmiDefinition::GeometricTolerance {
            tolerance: GeometricToleranceKind::Position,
            magnitude: PmiMagnitude::new(PmiValue::new(0.1, PmiQuantity::Length).unwrap()).unwrap(),
            defined_unit: None,
            defined_area_unit: None,
            defined_area_second_unit: None,
            datum_system: None,
            modifiers: Vec::new(),
            displacement: None,
            maximum_upper: None,
            zones: vec![
                zone(ToleranceZoneForm::Other(
                    "Cylindrical or Circular".to_owned(),
                )),
                zone(ToleranceZoneForm::Other("SPHERICAL".to_owned())),
                zone(ToleranceZoneForm::Other("square".to_owned())),
            ],
        },
    ));
    ir
}

fn zone(form: ToleranceZoneForm) -> ToleranceZone {
    ToleranceZone {
        form,
        projections: Vec::new(),
    }
}

fn normalized() -> CadIr {
    let mut ir = document();
    normalize(&mut ir).unwrap();
    ir
}

fn datum_element(ir: &CadIr, key: &str) -> DatumElement {
    let feature = ir
        .design
        .features
        .iter()
        .find(|candidate| candidate.id == feature(key))
        .unwrap();
    let FeatureOperation::Datum(datum) = &feature.definition.operation else {
        panic!("{key} is a datum");
    };
    datum.element.clone()
}

fn reference_picks(input: &ConstructionInput) -> &Picks<Ordered> {
    let Some(InputReference::Picks(selection)) = &input.reference else {
        panic!("the input picks");
    };
    &selection.picks
}

#[test]
fn positions_name_a_rollback_through_its_member() {
    let ir = normalized();
    assert_eq!(ir.design.scopes[0].current[0].at, after_feature("fillet"));
    let evaluation = &ir.design.evaluations[0];
    assert_eq!(evaluation.of, Evaluated::State(after_feature("fillet")));
    assert_eq!(
        evaluation.content.as_ref().unwrap().base,
        StateBase::Current(after_feature("fillet"))
    );
}

#[test]
fn a_rollback_follows_nested_ordered_containers_and_empty_ones() {
    let mut ir = document();
    let Container::Ordered { members, .. } = &mut ir.design.containers[1] else {
        panic!("the body is ordered");
    };
    members.clear();
    normalize(&mut ir).unwrap();
    assert_eq!(
        ir.design.scopes[0].current[0].at,
        Position::Start(id("container", "body"))
    );
}

#[test]
fn a_target_into_the_state_none_reads_states_none() {
    let ir = normalized();
    let targets: Vec<Option<StateRef>> = ir.model.pmi[0]
        .targets
        .iter()
        .map(|target| target.state.clone())
        .collect();
    assert_eq!(
        targets,
        [
            None,
            None,
            Some(state(Some("large"), after_feature("fillet"))),
            Some(state(None, after_feature("points"))),
        ]
    );
}

#[test]
fn a_target_with_a_path_keeps_a_named_configuration() {
    let mut ir = document();
    let path = crate::design::selection::OccurrencePath(Vec::new());
    ir.model.pmi[0].targets[1].path = Some(path);
    normalize(&mut ir).unwrap();
    assert_eq!(
        ir.model.pmi[0].targets[1].state,
        Some(state(Some("base"), after_feature("fillet")))
    );
}

#[test]
fn a_leading_z_translation_on_a_coincident_plane_is_an_offset_input() {
    let ir = normalized();
    let DatumElement::Plane(plane) = datum_element(&ir, "plane") else {
        panic!("a plane");
    };
    assert_eq!(plane.after, vec![translate_z(2.0)]);
    let [only] = plane.inputs.as_slice() else {
        panic!("one input");
    };
    assert_eq!(
        only.role,
        Some(InputRole {
            subject: Subject::Whole,
            relation: Relation::Offset,
        })
    );
    assert_eq!(only.value, Some(ScalarDimension::Length(length(5.0))));
    assert_eq!(reference_picks(only), &named(vec![xy_plane()]));
}

#[test]
fn a_point_keeps_its_translation_and_an_interior_parameter_ratio_stays() {
    let ir = normalized();
    let DatumElement::Points(points) = datum_element(&ir, "points") else {
        panic!("points");
    };
    let values: Vec<Option<ScalarDimension>> = points[..3]
        .iter()
        .map(|point| point.inputs[0].value.clone())
        .collect();
    let ratio = |value| Some(ScalarDimension::Ratio(Dimension::Literal(real(value))));
    assert_eq!(
        values,
        [
            ratio(1.0),
            Some(ScalarDimension::ParameterRatio(Dimension::Literal(real(
                0.25
            )))),
            ratio(0.25),
        ]
    );
    assert_eq!(points[3], self::points()[3]);
}

#[test]
fn fits_split_and_other_texts_take_their_arms() {
    let ir = normalized();
    assert_eq!(
        ir.design.parameters[0].tolerance,
        Some(fit(designation("JS", FitZone::Hole, "10"), None))
    );
    let PmiDefinition::Dimension(dimension) = &ir.model.pmi[0].definition else {
        panic!("a dimension");
    };
    assert_eq!(
        dimension.tolerance,
        Some(fit(
            designation("G", FitZone::Hole, "6"),
            Some(designation("H", FitZone::Shaft, "7")),
        ))
    );
    assert_eq!(
        dimension.modifiers,
        [
            DimensionModifier::Reference,
            DimensionModifier::ControlledRadius,
            DimensionModifier::Other("flat".to_owned()),
        ]
    );
    let PmiDefinition::GeometricTolerance { zones, .. } = &ir.model.pmi[1].definition else {
        panic!("a geometric tolerance");
    };
    let forms: Vec<&ToleranceZoneForm> = zones.iter().map(|zone| &zone.form).collect();
    assert_eq!(
        forms,
        [
            &ToleranceZoneForm::CylindricalOrCircular,
            &ToleranceZoneForm::Spherical,
            &ToleranceZoneForm::Other("square".to_owned()),
        ]
    );
}

#[test]
fn a_designation_that_is_not_letters_then_digits_is_kept() {
    let mut ir = document();
    ir.design.parameters[0].tolerance = Some(fit(designation("H 7", FitZone::Hole, ""), None));
    normalize(&mut ir).unwrap();
    assert_eq!(
        ir.design.parameters[0].tolerance,
        Some(fit(designation("H 7", FitZone::Hole, ""), None))
    );
}

#[test]
fn normalizing_twice_equals_normalizing_once() {
    let once = normalized();
    let mut twice = once.clone();
    normalize(&mut twice).unwrap();
    assert_eq!(twice, once);
    assert_ne!(once, document());
}

#[test]
fn explicit_terms_of_one_propagation_unite_in_an_unordered_selection() {
    let off = Some(Propagation::Off);
    let mut picks: Picks<Unordered> = rule(vec![
        term(
            SelectionRule::Explicit,
            vec![picks_feature("a")],
            off.clone(),
        ),
        term(
            SelectionRule::Of {
                kind: NameKind::Edge,
            },
            vec![picks_feature("c")],
            None,
        ),
        term(
            SelectionRule::Explicit,
            vec![picks_feature("b")],
            off.clone(),
        ),
    ]);
    selections::normalize(&mut picks);
    assert_eq!(
        picks,
        rule(vec![
            term(
                SelectionRule::Explicit,
                vec![picks_feature("a"), picks_feature("b")],
                off,
            ),
            term(
                SelectionRule::Of {
                    kind: NameKind::Edge,
                },
                vec![picks_feature("c")],
                None,
            ),
        ])
    );
}

#[test]
fn an_ordered_selection_unites_only_neighbouring_explicit_terms() {
    let of = || {
        term(
            SelectionRule::Of {
                kind: NameKind::Edge,
            },
            vec![picks_feature("c")],
            None,
        )
    };
    let mut apart: Picks<Ordered> = rule(vec![
        explicit(vec![picks_feature("a")]),
        of(),
        explicit(vec![picks_feature("b")]),
    ]);
    let before = apart.clone();
    selections::normalize(&mut apart);
    assert_eq!(apart, before);

    let mut neighbours: Picks<Ordered> = rule(vec![
        explicit(vec![picks_feature("a")]),
        explicit(vec![picks_feature("b")]),
    ]);
    selections::normalize(&mut neighbours);
    assert_eq!(
        neighbours,
        named(vec![picks_feature("a"), picks_feature("b")])
    );

    let mut repeated: Picks<Ordered> = rule(vec![
        explicit(vec![picks_feature("a")]),
        explicit(vec![picks_feature("a"), picks_feature("b")]),
    ]);
    let before = repeated.clone();
    selections::normalize(&mut repeated);
    assert_eq!(repeated, before);
}

#[test]
fn an_explicit_rule_with_a_cache_is_named_only_where_the_cache_lists_its_seeds() {
    let cached = |picks: Vec<Picked>| -> Picks<Ordered> {
        Picks::Rule {
            terms: vec![explicit(vec![picks_feature("a")])],
            excluded: Vec::new(),
            resolved: Some(Members::new(picks).unwrap()),
        }
    };
    let mut agrees = cached(vec![picks_feature("a")]);
    selections::normalize(&mut agrees);
    assert_eq!(agrees, named(vec![picks_feature("a")]));

    let mut differs = cached(vec![picks_feature("b")]);
    let before = differs.clone();
    selections::normalize(&mut differs);
    assert_eq!(differs, before);
}

#[test]
fn an_explicit_rule_with_propagation_or_exclusions_stays_a_rule() {
    let mut propagated: Picks<Ordered> = rule(vec![term(
        SelectionRule::Explicit,
        vec![picks_feature("a")],
        Some(Propagation::Minimal),
    )]);
    let before = propagated.clone();
    selections::normalize(&mut propagated);
    assert_eq!(propagated, before);

    let mut excluding: Picks<Ordered> = Picks::Rule {
        terms: vec![explicit(vec![picks_feature("a")])],
        excluded: vec![explicit(vec![picks_feature("b")])],
        resolved: None,
    };
    let before = excluding.clone();
    selections::normalize(&mut excluding);
    assert_eq!(excluding, before);
}
