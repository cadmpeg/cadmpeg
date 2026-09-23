// SPDX-License-Identifier: Apache-2.0
use cadmpeg_test_support::edit;

use crate::geometry::{CurveOffsetRange, OffsetSide, ProceduralCurve, ProceduralCurveDefinition};
use crate::ids::{CurveId, ProceduralCurveId};
use crate::math::Vector3;

fn id() -> ProceduralCurveId {
    ProceduralCurveId::mint("synthetic:test:procedural_curve#payload").unwrap()
}

fn source() -> CurveId {
    CurveId::mint("synthetic:test:curve#source").unwrap()
}

fn subset(range: [f64; 2]) -> ProceduralCurveDefinition {
    ProceduralCurveDefinition::Subset(
        crate::geometry::curve_payloads::SubsetCurveConstruction::try_new(
            source(),
            range,
            false,
            None,
        )
        .unwrap(),
    )
}

#[test]
fn curve_payload_admission_requires_finite_ordered_subset_ranges() {
    use crate::geometry::curve_payloads::SubsetCurveConstruction;
    let mut definition = subset([1.0, 1.0]);
    definition
        .set_legacy_cache(crate::geometry::LegacyCache::try_new(0.5).unwrap())
        .unwrap();
    let curve = ProceduralCurve::new(id(), definition);
    let wire = serde_json::to_value(&curve).unwrap();
    assert_eq!(
        wire["definition"]["parameter_range"],
        serde_json::json!([1.0, 1.0])
    );
    assert_eq!(
        serde_json::from_value::<ProceduralCurve>(wire.clone()).unwrap(),
        curve
    );
    for range in [[1.0, 0.0], [f64::NAN, 1.0], [0.0, f64::INFINITY]] {
        assert!(SubsetCurveConstruction::try_new(source(), range, false, None).is_err());
        let mut invalid = wire.clone();
        invalid["definition"]["parameter_range"] = serde_json::json!(range);
        assert!(
            serde_json::from_value::<ProceduralCurveDefinition>(invalid["definition"].clone())
                .is_err()
        );
        assert!(serde_json::from_value::<ProceduralCurve>(invalid).is_err());
    }
}

#[test]
fn a_distance_law_without_its_parameter_range_has_no_encoding() {
    use crate::geometry::curve_payloads::OffsetCurveConstruction;
    let side = OffsetSide::Direction {
        direction: Vector3::new(0.0, 0.0, 2.0),
        support: None,
    };
    let uniform = ProceduralCurveDefinition::Offset(
        OffsetCurveConstruction::try_new(
            source(),
            -2.0,
            side.clone(),
            Some(CurveOffsetRange::uniform([0.0, 1.0]).unwrap()),
        )
        .unwrap(),
    );
    let wire = serde_json::to_value(&uniform).unwrap();
    assert_eq!(wire["range"]["kind"], "uniform");
    assert!(wire["range"].get("distance_law").is_none());
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).unwrap(),
        uniform
    );

    let absent = ProceduralCurveDefinition::Offset(
        OffsetCurveConstruction::try_new(source(), -2.0, side, None).unwrap(),
    );
    let absent_wire = serde_json::to_value(&absent).unwrap();
    assert!(absent_wire.get("range").is_none());
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(absent_wire).unwrap(),
        absent
    );

    let mut law_without_range = wire.clone();
    law_without_range["range"] = serde_json::json!({"kind": "variable"});
    assert!(serde_json::from_value::<ProceduralCurveDefinition>(law_without_range).is_err());

    let mut bogus = wire;
    bogus["range"]["zz_bogus"] = serde_json::json!(1);
    let error = serde_json::from_value::<ProceduralCurveDefinition>(bogus)
        .unwrap_err()
        .to_string();
    assert!(error.contains("zz_bogus"), "{error}");
}

