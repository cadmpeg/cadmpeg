// SPDX-License-Identifier: Apache-2.0


#[test]
fn measure_index_revisits_shorter_paths_and_stops_cycles() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2,#3);#2=ITEM(#4);#3=ITEM(#5,#1);#4=ITEM(#5);#5=ITEM(#6);#6=MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#99);#99=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    crate::test_support::with_service_context(source, |_, ctx| {
        let mut losses = Vec::new();
        let reports = std::cell::RefCell::new(ctx.reserve_scoped(0,"test reports").expect("scope"));
        let cache = std::cell::RefCell::new(super::super::MeasureWalk::new(ctx).expect("cache"));
        let mut measurements = super::super::MeasureContext { length_scale: 1.0,angle_scale: 1.0,graph_limit: 4,cache: &cache,losses: (&mut losses,&reports) };
        let mut index = super::super::measure_carriers::CharacteristicIndex::new(ctx).expect("index");
        let summary = index.value_summary(&crate::parse::Value::Reference(1),0,&exchange,&measurements).expect("measure traversal");
        assert_eq!(summary[0],super::super::carriers::SetId::Leaf(6));
        measurements.graph_limit = 3;
        assert_eq!(index.value_summary(&crate::parse::Value::Reference(1),0,&exchange,&measurements).expect("depth cutoff")[0],super::super::carriers::SetId::Empty);
    });
}

#[test]
fn failed_measure_evaluation_reuses_a_shared_dag() {
    use std::fmt::Write as _;
    let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;");
    for id in 1..=32 {
        let next = ((id - 1) / 2 + 1) * 2 + 1;
        writeln!(source, "#{id}=ITEM(#{next},#{});", next + 1).expect("write graph");
    }
    source.push_str("#33=ITEM($);#34=ITEM($);ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("measure graph");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    // Each of 33 reachable nodes uses one active entry and one completed result.
    policy.limits.max_collection_items = 128;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut losses = Vec::new();
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            cache: &std::cell::RefCell::new(super::super::MeasureWalk::new(&ctx).expect("measure cache")),
            losses: (&mut losses, &reports),
        };
        assert!(super::super::measure(
            &crate::parse::Value::Reference(1),
            &exchange,
            &mut measurements,
            ctx
        )
        .expect("linear failed evaluation")
        .is_none());
        assert!(losses.is_empty());
    });
}

#[test]
fn measure_cycles_do_not_reuse_path_dependent_values() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2,2.);#2=ITEM(#1,1.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("measure cycle");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut losses = Vec::new();
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            cache: &std::cell::RefCell::new(super::super::MeasureWalk::new(&ctx).expect("measure cache")),
            losses: (&mut losses, &reports),
        };
        let mut walk = super::super::MeasureWalk::new(ctx).expect("measure cache");
        assert_eq!(
            super::super::measure_inner(
                &crate::parse::Value::Reference(1),
                &exchange,
                &mut walk,
                0,
                &mut measurements,
                ctx
            )
            .expect("first cycle root")
            .expect("first value")
            .value
            .get(),
            1.0
        );
        assert_eq!(
            super::super::measure_inner(
                &crate::parse::Value::Reference(2),
                &exchange,
                &mut walk,
                0,
                &mut measurements,
                ctx
            )
            .expect("second cycle root")
            .expect("second value")
            .value
            .get(),
            2.0
        );
    });
}

#[test]
fn measure_completion_cache_keeps_remaining_depth() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2);#2=ITEM(1.);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("measure depth graph");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut losses = Vec::new();
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 2,
            cache: &std::cell::RefCell::new(super::super::MeasureWalk::new(&ctx).expect("measure cache")),
            losses: (&mut losses, &reports),
        };
        let mut walk = super::super::MeasureWalk::new(ctx).expect("measure cache");
        let value = crate::parse::Value::Reference(1);
        assert!(super::super::measure_inner(
            &value,
            &exchange,
            &mut walk,
            1,
            &mut measurements,
            ctx
        )
        .expect("cut off child")
        .is_none());
        assert_eq!(
            super::super::measure_inner(&value, &exchange, &mut walk, 0, &mut measurements, ctx)
                .expect("complete child")
                .expect("value")
                .value
                .get(),
            1.0
        );
    });
}

