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
fn entity_union_partial_refusal_is_an_iterator_error() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(A()B());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid union input");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
        let error = exchange
            .entities_any(ctx, &["A"])
            .expect("one record admission fits")
            .next()
            .expect("partial refusal is yielded")
            .expect_err("two partial visits exceed the remaining budget");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "STEP entity union partial traversal"
                && Some(limit) == ctx.resource_refusal())
        );
    });
}

#[test]
fn matching_entity_partial_refusal_is_an_iterator_error() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=(A()B());ENDSEC;END-ISO-10303-21;";
    let (exchange, _) =
        crate::test_support::with_service_context(source, crate::parse::parse_inner)
            .expect("valid matching input");
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = 1;
    crate::test_support::with_policy_context(&[], &policy, |_, ctx| {
        let error = exchange
            .matching_entity_ids(ctx, |name| name == "A")
            .expect("one record admission fits")
            .next()
            .expect("partial refusal is yielded")
            .expect_err("two partial visits exceed the remaining budget");
        assert!(
            matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.operation == "STEP matching entity partial traversal"
                && Some(limit) == ctx.resource_refusal())
        );
    });
}
