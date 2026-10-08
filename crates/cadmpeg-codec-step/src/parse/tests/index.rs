// SPDX-License-Identifier: Apache-2.0
use super::super::{AnchorResolver, BTreeMap, Value};

#[test]
fn entity_index_is_not_part_of_exchange_equality() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POINT();ENDSEC;END-ISO-10303-21;";
    let (indexed, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("required invariant");
    let (untouched, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("required invariant");
    crate::test_support::with_service_context(source, |_, ctx| {
        assert_eq!(
            indexed
                .entities(ctx, "POINT")
                .expect("indexed point traversal")
                .map(|row| row.expect("indexed record lookup fits"))
                .count(),
            1
        );
    });
    assert_eq!(indexed, untouched);
}

#[test]
fn released_source_graph_drops_records_and_cached_entity_indexes() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POINT();ENDSEC;END-ISO-10303-21;";
    crate::test_support::with_service_context(source, |bytes, ctx| {
        let (mut exchange, _) = crate::parse::parse_inner(bytes, ctx).expect("required invariant");
        assert!(exchange
            .has_entity(ctx, "POINT")
            .expect("lookup fits the budget"));

        let _ = exchange.release_source_graph();

        assert!(exchange.records().is_empty());
        assert!(exchange.header().is_empty());
        assert!(exchange.data().is_empty());
        assert!(!exchange
            .has_entity(ctx, "POINT")
            .expect("lookup fits the budget"));
    });
}

#[test]
fn entity_unions_are_ordered_unique_and_name_order_independent() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#2=(A()B());#1=B();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("required invariant");

    crate::test_support::with_service_context(source, |_, ctx| {
        let forward = exchange
            .entities_any(ctx, &["A", "B"])
            .expect("forward union traversal")
            .map(|entity| entity.expect("union partial traversal").0)
            .collect::<Vec<_>>();
        let reverse = exchange
            .entities_any(ctx, &["B", "A"])
            .expect("reverse union traversal")
            .map(|entity| entity.expect("union partial traversal").0)
            .collect::<Vec<_>>();

        assert_eq!(forward, vec![1, 2]);
        assert_eq!(reverse, forward);
    });
}

#[test]
fn entity_union_queries_remain_ordered_across_repeated_queries() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#3=C();#2=(A()B());#1=B();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid record graph");
    crate::test_support::with_service_context(source, |_, ctx| {
        assert_eq!(
            exchange
                .entities_any(ctx, &["A", "B"])
                .expect("union traversal")
                .map(|entity| entity.expect("union partial traversal").0)
                .collect::<Vec<_>>(),
            vec![1, 2],
        );

        for names in [["B", "A"], ["B", "C"]] {
            let actual = exchange
                .entities_any(ctx, &names)
                .expect("union traversal")
                .map(|entity| entity.expect("union partial traversal").0)
                .collect::<Vec<_>>();
            let expected = exchange
                .records()
                .iter()
                .filter(|(_, record)| {
                    record
                        .partials
                        .iter()
                        .any(|partial| names.contains(&partial.name.as_str()))
                })
                .map(|(&id, _)| id)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    });
}

#[test]
fn anchor_budget_charges_only_resource_expansion() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::new();
        let mut resolver = AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits");
        resolver.remaining_nodes = 0;

        let ordinary = Value::List((0..1024).map(Value::Integer).collect());
        assert_eq!(
            resolver.resolve_root(&ordinary).expect("ordinary value"),
            ordinary
        );
        assert_eq!(resolver.remaining_nodes, 0);
    });
}

#[test]
fn anchor_budget_still_bounds_resource_materialization() {
    crate::test_support::with_service_context(b"", |_, ctx| {
        let anchors = BTreeMap::from([(
            "a".to_string(),
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
        )]);
        let mut resolver = AnchorResolver::new(&anchors, ctx).expect("empty resolver scope fits");
        resolver.remaining_nodes = 2;

        assert!(resolver
            .resolve_root(&Value::Resource("a".to_string()))
            .is_err());
    });
}

