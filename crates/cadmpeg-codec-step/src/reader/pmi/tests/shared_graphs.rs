// SPDX-License-Identifier: Apache-2.0
//! Path-expanded measure diagnostics keep their order and resource refusal.

use std::cell::RefCell;
use std::fmt::Write as _;

use crate::parse::Value;
use cadmpeg_core::decode::{DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

fn diamond(layers: u64) -> crate::parse::Exchange {
    let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;");
    for layer in 0..layers {
        let a = 3 * layer + 1;
        write!(
            source,
            "#{a}=LENGTH_ITEM(#{b},#{c});#{b}=ITEM(#{next});#{c}=ITEM(#{next});",
            b = a + 1,
            c = a + 2,
            next = a + 3
        )
        .expect("diamond");
    }
    write!(
        source,
        "#{}=LENGTH_ITEM($);ENDSEC;END-ISO-10303-21;",
        3 * layers + 1
    )
    .expect("leaf");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("exchange")
        .0
}

fn warning_paths(layer: u64, expected: &mut Vec<u64>) {
    expected.push(3 * layer + 1);
    if layer < 4 {
        warning_paths(layer + 1, expected);
        warning_paths(layer + 1, expected);
    }
}

#[test]
fn measure_diamond_keeps_each_path_warning_in_dfs_order() {
    let exchange = diamond(4);
    with_session(&DecodePolicy::service(), |ctx| {
        let mut losses = Vec::new();
        let reports = RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let cache = RefCell::new(super::super::MeasureWalk::new(ctx).expect("walk"));
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            cache: &cache,
            losses: (&mut losses, &reports),
        };
        let mut expected = Vec::new();
        warning_paths(0, &mut expected);
        assert_eq!(expected.len(), (1 << 5) - 1);
        for _ in 0..2 {
            assert!(
                super::super::measure(&Value::Reference(1), &exchange, &mut measurements, ctx)
                    .expect("walk")
                    .is_none()
            );
        }
        assert_eq!(losses.len(), 2 * expected.len());
        for (loss, id) in losses.iter().zip(expected.iter().cycle()) {
            assert_eq!(
                loss.code,
                crate::loss::StepLossCode::PmiLengthUnitUnresolved.kind()
            );
            assert_eq!(loss.message, format!("PMI length measure #{id} unit scale did not resolve; the document length scale was used"));
        }
    });
}

#[test]
fn measure_diamond_refuses_its_warning_output_budget_and_keeps_refusal() {
    let exchange = diamond(8);
    let mut policy = DecodePolicy::service();
    // Nine LENGTH_ITEM levels emit 2^9-1 warnings. The live record path
    // has fewer than 32 active entries; 256 total items cannot hold the output.
    policy.limits.max_collection_items = 256;
    with_session(&policy, |ctx| {
        let mut losses = Vec::new();
        let reports = RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let cache = RefCell::new(super::super::MeasureWalk::new(ctx).expect("walk"));
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            cache: &cache,
            losses: (&mut losses, &reports),
        };
        let error = super::super::measure(&Value::Reference(1), &exchange, &mut measurements, ctx)
            .expect_err("expanded warnings exceed budget");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("resource refusal");
        };
        assert_eq!(refusal.dimension, ResourceDimension::CollectionItems);
        assert_eq!(refusal.operation, "step_pmi_losses");
        assert!(losses.len() < (1 << 9) - 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

#[test]
fn measure_diamond_refuses_its_consumed_edge_budget_and_keeps_refusal() {
    let exchange = diamond(8);
    let operation = "STEP record value parameter traversal";
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            with_session(&policy, |ctx| {
                let mut losses = Vec::new();
                let reports = RefCell::new(ctx.reserve_scoped(0, "test reports")?);
                let cache = RefCell::new(super::super::MeasureWalk::new(ctx)?);
                let mut measurements = super::super::MeasureContext {
                    length_scale: 1.0,
                    angle_scale: 1.0,
                    graph_limit: 64,
                    cache: &cache,
                    losses: (&mut losses, &reports),
                };
                let result =
                    super::super::measure(&Value::Reference(1), &exchange, &mut measurements, ctx);
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(refusal.operation, operation);
                    assert!(refusal.used < 20_000);
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            })
        },
    );
}

