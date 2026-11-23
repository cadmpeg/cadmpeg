// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Debug;

use crate::codec::{
    Codec, CodecBackend, Confidence, DecodeBody, DecodeOptions, DecodeResult, Decoded, FormatId,
};
use crate::design::configuration::{
    Arrangement, ArrangementKind, ArrangementRef, DesignConfiguration, OccurrenceOverride,
    Overrides,
};
use crate::design::names::{CodecId, NativeName, NativeNameValue};
use crate::design::refs::{ConfigurationRef, EntityRef};
use crate::design::scope::OccurrenceDesign;
use crate::design::selection::{OccurrencePath, PathStep, StepRef, Witness};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{Curve, CurveGeometry, SolvedCurveGeometry};
use crate::ids::{EdgeId, OccurrenceId, ProductDefinitionId};
use crate::math::Point3;
use crate::process::feature::{MachiningFeature, MachiningFeatureClass};
use crate::process::operation::{GeometryRole, GeometrySource, ProcessGeometry};
use crate::process::state::{ProcessBase, ProcessContent, ProcessState, ProcessStateOf};
use crate::process::{ProcessConstruction, WorkpieceSelection};
use crate::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
use crate::report::loss::LossNote;
use crate::report::Severity;
use crate::source_fidelity::SourceFidelity;
use crate::topology::EdgeCarrier;
use crate::ContainerSummary;
use cadmpeg_core::decode::ContentScope;
use cadmpeg_core::decode::{DecodeContext, DecodePolicy, View};
use cadmpeg_core::text::NonBlankString;
use cadmpeg_core::CodecError;

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

/// Normalizes under the default policy, which gives up no record of these
/// documents.
fn normalize(ir: &mut CadIr) -> Result<(), CodecError> {
    let given_up = super::normalize(ir, &DecodePolicy::default())?;
    assert_eq!(given_up, Vec::<String>::new());
    Ok(())
}

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
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
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
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
    }
}