#[test]
fn offset_payload_preserves_direction_magnitude_and_requires_strict_ranges() {
    use crate::geometry::curve_payloads::OffsetCurveConstruction;
    let definition = |side, range| {
        OffsetCurveConstruction::try_new(source(), -2.0, side, range)
            .map(ProceduralCurveDefinition::Offset)
    };
    let direction = OffsetSide::Direction {
        direction: Vector3::new(0.0, 0.0, 2.0),
        support: None,
    };
    let valid = definition(direction.clone(), None).unwrap();
    let _curve: ProceduralCurve = ProceduralCurve::new(id(), valid.clone());
    for range in [[1.0, 0.0], [0.0, 0.0]] {
        let refused = CurveOffsetRange::uniform(range)
            .and_then(|range| definition(direction.clone(), Some(range)));
        assert_eq!(
            refused,
            Err(crate::geometry::ProceduralGeometryError::Payload(
                "curve offset distance, side, range, or law is invalid"
            ))
        );
        let mut wire = serde_json::to_value(&valid).unwrap();
        wire["range"] = serde_json::json!({"kind": "uniform", "parameter_range": range});
        let error = serde_json::from_value::<ProceduralCurveDefinition>(wire).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("curve offset distance, side, range, or law is invalid"),
            "{error}"
        );
    }
    assert!(definition(
        OffsetSide::PlaneNormal {
            normal: Vector3::new(0.0, 0.0, 2.0)
        },
        None
    )
    .is_err());
    let _curve: ProceduralCurve = ProceduralCurve::new(
        id(),
        definition(
            OffsetSide::PlaneNormal {
                normal: Vector3::new(0.0, 0.0, 1.0),
            },
            None,
        )
        .unwrap(),
    );
    assert!(definition(
        OffsetSide::Direction {
            direction: Vector3::new(0.0, 0.0, 0.0),
            support: None
        },
        None
    )
    .is_err());
}

#[test]
fn intersection_context_mutation_keeps_checked_ranges_and_cache_tolerance() {
    use crate::geometry::{IntcurveSupportContext, IntcurveSupportSide};
    use crate::ids::SurfaceId;

    let mut curve = ProceduralCurve::new(
        id(),
        ProceduralCurveDefinition::Intersection {
            context: IntcurveSupportContext::try_new(
                std::array::from_fn(|_| IntcurveSupportSide {
                    surface: None,
                    pcurve: None,
                }),
                [0.0, 1.0],
                std::array::from_fn(|_| Vec::new()),
            )
            .unwrap(),
            discontinuity_flag: false,
            cache: Some(crate::geometry::LegacyCache::try_new(0.5).unwrap()),
        },
    );
    let support = SurfaceId::mint("synthetic:test:surface#support").unwrap();
    let context = curve.intersection_context_mut().unwrap();
    context.set_surface(0, Some(support.clone()));
    assert!(edit::replace(context, |previous| {
        let sides = previous.sides().clone();
        let mut range = previous.parameter_range();
        let discontinuities = previous.discontinuities().clone();
        {
            let range: &mut [f64; 2] = &mut range;
            *range = [1.0, 0.0];
        };
        crate::geometry::IntcurveSupportContext::try_new(sides, range, discontinuities)
    })
    .is_err());
    assert_eq!(context.parameter_range(), [0.0, 1.0]);
    assert_eq!(context.sides()[0].surface.as_ref(), Some(&support));
    assert_eq!(curve.cache_fit_tolerance(), Some(0.5));
    assert!(ProceduralCurve::new(id(), subset([0.0, 1.0]))
        .intersection_context_mut()
        .is_none());
}