#[test]
fn measure_diamond_refuses_warning_text_before_publication() {
    let exchange = diamond(8);
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    with_session(&policy, |ctx| {
        let mut losses = Vec::new();
        let reports = RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let cache = RefCell::new(super::super::MeasureWalk::new(ctx).expect("walk"));
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            cache: &cache,
            losses: (&mut losses, &reports),
        };
        let error = super::super::measure(&Value::Reference(1), &exchange, &mut measurements, ctx)
            .expect_err("warning text does not fit");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("resource refusal");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "STEP PMI measure unit warning");
        assert!(losses.is_empty());
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
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

fn characteristic_diamond() -> crate::parse::Exchange {
    let mut records = String::new();
    for layer in 0..8 {
        let a = 3 * layer + 1;
        write!(
            records,
            "#{a}=ITEM((#{b},#{c}));#{b}=ITEM(#{next});#{c}=ITEM(#{next});",
            b = a + 1,
            c = a + 2,
            next = a + 3
        )
        .expect("diamond");
    }
    records.push_str("#25=MEASURE_REPRESENTATION_ITEM('\\X2\\D83D\\X0\\',1.);");
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("exchange")
        .0
}

#[test]
fn characteristic_diamond_shares_carrier_identity_and_diagnostics() {
    let exchange = characteristic_diamond();
    with_session(&DecodePolicy::service(), |ctx| {
        let mut losses = Vec::new();
        let reports = RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let cache = RefCell::new(super::super::MeasureWalk::new(ctx).expect("walk"));
        let measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            cache: &cache,
            losses: (&mut losses, &reports),
        };
        let mut index =
            super::super::measure_carriers::CharacteristicIndex::new(ctx).expect("index");
        let values = [Value::Reference(1)];
        let mut expected = None;
        for id in [2000, 2001, 2000] {
            let analysis = index
                .analysis(
                    (id, true),
                    &super::super::MeasureParameters::Items(&values),
                    &exchange,
                    &measurements,
                )
                .expect("shared analysis");
            let super::super::NominalSelection::Unique(value) = analysis.selection else {
                panic!("one carrier");
            };
            assert_eq!(
                value,
                cadmpeg_ir::pmi::PmiValue::new(1.0, cadmpeg_ir::pmi::PmiQuantity::Ratio)
                    .expect("measure")
            );
            assert_eq!(analysis.losses.len(), 1);
            assert_eq!(
                analysis.losses[0].code,
                crate::loss::StepLossCode::MetadataStringInvalid.kind()
            );
            if let Some(ref expected) = expected {
                assert_eq!(&analysis.losses, expected);
            } else {
                expected = Some(analysis.losses.clone());
            }
        }
    });
}

fn characteristic_diamond_refusal(operation: &str) {
    let exchange = characteristic_diamond();
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            with_session(&policy, |ctx| {
                let result = (|| {
                    let mut losses = Vec::new();
                    let reports = RefCell::new(ctx.reserve_scoped(0, "test reports")?);
                    let cache = RefCell::new(super::super::MeasureWalk::new(ctx)?);
                    let measurements = super::super::MeasureContext {
                        length_scale: 1.0,
                        angle_scale: 1.0,
                        graph_limit: 64,
                        cache: &cache,
                        losses: (&mut losses, &reports),
                    };
                    let mut index = super::super::measure_carriers::CharacteristicIndex::new(ctx)?;
                    index
                        .analysis(
                            (2000, true),
                            &super::super::MeasureParameters::Items(&[Value::Reference(1)]),
                            &exchange,
                            &measurements,
                        )
                        .map(|_| ())
                })();
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(refusal.operation, operation);
                    assert!(refusal.used < 50_000);
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            })
        },
    );
}

#[test]
fn characteristic_diamond_refuses_discovery_walk_and_keeps_refusal() {
    characteristic_diamond_refusal("STEP characteristic carrier parameter traversal");
}

#[test]
fn characteristic_diamond_refuses_diagnostic_replay_and_keeps_refusal() {
    characteristic_diamond_refusal("STEP characteristic measure diagnostic replay");
}

