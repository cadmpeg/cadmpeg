// SPDX-License-Identifier: Apache-2.0
//! Borrowed `PmDc` wire serialization through the native writer.

use super::super::{
    PmDcListMetadata, PmDcPairedMap, PmDcPairedMapWire, PmDcReference, PmDcReferenceList,
    PmDcReferenceListWire, PmDcU32List, PmDcU32ListWire, PMDC_LIST_CLONE_COUNT,
};
use serde::Serialize;

#[derive(Serialize)]
struct Record<'a, T> {
    id: &'static str,
    value: &'a T,
}

fn assert_borrowed_wire<T: Serialize, W: Serialize>(id: &'static str, value: &T, wire: W) {
    assert_eq!(
        serde_json::to_vec(value).expect("borrowed wire"),
        serde_json::to_vec(&wire).expect("owned wire")
    );
    let record = Record { id, value };
    let expected = serde_json::json!({"id": id, "value": wire});
    PMDC_LIST_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(&record, expected);
    PMDC_LIST_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
}

#[test]
fn pmdc_reference_list_refuses_retained_limit_before_clone() {
    let list = PmDcReferenceList::new(
        8,
        Some(PmDcListMetadata::U16([1, 2])),
        vec![PmDcReference {
            index: 7,
            qualified: false,
        }],
    )
    .expect("paired reference list");
    let wire = PmDcReferenceListWire::from(list.clone());
    assert_borrowed_wire("inventor:pmdc:references#1", &list, wire);
}

#[test]
fn pmdc_integer_list_refuses_retained_limit_before_clone() {
    let list = PmDcU32List::new(8, Some(PmDcListMetadata::U32([1, 2])), vec![7, 9])
        .expect("paired integer list");
    let wire = PmDcU32ListWire::from(list.clone());
    assert_borrowed_wire("inventor:pmdc:integers#1", &list, wire);
}

#[test]
fn pmdc_paired_map_refuses_retained_limit_before_clone() {
    let map = PmDcPairedMap::new(
        Some([1, 2]),
        vec![(
            PmDcReference {
                index: 7,
                qualified: false,
            },
            9_u32,
        )],
    )
    .expect("paired map");
    let wire = PmDcPairedMapWire::from(map.clone());
    assert_borrowed_wire("inventor:pmdc:map#1", &map, wire);
}