#[test]
fn failed_measure_dag_after_a_cycle_still_reuses_pure_completions() {
    use std::fmt::Write as _;
    let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2,#100);#2=ITEM(#1);");
    for id in 100..132 {
        let next = ((id - 100) / 2 + 1) * 2 + 100;
        writeln!(source, "#{id}=ITEM(#{next},#{});", next + 1).expect("write graph");
    }
    source.push_str("#132=ITEM($);#133=ITEM($);ENDSEC;END-ISO-10303-21;");
    let (exchange, _) =
        crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
            .expect("measure graph");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 128;
    crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
        let mut losses = Vec::new();
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "report fixture").expect("scope"));
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            cache: &std::cell::RefCell::new(super::super::MeasureWalk::new(&ctx).expect("measure cache")),
            losses: (&mut losses, &reports),
        };
        assert!(super::super::measure(
            &crate::parse::Value::Reference(1),
            &exchange,
            &mut measurements,
            ctx
        )
        .expect("pure completions survive prior cycle")
        .is_none());
        assert!(losses.is_empty());
    });
}

#[test]
fn failed_measure_cycle_reuses_duplicate_edges() {
    use std::fmt::Write as _;
    for count in [8, 16, 32] {
        let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;");
        for id in 1..count {
            writeln!(source, "#{id}=ITEM(#{next},#{next});", next = id + 1).expect("cyclic edge");
        }
        writeln!(source, "#{count}=ITEM(#1);ENDSEC;END-ISO-10303-21;").expect("cycle closure");
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("cycle graph");
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // Each record needs one active entry and one completed result.
        // Exponential replay exceeds this linear collection bound.
        policy.limits.max_collection_items = 2 * count;
        crate::test_support::with_policy_context(b"", &policy, |_, ctx| {
            let mut losses = Vec::new();
            let reports =
                std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
            let mut measurements = super::super::MeasureContext {
                length_scale: 1.0,
                angle_scale: 1.0,
                graph_limit: 64,
                cache: &std::cell::RefCell::new(super::super::MeasureWalk::new(&ctx).expect("measure cache")),
                losses: (&mut losses, &reports),
            };
            assert!(super::super::measure(
                &crate::parse::Value::Reference(1),
                &exchange,
                &mut measurements,
                ctx
            )
            .expect("linear cyclic failure")
            .is_none());
            assert!(losses.is_empty());
        });
    }
}

#[test]
fn failed_measure_cache_requires_the_same_active_prefix() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2,#3);#2=ITEM(#4,2.);#3=ITEM(#4);#4=ITEM(#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("contextual cycle");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut losses = Vec::new();
        let reports =
            std::cell::RefCell::new(ctx.reserve_scoped(0, "test reports").expect("scope"));
        let mut measurements = super::super::MeasureContext {
            length_scale: 1.0,
            angle_scale: 1.0,
            graph_limit: 64,
            cache: &std::cell::RefCell::new(super::super::MeasureWalk::new(&ctx).expect("measure cache")),
            losses: (&mut losses, &reports),
        };
        let mut walk = super::super::MeasureWalk::new(ctx).expect("cache");
        assert_eq!(
            super::super::measure_inner(
                &crate::parse::Value::Reference(2),
                &exchange,
                &mut walk,
                0,
                &mut measurements,
                ctx
            )
            .expect("first context")
            .expect("number")
            .value
            .get(),
            2.0
        );
        assert_eq!(
            super::super::measure_inner(
                &crate::parse::Value::Reference(3),
                &exchange,
                &mut walk,
                0,
                &mut measurements,
                ctx
            )
            .expect("second context")
            .expect("number")
            .value
            .get(),
            2.0
        );
    });
}

