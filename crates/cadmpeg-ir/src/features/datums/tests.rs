use super::{DatumAxis, DatumAxisDraft};
use crate::design::construction::{
    AnyConstruction, Construction, ConstructionInput, InputReference,
};
use crate::design::dimension::Dimension;
use crate::design::options::NativeOption;
use crate::features::geometry::{FeatureDirection3, FinitePoint3};
use crate::features::selections::DistinctMembers;
use crate::features::{FeatureContent, FeatureOperation, ParameterId, UnresolvedFamily};
use crate::math::{Point3, Vector3};
use crate::schema::rewrite::typed::{IdentityMap, RewriteIdentities, TextReplacements};

mod validation;

fn origin() -> FinitePoint3 {
    FinitePoint3::new(Point3::new(10.0, 20.0, 30.0)).unwrap()
}
fn direction() -> FeatureDirection3 {
    FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap()
}

#[test]
fn datum_axis_partial_states_have_one_checked_wire_spelling() {
    let input = ConstructionInput {
        role: None,
        reference: Some(InputReference::Built(Box::new(AnyConstruction::Point(
            Construction {
                resolved: Some(origin()),
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved_in: std::collections::BTreeMap::new(),
            },
        )))),
        value: None,
        reversed: None,
    };
    let option = std::collections::BTreeMap::from([(
        crate::codec_name!("f3d:ref_type"),
        NativeOption::Integer(Dimension::Literal(19)),
    )]);
    for draft in [
        DatumAxisDraft {
            origin: Some(origin()),
            direction: Some(direction()),
            ..Default::default()
        },
        DatumAxisDraft {
            origin: Some(origin()),
            ..Default::default()
        },
        DatumAxisDraft {
            direction: Some(direction()),
            ..Default::default()
        },
        DatumAxisDraft {
            inputs: vec![input],
            ..Default::default()
        },
        DatumAxisDraft {
            native_options: option,
            ..Default::default()
        },
    ] {
        let axis = DatumAxis::try_from(draft).unwrap();
        let text = serde_json::to_string(&axis).unwrap();
        let back: DatumAxis = serde_json::from_str(&text).unwrap();
        assert_eq!(back, axis);
        assert_eq!(serde_json::to_string(&back).unwrap(), text);
    }
    assert!(DatumAxis::try_from(DatumAxisDraft::default()).is_err());
    assert!(matches!(
        DatumAxisDraft::default().into_operation().unwrap(),
        FeatureOperation::Unresolved {
            family: UnresolvedFamily::DatumAxis
        }
    ));
    assert!(serde_json::from_str::<DatumAxis>(
        r#"{"inputs":[],"native_options":{},"source_settings":{}}"#
    )
    .is_err());
    assert!(serde_json::from_str::<DatumAxis>(r#"{"origin":null,"direction":{"x":0,"y":0,"z":1},"inputs":[],"native_options":{},"source_settings":{}}"#).is_err());
}

#[test]
fn datum_axis_walks_bound_dimensions_and_preserves_colliding_option_literals() {
    let source = "test:model:parameter#one";
    let target = "test:placed:parameter#one";
    let replacements = std::collections::BTreeMap::from([(source.to_owned(), target.to_owned())]);
    let replacements = TextReplacements::build(replacements.iter()).unwrap();
    let options = std::collections::BTreeMap::from([
        (
            crate::design::names::CodecName::try_from(source.to_owned()).unwrap(),
            NativeOption::Text(Dimension::Literal(
                cadmpeg_core::text::NonBlankString::new(source).unwrap(),
            )),
        ),
        (
            crate::design::names::CodecName::try_from(target.to_owned()).unwrap(),
            NativeOption::Real(Dimension::Negated(ParameterId::mint(source).unwrap())),
        ),
    ]);
    let axis = DatumAxis::try_from(DatumAxisDraft {
        native_options: options,
        ..Default::default()
    })
    .unwrap();
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut seen = Vec::new();
    axis.visit_identity_references(&ctx, &mut |identity| {
        seen.push(identity.to_owned());
        Ok(())
    })
    .unwrap();
    assert_eq!(seen, [source]);
    let mut map =
        IdentityMap::new(|identity: &str| Ok(identity.replace("test:model:", "test:placed:")))
            .with_text_replacements(&replacements);
    let rewritten = axis.rewrite_identities(&ctx, &mut map).unwrap();
    map.finish().unwrap();
    assert_eq!(rewritten.native_options().len(), 2);
    assert!(
        matches!(&rewritten.native_options()[source], NativeOption::Text(Dimension::Literal(text)) if text.as_str() == source)
    );
    assert!(
        matches!(&rewritten.native_options()[target], NativeOption::Real(Dimension::Negated(parameter)) if parameter.as_str() == target)
    );
}

#[cfg(feature = "schema")]
#[test]
fn datum_axis_schema_requires_a_readable_fact_and_nonnull_geometry() {
    let schema = serde_json::to_value(schemars::schema_for!(DatumAxis)).unwrap();
    assert_eq!(schema["allOf"][0]["anyOf"].as_array().unwrap().len(), 5);
    assert_eq!(
        schema["allOf"][0]["anyOf"][2]["properties"]["inputs"]["minItems"],
        1
    );
    assert!(!schema["properties"]["origin"].to_string().contains("null"));
    assert!(!schema["properties"]["direction"]
        .to_string()
        .contains("null"));
}

fn feature(operation: FeatureOperation, ordinal: u64) -> crate::features::Feature {
    crate::features::Feature {
        product_definition: None,
        id: format!("test:model:feature#{ordinal}").try_into().unwrap(),
        ordinal,
        name: None,
        suppressed: None,
        dependencies: DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: None,
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: crate::features::evaluation::FeatureEvaluation::from_definition(
            crate::features::FeatureDefinition::Operation(operation),
        ),
        native_ref: None,
    }
}

#[test]
fn datum_axis_validation_reaches_bound_parameter_owners_and_nested_subjects() {
    use crate::features::parameters::{DesignParameter, ParameterValue};
    let mut ir = crate::CadIr::empty();
    let prior = feature(FeatureOperation::datum_axis(origin(), direction()), 1);
    let parameter: ParameterId = "test:model:parameter#length".try_into().unwrap();
    ir.model.parameters.push(DesignParameter {
        product_definition: None,
        id: parameter.clone(),
        owner: Some(prior.id.clone()),
        ordinal: 0,
        name: "Offset".into(),
        expression: "1 mm".into(),
        display: None,
        value: Some(ParameterValue::Length(
            crate::scalar::Length::new(1.0).unwrap(),
        )),
        dependencies: DistinctMembers::default(),
        properties: std::collections::BTreeMap::new(),
        pmi: None,
        native_ref: None,
    });
    let draft = DatumAxisDraft {
        origin: Some(origin()),
        direction: Some(direction()),
        native_options: std::collections::BTreeMap::from([(
            crate::codec_name!("test:offset"),
            NativeOption::Length(Dimension::Parameter(parameter)),
        )]),
        ..Default::default()
    };
    ir.model.features.push(prior);
    ir.model
        .features
        .push(feature(draft.into_operation().unwrap(), 2));
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|f| f.message.contains("parameter owner must precede")));
    let prior = ir.model.features[0].id.clone();
    ir.model.features[1].dependencies.insert(prior);
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(
        !report
            .findings
            .iter()
            .any(|f| f.message.contains("datum-axis")),
        "{:?}",
        report.findings
    );
    ir.model.parameters[0].value = Some(ParameterValue::Boolean(true));
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|f| f.message.contains("incompatible value kind")));
    let bad = DatumAxisDraft {
        inputs: vec![ConstructionInput {
            role: Some(crate::design::construction::InputRole {
                subject: crate::design::construction::Subject::Origin,
                relation: crate::design::construction::Relation::Parallel,
            }),
            reference: Some(InputReference::Built(Box::new(AnyConstruction::Point(
                Construction {
                    resolved: Some(origin()),
                    inputs: Vec::new(),
                    after: Vec::new(),
                    solution: None,
                    mode: None,
                    resolved_in: std::collections::BTreeMap::new(),
                },
            )))),
            value: None,
            reversed: None,
        }],
        ..Default::default()
    };
    ir.model.features[1]
        .evaluation
        .set_definition(crate::features::FeatureDefinition::Operation(
            bad.into_operation().unwrap(),
        ));
    let report = crate::validate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(report
        .findings
        .iter()
        .any(|f| f.message.contains("construction subject")));
    assert!(report
        .findings
        .iter()
        .any(|f| f.message.contains("point operand cannot supply")));
}