#[test]
fn silhouette_admission_requires_a_nondegenerate_light_direction_and_finite_draft() {
    use crate::geometry::curve_payloads::SilhouetteCurveConstruction;
    use crate::geometry::{IntcurveSupportContext, IntcurveSupportSide, SilhouetteKind};
    use crate::ids::SurfaceId;
    use crate::scalar::FiniteReal;

    let context = || {
        IntcurveSupportContext::try_new(
            std::array::from_fn(|_| IntcurveSupportSide {
                surface: None,
                pcurve: None,
            }),
            [0.0, 1.0],
            std::array::from_fn(|_| Vec::new()),
        )
        .unwrap()
    };
    let cast_surface = SurfaceId::mint("synthetic:test:surface#cast").unwrap();
    let silhouette = |kind, light_direction| {
        SilhouetteCurveConstruction::try_new(context(), kind, cast_surface.clone(), light_direction)
            .map(ProceduralCurveDefinition::Silhouette)
    };
    let valid = silhouette(
        SilhouetteKind::Taper {
            draft_factor: FiniteReal::new(0.5).unwrap(),
        },
        Vector3::new(0.0, 0.0, 2.0),
    )
    .unwrap();
    let curve = ProceduralCurve::new(id(), valid);
    let wire = serde_json::to_value(&curve).unwrap();
    assert_eq!(
        wire["definition"]["silhouette"]["draft_factor"],
        serde_json::json!(0.5)
    );
    assert_eq!(
        serde_json::from_value::<ProceduralCurve>(wire.clone()).unwrap(),
        curve
    );
    for light_direction in [
        Vector3::new(0.0, 0.0, 0.0),
        Vector3::new(f64::NAN, 0.0, 1.0),
        Vector3::new(0.0, f64::INFINITY, 0.0),
    ] {
        assert!(silhouette(SilhouetteKind::Standard {}, light_direction).is_err());
        let mut invalid = wire.clone();
        invalid["definition"]["light_direction"] = serde_json::to_value(light_direction).unwrap();
        assert!(
            serde_json::from_value::<ProceduralCurveDefinition>(invalid["definition"].clone())
                .is_err()
        );
        assert!(serde_json::from_value::<ProceduralCurve>(invalid).is_err());
    }
    assert!(FiniteReal::new(f64::NAN).is_none());
    let mut invalid = wire;
    invalid["definition"]["silhouette"]["draft_factor"] = serde_json::json!("nan");
    assert!(
        serde_json::from_value::<ProceduralCurveDefinition>(invalid["definition"].clone()).is_err()
    );
    assert!(serde_json::from_value::<ProceduralCurve>(invalid).is_err());
}

#[test]
fn rejected_curve_definition_replacements_preserve_serialized_owner() {
    let mut definition = subset([0.0, 1.0]);
    definition
        .set_legacy_cache(crate::geometry::LegacyCache::try_new(0.5).unwrap())
        .unwrap();
    let curve = ProceduralCurve::new(id(), definition);
    let before = serde_json::to_vec(&curve).unwrap();
    for tolerance in [-1.0, f64::NAN, f64::INFINITY] {
        assert!(crate::geometry::LegacyCache::try_new(tolerance).is_err());
        assert_eq!(serde_json::to_vec(&curve).unwrap(), before);
    }
}

#[test]
fn an_offset_side_states_its_carrier_and_denies_the_other_one() {
    use crate::geometry::curve_payloads::OffsetCurveConstruction;
    let definition = |side| {
        OffsetCurveConstruction::try_new(source(), -2.0, side, None)
            .map(ProceduralCurveDefinition::Offset)
            .unwrap()
    };
    let normal = definition(OffsetSide::PlaneNormal {
        normal: Vector3::new(0.0, 0.0, 1.0),
    });
    let direction = definition(OffsetSide::Direction {
        direction: Vector3::new(0.0, 0.0, 2.0),
        support: None,
    });

    let normal_wire = serde_json::to_value(&normal).unwrap();
    assert_eq!(normal_wire["side"]["side"], "plane_normal");
    assert!(normal_wire["side"].get("direction").is_none());
    let direction_wire = serde_json::to_value(&direction).unwrap();
    assert_eq!(direction_wire["side"]["side"], "direction");
    assert!(direction_wire["side"].get("normal").is_none());
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(normal_wire.clone()).unwrap(),
        normal
    );
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(direction_wire.clone()).unwrap(),
        direction
    );

    let mut both = normal_wire;
    both["direction"] = serde_json::json!({"x": 0.0, "y": 0.0, "z": 2.0});
    let error = serde_json::from_value::<ProceduralCurveDefinition>(both)
        .unwrap_err()
        .to_string();
    assert!(error.contains("direction"), "{error}");

    let mut stray_support = serde_json::to_value(&normal).unwrap();
    stray_support["support"] = serde_json::json!("test:model:surface#0");
    let error = serde_json::from_value::<ProceduralCurveDefinition>(stray_support)
        .unwrap_err()
        .to_string();
    assert!(error.contains("support"), "{error}");
}