#[test]
fn branching_measure_cycles_share_failures_across_prefixes_and_roots() {
    use std::fmt::Write as _;
    for count in [2_u64,4,8,12] {
        let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;");
        for i in 0..count {
            let a = 3*i+1;
            writeln!(source,"#{a}=ITEM(#{b},#{c});#{b}=ITEM(#{next});#{c}=ITEM(#{next});", b=a+1,c=a+2,next=a+3).expect("branching cycle");
        }
        write!(source,"#{}=ITEM(",3*count+1).expect("terminal");
        for i in 0..count {
            if i != 0 { source.push(','); }
            write!(source,"#{},#{}",3*i+2,3*i+3).expect("back edges");
        }
        source.push_str(");ENDSEC;END-ISO-10303-21;");
        let (exchange,_) = crate::test_support::with_service_context(source.as_bytes(),crate::parse::parse_inner).expect("cycle graph");
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        // Each (record,remaining depth) is evaluated at most once. There are
        // at most 64 depth states per record; SCC failures also span roots.
        policy.limits.max_collection_items = 128*(3*count+1);
        policy.limits.max_work_units = 10_000*(3*count+1);
        crate::test_support::with_policy_context(b"",&policy,|_,ctx| {
            let mut losses = Vec::new();
            let reports = std::cell::RefCell::new(ctx.reserve_scoped(0,"test reports").expect("scope"));
            let cache = std::cell::RefCell::new(super::super::MeasureWalk::new(ctx).expect("cache"));
            let mut measurements = super::super::MeasureContext { length_scale: 1.0,angle_scale: 1.0,graph_limit: 64,cache: &cache,losses: (&mut losses,&reports) };
            for id in [1,2,3,3*count+1] {
                assert!(super::super::measure(&crate::parse::Value::Reference(id),&exchange,&mut measurements,ctx).expect("shared cyclic failure fits linear budget").is_none());
            }
            assert!(losses.is_empty());
        });
    }
}

#[test]
fn cyclic_measure_failures_preserve_outer_warning_multiplicity() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2,#3,1.);#2=LENGTH_ITEM(#4);#3=ITEM(#4);#4=ITEM(#2);ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner).expect("exchange");
    crate::test_support::with_service_context(b"", |_, ctx| {
        let mut losses = Vec::new();
        let reports = std::cell::RefCell::new(ctx.reserve_scoped(0,"test reports").expect("scope"));
        let cache = std::cell::RefCell::new(super::super::MeasureWalk::new(ctx).expect("cache"));
        let mut measurements = super::super::MeasureContext { length_scale: 1.0,angle_scale: 1.0,graph_limit: 64,cache: &cache,losses: (&mut losses,&reports) };
        for _ in 0..2 {
            assert_eq!(super::super::measure(&crate::parse::Value::Reference(1),&exchange,&mut measurements,ctx).expect("cyclic measure").expect("number").value.get(),1.0);
        }
        assert_eq!(losses.len(),4);
        for loss in losses {
            assert_eq!(loss.code,crate::loss::StepLossCode::PmiLengthUnitUnresolved.kind());
            assert_eq!(loss.message,"PMI length measure #2 unit scale did not resolve; the document length scale was used");
        }
    });
}

#[test]
fn successful_measure_roots_share_cyclic_empty_prefixes() {
    use std::fmt::Write as _;
    for count in [32_u64,64,128] {
        let mut source = String::from("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM((");
        for id in 10..10+count { if id!=10 { source.push(','); } write!(source,"#{id}").expect("prefix"); }
        source.push_str("),1.);");
        for id in 10..10+count { write!(source,"#{id}=ITEM(#1);").expect("back edge"); }
        for id in 1000..1000+count { write!(source,"#{id}=ITEM(#1);").expect("root"); }
        source.push_str("ENDSEC;END-ISO-10303-21;");
        let (exchange,_) = crate::test_support::with_service_context(source.as_bytes(),crate::parse::parse_inner).expect("exchange");
        let mut policy = cadmpeg_core::decode::DecodePolicy::service();
        policy.limits.max_collection_items = 40*(2*count);
        policy.limits.max_work_units = 4000*(2*count)+50_000;
        crate::test_support::with_policy_context(b"",&policy,|_,ctx| {
            let mut losses = Vec::new();
            let reports = std::cell::RefCell::new(ctx.reserve_scoped(0,"test reports").expect("scope"));
            let cache = std::cell::RefCell::new(super::super::MeasureWalk::new(ctx).expect("cache"));
            let mut measurements = super::super::MeasureContext { length_scale: 1.0,angle_scale: 1.0,graph_limit: 64,cache: &cache,losses: (&mut losses,&reports) };
            for id in 1000..1000+count {
                assert_eq!(super::super::measure(&crate::parse::Value::Reference(id),&exchange,&mut measurements,ctx).expect("shared successful measure").expect("number").value.get(),1.0);
            }
            assert!(losses.is_empty());
        });
    }
}