#[test]
fn axis_source_literals_preserve_both_collision_keys_while_other_feature_maps_keep_identity_behavior(
) {
    let source = "test:model:feature#1";
    let destination = "test:placed:feature#1";
    let mut axis = feature(FeatureOperation::datum_axis(origin(), direction()), 1);
    axis.source_properties = std::collections::BTreeMap::from([
        (
            cadmpeg_core::text::NonBlankString::new(source).unwrap(),
            source.into(),
        ),
        (
            cadmpeg_core::text::NonBlankString::new(destination).unwrap(),
            destination.into(),
        ),
    ]);
    axis.name = Some(source.into());
    axis.source_tag = Some(source.into());
    axis.source_text = Some(source.into());
    axis.source_content
        .0
        .push(crate::features::FeatureSourceContent::Text(source.into()));
    let mut other = feature(
        FeatureOperation::DatumPoint {
            position: origin(),
            construction: None,
        },
        2,
    );
    other
        .source_properties
        .insert(cadmpeg_core::nonblank_literal!("profile"), source.into());
    let replacements =
        std::collections::BTreeMap::from([(source.to_owned(), destination.to_owned())]);
    let replacements = TextReplacements::build(replacements.iter()).unwrap();
    let ctx = cadmpeg_core::decode::test_support::service_decode_context();
    let mut map = IdentityMap::new(|id: &str| Ok(id.replace("test:model:", "test:placed:")))
        .with_text_replacements(&replacements);
    let axis = axis.rewrite_identities(&ctx, &mut map).unwrap();
    let other = other.rewrite_identities(&ctx, &mut map).unwrap();
    map.finish().unwrap();
    assert_eq!(axis.id.as_str(), destination);
    assert_eq!(axis.source_properties.len(), 2);
    assert_eq!(
        axis.source_properties[&cadmpeg_core::text::NonBlankString::new(source).unwrap()],
        source
    );
    assert_eq!(axis.name.as_deref(), Some(source));
    assert_eq!(axis.source_tag.as_deref(), Some(source));
    assert_eq!(axis.source_text.as_deref(), Some(source));
    assert!(
        matches!(&axis.source_content.0[0],crate::features::FeatureSourceContent::Text(text) if text==source)
    );
    assert_eq!(
        other.source_properties[&cadmpeg_core::nonblank_literal!("profile")],
        destination
    );
}