fn usage_exchange(count: u64, usage_count: u64) -> crate::parse::Exchange {
    let mut records = String::from("#1=SHAPE_ASPECT('','',#99,.T.);#99=CARTESIAN_POINT('',(0.,0.,0.));#100=CARTESIAN_POINT('',(1.,0.,0.));");
    for id in 1000..1000 + count {
        write!(records, "#{id}=DIMENSIONAL_SIZE(#1,'');").expect("dimension");
    }
    for id in 10000..10000 + usage_count {
        write!(
            records,
            "#{id}=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#1,$,#{});",
            99 + id % 2
        )
        .expect("usage");
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("exchange");
    exchange
}

#[test]
fn geometric_usage_shares_repeated_applications_and_keeps_targets_and_claims() {
    for count in [16_u64, 32, 64, 128] {
        let exchange = usage_exchange(count, count);
        let control = usage_exchange(count, 2);
        let first_work = first_evaluation_work(|ctx| {
            let setup = cadmpeg_test_support::service_decode_context();
            let mut ir = cadmpeg_ir::CadIr::empty();
            let geometry = crate::reader::geometry::decode(&control, &mut ir, &setup)
                .expect("control geometry");
            let carriers =
                crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("control carriers");
            let topology = crate::reader::topology::decode(&control, &mut ir, &carriers, &setup)
                .expect("control topology");
            super::super::decode(&control, &geometry.value, &topology.value, &mut ir, ctx)
                .expect("two unique applications");
        });
        let setup = cadmpeg_test_support::service_decode_context();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let geometry =
            crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup)
            .expect("topology");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = first_work + 3000 * (2 * count + 4);
        with_session(&policy, |ctx| {
            let (outcome, _report_text) =
                super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, ctx)
                    .expect("shared applications fit");
            assert!(outcome.losses.is_empty());
            for id in 10000..10000 + count {
                assert!(outcome.claims.contains(&id));
            }
            assert_eq!(u64::try_from(ir.model.pmi.len()).expect("count"), count);
            for annotation in &ir.model.pmi {
                assert_eq!(annotation.targets.len(), 3);
                assert!(matches!(
                    annotation.targets[0],
                    cadmpeg_ir::pmi::PmiTarget::ShapeAspect { .. }
                ));
                let cadmpeg_ir::pmi::PmiTarget::Point { ref point } = annotation.targets[1] else {
                    panic!("first point");
                };
                assert!(point.as_str().ends_with("#99"));
                let cadmpeg_ir::pmi::PmiTarget::Point { ref point } = annotation.targets[2] else {
                    panic!("second point");
                };
                assert!(point.as_str().ends_with("#100"));
            }
        });
    }
}

#[test]
fn characteristic_ambiguity_admits_warning_text_before_publication() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=DIMENSIONAL_SIZE($,'width');#2=SHAPE_DIMENSION_REPRESENTATION('',(#10,#11),$);#10=MEASURE_REPRESENTATION_ITEM('',1.);#11=MEASURE_REPRESENTATION_ITEM('',2.);#100=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#1,#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let setup = cadmpeg_test_support::service_decode_context();
    let geometry =
        crate::reader::geometry::decode(&exchange, &mut cadmpeg_ir::CadIr::empty(), &setup)
            .expect("geometry");
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    with_session(&policy, |ctx| {
        let mut losses = Vec::new();
        let reports = RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let mut scratch = ctx.reserve_scoped(0, "test values").expect("scope");
        let error = super::super::characteristic_values(
            &exchange,
            &geometry.value,
            (&mut losses, &reports),
            64,
            &mut scratch,
            ctx,
        )
        .expect_err("ambiguity warning text does not fit");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("resource refusal");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "STEP PMI diagnostic text");
        assert!(losses.is_empty());
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}

#[test]
fn geometric_usage_shares_unsupported_pairs_without_claiming_records() {
    for definition in [1, 3] {
        let mut records = String::from("#1=SHAPE_ASPECT('','',#99,.T.);#3=ITEM();#99=ITEM();");
        for id in 1000..1064 {
            write!(records, "#{id}=DIMENSIONAL_SIZE(#1,'');").expect("dimension");
        }
        for id in 10000..10064 {
            write!(
                records,
                "#{id}=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#{definition},$,#99);"
            )
            .expect("usage");
        }
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("exchange");
        let setup = cadmpeg_test_support::service_decode_context();
        let mut ir = cadmpeg_ir::CadIr::empty();
        let geometry =
            crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
        let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
        let topology = crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup)
            .expect("topology");
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 3000 * 132;
        with_session(&policy, |ctx| {
            let (outcome, _report_text) =
                super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, ctx)
                    .expect("shared unsupported queries fit");
            assert!(outcome.losses.is_empty());
            for id in 10000..10064 {
                assert!(!outcome.claims.contains(&id));
            }
            assert_eq!(ir.model.pmi.len(), 64);
            assert!(ir
                .model
                .pmi
                .iter()
                .all(|annotation| annotation.targets.len() == 1));
        });
    }
}