#[test]
fn the_law_curve_version_form_refuses_a_non_finite_interval_bound() {
    let admitted = crate::geometry::LawCurveVersionForm::try_new(20_900, 0, [Some(-1.0), None])
        .expect("finite law curve version form");
    let wire = serde_json::to_value(&admitted).unwrap();
    assert_eq!(wire["stamp"], serde_json::json!(20_900));
    assert_eq!(wire["post_enum"], serde_json::json!(0));
    assert_eq!(wire["parameter_range"], serde_json::json!([-1.0, null]));
    assert_eq!(
        serde_json::from_value::<crate::geometry::LawCurveVersionForm>(wire).unwrap(),
        admitted
    );

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for range in [[Some(value), None], [None, Some(value)]] {
            assert!(crate::geometry::LawCurveVersionForm::try_new(20_900, 0, range).is_err());
            assert!(crate::geometry::LawCurveVersionForm::try_from(
                crate::geometry::LawCurveVersionFormWire {
                    stamp: 20_900,
                    post_enum: 0,
                    parameter_range: range,
                }
            )
            .is_err());
        }
    }
}

fn law_support_context() -> crate::geometry::IntcurveSupportContext {
    crate::geometry::IntcurveSupportContext::try_new(
        [
            crate::geometry::IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
            crate::geometry::IntcurveSupportSide {
                surface: None,
                pcurve: None,
            },
        ],
        [0.0, 1.0],
        [Vec::new(), Vec::new(), Vec::new()],
    )
    .expect("finite ordered support context")
}

fn named_law(variable: crate::geometry::LawExpression) -> crate::geometry::LawFormula {
    crate::geometry::LawFormula::Named {
        name: cadmpeg_core::nonblank_literal!("primary_law"),
        variables: vec![variable],
    }
}

fn law_definition(
    primary: crate::geometry::FiniteLawFormula,
    additional: Vec<crate::geometry::FiniteLawFormula>,
) -> ProceduralCurveDefinition {
    ProceduralCurveDefinition::Law {
        context: law_support_context(),
        version: None,
        extension: 3,
        primary,
        additional,
        cache: None,
    }
}

/// Every law-expression shape that carries a scalar, each with one slot set to
/// the degenerate value and the rest admitted.
fn degenerate_law_expressions(value: f64) -> Vec<crate::geometry::LawExpression> {
    use crate::geometry::LawExpression;
    let mut scalars = [0.0; 13];
    scalars[7] = value;
    vec![
        LawExpression::Double { value },
        LawExpression::Point {
            value: crate::math::Point3::new(0.0, value, 0.0),
        },
        LawExpression::Vector {
            value: Vector3::new(value, 0.0, 0.0),
        },
        LawExpression::Transform {
            scalars,
            enums: [0; 3],
        },
        LawExpression::TransformVec {
            vectors: [
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, value, 0.0),
            ],
            scale: 1.0,
            flags: [false; 3],
        },
        LawExpression::TransformVec {
            vectors: [
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(0.0, 0.0, 0.0),
            ],
            scale: value,
            flags: [false; 3],
        },
        LawExpression::Edge {
            curve: crate::geometry::LoftPathCurve {
                id: source(),
                endpoints: None,
            },
            parameters: [0.0, value],
        },
        LawExpression::Spline {
            native_id: 1,
            knots: vec![0.0, value],
            controls: vec![1.0],
            point: crate::math::Point3::new(0.0, 0.0, 0.0),
        },
        LawExpression::Spline {
            native_id: 1,
            knots: vec![0.0, 1.0],
            controls: vec![value],
            point: crate::math::Point3::new(0.0, 0.0, 0.0),
        },
        LawExpression::Spline {
            native_id: 1,
            knots: vec![0.0, 1.0],
            controls: vec![1.0],
            point: crate::math::Point3::new(value, 0.0, 0.0),
        },
        LawExpression::Algebraic {
            operator: "+".to_owned(),
            operands: vec![
                LawExpression::Integer { value: 1 },
                LawExpression::Algebraic {
                    operator: "*".to_owned(),
                    operands: vec![LawExpression::Double { value }],
                },
            ],
        },
    ]
}

