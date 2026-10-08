// SPDX-License-Identifier: Apache-2.0

fn assert_scan_work_refusal<T>(
    operation: &str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) {
    cadmpeg_test_support::refusal::resource_limit_at(
        cadmpeg_core::decode::ResourceDimension::WorkUnits,
        operation,
        |cap| {
            let mut policy = cadmpeg_core::decode::DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let ctx = cadmpeg_core::decode::DecodeContext::new(&arena, &policy, false);
            run(&ctx)
        },
    );
}

#[test]
fn transform_preflight_refuses_work_before_chain_step() {
    let directory = [super::transform_entry(1, 0), super::transform_entry(3, 1)];
    assert_scan_work_refusal("iges transform preflight walk", |ctx| {
        super::super::enforce_transform_depth(&directory, ctx)
    });
}

#[test]
fn consumed_support_closure_refuses_work_before_advance() {
    let directory = [super::transform_entry(1, 0), super::transform_entry(3, 1)];
    let records = std::collections::BTreeMap::new();
    assert_scan_work_refusal("iges consumed-support closure traversal", |ctx| {
        super::super::consumed_support_sequences(&directory, &records, ctx).map(|_| ())
    });
}

#[test]
fn linear_nurbs_parameters_admit_knots_outside_the_returned_interval() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let knots: Vec<_> = (0..1002).map(f64::from).collect();
    let error = cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        "iges linear NURBS parameter knots",
        |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            super::super::linear_nurbs_parameters(1, &knots, 1000, false, [500.25, 500.75], &ctx)
                .map(|_| ())
        },
    );
    assert!(
        matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit) if limit.additional == 1002)
    );
    crate::test_support::with_service_context(&[], |ctx| {
        let (parameters, _storage) =
            super::super::linear_nurbs_parameters(1, &knots, 1000, false, [500.25, 500.75], ctx)
                .unwrap()
                .unwrap();
        assert_eq!(parameters, [500.25, 500.75]);
    });
}

#[test]
fn invalid_first_linear_nurbs_knot_does_not_admit_the_tail() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::super::linear_nurbs_parameters(
        1,
        &[f64::NAN, 0.0, 1.0, 1.0],
        2,
        false,
        [0.0, 1.0],
        &ctx
    )
    .unwrap()
    .is_none());
}

#[test]
fn declared_control_intervals_stop_at_the_first_missing_coordinate() {
    use crate::parameter::{ParameterRecord, Token, TokenValue};
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    const CONTROL_COUNT: usize = 2000;
    const POLE_START: usize = 7 + (CONTROL_COUNT + 2) + CONTROL_COUNT;
    const PARAMETER_END: usize = POLE_START + CONTROL_COUNT * 3 + 2;
    let mut tokens = vec![
        Token {
            value: TokenValue::Integer(0),
            span: 0..0
        };
        PARAMETER_END
    ];
    tokens[1].value = TokenValue::Integer(1999);
    tokens[2].value = TokenValue::Integer(1);
    tokens[POLE_START].value = TokenValue::Omitted;
    let record =
        ParameterRecord::from_test_tokens(1, 1..2, Vec::new(), PARAMETER_END, tokens, Vec::new());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(super::super::type126_declared_control_points(
        &record,
        crate::global::RealPrecision {
            single_significance: 6,
            double_significance: 15
        },
        &ctx
    )
    .unwrap()
    .is_none());
}

#[test]
fn source_sequence_getters_refuse_their_tree_searches() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    crate::test_support::with_service_context(&[], |setup| {
        let stem = crate::ids::Stem::directory(1_u32);
        let body = crate::ids::body(&stem);
        let face = crate::ids::face(&stem);
        let curve = crate::ids::curve(&stem);
        let surface = crate::ids::surface(&stem);
        let point = crate::ids::point(&stem);
        let mut sequences = super::super::SourceSequences::new(setup).unwrap();
        sequences.record_body(&body, 1, &stem, setup).unwrap();
        sequences.record_face(&face, 1, setup).unwrap();
        sequences.record_curve(&curve, 1, setup).unwrap();
        sequences.record_surface(&surface, 1, setup).unwrap();
        sequences.record_point(&point, &stem, setup).unwrap();
        for kind in [
            "body",
            "face",
            "curve",
            "surface",
            "point",
            "body_neutral_form",
        ] {
            assert_scan_work_refusal("iges source sequence lookup", |ctx| match kind {
                "body" => sequences.body(&body, ctx),
                "face" => sequences.face(&face, ctx),
                "curve" => sequences.curve(&curve, ctx),
                "surface" => sequences.surface(&surface, ctx),
                "point" => sequences.point(&point, ctx),
                "body_neutral_form" => sequences.body_neutral_form(&body, ctx),
                _ => unreachable!(),
            });
        }
        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
        assert_eq!(sequences.point(&point, &ctx).unwrap(), Some(1));
    });
}
