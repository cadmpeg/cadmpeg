// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

#[test]
fn measure_index_revisits_shorter_paths_and_stops_cycles() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=ITEM(#2,#3);#2=ITEM(#4);#3=ITEM(#5,#1);#4=ITEM(#5);#5=ITEM(#6);#6=MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#99);#99=ITEM();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("exchange");
    crate::test_support::with_service_context(source, |_, ctx| {
        let mut visited = BTreeMap::new();
        let mut measures = BTreeSet::new();
        super::super::collect_measure_ids(
            &crate::parse::Value::Reference(1),
            &exchange,
            &mut visited,
            0,
            4,
            &mut measures,
            ctx,
        )
        .expect("measure traversal");
        assert_eq!(measures, BTreeSet::from([6]));
        assert_eq!(visited.get(&5), Some(&2));
        assert_eq!(visited.get(&6), Some(&3));
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
