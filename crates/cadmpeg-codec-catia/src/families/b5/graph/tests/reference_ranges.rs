// SPDX-License-Identifier: Apache-2.0
//! Count-bounded admission for compact B5 object-reference tokens.

use crate::families::b5::graph::B5Record;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

const REFERENCE_RANGE_SCAN: &str = "catia_b5_record_reference_range_scan";

#[test]
fn b5_record_reference_range_admission_preserves_lazy_truncation_and_work_refusal() {
    let record = B5Record {
        offset: 0,
        family: 0xb5,
        class: 0x23,
        object_id: 40,
        payload: vec![0x83, 0x18, 30, 0, 0x18, 31, 0, 0x01],
    };

    let service = crate::test_support::with_service_context(|ctx| {
        super::super::record_references(ctx, &record)
            .map(|references| references.collect::<Vec<_>>())
    })
    .expect("service record-reference scan budget");
    assert_eq!(service, vec![30, 31]);

    let refused = crate::test_support::with_work_refusal(REFERENCE_RANGE_SCAN, |ctx| {
        let result = super::super::record_references(ctx, &record)
            .map(|references| references.collect::<Vec<_>>());
        if let Err(CodecError::ResourceLimit(limit)) = &result {
            assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
            assert_eq!(ctx.resource_refusal().as_ref(), Some(limit));
        }
        result
    });
    assert!(matches!(
        refused,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == REFERENCE_RANGE_SCAN
    ));
}
