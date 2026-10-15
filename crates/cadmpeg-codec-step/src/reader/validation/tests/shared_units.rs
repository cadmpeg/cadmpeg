// SPDX-License-Identifier: Apache-2.0
//! Derived-unit scale and claim expansion are shared across measures.

use cadmpeg_core::{
    decode::{DecodePolicy, ResourceDimension},
    CodecError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

fn exchange(count: u64, resolved: bool) -> crate::parse::Exchange {
    let mut records = String::from("#1=DERIVED_UNIT((");
    for id in 10..10 + count {
        if id != 10 {
            records.push(',');
        }
        write!(records, "#{id}").expect("element");
    }
    records.push_str("));");
    if resolved {
        records.push_str("#2=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));");
    } else {
        records.push_str("#2=ITEM();");
    }
    for id in 10..10 + count {
        write!(records, "#{id}=DERIVED_UNIT_ELEMENT(#2,0.);").expect("derived element");
    }
    for id in 1000..1000 + count {
        write!(
            records,
            "#{id}=MEASURE_REPRESENTATION_ITEM('',AREA_MEASURE(2.),#1);"
        )
        .expect("measure");
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("exchange")
        .0
}

#[test]
fn validation_reuses_shared_derived_scales_and_keeps_each_fallback_warning() {
    for resolved in [true, false] {
        for count in [16_u64, 32, 64] {
            let exchange = exchange(count, resolved);
            let first_work = first_evaluation_work(|ctx| {
                let mut scales = super::super::UnitScales::default();
                let mut expanded = BTreeSet::new();
                let mut typed = BTreeSet::from([1]);
                let mut scratch = ctx.reserve_scoped(0, "test unit scratch").expect("scope");
                let mut claims = ctx.reserve_scoped(0, "test unit claims").expect("scope");
                let reports =
                    std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
                let mut losses = Vec::new();
                super::super::expected_value(
                    1000,
                    &exchange.records()[&1000],
                    (&exchange, &mut scales, &mut scratch),
                    3.0,
                    (&mut losses, &reports),
                    ctx,
                )
                .expect("first derived scale");
                claims
                    .with_storage(|| {
                        super::super::collect_unit_records(
                            1,
                            &exchange,
                            &mut typed,
                            (&mut expanded, &mut scratch),
                            ctx,
                        )
                    })
                    .expect("first claim expansion");
            });
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = first_work + 2000 * count;
            with_session(&policy, |ctx| {
                let mut scales = super::super::UnitScales::default();
                let mut expanded = BTreeSet::new();
                let mut typed = BTreeSet::from([1]);
                let mut scratch = ctx.reserve_scoped(0, "test unit scratch").expect("scope");
                let mut claims = ctx.reserve_scoped(0, "test unit claims").expect("scope");
                let reports =
                    std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
                let mut losses = Vec::new();
                for id in 1000..1000 + count {
                    let record = &exchange.records()[&id];
                    let value = super::super::expected_value(
                        id,
                        record,
                        (&exchange, &mut scales, &mut scratch),
                        3.0,
                        (&mut losses, &reports),
                        ctx,
                    )
                    .expect("shared derived scale");
                    let Some(super::super::Expected::Area(value)) = value else {
                        panic!("area measure");
                    };
                    assert_eq!(value, if resolved { 2.0 } else { 18.0 });
                    claims
                        .with_storage(|| {
                            super::super::collect_unit_records(
                                1,
                                &exchange,
                                &mut typed,
                                (&mut expanded, &mut scratch),
                                ctx,
                            )
                        })
                        .expect("shared claims");
                }
                assert_eq!(
                    scales.derived,
                    BTreeMap::from([(1, resolved.then_some(1.0))])
                );
                assert_eq!(expanded, BTreeSet::from([1]));
                assert_eq!(
                    typed,
                    [1, 2]
                        .into_iter()
                        .chain(10..10 + count)
                        .collect::<BTreeSet<_>>()
                );
                if resolved {
                    assert!(losses.is_empty());
                } else {
                    assert_eq!(u64::try_from(losses.len()).expect("count"), count);
                    for (loss, id) in losses.iter().zip(1000..1000 + count) {
                        assert_eq!(
                            loss.code,
                            crate::loss::StepLossCode::ValidationMeasureUnitUnresolved.kind()
                        );
                        assert_eq!(loss.message, format!("geometric validation AREA_MEASURE measure #{id} unit scale did not resolve; the document length scale was used"));
                    }
                }
            });
        }
    }
}

#[test]
fn validation_derived_unit_walk_keeps_its_work_refusal() {
    let exchange = exchange(16, true);
    let operation = "STEP validation elements traversal";
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            with_session(&policy, |ctx| {
                let reports = std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports")?);
                let mut scratch = ctx.reserve_scoped(0, "test scratch")?;
                let result = super::super::expected_value(
                    1000,
                    &exchange.records()[&1000],
                    (
                        &exchange,
                        &mut super::super::UnitScales::default(),
                        &mut scratch,
                    ),
                    3.0,
                    (&mut Vec::new(), &reports),
                    ctx,
                );
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(refusal.operation, operation);
                    assert!(refusal.used < 10_000);
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            })
        },
    );
}

