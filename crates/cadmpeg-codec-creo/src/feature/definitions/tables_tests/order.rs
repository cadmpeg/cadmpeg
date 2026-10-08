// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::feature::definitions::{
    order_table as parse_order_table, positional_order_table as parse_positional_order_table,
    FeatureOrderRow, FeatureOrderTable,
};

fn order_table(payload: &[u8], start: usize, end: usize) -> Option<FeatureOrderTable> {
    crate::decode::with_test_decode_ctx(|ctx| parse_order_table(ctx, payload, start, end))
        .expect("named order table admitted")
}

fn positional_order_table(
    payload: &[u8],
    start: usize,
    end: usize,
    table_class: u32,
) -> Option<FeatureOrderTable> {
    crate::decode::with_test_decode_ctx(|ctx| {
        parse_positional_order_table(ctx, payload, start, end, table_class)
    })
    .expect("positional order table admitted")
}

fn order_with_limit(
    limit: u64,
    positional: bool,
) -> Result<Option<FeatureOrderTable>, cadmpeg_core::CodecError> {
    let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\
            \xe0\x01ext_id\0\x09\xe0\x01int_id\0\x01\
            \xe0\x01bitmask\0\x00\xf1\xf7\x42\xe2\x0a\x02\x01";
    let replay = b"prefix\xf8\x02\xf7\x42\xfb\xe2\xf7\x43\
            \x09\x01\x00\xf1\xf7\x42\xe2\x0a\x02\x01";
    with_trim_limits(limit, u64::MAX, |ctx| {
        if positional {
            parse_positional_order_table(ctx, replay, 0, replay.len(), 66)
        } else {
            parse_order_table(ctx, named, 0, named.len())
        }
    })
}

#[test]
fn order_rows_and_identity_indexes_refuse_before_growth() {
    for positional in [false, true] {
        let table = crate::test_support::assert_refusal_order(
            ResourceDimension::CollectionItems,
            &[
                "creo order rows",
                "creo order external ID index",
                "creo order internal ID index",
            ],
            |limit| order_with_limit(limit, positional),
        )
        .expect("one order table");
        assert_eq!(table.rows.len(), 1);
    }
}

#[test]
fn positional_order_table_replays_prototype_and_following_rows() {
    let payload = b"prefix\xf8\x03\xf7\x42\xfb\xe2\xf7\x43\
            \x09\x01\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

    let order =
        positional_order_table(payload, 0, payload.len(), 66).expect("positional order_table");

    assert_eq!(order.declared_count, 3);
    assert!(order.has_prototype);
    assert!(order.is_complete());
    assert_eq!(order.entity_ref, Some(66));
    assert_eq!(order.rows.len(), 2);
    assert_eq!(order.rows[0].external_id, 10);
    assert_eq!(order.rows[0].internal_id, 2);
    assert_eq!(order.rows[0].bitmask, 1);
    assert_eq!(order.rows[1].external_id, 11);
    assert_eq!(order.internal_id(10), Some(2));
    assert_eq!(order.external_id(2), Some(10));

    let mut duplicate_external = order.clone();
    duplicate_external.declared_count += 1;
    duplicate_external.rows.push(FeatureOrderRow {
        external_id: 10,
        internal_id: 4,
        bitmask: 0,
        offset: 20,
    });
    assert_eq!(duplicate_external.internal_id(10), None);
    assert_eq!(duplicate_external.external_id(2), None);
    let mut duplicate_internal = order;
    duplicate_internal.declared_count += 1;
    duplicate_internal.rows.push(FeatureOrderRow {
        external_id: 12,
        internal_id: 2,
        bitmask: 0,
        offset: 21,
    });
    assert_eq!(duplicate_internal.external_id(2), None);
    assert_eq!(duplicate_internal.internal_id(10), None);
}

#[test]
fn named_order_table_replays_prototype_and_following_rows() {
    let payload = b"order_table\0\xf8\x03\xf7\x42\xfb\xe2\
            \xe0\x01ext_id\0\x09\xe0\x01int_id\0\x01\
            \xe0\x01bitmask\0\x00\xf1\xf7\x42\xe2\
            \x0a\x02\x01\xe2\x0b\x03\x00";

    let order = order_table(payload, 0, payload.len()).expect("named order_table");

    assert_eq!(order.declared_count, 3);
    assert!(order.has_prototype);
    assert!(order.is_complete());
    assert_eq!(order.entity_ref, Some(66));
    assert_eq!(order.rows.len(), 2);
    assert_eq!(order.external_id(2), Some(10));
    assert_eq!(order.internal_id(11), Some(3));
}

#[test]
fn order_tables_retain_extents_without_decoded_rows() {
    let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\xf1\xf7\x42\xe2";
    let order = order_table(named, 0, named.len()).expect("named order_table header");
    assert_eq!(order.declared_count, 2);
    assert!(!order.has_prototype);
    assert!(!order.is_complete());
    assert_eq!(order.entity_ref, Some(66));
    assert!(order.rows.is_empty());

    let positional = b"\xf8\x02\xf7\x42\xfb\xe2";
    let order = positional_order_table(positional, 0, positional.len(), 66)
        .expect("positional order_table header");
    assert_eq!(order.declared_count, 2);
    assert!(!order.has_prototype);
    assert!(!order.is_complete());
    assert_eq!(order.entity_ref, Some(66));
    assert!(order.rows.is_empty());
}

#[test]
fn incomplete_order_tables_do_not_resolve_identifiers() {
    let named = b"order_table\0\xf8\x02\xf7\x42\xfb\xe2\
            \xf1\xf7\x42\xe2\x0a\x02\x00";
    let order = order_table(named, 0, named.len()).expect("named order_table");
    assert_eq!(order.rows.len(), 1);
    assert!(!order.is_complete());
    assert_eq!(order.internal_id(10), None);
    assert_eq!(order.external_id(2), None);

    let positional = b"\xf8\x02\xf7\x42\xfb\xe2";
    let order = positional_order_table(positional, 0, positional.len(), 66)
        .expect("positional order_table");
    assert!(!order.is_complete());
    assert_eq!(order.internal_id(10), None);
}
