// SPDX-License-Identifier: Apache-2.0
//! Borrowed sketch native wire checks.

use super::super::{PmDcTransformPayload, PmDcTransformPayloadWire, PointTail, PointTailWire};
use crate::compact_matrix::CompactMatrix;
use crate::pmdc::{
    PmDcContentHeader, PmDcListMetadata, PmDcReference, PmDcReferenceList, PMDC_LIST_CLONE_COUNT,
};
use serde::Serialize;

#[derive(Serialize)]
struct Record<'a, T> {
    id: &'static str,
    value: &'a T,
}

#[test]
fn point_tail_borrowed_wire_refuses_retained_limit_before_clone() {
    let tail = PointTail::Present {
        state: 7,
        associations: PmDcReferenceList::new(
            8,
            Some(PmDcListMetadata::U16([1, 2])),
            vec![PmDcReference {
                index: 3,
                qualified: false,
            }],
        )
        .expect("paired point associations"),
    };
    let wire = PointTailWire::from(tail.clone());
    assert_eq!(
        serde_json::to_vec(&tail).expect("borrowed tail"),
        serde_json::to_vec(&wire).expect("owned tail")
    );
    let record = Record {
        id: "inventor:pmdc:point-tail#1",
        value: &tail,
    };
    PMDC_LIST_CLONE_COUNT.with(|count| count.set(0));
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": wire}),
    );
    PMDC_LIST_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));

    assert_eq!(
        serde_json::to_vec(&PointTail::Absent).expect("borrowed absent tail"),
        serde_json::to_vec(&PointTailWire::from(PointTail::Absent)).expect("owned absent tail")
    );
}

#[test]
fn transform_payload_streams_flat_matrix_under_retained_limit() {
    let reference = PmDcReference {
        index: 3,
        qualified: false,
    };
    let transform = PmDcTransformPayload {
        save_version_major: 16,
        header: PmDcContentHeader {
            header_value: 0,
            header_id: 18,
            next: reference,
            flags: 0,
            context: reference,
            source_index: 1,
        },
        prefix_present: true,
        matrix: CompactMatrix::try_from_rows(0, 0, [[2.0; 4]; 4]).expect("finite compact matrix"),
    };
    let wire = PmDcTransformPayloadWire::from(transform.clone());
    assert_eq!(
        serde_json::to_vec(&transform).expect("borrowed transform"),
        serde_json::to_vec(&wire).expect("owned transform")
    );
    let record = Record {
        id: "inventor:pmdc:transform#1",
        value: &transform,
    };
    cadmpeg_test_support::native_serialization::assert_native_limit(
        &record,
        serde_json::json!({"id": record.id, "value": wire}),
    );
}
