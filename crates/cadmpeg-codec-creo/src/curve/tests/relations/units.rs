// SPDX-License-Identifier: Apache-2.0
use super::{parse_relation_expression, EPS_RELATION_VALUE};
use crate::curve::tests::evaluate_expression_program;
use crate::curve::{
    CurveExpressionLine, CurveExpressionQuantity, CurveExpressionValue, ExternalRelationSymbols,
    RelationEvaluationContext,
};
use std::collections::BTreeMap;

#[test]
fn bracketed_relation_units_are_not_dependencies() {
    let lines = [
        CurveExpressionLine {
            text: "length=5[mm]+offset[inch]".to_owned(),
            offset: 0,
        },
        CurveExpressionLine {
            text: "compound=pressure[N/mm^2]".to_owned(),
            offset: 1,
        },
        CurveExpressionLine {
            text: "fall=G*2[s]^2".to_owned(),
            offset: 2,
        },
    ];
    let assignments =
        evaluate_expression_program(&lines, None, &ExternalRelationSymbols::default());

    assert_eq!(assignments[0].dependencies, ["offset"]);
    assert_eq!(assignments[0].value, None);
    assert_eq!(assignments[1].dependencies, ["pressure"]);
    assert_eq!(assignments[1].value, None);
    assert!(assignments[2].dependencies.is_empty());
    assert_eq!(
        assignments[2].value,
        Some(CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(39_200.0).expect("finite relation fixture")
        ))
    );

    let values = BTreeMap::new();
    let cases = [
        (
            "5[mm]+.2[cm]",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(7.0).expect("finite relation fixture"),
            ),
        ),
        (
            "1[inch]",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(25.4).expect("finite relation fixture"),
            ),
        ),
        (
            "PI[rad]",
            CurveExpressionValue::Angle(
                cadmpeg_ir::scalar::FiniteReal::new(180.0).expect("finite relation fixture"),
            ),
        ),
        (
            "sin(PI[rad]/2)",
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"),
            ),
        ),
        (
            "1[mm]*2",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture"),
            ),
        ),
        (
            "1[mm]/.1[cm]",
            CurveExpressionValue::Number(
                cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture"),
            ),
        ),
    ];
    for (expression, expected) in cases {
        let actual = parse_relation_expression::<CurveExpressionValue>(
            expression,
            &values,
            RelationEvaluationContext::default(),
        )
        .expect(expression);
        match (actual, expected) {
            (CurveExpressionValue::Number(actual), CurveExpressionValue::Number(expected))
            | (CurveExpressionValue::Length(actual), CurveExpressionValue::Length(expected))
            | (CurveExpressionValue::Angle(actual), CurveExpressionValue::Angle(expected)) => {
                assert!(
                    (actual.get() - expected.get()).abs() < EPS_RELATION_VALUE,
                    "{expression}"
                );
            }
            _ => panic!("unexpected value kind for {expression}"),
        }
    }
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[mm]+1[deg]",
            &values,
            RelationEvaluationContext::default(),
        ),
        None
    );

    let pressure = parse_relation_expression::<CurveExpressionValue>(
        "1[N/mm^2]",
        &values,
        RelationEvaluationContext::default(),
    );
    assert_eq!(
        pressure,
        Some(CurveExpressionValue::Quantity(
            CurveExpressionQuantity::new(1_000.0, [-1, 1, -2, 0, 0])
                .expect("valid residual dimension fixture")
        ))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[(N/mm^2)]",
            &values,
            RelationEvaluationContext::default(),
        ),
        pressure
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[N/mm^2]/1[N/mm^2]",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")
        ))
    );
    for expression in [
        "1[sq_in]/1[in]^2",
        "1[cu_ft]/1[ft]^3",
        "1[joule]/(1[N]*1[m])",
        "1[kW]/(1000[joule]/1[s])",
        "1[MPa]/1[N/mm^2]",
        "1[ton]/(1000[kg]*9.80665[m/s^2])",
    ] {
        let Some(CurveExpressionValue::Number(value)) =
            parse_relation_expression::<CurveExpressionValue>(
                expression,
                &values,
                RelationEvaluationContext::default(),
            )
        else {
            panic!("unexpected value kind for {expression}");
        };
        assert!(
            (value.get() - 1.0).abs() < EPS_RELATION_VALUE,
            "{expression}"
        );
    }
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[psi]/1[Pa]",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(6_894.757_293_168_361)
                .expect("finite relation fixture")
        ))
    );
    for (expression, expected_kelvin) in [
        ("0[C]", 273.15),
        ("32[F]", 273.15),
        ("273.15[K]", 273.15),
        ("491.67[R]", 273.15),
    ] {
        let Some(CurveExpressionValue::Quantity(value)) =
            parse_relation_expression::<CurveExpressionValue>(
                expression,
                &values,
                RelationEvaluationContext::default(),
            )
        else {
            panic!("unexpected value kind for {expression}");
        };
        assert!(
            (value.value() - expected_kelvin).abs() < EPS_RELATION_VALUE,
            "{expression}"
        );
        assert_eq!(value.powers()[4], 1, "{expression}");
        assert_eq!(
            [
                value.powers()[0],
                value.powers()[1],
                value.powers()[2],
                value.powers()[3],
            ],
            [0; 4],
            "{expression}"
        );
    }
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "1[C/s]",
            &values,
            RelationEvaluationContext::default(),
        ),
        None
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "2[mm]^2",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Quantity(
            CurveExpressionQuantity::new(4.0, [2, 0, 0, 0, 0])
                .expect("valid residual dimension fixture")
        ))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "sqrt(4[mm^2])",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(2.0).expect("finite relation fixture")
        ))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "min(abs(-2[cm]),30[mm])",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Length(
            cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite relation fixture")
        ))
    );
    assert_eq!(
        parse_relation_expression::<CurveExpressionValue>(
            "near(1[inch],25[mm],1[mm])",
            &values,
            RelationEvaluationContext::default(),
        ),
        Some(CurveExpressionValue::Number(
            cadmpeg_ir::scalar::FiniteReal::new(1.0).expect("finite relation fixture")
        ))
    );
    let dimensioned_cases = [
        (
            "if(1,2[cm],1[inch])",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite relation fixture"),
            ),
        ),
        (
            "bound(30[mm],1[cm],2[cm])",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite relation fixture"),
            ),
        ),
        (
            "dead(25[mm],1[cm],2[cm])",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture"),
            ),
        ),
        (
            "mod(25[mm],1[cm])",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(5.0).expect("finite relation fixture"),
            ),
        ),
        (
            "sign(2[cm],-1[s])",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(-20.0).expect("finite relation fixture"),
            ),
        ),
        (
            "ceil(2.1[mm])",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(3.0).expect("finite relation fixture"),
            ),
        ),
        (
            "ceil(12.5[mm],-1)",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(20.0).expect("finite relation fixture"),
            ),
        ),
        (
            "floor(2.19[cm],1)",
            CurveExpressionValue::Length(
                cadmpeg_ir::scalar::FiniteReal::new(21.9).expect("finite relation fixture"),
            ),
        ),
        (
            "atan(1)",
            CurveExpressionValue::Angle(
                cadmpeg_ir::scalar::FiniteReal::new(45.0).expect("finite relation fixture"),
            ),
        ),
    ];
    for (expression, expected) in dimensioned_cases {
        assert_eq!(
            parse_relation_expression::<CurveExpressionValue>(
                expression,
                &values,
                RelationEvaluationContext::default(),
            ),
            Some(expected),
            "{expression}"
        );
    }
    let Some(CurveExpressionValue::Angle(angle)) = parse_relation_expression::<CurveExpressionValue>(
        "atan2(1[cm],5[mm])",
        &values,
        RelationEvaluationContext::default(),
    ) else {
        panic!("dimensioned atan2 angle");
    };
    assert!((angle.get() - 2.0f64.atan().to_degrees()).abs() < EPS_RELATION_VALUE);
    for incompatible in [
        "if(1,1[mm],1[s])",
        "bound(1[mm],0[s],2[mm])",
        "mod(1[mm],1[s])",
        "atan2(1[mm],1[s])",
    ] {
        assert_eq!(
            parse_relation_expression::<CurveExpressionValue>(
                incompatible,
                &values,
                RelationEvaluationContext::default(),
            ),
            None,
            "{incompatible}"
        );
    }
    let force_ratio = parse_relation_expression::<CurveExpressionValue>(
        "1[lbf]/1[N]",
        &values,
        RelationEvaluationContext::default(),
    );
    let Some(CurveExpressionValue::Number(force_ratio)) = force_ratio else {
        panic!("force ratio");
    };
    assert!((force_ratio.get() - 4.448_221_615_260_5).abs() < EPS_RELATION_VALUE);
    for malformed in ["1[N/mm^]", "1[N//mm]", "1[N^128]"] {
        assert_eq!(
            parse_relation_expression::<CurveExpressionValue>(
                malformed,
                &values,
                RelationEvaluationContext::default(),
            ),
            None,
            "{malformed}"
        );
    }
}