#[test]
fn placement_ambiguity_admits_warning_text_before_publication() {
    let source = include_bytes!("data/ap12_composite_placement_first.p21");
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology =
        crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup).expect("topology");
    let operation = "STEP PMI placement ambiguity warning";
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::RetainedBytes,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = limit;
            with_session(&policy, |ctx| {
                let mut output = ir.clone();
                let result = super::super::decode(
                    &exchange,
                    &geometry.value,
                    &topology.value,
                    &mut output,
                    ctx,
                )
                .map(|_| ());
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(refusal.operation, operation);
                    assert!(refusal.used < 10_000);
                    assert!(output.model.pmi.is_empty());
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            })
        },
    );
}

fn measure_warning_diamond_refusal(operation: &str) {
    let exchange = diamond(4);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            with_session(&policy, |ctx| {
                let mut losses = Vec::new();
                let reports = RefCell::new(ctx.reserve_scoped(0, "test reports")?);
                let cache = RefCell::new(super::super::MeasureWalk::new(ctx)?);
                let mut measurements = super::super::MeasureContext {
                    length_scale: 1.0,
                    angle_scale: 1.0,
                    graph_limit: 64,
                    cache: &cache,
                    losses: (&mut losses, &reports),
                };
                let result = (|| {
                    super::super::measure(&Value::Reference(1), &exchange, &mut measurements, ctx)?;
                    super::super::measure(&Value::Reference(1), &exchange, &mut measurements, ctx)
                })();
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(refusal.operation, operation);
                    assert!(refusal.used < 100_000);
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            })
        },
    );
}

fn value_diamond(leaf: &str) -> crate::parse::Exchange {
    let mut records = String::new();
    for layer in 0..8 {
        let a = 3 * layer + 1;
        write!(
            records,
            "#{a}=ITEM(#{b},#{c});#{b}=ITEM(#{next});#{c}=ITEM(#{next});",
            b = a + 1,
            c = a + 2,
            next = a + 3
        )
        .expect("diamond");
    }
    write!(records, "#25=ITEM({leaf});#1000=ITEM(#3);").expect("leaf and second root");
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("exchange")
        .0
}

fn measure_value_diamond_refusal(operation: &str, leaf: &str) {
    let exchange = value_diamond(leaf);
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::WorkUnits,
        operation,
        |limit| {
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = limit;
            with_session(&policy, |ctx| {
                let mut losses = Vec::new();
                let reports = RefCell::new(ctx.reserve_scoped(0, "test reports")?);
                let cache = RefCell::new(super::super::MeasureWalk::new(ctx)?);
                let mut measurements = super::super::MeasureContext {
                    length_scale: 1.0,
                    angle_scale: 1.0,
                    graph_limit: 64,
                    cache: &cache,
                    losses: (&mut losses, &reports),
                };
                let result = (|| {
                    super::super::measure(&Value::Reference(1), &exchange, &mut measurements, ctx)?;
                    super::super::measure(
                        &Value::Reference(1000),
                        &exchange,
                        &mut measurements,
                        ctx,
                    )
                })();
                if let Err(CodecError::ResourceLimit(ref refusal)) = result {
                    assert_eq!(refusal.operation, operation);
                    assert!(refusal.used < 50_000);
                    assert_eq!(ctx.resource_refusal(), Some(*refusal));
                }
                result
            })
        },
    );
}

#[test]
fn measure_diamond_selected_path_walk_keeps_its_work_refusal() {
    measure_value_diamond_refusal("STEP measure selected path traversal", "1.");
}

#[test]
fn measure_diamond_shared_path_walk_keeps_its_work_refusal() {
    measure_value_diamond_refusal("STEP measure shared path traversal", "1.");
}