#[test]
fn datum_axis_validation_reaches_solved_plane_frame_and_curve_values() {
    use crate::design::construction::{CurveValue, FrameValue, PlaneValue};
    use crate::units::UnitVector3;
    let unit = |x, y, z| UnitVector3::new(Vector3::new(x, y, z)).unwrap();
    let frame = FrameValue {
        origin: origin(),
        x_axis: unit(1.0, 0.0, 0.0),
        y_axis: unit(0.0, 1.0, 0.0),
        z_axis: unit(0.0, 0.0, 1.0),
    };
    let cases = [
        (
            AnyConstruction::Frame(Construction {
                resolved: Some(frame.clone()),
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved_in: std::collections::BTreeMap::new(),
            }),
            None,
        ),
        (
            AnyConstruction::Plane(Construction {
                resolved: Some(PlaneValue {
                    origin: origin(),
                    normal: frame.z_axis,
                    x_axis: frame.z_axis,
                }),
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved_in: std::collections::BTreeMap::new(),
            }),
            Some("plane axes must be perpendicular"),
        ),
        (
            AnyConstruction::Frame(Construction {
                resolved: Some(FrameValue {
                    z_axis: unit(0.0, 0.0, -1.0),
                    ..frame.clone()
                }),
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved_in: std::collections::BTreeMap::new(),
            }),
            Some("orthonormal and right-handed"),
        ),
        (
            AnyConstruction::Curve(Construction {
                resolved: Some(CurveValue::Arc {
                    center: FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(),
                    normal: frame.z_axis,
                    radius: crate::scalar::PositiveLength::new(1.0).unwrap(),
                    start: FinitePoint3::new(Point3::new(2.0, 0.0, 0.0)).unwrap(),
                    end: FinitePoint3::new(Point3::new(0.0, 1.0, 0.0)).unwrap(),
                }),
                inputs: Vec::new(),
                after: Vec::new(),
                solution: None,
                mode: None,
                resolved_in: std::collections::BTreeMap::new(),
            }),
            Some("arc endpoint is outside"),
        ),
    ];
    for (built, expected) in cases {
        let mut ir = crate::CadIr::empty();
        ir.model.features.push(feature(
            DatumAxisDraft {
                inputs: vec![ConstructionInput {
                    role: None,
                    reference: Some(InputReference::Built(Box::new(built))),
                    value: None,
                    reversed: None,
                }],
                ..Default::default()
            }
            .into_operation()
            .unwrap(),
            1,
        ));
        let findings = crate::validate_neutral(&ir, Vec::new()).unwrap().findings;
        if let Some(expected) = expected {
            assert!(
                findings.iter().any(|f| f.message.contains(expected)),
                "{findings:?}"
            );
        } else {
            assert!(
                findings.iter().all(|f| !f.message.contains("construction")),
                "{findings:?}"
            );
        }
    }
}

