// SPDX-License-Identifier: Apache-2.0
use super::super::{parse, AnchorResolver, BTreeMap, Value};

#[test]
fn entity_index_is_not_part_of_exchange_equality() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POINT();ENDSEC;END-ISO-10303-21;";
    let (indexed, _) = parse(source).expect("required invariant");
    let (untouched, _) = parse(source).expect("required invariant");
    assert_eq!(indexed.entities("POINT").count(), 1);
    assert_eq!(indexed, untouched);
}

#[test]
fn released_source_graph_drops_records_and_cached_entity_indexes() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#1=POINT();ENDSEC;END-ISO-10303-21;";
    let (mut exchange, _) = parse(source).expect("required invariant");
    assert!(exchange.has_entity("POINT"));

    exchange.release_source_graph();

    assert!(exchange.records.is_empty());
    assert!(exchange.header.is_empty());
    assert!(exchange.data.is_empty());
    assert!(!exchange.has_entity("POINT"));
}

#[test]
fn entity_unions_are_ordered_unique_and_name_order_independent() {
    let source = b"ISO-10303-21;HEADER;FILE_DESCRIPTION(('test'),'2;1');FILE_NAME('','',(''),(''),'','','');FILE_SCHEMA(('AP242'));ENDSEC;DATA;#2=(A()B());#1=B();ENDSEC;END-ISO-10303-21;";
    let (exchange, _) = parse(source).expect("required invariant");

    let forward = exchange
        .entities_any(&["A", "B"])
        .map(|(id, _)| id)
        .collect::<Vec<_>>();
    let reverse = exchange
        .entities_any(&["B", "A"])
        .map(|(id, _)| id)
        .collect::<Vec<_>>();

    assert_eq!(forward, vec![1, 2]);
    assert_eq!(reverse, forward);
}

#[test]
fn anchor_budget_charges_only_resource_expansion() {
    let anchors = BTreeMap::new();
    let mut resolver = AnchorResolver::new(&anchors, None);
    resolver.remaining_nodes = 0;

    let ordinary = Value::List((0..1024).map(Value::Integer).collect());
    assert_eq!(
        resolver.resolve_root(&ordinary).expect("ordinary value"),
        ordinary
    );
    assert_eq!(resolver.remaining_nodes, 0);
}

#[test]
fn anchor_budget_still_bounds_resource_materialization() {
    let anchors = BTreeMap::from([(
        "a".to_string(),
        Value::List(vec![Value::Integer(1), Value::Integer(2)]),
    )]);
    let mut resolver = AnchorResolver::new(&anchors, None);
    resolver.remaining_nodes = 2;

    assert!(resolver
        .resolve_root(&Value::Resource("a".to_string()))
        .is_err());
}