#[test]
fn measure_diamond_completion_walk_keeps_its_work_refusal() {
    measure_value_diamond_refusal("STEP failed measure component completion", "$");
}

#[test]
fn measure_diamond_cached_warning_replay_keeps_its_work_refusal() {
    measure_warning_diamond_refusal("STEP measure root loss replay");
}

#[test]
fn measure_diamond_warning_dependency_walk_keeps_its_work_refusal() {
    measure_warning_diamond_refusal("STEP measure warning dependency traversal");
}

fn unit_chain(count: u64, angle: bool, resolved: bool) -> crate::parse::Exchange {
    let mut records = String::new();
    let quantity = if angle { "PLANE_ANGLE" } else { "LENGTH" };
    for layer in 0..64 {
        let unit = 2 * layer + 1;
        let factor = unit + 1;
        write!(records, "#{unit}=({quantity}_UNIT() NAMED_UNIT(*) CONVERSION_BASED_UNIT('shared',#{factor}));#{factor}=MEASURE_WITH_UNIT({quantity}_MEASURE(1.),#{});", unit+2).expect("unit chain");
    }
    let name = if resolved {
        if angle {
            "RADIAN"
        } else {
            "METRE"
        }
    } else {
        "UNKNOWN"
    };
    let prefix = if angle { "$" } else { ".MILLI." };
    write!(
        records,
        "#129=({quantity}_UNIT() NAMED_UNIT(*) SI_UNIT({prefix},.{name}.));"
    )
    .expect("base unit");
    for id in 1000..1000 + count {
        write!(
            records,
            "#{id}=MEASURE_WITH_UNIT({quantity}_MEASURE(2.),#1);"
        )
        .expect("measure");
    }
    let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
    crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
        .expect("exchange")
        .0
}

#[test]
fn measure_shares_explicit_unit_roots_and_keeps_each_unresolved_warning() {
    for angle in [false, true] {
        for resolved in [false, true] {
            for count in [16_u64, 32, 64] {
                let exchange = unit_chain(count, angle, resolved);
                let first_work = first_evaluation_work(|ctx| {
                    let mut losses = Vec::new();
                    let reports =
                        RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
                    let cache = RefCell::new(super::super::MeasureWalk::new(ctx).expect("walk"));
                    let mut measurements = super::super::MeasureContext {
                        length_scale: 3.0,
                        angle_scale: 4.0,
                        graph_limit: 256,
                        cache: &cache,
                        losses: (&mut losses, &reports),
                    };
                    super::super::measure(
                        &Value::Reference(1000),
                        &exchange,
                        &mut measurements,
                        ctx,
                    )
                    .expect("first native unit evaluation");
                });
                let mut policy = DecodePolicy::service();
                policy.limits.max_work_units = first_work + 2000 * count;
                with_session(&policy, |ctx| {
                    let mut losses = Vec::new();
                    let reports =
                        RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
                    let cache = RefCell::new(super::super::MeasureWalk::new(ctx).expect("walk"));
                    let mut measurements = super::super::MeasureContext {
                        length_scale: 3.0,
                        angle_scale: 4.0,
                        graph_limit: 256,
                        cache: &cache,
                        losses: (&mut losses, &reports),
                    };
                    let quantity = if angle {
                        cadmpeg_ir::pmi::PmiQuantity::Angle
                    } else {
                        cadmpeg_ir::pmi::PmiQuantity::Length
                    };
                    let expected = if resolved {
                        2.0
                    } else if angle {
                        8.0
                    } else {
                        6.0
                    };
                    for id in 1000..1000 + count {
                        assert_eq!(
                            super::super::measure(
                                &Value::Reference(id),
                                &exchange,
                                &mut measurements,
                                ctx
                            )
                            .expect("shared unit root"),
                            cadmpeg_ir::pmi::PmiValue::new(expected, quantity)
                        );
                    }
                    assert_eq!(
                        cache.borrow().unit_scales,
                        std::collections::BTreeMap::from([((1, angle), resolved.then_some(1.0))])
                    );
                    if resolved {
                        assert!(losses.is_empty());
                    } else {
                        assert_eq!(u64::try_from(losses.len()).expect("count"), count);
                        let code = if angle {
                            crate::loss::StepLossCode::PmiAngleUnitUnresolved
                        } else {
                            crate::loss::StepLossCode::PmiLengthUnitUnresolved
                        };
                        for (loss, id) in losses.iter().zip(1000..1000 + count) {
                            let label = if angle { "angle" } else { "length" };
                            let scale = if angle { "plane-angle" } else { "length" };
                            assert_eq!(loss.code, code.kind());
                            assert_eq!(loss.message, format!("PMI {label} measure #{id} unit scale did not resolve; the document {scale} scale was used"));
                        }
                    }
                });
            }
        }
    }
}

