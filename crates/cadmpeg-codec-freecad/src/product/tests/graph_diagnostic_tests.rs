// SPDX-License-Identifier: Apache-2.0
//! Resource admission for product graph diagnostics.

use super::super::{
    linked_prototype_transform, occurrence_count, product_record_index, transfer_neutral,
};
use crate::native::{self, ProductNodeRecord};
use crate::test_support::assert_retained_refusal_at;
use std::collections::HashMap;

fn occurrence(object: &str, count: Option<u64>) -> ProductNodeRecord {
    ProductNodeRecord {
        id: format!("fcstd:native:product#{object}"),
        object: format!("fcstd:native:object#{object}"),
        node: native::ProductNode::Occurrence(native::LinkOccurrence {
            members: Vec::new(),
            prototype: Some(format!("fcstd:native:object#{object}")),
            external_document: None,
            local_transform: None,
            placement_property: None,
            array: native::LinkArray::try_new(
                count,
                Vec::new(),
                Vec::new(),
                Vec::new(),
                Vec::new(),
            )
            .expect("empty array carriers"),
            link_transform: Some(true),
            linked_subelements: Vec::new(),
            claim_child: None,
            copy_on_change: None,
            scale: None,
        }),
    }
}

#[test]
fn duplicate_product_record_refuses_before_diagnostic_allocation() {
    let record = super::node("A", &[]);
    assert_retained_refusal_at(&[], "fcstd product duplicate record", |ctx| {
        product_record_index(ctx, &[record.clone(), record.clone()]).map(|_| ())
    });
}

#[test]
fn product_parent_conflict_refuses_before_diagnostic_allocation() {
    let records = [super::node("A", &["C"]), super::node("B", &["C"])];
    assert_retained_refusal_at(&[], "fcstd product parent conflict", |ctx| {
        transfer_neutral(ctx, &records, &[], &[], &[], &[], &[]).map(|_| ())
    });
}

#[test]
fn nested_product_cycle_refuses_before_diagnostic_allocation() {
    let record = occurrence("Loop", None);
    let records = HashMap::from([(record.object.as_str(), &record)]);
    let placements = HashMap::new();
    assert_retained_refusal_at(&[], "fcstd nested product cycle", |ctx| {
        linked_prototype_transform(
            ctx,
            &record,
            &records,
            &placements,
            &mut vec![record.object.clone()],
        )
    });
}

#[test]
fn product_array_count_refuses_before_diagnostic_allocation() {
    let record = occurrence("Many", Some(1_000_001));
    assert_retained_refusal_at(&[], "fcstd product array count limit", |ctx| {
        occurrence_count(ctx, &record)
    });
}