#[test]
fn validation_unit_claim_walk_keeps_its_work_refusal() {
    let exchange = exchange(16, true);
    let operation = "STEP derived unit element traversal";
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            with_session(&policy, |ctx| {
                let mut scratch = ctx.reserve_scoped(0, "test scratch")?;
                let mut claims = ctx.reserve_scoped(0, "test claims")?;
                let result = claims.with_storage(|| {
                    super::super::collect_unit_records(
                        1,
                        &exchange,
                        &mut BTreeSet::new(),
                        (&mut BTreeSet::new(), &mut scratch),
                        ctx,
                    )
                });
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(refusal.operation, operation);
                    assert!(refusal.used < 10_000);
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            })
        },
    );
}

fn with_session<T>(
    policy: &DecodePolicy,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(b"", &arena, policy).expect("root");
    let result = run(&ctx);
    if let Some(refusal) = ctx.resource_refusal() {
        assert!(
            matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(sticky)) if sticky == refusal)
        );
    } else {
        ctx.finish_session().expect("completed session");
    }
    result
}

#[test]
fn validation_cached_unresolved_unit_keeps_each_fallback_scale() {
    let exchange = exchange(2, false);
    with_session(&DecodePolicy::service(), |ctx| {
        let mut scales = super::super::UnitScales::default();
        let mut scratch = ctx.reserve_scoped(0, "test unit scratch").expect("scope");
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let mut losses = Vec::new();
        for (fallback, expected) in [(3.0, 18.0), (4.0, 32.0)] {
            let value = super::super::expected_value(
                1000,
                &exchange.records()[&1000],
                (&exchange, &mut scales, &mut scratch),
                fallback,
                (&mut losses, &reports),
                ctx,
            )
            .expect("fallback context");
            assert!(
                matches!(value, Some(super::super::Expected::Area(value)) if value == expected)
            );
        }
        assert_eq!(scales.derived, BTreeMap::from([(1, None)]));
        assert_eq!(losses.len(), 2);
        assert_eq!(losses[0], losses[1]);
        assert_eq!(
            losses[0].code,
            crate::loss::StepLossCode::ValidationMeasureUnitUnresolved.kind()
        );
    });
}

// Measure one completed first evaluation. Repeated work must fit the separate
// per-record allowance; the control does not run the repeated queries.
fn first_evaluation_work(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>)) -> u64 {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
        ResourceDimension::WorkUnits,
        "test completed first evaluation",
        Some(1),
    );
    with_session(&policy, |ctx| {
        run(ctx);
        let error = ctx
            .charge_work(1, "test completed first evaluation")
            .expect_err("observe completed work");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("work observation");
        };
        assert_eq!(refusal.operation, "test completed first evaluation");
        refusal.used
    })
}

fn shared_base_chain(elements: u64) -> crate::parse::Exchange {
    let mut records = String::from("#1=DERIVED_UNIT((");
    for id in 10..10 + elements {
        if id != 10 {
            records.push(',');
        }
        write!(records, "#{id}").expect("element");
    }
    records.push_str("));");
    for id in 10..10 + elements {
        write!(records, "#{id}=DERIVED_UNIT_ELEMENT(#2,0.);").expect("element");
    }
    for layer in 0..64 {
        let unit = if layer == 0 { 2 } else { 3000 + layer };
        let factor = 4000 + layer;
        let next = 3001 + layer;
        write!(records, "#{unit}=(LENGTH_UNIT() NAMED_UNIT(*) CONVERSION_BASED_UNIT('shared',#{factor}));#{factor}=MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#{next});").expect("base chain");
    }
    records.push_str("#3064=(LENGTH_UNIT() NAMED_UNIT(*) SI_UNIT(.MILLI.,.METRE.));#1000=MEASURE_REPRESENTATION_ITEM('',AREA_MEASURE(2.),#1);");
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("exchange")
        .0
}

#[test]
fn validation_shares_base_unit_roots_across_distinct_elements() {
    let control = shared_base_chain(1);
    let first_work = first_evaluation_work(|ctx| {
        let mut scales = super::super::UnitScales::default();
        let mut storage = ctx.reserve_scoped(0, "test unit scratch").expect("scope");
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        super::super::expected_value(
            1000,
            &control.records()[&1000],
            (&control, &mut scales, &mut storage),
            3.0,
            (&mut Vec::new(), &reports),
            ctx,
        )
        .expect("first base root");
    });
    for count in [16, 32, 64] {
        let exchange = shared_base_chain(count);
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = first_work + 2000 * count;
        with_session(&policy, |ctx| {
            let mut scales = super::super::UnitScales::default();
            let mut storage = ctx.reserve_scoped(0, "test unit scratch").expect("scope");
            let reports =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
            let mut losses = Vec::new();
            let expected = super::super::expected_value(
                1000,
                &exchange.records()[&1000],
                (&exchange, &mut scales, &mut storage),
                3.0,
                (&mut losses, &reports),
                ctx,
            )
            .expect("shared base root");
            assert!(matches!(expected, Some(super::super::Expected::Area(value)) if value == 2.0));
            assert_eq!(scales.derived, BTreeMap::from([(1, Some(1.0))]));
            assert_eq!(scales.length, BTreeMap::from([(2, Some(1.0))]));
            assert!(losses.is_empty());
        });
    }
}