#[test]
fn measure_explicit_unit_cache_separates_length_and_angle() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(LENGTH_UNIT() PLANE_ANGLE_UNIT() NAMED_UNIT(*) SI_UNIT($,.RADIAN.));ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    with_session(&DecodePolicy::service(), |ctx| {
        let mut walk = super::super::MeasureWalk::new(ctx).expect("walk");
        for _ in 0..2 {
            assert_eq!(
                walk.unit_scale(1, false, &exchange, ctx).expect("length"),
                None
            );
            assert_eq!(
                walk.unit_scale(1, true, &exchange, ctx).expect("angle"),
                Some(1.0)
            );
        }
        assert_eq!(
            walk.unit_scales,
            std::collections::BTreeMap::from([((1, false), None), ((1, true), Some(1.0))])
        );
    });
}

#[test]
fn datum_target_form_trims_unicode_through_exact_boundary_visits() {
    let mut policy = DecodePolicy::service();
    // Each direction visits one whitespace and one stopping character.
    policy.limits.max_work_units = 4;
    with_session(&policy, |ctx| {
        assert_eq!(
            super::super::datum_target_form("\u{2003}point\u{2003}", ctx)
                .expect("four boundary visits"),
            cadmpeg_ir::pmi::DatumTargetForm::Point
        );
    });
    let mut empty_policy = DecodePolicy::service();
    empty_policy.limits.max_work_units = 0;
    with_session(&empty_policy, |ctx| {
        assert_eq!(
            super::super::datum_target_form("", ctx).expect("no boundary visits"),
            cadmpeg_ir::pmi::DatumTargetForm::Other(String::new())
        );
    });
    with_session(&DecodePolicy::service(), |ctx| {
        for text in ["", "\u{2003}\t", "\u{2003}polygon\u{2003}"] {
            assert_eq!(
                super::super::datum_target_form(text, ctx).expect("original other form"),
                cadmpeg_ir::pmi::DatumTargetForm::Other(text.to_owned())
            );
        }
    });
}

#[test]
fn datum_target_form_boundary_visit_keeps_its_work_refusal() {
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    with_session(&policy, |ctx| {
        let error = super::super::datum_target_form("\u{2003}point\u{2003}", ctx)
            .expect_err("fourth boundary visit cannot proceed");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("resource refusal");
        };
        assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
        assert_eq!(refusal.operation, "STEP datum target form boundary scan");
        assert_eq!(refusal.used, 3);
        assert_eq!(refusal.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(refusal));
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

#[test]
fn discarded_tolerance_warning_text_refuses_when_promoted() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=PLUS_MINUS_TOLERANCE($,$);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    let setup = cadmpeg_test_support::service_decode_context();
    let mut ir = cadmpeg_ir::CadIr::empty();
    let geometry = crate::reader::geometry::decode(&exchange, &mut ir, &setup).expect("geometry");
    let carriers = crate::reader::index::CarrierIndex::from_ir(&ir, &setup).expect("carriers");
    let topology =
        crate::reader::topology::decode(&exchange, &mut ir, &carriers, &setup).expect("topology");
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    with_session(&policy, |ctx| {
        let (outcome, report_text) =
            super::super::decode(&exchange, &geometry.value, &topology.value, &mut ir, ctx)
                .expect("temporary report text");
        assert_eq!(outcome.losses.len(), 1);
        assert_eq!(
            outcome.losses[0].message,
            "PLUS_MINUS_TOLERANCE #1 has no resolvable dimension and limits"
        );
        let error = report_text
            .commit()
            .expect_err("retained report text cannot fit");
        let CodecError::ResourceLimit(refusal) = error else {
            panic!("text promotion refusal");
        };
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(refusal.operation, "STEP PMI report text storage");
        assert_eq!(ctx.resource_refusal(), Some(refusal));
    });
}