#[test]
fn datum_axis_validation_keeps_curve_origins_and_two_pick_parameters() {
    use crate::design::construction::{InputRole, Relation, ScalarDimension, Subject};
    use crate::design::selection::OrderedSelection;
    let picks: OrderedSelection = serde_json::from_value(serde_json::json!({
        "at":{"kind":"input"}, "picks":{"kind":"named","value":[
            {"target":{"kind":"principal","value":{"entity":"x_axis"}}},
            {"target":{"kind":"principal","value":{"entity":"origin"}}}
        ]}
    }))
    .unwrap();
    let coordinates = ConstructionInput {
        role: Some(InputRole {
            subject: Subject::Origin,
            relation: Relation::Coordinates,
        }),
        reference: None,
        value: Some(ScalarDimension::Position {
            x: Dimension::Literal(crate::scalar::Length::new(1.0).unwrap()),
            y: Dimension::Literal(crate::scalar::Length::new(2.0).unwrap()),
            z: Dimension::Literal(crate::scalar::Length::new(3.0).unwrap()),
        }),
        reversed: None,
    };
    let mut ir = crate::CadIr::empty();
    ir.model.features.push(feature(
        DatumAxisDraft {
            inputs: vec![
                ConstructionInput {
                    role: Some(InputRole {
                        subject: Subject::Whole,
                        relation: Relation::AtParameter,
                    }),
                    reference: Some(InputReference::Picks(picks)),
                    value: Some(ScalarDimension::Ratio(Dimension::Literal(
                        crate::scalar::FiniteReal::new(0.5).unwrap(),
                    ))),
                    reversed: None,
                },
                ConstructionInput {
                    role: None,
                    reference: Some(InputReference::Built(Box::new(AnyConstruction::Curve(
                        Construction {
                            inputs: vec![coordinates],
                            after: Vec::new(),
                            solution: None,
                            mode: None,
                            resolved: None,
                            resolved_in: std::collections::BTreeMap::new(),
                        },
                    )))),
                    value: None,
                    reversed: None,
                },
            ],
            ..Default::default()
        }
        .into_operation()
        .unwrap(),
        1,
    ));
    let report = crate::validate_neutral(&ir, Vec::new()).unwrap();
    assert!(
        report
            .findings
            .iter()
            .all(|f| !f.message.contains("construction")),
        "{:?}",
        report.findings
    );
}

