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
    // Validation visits all 1002 knots and performs its current terminal
    // probe. The source pass then admits one knot before reading it.
    assert!(matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::WorkUnits
            && limit.operation == "iges linear NURBS parameter knots"
            && (limit.limit, limit.used, limit.additional) == (1003, 1003, 1)));
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

#[test]
fn conflicting_affine_constraints_stop_after_the_first_origin() {
    const COUNT: usize = 2_000;
    let mut values = vec![0.0; COUNT];
    values[2] = 1.0;
    let uncertainties = vec![0.0; COUNT];
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // One finite-value pass and its end probe, one origin, and two pairs.
    policy.limits.max_work_units = u64::try_from(COUNT).unwrap() + 4;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(!super::super::declared_affine_progression(&values, &uncertainties, &ctx).unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn free_geometry_body_name_refuses_retained_bytes() {
    use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
    use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};

    let bytes = crate::test_support::test_curves_and_surfaces::line_file(0);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes, "iges free geometry body name", |cap| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = cap;
            crate::IgesCodec.decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions {
                policy, ..DecodeOptions::default()
            }).map_err(|error| match error {
                DecodeFailure::Codec(error) => error,
                other => panic!("{other:?}"),
            })
        },
    );
}

fn invalid_linear_knot_boundary(control_count: usize, invalid_index: usize,
    invalid_value: f64, exact: bool) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut knots: Vec<_> = (0..control_count + 2)
        .map(|index| f64::from(u32::try_from(index).unwrap())).collect();
    knots[invalid_index] = invalid_value;
    let before: Vec<_> = knots.iter().map(|value| value.to_bits()).collect();
    let range = [1.0, f64::from(u32::try_from(control_count).unwrap())];
    // The cheap domain checks pass. Validation visits each knot through
    // the first false predicate, then returns before creating parameters.
    let visits = u64::try_from(invalid_index + 1).unwrap();
    let cap = visits - u64::from(!exact);
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = cap;
    policy.limits.max_collection_items = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::linear_nurbs_parameters(1, &knots, control_count, false, range, &ctx);
    let first = if exact {
        assert!(result.unwrap().is_none());
        assert_eq!(knots.iter().map(|value| value.to_bits()).collect::<Vec<_>>(), before);
        let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1,
            "test exact invalid linear knot work") else {
            panic!("expected exact short-circuit validation work");
        };
        assert_eq!(first.operation, "test exact invalid linear knot work");
        first
    } else {
        let first = match result.as_ref() {
            Err(CodecError::ResourceLimit(first)) => *first,
            _ => panic!("expected refusal before the invalid knot visit"),
        };
        drop(result);
        assert_eq!(first.operation, "iges linear NURBS knot validation");
        first
    };
    assert_eq!(first.dimension, ResourceDimension::WorkUnits);
    assert_eq!((first.limit, first.used, first.additional), (cap, cap, 1));
    for _ in 0..64 {
        for source in [&knots[..], &[][..]] {
            let result = super::super::linear_nurbs_parameters(1, source,
                control_count, false, range, &ctx);
            assert!(matches!(result.as_ref(), Err(CodecError::ResourceLimit(last)) if *last == first));
            drop(result);
            assert_eq!(knots.iter().map(|value| value.to_bits()).collect::<Vec<_>>(), before);
        }
    }
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first));
}

#[test]
fn invalid_first_and_last_linear_knots_refuse_before_the_actual_visit() {
    for count in [2, 64] {
        for (index, value) in [(0, f64::NAN), (count + 1, f64::NAN), (count + 1, -1.0)] {
            invalid_linear_knot_boundary(count, index, value, false);
        }
    }
}

#[test]
fn invalid_first_and_last_linear_knots_accept_exact_recovery_without_storage() {
    for count in [2, 64] {
        for (index, value) in [(0, f64::NAN), (count + 1, f64::NAN), (count + 1, -1.0)] {
            invalid_linear_knot_boundary(count, index, value, true);
        }
    }
}

fn linear_parameter_collection_boundary(count: usize, items: usize, accepts: bool) {
    use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let mut knots = vec![0.0, 0.0];
    knots.extend((1..count).map(|index| f64::from(u32::try_from(index).unwrap())));
    knots.push(f64::from(u32::try_from(count - 1).unwrap()));
    let before = knots.clone();
    let range = [0.0, f64::from(u32::try_from(count - 1).unwrap())];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = u64_from_index(items);
    // Amortized output backing and its old-buffer overlap fit within four
    // times the actual output count. Retained output is not required here.
    policy.limits.max_materialized_bytes = u64_from_index(4 * count * std::mem::size_of::<f64>());
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_entities = 0;
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let result = super::super::linear_nurbs_parameters(1, &knots, count, false, range, &ctx);
    if accepts {
        let (parameters, storage) = result.unwrap().unwrap();
        let expected: Vec<_> = (0..count)
            .map(|index| f64::from(u32::try_from(index).unwrap())).collect();
        assert_eq!(parameters, expected);
        assert_eq!(knots, before);
        drop(parameters);
        drop(storage);
        let released = ctx.reserve_scoped(policy.limits.max_materialized_bytes,
            "test linear parameter scratch released").unwrap();
        drop(released);
        ctx.finish_session().unwrap();
    } else {
        let first = match result.as_ref() {
            Err(CodecError::ResourceLimit(first)) => *first,
            _ => panic!("expected actual parameter collection refusal"),
        };
        drop(result);
        assert_eq!(first.dimension, ResourceDimension::CollectionItems);
        assert_eq!(first.operation, "iges linear NURBS parameters");
        assert_eq!((first.limit, first.used, first.additional),
            (u64_from_index(items), u64_from_index(items), 1));
        for _ in 0..64 {
            for source in [knots.as_slice(), &[]] {
                let replay = super::super::linear_nurbs_parameters(1, source,
                    count, false, range, &ctx);
                assert!(matches!(replay.as_ref(),
                    Err(CodecError::ResourceLimit(last)) if *last == first));
                drop(replay);
                assert_eq!(knots, before);
            }
        }
        assert!(matches!(ctx.finish_session(),
            Err(CodecError::ResourceLimit(last)) if last == first));
    }
}

#[test]
fn linear_parameters_first_internal_knot_refuses_collection() {
    // The start endpoint is the only output before the first internal knot.
    for count in [4, 64] {
        linear_parameter_collection_boundary(count, 1, false);
    }
}

#[test]
fn linear_parameters_last_internal_knot_refuses_collection() {
    // One start and N-3 earlier internal knots precede the last internal knot.
    for count in [4, 64] {
        linear_parameter_collection_boundary(count, count - 2, false);
    }
}

#[test]
fn linear_parameters_accept_exact_collection_and_release_backing() {
    // One start, N-2 internal knots and one end consume exactly N output slots.
    for count in [2, 4, 64] {
        linear_parameter_collection_boundary(count, count, true);
    }
}