#[test]
fn the_law_curve_admission_refuses_every_non_finite_formula_constant() {
    use crate::geometry::{FiniteLawFormula, LawExpression, LawFormula};

    let primary = named_law(LawExpression::Edge {
        curve: crate::geometry::LoftPathCurve {
            id: source(),
            endpoints: None,
        },
        parameters: [-0.5, 1.5],
    });
    let admitted = law_definition(
        FiniteLawFormula::try_new(primary.clone()).expect("finite primary law"),
        vec![FiniteLawFormula::try_new(LawFormula::Null {}).expect("finite additional law")],
    );

    // The carrier is transparent: the wire states the formula itself.
    let wire = serde_json::to_value(&admitted).expect("serializes");
    assert_eq!(wire["kind"], "law");
    assert_eq!(wire["extension"], serde_json::json!(3));
    assert_eq!(wire["primary"]["kind"], "named");
    assert_eq!(wire["primary"]["name"], "primary_law");
    assert_eq!(
        wire["primary"]["variables"][0]["parameters"],
        serde_json::json!([-0.5, 1.5])
    );
    assert_eq!(wire["additional"], serde_json::json!([{"kind": "null"}]));
    assert_eq!(
        serde_json::from_value::<ProceduralCurveDefinition>(wire.clone()).expect("round trip"),
        admitted
    );

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for expression in degenerate_law_expressions(value) {
            let formula = named_law(expression);
            assert!(
                FiniteLawFormula::try_new(formula.clone()).is_err(),
                "{formula:?}"
            );
            assert!(!formula.values_are_finite(), "{formula:?}");
        }
    }

    // The wire route. The reader is the derived `Deserialize`, which reads a
    // `LawFormula` and admits it through `TryFrom`, so the admission is
    // exercised on the value that reader hands it.
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for expression in degenerate_law_expressions(value) {
            assert!(FiniteLawFormula::try_from(named_law(expression)).is_err());
        }
    }
    let deep = nested_algebraic_law(LAW_EXPRESSION_DEPTH_LIMIT_FOR_TEST + 1);
    assert!(FiniteLawFormula::try_from(deep.clone()).is_err());

    // No CADIR document states either refused value. JSON spells neither NaN
    // nor an infinity, and a formula past the depth bound nests deeper than the
    // reader's own recursion limit.
    for literal in ["1e400", "-1e400"] {
        let error = serde_json::from_str::<f64>(literal)
            .expect_err("JSON states no infinity")
            .to_string();
        assert!(error.contains("number out of range"), "{literal}: {error}");
    }
    assert!(serde_json::Number::from_f64(f64::NAN).is_none());
    let error =
        serde_json::from_str::<LawFormula>(&serde_json::to_string(&deep).expect("serializes"))
            .expect_err("past the reader recursion limit")
            .to_string();
    assert!(error.contains("recursion limit exceeded"), "{error}");
}