#[test]
fn datum_axis_validation_rejects_bad_witness_frames_and_construction_controls() {
    use crate::design::construction::{FrameValue, PostStep};
    use crate::design::selection::{
        Anchor, Members, PickTarget, Picked, Picks, PrincipalEntity, Selection, Witness,
    };
    let x = crate::units::UnitVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap();
    let y = crate::units::UnitVector3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap();
    let bad = FrameValue {
        origin: origin(),
        x_axis: x,
        y_axis: y,
        z_axis: x,
    };
    let selection = Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![Picked {
                target: PickTarget::Principal {
                    entity: PrincipalEntity::Origin,
                    container: None,
                },
                context: None,
                native: None,
                domain: None,
                resolved: None,
                witness: Some(Witness {
                    point: None,
                    direction: None,
                    endpoints: None,
                    frame: Some(bad),
                    curves: Vec::new(),
                    surface: None,
                    body: None,
                }),
            }])
            .unwrap(),
        ),
        native: None,
    };
    let mut ir = crate::CadIr::empty();
    ir.model.features.push(feature(
        DatumAxisDraft {
            native_options: std::collections::BTreeMap::from([(
                crate::codec_name!("test:witness"),
                NativeOption::Selection(selection),
            )]),
            inputs: vec![ConstructionInput {
                role: None,
                reference: Some(InputReference::Built(Box::new(AnyConstruction::Point(
                    Construction {
                        resolved: Some(origin()),
                        solution: Some(0),
                        after: vec![
                            PostStep::Flip(Dimension::Literal(false)),
                            PostStep::AxisRotate {
                                axis: std::array::from_fn(|_| {
                                    Dimension::Literal(crate::scalar::FiniteReal::new(0.0).unwrap())
                                }),
                                angle: Dimension::Literal(crate::scalar::Angle::new(1.0).unwrap()),
                            },
                        ],
                        inputs: Vec::new(),
                        mode: None,
                        resolved_in: std::collections::BTreeMap::new(),
                    },
                )))),
                value: None,
                reversed: None,
            }],
            ..Default::default()
        }
        .into_operation()
        .unwrap(),
        1,
    ));
    let findings = crate::validate_neutral(&ir, Vec::new()).unwrap().findings;
    for message in [
        "solution must be positive",
        "literal false Flip",
        "AxisRotate must bind",
        "orthonormal and right-handed",
    ] {
        assert!(
            findings.iter().any(|f| f.message.contains(message)),
            "{message}: {findings:?}"
        );
    }
}

#[test]
fn datum_axis_bound_values_require_parameter_records() {
    let mut ir = crate::CadIr::empty();
    let prior = feature(FeatureOperation::datum_axis(origin(), direction()), 1);
    let wrong_target = ParameterId::mint(prior.id.as_str()).unwrap();
    let mut consumer = feature(
        DatumAxisDraft {
            native_options: std::collections::BTreeMap::from([(
                crate::codec_name!("test:length"),
                NativeOption::Length(Dimension::Parameter(wrong_target)),
            )]),
            ..Default::default()
        }
        .into_operation()
        .unwrap(),
        2,
    );
    consumer.dependencies.insert(prior.id.clone());
    ir.model.features.extend([prior, consumer]);
    let findings = crate::validate_neutral(&ir, Vec::new()).unwrap().findings;
    assert!(
        findings.iter().any(|finding| finding
            .message
            .contains("target must name a design parameter")),
        "{findings:?}"
    );
}

#[test]
fn datum_axis_picks_require_the_declared_target_arena() {
    use crate::design::selection::{Anchor, Members, PickTarget, Picked, Picks, Selection};
    let mut ir = crate::examples::unit_cube().unwrap();
    let point = ir.geometry.points[0].id.as_str();
    let target = PickTarget::Feature(crate::features::FeatureId::mint(point).unwrap());
    let picks = Selection {
        at: Anchor::Input,
        picks: Picks::Named(
            Members::new(vec![Picked {
                target,
                context: None,
                native: None,
                domain: None,
                witness: None,
                resolved: None,
            }])
            .unwrap(),
        ),
        native: None,
    };
    ir.model.features.push(feature(
        DatumAxisDraft {
            native_options: std::collections::BTreeMap::from([(
                crate::codec_name!("test:target"),
                NativeOption::Selection(picks),
            )]),
            ..Default::default()
        }
        .into_operation()
        .unwrap(),
        1,
    ));
    let findings = crate::validate_neutral(&ir, Vec::new()).unwrap().findings;
    assert!(
        findings.iter().any(|finding| finding
            .message
            .contains("datum-axis selection target must name a feature")),
        "{findings:?}"
    );
}