#[test]
fn indexed_union_lookup_and_result_visits_preserve_refusal() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#3=C();#2=(A()B());#1=B();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid index input");
    for operation in [
        "STEP entity name lookup",
        "STEP indexed entity identifier traversal",
        "STEP indexed entity record lookup",
    ] {
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
                    let result = exchange
                        .entities_any(ctx, &["A", "B"])?
                        .collect::<Result<Vec<_>, _>>()
                        .map(|_| ());
                    if let Err(cadmpeg_core::CodecError::ResourceLimit(refusal)) = &result {
                        assert_eq!(ctx.resource_refusal().as_ref(), Some(refusal));
                    }
                    result
                })
            },
        );
    }
    crate::test_support::with_service_context(&[], |_, ctx| {
        let ids = exchange
            .entities_any(ctx, &["MISSING", "B", "A", "B"])
            .expect("indexed query fits service budget")
            .map(|entity| entity.expect("indexed query fits service budget").0)
            .collect::<Vec<_>>();
        assert_eq!(ids, [1, 2]);
        assert_eq!(
            exchange
                .entities_any(ctx, &[])
                .expect("indexed query fits service budget")
                .count(),
            0
        );
        assert_eq!(
            exchange
                .matching_entity_ids(ctx, |name| matches!(name, "A" | "B"))
                .expect("indexed query fits service budget")
                .collect::<Result<Vec<_>, _>>()
                .expect("indexed query fits service budget"),
            ids
        );
    });
}

#[test]
fn single_name_queries_do_not_reorder_the_indexed_records() {
    fn matching_work(record_count: u64) -> u64 {
        let records = (1..=record_count)
            .rev()
            .map(|id| format!("#{id}=A();"))
            .collect::<String>();
        let source = format!("ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;{records}ENDSEC;END-ISO-10303-21;");
        let (exchange, _) =
            crate::test_support::with_service_context(source.as_bytes(), crate::parse::parse_inner)
                .expect("valid unordered source records");
        crate::test_support::with_service_context(&[], |_, ctx| {
            let ids = exchange
                .entities_any(ctx, &["A"])
                .expect("single-name query fits")
                .map(|row| row.expect("indexed record lookup fits").0)
                .collect::<Vec<_>>();
            assert_eq!(ids, (1..=record_count).collect::<Vec<_>>());
        });
        crate::test_support::with_service_context(&[], |_, ctx| {
            let ids = exchange
                .matching_entity_ids(ctx, |name| name == "A")
                .expect("single-name match fits")
                .collect::<Result<Vec<_>, _>>()
                .expect("indexed IDs fit");
            assert_eq!(ids, (1..=record_count).collect::<Vec<_>>());
            let cadmpeg_core::CodecError::ResourceLimit(refusal) = ctx
                .charge_work(u64::MAX, "test completed indexed matching work")
                .expect_err("work probe refuses")
            else {
                panic!("work refusal required");
            };
            refusal.used
        })
    }
    // Doubling the records permits twice the linear copy/visit work plus the
    // same name traversal. No reorder work is needed for one indexed list.
    assert!(matching_work(128) <= 2 * matching_work(64));
}

#[test]
fn single_name_enumeration_needs_no_collection_or_materialized_storage() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#2=A();#1=A();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("valid indexed records");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
        let ids = exchange.entities(ctx, "A").expect("borrowed index lookup")
            .map(|row| row.expect("borrowed record lookup").0).collect::<Vec<_>>();
        assert_eq!(ids, [1, 2]);
    });
}

#[test]
fn single_name_enumeration_admits_each_yield_before_lookup() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#2=A();#1=A();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = crate::test_support::with_service_context(source, crate::parse::parse_inner)
        .expect("valid indexed records");
    for operation in ["STEP indexed entity identifier traversal", "STEP indexed entity record lookup"] {
        cadmpeg_test_support::refusal::resource_limit_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits, operation, |cap| {
                let mut policy = cadmpeg_core::decode::DecodePolicy::service();
                policy.limits.max_work_units = cap;
                crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
                    exchange.entities(ctx, "A")?.collect::<Result<Vec<_>, _>>().map(|_| ())
                })
            });
    }
}