#[test]
fn shared_measure_values_do_not_cross_the_active_prefix() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#4,#6,#3);#2=ITEM(#1);#3=ITEM(#5,#8,#6);#4=ITEM(#2,#3,3.);#5=ITEM(#3,#7,#6);#6=ITEM(#5,#4);#7=ITEM(#6,6.);#8=ITEM(#8,#8);ENDSEC;END-ISO-10303-21;";
    let (exchange,_) = crate::test_support::with_service_context(source,crate::parse::parse_inner).expect("exchange");
    crate::test_support::with_service_context(b"",|_,ctx| {
        let mut losses = Vec::new();
        let reports = std::cell::RefCell::new(ctx.reserve_scoped(0,"test reports").expect("scope"));
        let cache = std::cell::RefCell::new(super::super::MeasureWalk::new(ctx).expect("cache"));
        let mut measurements = super::super::MeasureContext { length_scale: 1.0,angle_scale: 1.0,graph_limit: 8,cache: &cache,losses: (&mut losses,&reports) };
        for (id,expected) in [(1,Some(6.)),(6,Some(6.)),(4,Some(6.)),(3,Some(3.)),(7,Some(3.)),(5,Some(3.)),(2,Some(6.)),(8,None)] {
            assert_eq!(super::super::measure(&crate::parse::Value::Reference(id),&exchange,&mut measurements,ctx).expect("context-sensitive selection").map(|value|value.value.get()),expected);
        }
        assert!(losses.is_empty());
    });
}

#[test]
fn failed_measure_frames_close_only_after_a_loss_free_root() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#6);#2=ITEM(#8,#4);#3=ITEM(3.,#5,#4,#8);#4=LENGTH_ITEM(#4,#6,#8);#5=ITEM(#1);#6=ITEM(#1,#1,#4,#4);#7=LENGTH_ITEM(#7,#8,5.,1.);#8=ITEM(#1);ENDSEC;END-ISO-10303-21;";
    let (exchange,_) = crate::test_support::with_service_context(source,crate::parse::parse_inner).expect("exchange");
    crate::test_support::with_service_context(b"",|_,ctx| {
        let mut losses = Vec::new();
        let reports = std::cell::RefCell::new(ctx.reserve_scoped(0,"test reports").expect("scope"));
        let cache = std::cell::RefCell::new(super::super::MeasureWalk::new(ctx).expect("walk"));
        let mut measurements = super::super::MeasureContext { length_scale: 1.0,angle_scale: 1.0,graph_limit: 8,cache: &cache,losses: (&mut losses,&reports) };
        for id in [5,2,1,3,4,8,6] { super::super::measure(&crate::parse::Value::Reference(id),&exchange,&mut measurements,ctx).expect("preceding root"); }
        let start = measurements.losses.0.len();
        let value = super::super::measure(&crate::parse::Value::Reference(7),&exchange,&mut measurements,ctx).expect("root after warning-dependent failures").expect("numeric value");
        assert_eq!(value, cadmpeg_ir::pmi::PmiValue::new(5.0,cadmpeg_ir::pmi::PmiQuantity::Length).expect("length"));
        let actual = &measurements.losses.0[start..];
        assert_eq!(actual.len(),3);
        for (loss,id) in actual.iter().zip([7,4,4]) {
            assert_eq!(loss.code,crate::loss::StepLossCode::PmiLengthUnitUnresolved.kind());
            assert_eq!(loss.message,format!("PMI length measure #{id} unit scale did not resolve; the document length scale was used"));
        }
    });
}