#[test]
fn datum_axis_target_checks_use_declarations_and_reach_nested_contexts() {
    use crate::design::refs::{ConfigurationRef, EntityRef, Member, Position};
    use crate::design::selection::{
        Anchor, DefinitionRef, Members, NativeOwner, OccurrencePath, PathStep, PickContext,
        PickTarget, Picked, Picks, PrincipalEntity, Selection, StepRef,
    };
    let mut base = crate::examples::unit_cube().unwrap();
    let mut prior = feature(FeatureOperation::datum_axis(origin(), direction()), 1);
    prior.id = crate::features::FeatureId::mint("test:target:point#opaque-feature").unwrap();
    let feature_id = prior.id.clone();
    base.model.features.push(prior);
    let point = base.geometry.points[0].id.as_str().to_owned();
    let principal = || PickTarget::Principal {
        entity: PrincipalEntity::Origin,
        container: None,
    };
    for case in 0..10 {
        let mut picked = Picked {
            target: principal(),
            context: None,
            native: None,
            domain: None,
            witness: None,
            resolved: None,
        };
        let mut at = Anchor::Input;
        let expected = match case {
            0 => {
                picked.target = PickTarget::Feature(feature_id.clone());
                None
            }
            1 => {
                picked.resolved = Some(vec![EntityRef::Body(base.geometry.bodies[0].id.clone())]);
                None
            }
            2 => {
                picked.resolved = Some(vec![EntityRef::Body(
                    crate::ids::BodyId::mint(&point).unwrap(),
                )]);
                Some("resolved entity target")
            }
            3 => {
                picked.context = Some(PickContext::Definition(DefinitionRef {
                    definition: crate::ids::ProductDefinitionId::mint(&point).unwrap(),
                    configuration: None,
                }));
                Some("context target")
            }
            4 => {
                picked.context = Some(PickContext::Occurrence(OccurrencePath(vec![PathStep {
                    occurrence: StepRef::Occurrence(
                        crate::ids::OccurrenceId::mint(&point).unwrap(),
                    ),
                    configuration: Some(ConfigurationRef::Local(
                        crate::features::ConfigurationId::mint(&point).unwrap(),
                    )),
                }])));
                Some("path target")
            }
            5 => {
                at = Anchor::Pinned(Position::After(Member::Feature(
                    crate::features::FeatureId::mint(&point).unwrap(),
                )));
                Some("position target")
            }
            6 => {
                picked.target = PickTarget::Native {
                    within: Some(NativeOwner::Sketch(
                        crate::sketches::SketchId::mint(&point).unwrap(),
                    )),
                };
                Some("native owner")
            }
            7 => {
                picked.target = PickTarget::SketchEntity(
                    crate::sketches::SketchEntityId::mint(&point).unwrap(),
                );
                Some("selection target")
            }
            8 => {
                picked.target = PickTarget::SpatialSketchEntity(
                    crate::sketches::SpatialSketchEntityId::mint(&point).unwrap(),
                );
                Some("selection target")
            }
            _ => {
                picked.target = PickTarget::Datum {
                    feature: feature_id.clone(),
                    part: None,
                };
                None
            }
        };
        let selection = Selection {
            at,
            picks: Picks::Named(Members::new(vec![picked]).unwrap()),
            native: None,
        };
        let mut consumer = feature(
            DatumAxisDraft {
                source_settings: std::collections::BTreeMap::from([(
                    crate::codec_name!("test:selection"),
                    NativeOption::Selection(selection),
                )]),
                ..Default::default()
            }
            .into_operation()
            .unwrap(),
            2,
        );
        consumer.dependencies.insert(feature_id.clone());
        let mut ir = base.clone();
        ir.model.features.push(consumer);
        ir.model
            .features
            .sort_by(|left, right| left.id.cmp(&right.id));
        crate::test_support::add_root(&mut ir);
        let findings = crate::validate_neutral(&ir, Vec::new()).unwrap().findings;
        if let Some(expected) = expected {
            assert!(
                findings
                    .iter()
                    .any(|finding| finding.message.contains(expected)),
                "case {case}: {findings:?}"
            );
            if case == 4 {
                assert!(
                    findings
                        .iter()
                        .any(|finding| finding.message.contains("configuration target")),
                    "{findings:?}"
                );
            }
        } else {
            assert!(
                findings.iter().all(|finding| !matches!(
                    finding.severity,
                    crate::report::Severity::Error | crate::report::Severity::Blocking
                )),
                "case {case}: {findings:?}"
            );
        }
    }
}