#[test]
fn numerical_audit_finite_law_checks_edge_curve_endpoints() {
    use crate::geometry::{FiniteLawFormula, LawExpression, LoftPathCurve};

    for endpoints in [None, Some([None, Some(1.0)]), Some([Some(0.0), None])] {
        let formula = named_law(LawExpression::Edge {
            curve: LoftPathCurve {
                id: source(),
                endpoints,
            },
            parameters: [0.0, 1.0],
        });
        assert!(FiniteLawFormula::try_new(formula).is_ok());
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for slot in 0..2 {
            let mut endpoints = [Some(0.0), Some(1.0)];
            endpoints[slot] = Some(invalid);
            let formula = named_law(LawExpression::Edge {
                curve: LoftPathCurve {
                    id: source(),
                    endpoints: Some(endpoints),
                },
                parameters: [0.0, 1.0],
            });
            assert!(!formula.values_are_finite());
            assert!(FiniteLawFormula::try_new(formula).is_err());
        }
    }
}

/// The recursion bound the law-expression walk states.
const LAW_EXPRESSION_DEPTH_LIMIT_FOR_TEST: usize = 64;

fn nested_algebraic_law(depth: usize) -> crate::geometry::LawFormula {
    let mut expression = crate::geometry::LawExpression::Double { value: 1.0 };
    for _ in 0..depth {
        expression = crate::geometry::LawExpression::Algebraic {
            operator: "+".to_owned(),
            operands: vec![expression],
        };
    }
    named_law(expression)
}

#[test]
fn the_law_expression_walk_refuses_an_operand_tree_past_its_depth_limit() {
    use crate::geometry::FiniteLawFormula;

    assert!(
        FiniteLawFormula::try_new(nested_algebraic_law(LAW_EXPRESSION_DEPTH_LIMIT_FOR_TEST))
            .is_ok()
    );
    assert!(FiniteLawFormula::try_new(nested_algebraic_law(
        LAW_EXPRESSION_DEPTH_LIMIT_FOR_TEST + 1
    ))
    .is_err());
}

#[test]
fn curve_offset_intervals_are_admitted_with_the_offset_refusal() {
    use crate::geometry::{CurveOffsetDistanceLaw, CurveOffsetLawBasis, ProceduralGeometryError};

    let refusal = Err(ProceduralGeometryError::Payload(
        "curve offset distance, side, range, or law is invalid",
    ));
    let linear = |control_range| {
        CurveOffsetDistanceLaw::linear(CurveOffsetLawBasis::ArcLength, [1.0, 2.0], control_range)
    };
    let law = linear([0.0, 10.0]).expect("increasing controls");
    let CurveOffsetDistanceLaw::Linear { control_range, .. } = &law else {
        panic!("a linear law");
    };
    assert_eq!(control_range.endpoints(), [0.0, 10.0]);
    let range = CurveOffsetRange::variable([0.0, 1.0], law.clone()).expect("increasing range");
    let CurveOffsetRange::Variable {
        parameter_range, ..
    } = &range
    else {
        panic!("a variable range");
    };
    assert_eq!(parameter_range.endpoints(), [0.0, 1.0]);
    let wire = serde_json::to_value(&range).unwrap();
    assert_eq!(wire["parameter_range"], serde_json::json!([0.0, 1.0]));
    assert_eq!(
        wire["distance_law"]["control_range"],
        serde_json::json!([0.0, 10.0])
    );
    assert_eq!(
        serde_json::from_value::<CurveOffsetRange>(wire.clone()).unwrap(),
        range
    );
    for invalid in [
        [1.0, 0.0],
        [0.0, 0.0],
        [f64::NAN, 1.0],
        [0.0, f64::INFINITY],
    ] {
        assert_eq!(linear(invalid), refusal.clone().map(|()| law.clone()));
        assert_eq!(
            CurveOffsetRange::variable(invalid, law.clone()),
            refusal.clone().map(|()| range.clone())
        );
        assert_eq!(
            CurveOffsetRange::uniform(invalid),
            refusal.clone().map(|()| range.clone())
        );
    }
    let mut decreasing = wire;
    decreasing["distance_law"]["control_range"] = serde_json::json!([10.0, 0.0]);
    let error = serde_json::from_value::<CurveOffsetRange>(decreasing).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("curve offset distance, side, range, or law is invalid"),
        "{error}"
    );
}