/// Points: at the end of a picked feature's curve, inside it, along a built
/// curve, and on the XY plane moved along Z, which is no plane's offset.
fn points() -> Vec<Construction<kinds::Point>> {
    vec![
        point_at(selection(named(vec![picks_feature("fillet")])), 1.0),
        point_at(selection(named(vec![picks_feature("fillet")])), 0.25),
        point_at(
            InputReference::Built(Box::new(AnyConstruction::Curve(Construction {
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved: None,
                resolved_in: BTreeMap::new(),
            }))),
            0.25,
        ),
        Construction {
            inputs: vec![input(
                Relation::Coincident,
                selection(named(vec![xy_plane()])),
                None,
            )],
            after: vec![translate_z(5.0)],
            solution: None,
            mode: None,
            resolved: None,
            resolved_in: BTreeMap::new(),
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
        unproduced: BTreeSet::new(),
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
        modifiers: [
            DimensionModifier::Other(cadmpeg_core::nonblank_literal!("Auxiliary")),
            DimensionModifier::Reference,
            DimensionModifier::Other(cadmpeg_core::nonblank_literal!("CONTROLLED RADIUS")),
            DimensionModifier::Other(cadmpeg_core::nonblank_literal!("flat")),
        ]
        .into(),
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
fn an_empty_container_rolls_back_to_its_start() {
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
            DimensionModifier::Other(cadmpeg_core::nonblank_literal!("flat")),
        ]
        .into()
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

/// The one-step path through occurrence `cube`, reading `configuration`
/// where it is stated.
fn path(configuration: Option<&str>) -> OccurrencePath {
    OccurrencePath(vec![PathStep {
        occurrence: StepRef::Occurrence(id("occurrence", "cube")),
        configuration: configuration.map(|key| ConfigurationRef::Local(id("configuration", key))),
    }])
}

#[test]
fn a_rollback_after_a_nested_container_is_the_state_entering_it() {
    let mut ir = document();
    let Container::Ordered { members, .. } = &mut ir.design.containers[1] else {
        panic!("the body is ordered");
    };
    *members = vec![Member::Container(id("container", "inner"))];
    ir.design.containers.push(Container::Ordered {
        id: id("container", "inner"),
        kind: OrderedKind::Body,
        members: vec![Member::Feature(feature("fillet"))],
        folders: Vec::new(),
        rollback: Rollback::End,
        placement: None,
    });
    normalize(&mut ir).unwrap();
    let start = Position::Start(id("container", "body"));
    assert_eq!(ir.design.scopes[0].current[0].at, start);
    let evaluation = &ir.design.evaluations[0];
    assert_eq!(evaluation.of, Evaluated::State(start.clone()));
    assert_eq!(
        evaluation.content.as_ref().unwrap().base,
        StateBase::Current(start)
    );
}

/// The geometry of a line edge of the unit cube, and the edge.
fn line_edge(ir: &CadIr) -> (EdgeId, SolvedCurveGeometry) {
    ir.geometry
        .edges
        .iter()
        .find_map(|edge| {
            let (EdgeCarrier::Curve(carrier) | EdgeCarrier::Bounded(carrier, _)) = &edge.carrier
            else {
                return None;
            };
            let curve = ir
                .geometry
                .curves
                .iter()
                .find(|curve| &curve.id == carrier)?;
            match &curve.geometry {
                CurveGeometry::Solved(line @ SolvedCurveGeometry::Line(_)) => {
                    Some((edge.id.clone(), line.clone()))
                }
                _ => None,
            }
        })
        .unwrap()
}

#[test]
fn a_parameter_ratio_on_a_line_edge_is_a_ratio() {
    let mut ir = document();
    let (edge, line) = line_edge(&ir);
    let unnamed = || pick(PickTarget::Native { within: None });
    let resolved = Picked {
        resolved: Some(vec![EntityRef::Edge(edge)]),
        ..unnamed()
    };
    let witnessed = Picked {
        witness: Some(Witness {
            point: None,
            direction: None,
            endpoints: None,
            frame: None,
            curves: vec![line],
            surface: None,
            body: None,
        }),
        ..unnamed()
    };
    ir.design.features.push(datum(
        "edges",
        DatumElement::Points(vec![
            point_at(selection(named(vec![resolved])), 0.25),
            point_at(selection(named(vec![witnessed])), 0.25),
        ]),
    ));
    normalize(&mut ir).unwrap();
    let DatumElement::Points(points) = datum_element(&ir, "edges") else {
        panic!("points");
    };
    let values: Vec<Option<ScalarDimension>> = points
        .iter()
        .map(|point| point.inputs[0].value.clone())
        .collect();
    let ratio = Some(ScalarDimension::Ratio(Dimension::Literal(real(0.25))));
    assert_eq!(values, [ratio.clone(), ratio]);
}

#[test]
fn a_parameter_ratio_on_an_edge_that_a_stored_state_holds_as_a_spline_stays() {
    let mut ir = document();
    let (edge, _) = line_edge(&ir);
    let carrier = ir
        .geometry
        .edges
        .iter()
        .find(|record| record.id == edge)
        .and_then(|record| match &record.carrier {
            EdgeCarrier::Curve(curve) | EdgeCarrier::Bounded(curve, _) => Some(curve.clone()),
            EdgeCarrier::Free | EdgeCarrier::Endpoints(_) => None,
        })
        .unwrap();
    let spline = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(5.0, 3.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap();
    ir.design.evaluations[0]
        .content
        .as_mut()
        .unwrap()
        .delta
        .records
        .curves
        .push(Curve {
            id: carrier,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(spline)),
            parameter_range: None,
            source_object: None,
            native_ref: None,
        });
    let resolved = Picked {
        resolved: Some(vec![EntityRef::Edge(edge)]),
        ..pick(PickTarget::Native { within: None })
    };
    ir.design.features.push(datum(
        "edges",
        DatumElement::Points(vec![point_at(selection(named(vec![resolved])), 0.25)]),
    ));
    normalize(&mut ir).unwrap();
    let DatumElement::Points(points) = datum_element(&ir, "edges") else {
        panic!("points");
    };
    assert_eq!(
        points[0].inputs[0].value,
        Some(ScalarDimension::ParameterRatio(Dimension::Literal(real(
            0.25
        ))))
    );
}

#[test]
fn process_records_take_their_one_spelling() {
    let mut ir = document();
    let workpiece = WorkpieceSelection {
        workpiece: None,
        selection: Selection {
            at: Anchor::Input,
            picks: rule(vec![explicit(vec![picks_feature("a")])]),
            native: None,
        },
    };
    let end = ProcessConstruction {
        workpiece: None,
        construction: point_at(selection(named(vec![picks_feature("fillet")])), 1.0),
    };
    let geometry = |source| ProcessGeometry {
        role: GeometryRole::Machine,
        source,
        slot: None,
        combine: None,
    };
    ir.process.features.push(MachiningFeature {
        id: id("machining", "pocket"),
        name: None,
        class: MachiningFeatureClass::ClosedPocket {},
        native_kind: NativeName {
            codec: CodecId::from(NonBlankString::new("test").unwrap()),
            value: NativeNameValue::Text(NonBlankString::new("pocket").unwrap()),
        },
        geometry: vec![
            geometry(GeometrySource::Selection(vec![workpiece])),
            geometry(GeometrySource::Points(vec![end])),
        ],
        native_ref: None,
    });
    ir.process.states.push(ProcessState {
        id: id("process", "stock"),
        of: ProcessStateOf::Stock(id("setup", "one")),
        configuration: None,
        status: EvaluationStatus::Current,
        content: Some(ProcessContent {
            base: ProcessBase::Design(StateBase::Current(after_body())),
            delta: StateDelta::default(),
        }),
        feature: None,
    });
    normalize(&mut ir).unwrap();
    let [selected, points] = ir.process.features[0].geometry.as_slice() else {
        panic!("two geometries");
    };
    let GeometrySource::Selection(selections) = &selected.source else {
        panic!("a selection");
    };
    assert_eq!(
        selections[0].selection.picks,
        named(vec![picks_feature("a")])
    );
    let GeometrySource::Points(points) = &points.source else {
        panic!("points");
    };
    assert_eq!(
        points[0].construction.inputs[0].value,
        Some(ScalarDimension::Ratio(Dimension::Literal(real(1.0))))
    );
    assert_eq!(
        ir.process.states[0].content.as_ref().unwrap().base,
        ProcessBase::Design(StateBase::Current(after_feature("fillet")))
    );
}

/// `document()` placed in an assembly: definition `root` owns occurrence
/// `cube` of `part` and occurrence `other` of `other`, and `part` owns
/// occurrence `inner` of the embedded definition `inner`. `part`'s scope
/// spans `inner`, which has no scope of its own, and holds the
/// configurations `base`, its active one, and `large`. `root` and `other`
/// have scopes with no configuration.
fn placed() -> CadIr {
    let mut ir = document();
    for key in ["root", "part", "inner", "other"] {
        ir.model.product_definitions.push(ProductDefinition::new(
            id("definition", key),
            Storage::Embedded,
        ));
    }
    for (owner, key, prototype) in [
        ("root", "cube", "part"),
        ("root", "other", "other"),
        ("part", "inner", "inner"),
    ] {
        let occurrence: OccurrenceId = id("occurrence", key);
        let owner: ProductDefinitionId = id("definition", owner);
        ir.model
            .product_definitions
            .iter_mut()
            .find(|definition| definition.id == owner)
            .unwrap()
            .occurrences
            .push(occurrence.clone());
        ir.model.occurrences.push(Occurrence::new(
            occurrence,
            id("definition", prototype),
            None,
        ));
    }
    for key in ["root", "other"] {
        ir.design.scopes.push(DesignScope {
            definition: id("definition", key),
            roots: Vec::new(),
            active_configuration: None,
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current: Vec::new(),
            unproduced: BTreeSet::new(),
        });
    }
    for key in ["base", "large"] {
        ir.design.configurations.push(DesignConfiguration {
            id: id("configuration", key),
            scope: id("definition", "part"),
            name: key.to_owned(),
            parent: None,
            comment: None,
            overrides: Overrides::default(),
            active_arrangements: BTreeSet::new(),
            materials: Vec::new(),
            native_ref: None,
        });
    }
    ir
}

/// The path through the occurrences `keys`, outermost first, each step
/// reading the configuration paired with it where one is stated.
fn steps(keys: &[(&str, Option<&str>)]) -> OccurrencePath {
    OccurrencePath(
        keys.iter()
            .map(|(key, configuration)| PathStep {
                occurrence: StepRef::Occurrence(id("occurrence", key)),
                configuration: configuration.map(|configuration| {
                    ConfigurationRef::Local(id("configuration", configuration))
                }),
            })
            .collect(),
    )
}

/// The root scope's entry for occurrence `cube`, selecting `configuration`.
fn cube_uses(ir: &mut CadIr, configuration: &str) {
    entry(ir, "root", "cube", local(configuration));
}

/// The scope of `definition`.
fn scope_of<'a>(ir: &'a mut CadIr, definition: &str) -> &'a mut DesignScope {
    let definition: ProductDefinitionId = id("definition", definition);
    ir.design
        .scopes
        .iter_mut()
        .find(|scope| scope.definition == definition)
        .unwrap()
}

/// The entry of `definition`'s scope for `occurrence`, selecting
/// `configuration`.
fn entry(ir: &mut CadIr, definition: &str, occurrence: &str, configuration: ConfigurationRef) {
    scope_of(ir, definition).occurrences.insert(
        id("occurrence", occurrence),
        OccurrenceDesign {
            configuration: Some(configuration),
            arrangements: Vec::new(),
            grounded: None,
            flexible: None,
            parameters: BTreeMap::new(),
        },
    );
}

fn local(key: &str) -> ConfigurationRef {
    ConfigurationRef::Local(id("configuration", key))
}

fn by_name(name: &str) -> ConfigurationRef {
    ConfigurationRef::Named(NonBlankString::new(name).unwrap())
}

/// The step through occurrence `key`, selecting `configuration`.
fn step(key: &str, configuration: Option<ConfigurationRef>) -> PathStep {
    PathStep {
        occurrence: StepRef::Occurrence(id("occurrence", key)),
        configuration,
    }
}

/// Makes `key` the root scope's active configuration: a configuration of
/// `root` whose parent is `parent` and whose override for `occurrence` is
/// `entry`.
fn root_configured(
    ir: &mut CadIr,
    key: &str,
    parent: Option<&str>,
    occurrence: &str,
    entry: OccurrenceOverride,
) {
    let mut overrides = Overrides::default();
    overrides
        .occurrences
        .insert(id("occurrence", occurrence), entry);
    ir.design.configurations.push(DesignConfiguration {
        id: id("configuration", key),
        scope: id("definition", "root"),
        name: key.to_owned(),
        parent: parent.map(|parent| id("configuration", parent)),
        comment: None,
        overrides,
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    });
    scope_of(ir, "root").active_configuration = Some(id("configuration", key));
}

/// The state `in_base` names, spelled through its member.
fn base_state() -> StateRef {
    state(Some("base"), after_feature("fillet"))
}

/// The states of the first annotation's targets once `ir` is normalized.
fn normalized_states(mut ir: CadIr) -> Vec<Option<StateRef>> {
    normalize(&mut ir).unwrap();
    ir.model.pmi[0]
        .targets
        .iter()
        .map(|target| target.state.clone())
        .collect()
}

/// A target on the cube's body at `path`, in the current state of `base`.
fn in_base(path: OccurrencePath) -> InstanceTarget<PmiTarget> {
    InstanceTarget {
        path: Some(path),
        ..target(Some(state(Some("base"), after_body())))
    }
}

#[test]
fn a_target_whose_path_selects_the_active_configuration_states_none() {
    let mut ir = placed();
    ir.model.pmi[0].targets[1].path = Some(path(None));
    normalize(&mut ir).unwrap();
    assert_eq!(ir.model.pmi[0].targets[1].state, None);
}

#[test]
fn a_target_whose_path_selects_another_configuration_keeps_its_state() {
    let mut ir = placed();
    ir.model.pmi[0].targets[1].path = Some(path(Some("large")));
    ir.model.pmi[0].targets.push(in_base(path(None)));
    cube_uses(&mut ir, "large");
    normalize(&mut ir).unwrap();
    let kept = Some(state(Some("base"), after_feature("fillet")));
    assert_eq!(ir.model.pmi[0].targets[1].state, kept);
    assert_eq!(ir.model.pmi[0].targets[4].state, kept);
}

#[test]
fn a_target_in_a_spanned_definition_keeps_its_state() {
    let mut ir = placed();
    ir.model.pmi[0].targets = vec![
        in_base(steps(&[("cube", None), ("inner", None)])),
        in_base(steps(&[("cube", Some("base")), ("inner", None)])),
    ];
    assert_eq!(normalized_states(ir), vec![Some(base_state()); 2]);
}

#[test]
fn a_target_through_an_occurrence_of_another_scope_keeps_its_state() {
    let mut ir = placed();
    ir.model.pmi[0].targets = vec![
        in_base(steps(&[("other", None)])),
        in_base(steps(&[("absent", None)])),
    ];
    normalize(&mut ir).unwrap();
    let kept = Some(state(Some("base"), after_feature("fillet")));
    assert!(ir.model.pmi[0]
        .targets
        .iter()
        .all(|target| target.state == kept));
}

/// `placed()` where the root's active configuration `swap` replaces the
/// prototype of occurrence `other` with `prototype`.
fn other_replaced_by(prototype: &str) -> CadIr {
    let mut ir = placed();
    root_configured(
        &mut ir,
        "swap",
        None,
        "other",
        OccurrenceOverride {
            prototype: Some(id("definition", prototype)),
            ..OccurrenceOverride::default()
        },
    );
    ir
}

#[test]
fn a_target_through_an_occurrence_whose_prototype_is_replaced_keeps_its_state() {
    let mut ir = other_replaced_by("part");
    ir.model.pmi[0].targets = vec![in_base(steps(&[("other", None)]))];
    assert_eq!(normalized_states(ir), vec![Some(base_state())]);
}

#[test]
fn a_path_into_the_prototype_a_configuration_replaced_keeps_the_state() {
    let through_deep = |mut ir: CadIr| {
        place(&mut ir, "other", "deep", "part");
        ir.model.pmi[0].targets = vec![in_base(steps(&[("other", None), ("deep", None)]))];
        normalized_states(ir)
    };
    assert_eq!(through_deep(placed()), vec![None]);
    assert_eq!(
        through_deep(other_replaced_by("inner")),
        vec![Some(base_state())]
    );
}

#[test]
fn a_base_selection_of_the_replaced_prototype_keeps_the_state() {
    let mut ir = other_replaced_by("part");
    // `alt` is a configuration of `other`, the original prototype.
    ir.design.configurations.push(DesignConfiguration {
        id: id("configuration", "alt"),
        scope: id("definition", "other"),
        name: "alt".to_owned(),
        parent: None,
        comment: None,
        overrides: Overrides::default(),
        active_arrangements: BTreeSet::new(),
        materials: Vec::new(),
        native_ref: None,
    });
    entry(&mut ir, "root", "other", local("alt"));
    ir.model.pmi[0].targets = vec![in_base(steps(&[("other", None)]))];
    assert_eq!(normalized_states(ir), vec![Some(base_state())]);
}

#[test]
fn a_step_that_selects_a_configuration_by_name_keeps_the_state() {
    let mut ir = placed();
    ir.model.pmi[0].targets = vec![
        in_base(OccurrencePath(vec![step("cube", Some(by_name("Big")))])),
        in_base(OccurrencePath(vec![step("cube", Some(by_name("large")))])),
    ];
    assert_eq!(normalized_states(ir), vec![Some(base_state()); 2]);
}

#[test]
fn a_scope_entry_or_override_that_selects_a_configuration_by_name_keeps_the_state() {
    let only_cube = |mut ir: CadIr| {
        ir.model.pmi[0].targets = vec![in_base(steps(&[("cube", None)]))];
        normalized_states(ir)
    };
    let mut in_entry = placed();
    entry(&mut in_entry, "root", "cube", by_name("Big"));
    let mut in_override = placed();
    root_configured(
        &mut in_override,
        "variant",
        None,
        "cube",
        OccurrenceOverride {
            configuration: Some(by_name("Big")),
            ..OccurrenceOverride::default()
        },
    );
    let mut to_large = placed();
    root_configured(
        &mut to_large,
        "variant",
        None,
        "cube",
        OccurrenceOverride {
            configuration: Some(local("large")),
            ..OccurrenceOverride::default()
        },
    );
    assert_eq!(
        [in_entry, in_override, to_large].map(only_cube),
        [
            vec![Some(base_state())],
            vec![Some(base_state())],
            vec![Some(base_state())]
        ]
    );
}

#[test]
fn an_ancestor_that_selects_a_configuration_by_name_keeps_the_state_below_it() {
    let mut ir = placed();
    ir.model.pmi[0].targets = vec![
        in_base(OccurrencePath(vec![
            step("cube", Some(by_name("Big"))),
            step("inner", None),
        ])),
        in_base(OccurrencePath(vec![
            step("cube", None),
            step("inner", Some(by_name("Big"))),
        ])),
    ];
    let mut in_entry = placed();
    entry(&mut in_entry, "part", "inner", by_name("Big"));
    in_entry.model.pmi[0].targets = vec![in_base(steps(&[("cube", None), ("inner", None)]))];
    let mut states = normalized_states(ir);
    states.extend(normalized_states(in_entry));
    assert_eq!(states, vec![Some(base_state()); 3]);
}

#[test]
fn a_selection_of_a_configuration_the_scope_does_not_hold_keeps_the_state() {
    let mut ir = placed();
    ir.model.pmi[0].targets = vec![
        in_base(steps(&[("cube", Some("gone"))])),
        in_base(steps(&[("cube", Some("gone")), ("inner", None)])),
    ];
    let mut in_entry = placed();
    cube_uses(&mut in_entry, "gone");
    in_entry.model.pmi[0].targets = vec![in_base(steps(&[("cube", None)]))];
    let mut states = normalized_states(ir);
    states.extend(normalized_states(in_entry));
    assert_eq!(states, vec![Some(base_state()); 3]);
}

#[test]
fn a_path_whose_instance_the_resolver_cannot_establish_keeps_the_state() {
    let with_path = |mut ir: CadIr, path: OccurrencePath| {
        ir.model.pmi[0].targets = vec![in_base(path)];
        normalized_states(ir)
    };
    let cube = || steps(&[("cube", None)]);
    let mut native = cube();
    native.0.push(PathStep {
        occurrence: StepRef::Native(NativeName {
            codec: CodecId::from(NonBlankString::new("test").unwrap()),
            value: NativeNameValue::Text(NonBlankString::new("n").unwrap()),
        }),
        configuration: None,
    });
    let mut replaced = placed();
    root_configured(
        &mut replaced,
        "swap",
        None,
        "cube",
        OccurrenceOverride {
            prototype: Some(id("definition", "other")),
            ..OccurrenceOverride::default()
        },
    );
    let mut cycle = placed();
    root_configured(
        &mut cycle,
        "loop",
        Some("loop"),
        "cube",
        OccurrenceOverride::default(),
    );
    let mut root_dangling = placed();
    scope_of(&mut root_dangling, "root").active_configuration = Some(id("configuration", "gone"));
    let mut part_dangling = placed();
    scope_of(&mut part_dangling, "part").active_configuration = Some(id("configuration", "gone"));
    part_dangling.model.pmi[0].targets = vec![InstanceTarget {
        path: Some(cube()),
        ..target(Some(state(Some("gone"), after_body())))
    }];
    let mut unowned = placed();
    unowned.model.occurrences.push(Occurrence::new(
        id("occurrence", "loose"),
        id("definition", "inner"),
        None,
    ));
    let mut unowned_part = placed();
    unowned_part.model.occurrences.push(Occurrence::new(
        id("occurrence", "loose"),
        id("definition", "part"),
        None,
    ));
    let mut twice = placed();
    twice.model.occurrences.push(Occurrence::new(
        id("occurrence", "cube"),
        id("definition", "other"),
        None,
    ));
    let mut foreign_active = placed();
    scope_of(&mut foreign_active, "root").active_configuration = Some(id("configuration", "base"));
    let mut arranged = placed();
    arranged
        .design
        .arrangements
        .push(arrangement("shown", "root", Overrides::default()));
    scope_of(&mut arranged, "root")
        .active_arrangements
        .insert(id("arrangement", "shown"));
    let mut arranged_in_configuration = placed();
    root_configured(
        &mut arranged_in_configuration,
        "shown",
        None,
        "cube",
        OccurrenceOverride::default(),
    );
    arranged_in_configuration
        .design
        .arrangements
        .push(arrangement("shown", "root", Overrides::default()));
    arranged_in_configuration
        .design
        .configurations
        .last_mut()
        .unwrap()
        .active_arrangements
        .insert(id("arrangement", "shown"));
    let outcomes = [
        ("native", with_path(placed(), native)),
        ("empty", with_path(placed(), OccurrencePath(Vec::new()))),
        ("replaced", with_path(replaced, cube())),
        ("cycle", with_path(cycle, cube())),
        ("root dangling", with_path(root_dangling, cube())),
        ("unowned", with_path(unowned, steps(&[("loose", None)]))),
        (
            "unowned part",
            with_path(unowned_part, steps(&[("loose", None)])),
        ),
        ("twice", with_path(twice, cube())),
        (
            "not a chain",
            with_path(placed(), steps(&[("other", None), ("cube", None)])),
        ),
        ("foreign active", with_path(foreign_active, cube())),
        ("arranged", with_path(arranged, cube())),
        (
            "arranged in configuration",
            with_path(arranged_in_configuration, cube()),
        ),
    ];
    assert_eq!(
        outcomes,
        outcomes
            .clone()
            .map(|(case, _)| (case, vec![Some(base_state())]))
    );
    assert_eq!(
        normalized_states(part_dangling),
        [Some(state(Some("gone"), after_feature("fillet")))]
    );
    let mut other_state = placed();
    other_state.model.pmi[0].targets = vec![InstanceTarget {
        path: Some(cube()),
        ..target(Some(state(Some("large"), after_body())))
    }];
    assert_eq!(
        normalized_states(other_state),
        [Some(state(Some("large"), after_feature("fillet")))]
    );
}

/// Adds occurrence `key` of `prototype`, owned by `owner`.
fn place(ir: &mut CadIr, owner: &str, key: &str, prototype: &str) {
    let occurrence: OccurrenceId = id("occurrence", key);
    let owner: ProductDefinitionId = id("definition", owner);
    ir.model
        .product_definitions
        .iter_mut()
        .find(|definition| definition.id == owner)
        .unwrap()
        .occurrences
        .push(occurrence.clone());
    ir.model.occurrences.push(Occurrence::new(
        occurrence,
        id("definition", prototype),
        None,
    ));
}

/// Adds the embedded definition `key`, with a scope that holds no
/// configuration where `scoped`.
fn define(ir: &mut CadIr, key: &str, scoped: bool) {
    ir.model.product_definitions.push(ProductDefinition::new(
        id("definition", key),
        Storage::Embedded,
    ));
    if scoped {
        ir.design.scopes.push(DesignScope {
            definition: id("definition", key),
            roots: Vec::new(),
            active_configuration: None,
            active_arrangements: BTreeSet::new(),
            occurrences: BTreeMap::new(),
            current: Vec::new(),
            unproduced: BTreeSet::new(),
        });
    }
}

/// `placed()` one level down: definition `outer`, whose scope holds no
/// configuration, owns occurrence `sub` of `root`.
fn nested() -> CadIr {
    let mut ir = placed();
    define(&mut ir, "outer", true);
    place(&mut ir, "outer", "sub", "root");
    ir
}

/// An arrangement of `scope`'s scope with `overrides`.
fn arrangement(key: &str, scope: &str, overrides: Overrides) -> Arrangement {
    Arrangement {
        id: id("arrangement", key),
        scope: id("definition", scope),
        name: NonBlankString::new(key).unwrap(),
        kind: ArrangementKind::Positional,
        configuration: None,
        overrides,
        appearances: Vec::new(),
        moves: Vec::new(),
        nested: Vec::new(),
        native_ref: None,
    }
}

#[test]
fn a_target_whose_path_nothing_selects_through_states_none() {
    let mut ir = nested();
    let through = || steps(&[("sub", None), ("cube", None)]);
    ir.model.pmi[0].targets = vec![
        target(Some(state(Some("base"), after_body()))),
        in_base(steps(&[("cube", None)])),
        in_base(through()),
        InstanceTarget {
            path: Some(through()),
            ..target(Some(state(None, after_body())))
        },
    ];
    assert_eq!(normalized_states(ir), [None, None, None, None]);
}

#[test]
fn an_arrangement_an_ancestor_uses_keeps_the_state() {
    let mut ir = nested();
    let mut overrides = Overrides::default();
    overrides.occurrences.insert(
        id("occurrence", "cube"),
        OccurrenceOverride {
            configuration: Some(local("large")),
            ..OccurrenceOverride::default()
        },
    );
    ir.design
        .arrangements
        .push(arrangement("b", "root", overrides));
    let uses = |reference: ArrangementRef| {
        let mut ir = ir.clone();
        scope_of(&mut ir, "outer").occurrences.insert(
            id("occurrence", "sub"),
            OccurrenceDesign {
                configuration: None,
                arrangements: vec![reference],
                grounded: None,
                flexible: None,
                parameters: BTreeMap::new(),
            },
        );
        ir.model.pmi[0].targets = vec![
            in_base(steps(&[("sub", None), ("cube", None)])),
            in_base(steps(&[("cube", None)])),
        ];
        normalized_states(ir)
    };
    let named = ArrangementRef::Named(NonBlankString::new("b").unwrap());
    assert_eq!(
        [
            uses(ArrangementRef::Local(id("arrangement", "b"))),
            uses(named)
        ],
        [vec![Some(base_state()); 2], vec![Some(base_state()); 2]]
    );
}

#[test]
fn a_path_that_starts_inside_a_spanned_definition_keeps_the_state_its_scope_selects() {
    let mut ir = placed();
    define(&mut ir, "mid", false);
    place(&mut ir, "root", "m", "mid");
    place(&mut ir, "mid", "deep", "part");
    ir.model.pmi[0].targets = vec![in_base(steps(&[("deep", None)]))];
    let unselected = normalized_states(ir.clone());
    entry(&mut ir, "root", "deep", local("large"));
    assert_eq!(
        [unselected, normalized_states(ir)],
        [vec![None], vec![Some(base_state())]]
    );
}

#[test]
fn a_state_with_no_configuration_stays_where_the_path_selects_another() {
    let mut ir = placed();
    let none_at = |path: OccurrencePath| InstanceTarget {
        path: Some(path),
        ..target(Some(state(None, after_body())))
    };
    ir.model.pmi[0].targets = vec![
        none_at(steps(&[("cube", Some("large"))])),
        none_at(steps(&[("cube", None)])),
    ];
    assert_eq!(
        normalized_states(ir),
        [Some(state(None, after_feature("fillet"))), None]
    );
}

#[test]
fn a_definition_stored_in_a_file_of_its_own_is_not_spanned() {
    let mut states = Vec::new();
    for storage in [
        Storage::External(ExternalDocument::default()),
        Storage::File {
            document: ExternalDocument::default(),
            source: None,
        },
    ] {
        let mut ir = placed();
        let inner: ProductDefinitionId = id("definition", "inner");
        ir.model
            .product_definitions
            .iter_mut()
            .find(|definition| definition.id == inner)
            .unwrap()
            .storage = storage;
        ir.model.pmi[0].targets = vec![in_base(steps(&[("cube", Some("base")), ("inner", None)]))];
        states.extend(normalized_states(ir));
    }
    assert_eq!(states, vec![Some(base_state()); 2]);
}

/// A point at `ParameterRatio` 1 of nothing, inside `depth - 1` points
/// each coincident with the one it holds.
fn nested_point(depth: usize) -> Construction<kinds::Point> {
    let mut point = Construction {
        inputs: vec![ConstructionInput {
            role: Some(InputRole {
                subject: Subject::Whole,
                relation: Relation::AtParameter,
            }),
            reference: None,
            value: Some(ScalarDimension::ParameterRatio(Dimension::Literal(real(
                1.0,
            )))),
            reversed: None,
        }],
        after: Vec::new(),
        solution: None,
        mode: None,
        resolved: None,
        resolved_in: BTreeMap::new(),
    };
    for _ in 1..depth {
        point = Construction {
            inputs: vec![input(
                Relation::Coincident,
                InputReference::Built(Box::new(AnyConstruction::Point(point))),
                None,
            )],
            after: Vec::new(),
            solution: None,
            mode: None,
            resolved: None,
            resolved_in: BTreeMap::new(),
        };
    }
    point
}

/// The value of the innermost point of a datum's nested point.
fn innermost_value(ir: &CadIr, key: &str) -> Option<ScalarDimension> {
    let DatumElement::Points(points) = datum_element(ir, key) else {
        panic!("points");
    };
    let mut point = &points[0];
    while let Some(InputReference::Built(built)) = &point.inputs[0].reference {
        let AnyConstruction::Point(inner) = built.as_ref() else {
            panic!("a point");
        };
        point = inner;
    }
    point.inputs[0].value.clone()
}

/// Decodes the unit cube with a datum whose point nests 300 deep, past the
/// default recursion ceiling, and one whose point nests one deep.
struct DeepCodec;

impl CodecBackend for DeepCodec {
    const FORMAT: FormatId = FormatId::new("test");

    fn detect_impl(&self, _: &DecodeContext<'_>, _: View<'_>) -> Result<Confidence, CodecError> {
        Ok(Confidence::No)
    }

    fn inspect_impl(
        &self,
        _: &DecodeContext<'_>,
        _: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        panic!("the normalizer tests do not inspect")
    }

    fn decode_impl(&self, _: &DecodeContext<'_>, _: View<'_>) -> Result<Decoded, CodecError> {
        let mut ir = crate::examples::unit_cube().unwrap();
        ir.design
            .features
            .push(datum("deep", DatumElement::Points(vec![nested_point(300)])));
        ir.design.features.push(datum(
            "shallow",
            DatumElement::Points(vec![nested_point(1)]),
        ));
        Ok(Decoded {
            ir,
            body: DecodeBody::new(crate::report::decode::DecodeTransfer::full(true)),
            source_fidelity: SourceFidelity::new(crate::hash::digest::Sha256Digest::digest(
                b"deep",
            )),
        })
    }
}

fn decode_deep(policy: DecodePolicy) -> DecodeResult {
    let options = DecodeOptions {
        policy,
        ..DecodeOptions::default()
    };
    DeepCodec
        .decode(&mut std::io::Cursor::new(vec![1u8]), &options)
        .unwrap()
}

fn incomplete(result: &DecodeResult) -> Vec<&LossNote> {
    result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code.to_string() == "decode/normalize.incomplete")
        .collect()
}

#[test]
fn a_record_too_deep_to_normalize_is_kept_as_decoded_with_a_warning() {
    let result = decode_deep(DecodePolicy::default());
    let ratio = |value| Some(ScalarDimension::Ratio(Dimension::Literal(real(value))));
    assert_eq!(innermost_value(result.ir(), "shallow"), ratio(1.0));
    assert_eq!(
        innermost_value(result.ir(), "deep"),
        Some(ScalarDimension::ParameterRatio(Dimension::Literal(real(
            1.0
        ))))
    );
    let warnings = incomplete(&result);
    let [warning] = warnings.as_slice() else {
        panic!("one normalizer warning");
    };
    assert_eq!(warning.severity, Severity::Warning);
    assert!(warning
        .message
        .split_whitespace()
        .any(|token| token == feature("deep").as_str()));
}

#[test]
fn the_normalizer_reads_the_callers_ceilings() {
    let mut policy = DecodePolicy::default();
    policy.limits.max_recursion_depth = 1024;
    let result = decode_deep(policy);
    let ratio = Some(ScalarDimension::Ratio(Dimension::Literal(real(1.0))));
    assert_eq!(innermost_value(result.ir(), "deep"), ratio);
    assert!(incomplete(&result).is_empty());
}