#[test]
fn datum_axis_resolved_configuration_keys_require_configuration_records() {
    let mut ir = crate::examples::unit_cube().unwrap();
    let prior = feature(FeatureOperation::datum_axis(origin(), direction()), 1);
    let key = crate::features::ConfigurationId::mint(prior.id.as_str()).unwrap();
    let built = Construction::<crate::design::construction::kinds::Point> {
        inputs: vec![ConstructionInput {
            role: None,
            reference: None,
            value: None,
            reversed: None,
        }],
        resolved: Some(origin()),
        resolved_in: std::collections::BTreeMap::from([(key, origin())]),
        after: Vec::new(),
        solution: None,
        mode: None,
    };
    let mut consumer = feature(
        DatumAxisDraft {
            inputs: vec![ConstructionInput {
                role: None,
                reference: Some(InputReference::Built(Box::new(AnyConstruction::Point(
                    built,
                )))),
                value: None,
                reversed: None,
            }],
            ..Default::default()
        }
        .into_operation()
        .unwrap(),
        2,
    );
    consumer.dependencies.insert(prior.id.clone());
    ir.model.features.extend([prior, consumer]);
    let findings = crate::validate_neutral(&ir, Vec::new()).unwrap().findings;
    assert!(
        findings.iter().any(|finding| finding
            .message
            .contains("resolved configuration must name a design configuration")),
        "{findings:?}"
    );
}

#[test]
fn datum_axis_region_boundaries_require_the_selected_sketch_owner() {
    use crate::design::selection::{
        Anchor, BoundaryUse, Members, PickTarget, Picked, Picks, Selection, SketchRegion,
    };
    use crate::sketches::{
        Sketch, SketchEntity, SketchGeometry, SketchGeometryDefinition, SketchId, SketchPlacement,
        SketchProfiles,
    };
    let mut base = crate::examples::unit_cube().unwrap();
    let first = SketchId::mint("test:model:sketch#a").unwrap();
    let second = SketchId::mint("test:model:sketch#b").unwrap();
    for id in [&first, &second] {
        base.model.sketches.push(Sketch {
            id: id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: SketchPlacement::Unresolved {},
            profiles: SketchProfiles::default(),
            native_ref: None,
        });
    }
    let circle = SketchEntity::new(
        crate::sketches::SketchEntityId::mint("test:model:sketch-entity#circle").unwrap(),
        first.clone(),
        SketchGeometry::from_admitted_definition(SketchGeometryDefinition::Circle {
            center: crate::units::FinitePoint2::new(crate::math::Point2::new(0.0, 0.0)).unwrap(),
            radius: crate::scalar::PositiveLength::new(1.0).unwrap(),
        }),
    );
    let entity = circle.id().clone();
    base.model.sketch_entities.push(circle);
    for (sketch, mismatch) in [(first, false), (second, true)] {
        let selection = Selection {
            at: Anchor::Input,
            picks: Picks::Named(
                Members::new(vec![Picked {
                    target: PickTarget::SketchRegion {
                        sketch,
                        region: SketchRegion {
                            outer: vec![BoundaryUse {
                                entity: entity.clone(),
                                fragment: None,
                            }],
                            holes: Vec::new(),
                        },
                    },
                    context: None,
                    native: None,
                    domain: None,
                    witness: None,
                    resolved: None,
                }])
                .unwrap(),
            ),
            native: None,
        };
        let mut ir = base.clone();
        ir.model.features.push(feature(
            DatumAxisDraft {
                source_settings: std::collections::BTreeMap::from([(
                    crate::codec_name!("test:region"),
                    NativeOption::Selection(selection),
                )]),
                ..Default::default()
            }
            .into_operation()
            .unwrap(),
            1,
        ));
        let findings = crate::validate_neutral(&ir, Vec::new()).unwrap().findings;
        let reference_errors = findings
            .iter()
            .filter(|finding| finding.check == crate::report::check::Check::ReferentialIntegrity)
            .collect::<Vec<_>>();
        assert_eq!(
            reference_errors.iter().any(|finding| finding
                .message
                .contains("region boundary must belong to its selected sketch")),
            mismatch,
            "{findings:?}"
        );
        if !mismatch {
            assert!(reference_errors.is_empty(), "{findings:?}");
        }
    }
}
