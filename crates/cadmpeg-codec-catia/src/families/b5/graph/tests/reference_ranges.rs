// SPDX-License-Identifier: Apache-2.0
//! Lazy, caller-charged traversal of compact B5 object-reference tokens.

use crate::families::b5::graph::B5RecordBuf;
use cadmpeg_core::decode::ResourceDimension;
use cadmpeg_core::CodecError;

const LOOP_SURFACE_REFERENCE: &str = "catia_b5_topology_loop_surface_reference";

fn loop_record(payload: Vec<u8>) -> B5RecordBuf {
    B5RecordBuf {
        offset: 0,
        family: 0xb5,
        class: 0x62,
        object_id: 40,
        payload,
    }
}

#[test]
fn b5_record_references_stop_at_the_first_non_reference_token() {
    let record = loop_record(vec![0x83, 0x18, 30, 0, 0x18, 31, 0, 0x01]);
    assert_eq!(
        super::super::record_references(&record.record()).collect::<Vec<_>>(),
        vec![30, 31]
    );
}

#[test]
fn b5_loop_surface_reference_charges_each_step_it_takes() {
    let record = loop_record(vec![0x83, 0x18, 30, 0, 0x18, 31, 0, 0x01]);
    let surfaces = crate::test_support::with_service_context(|ctx| {
        super::super::topology_surface_references(ctx, &[record.record()])
    })
    .expect("service budget");
    assert_eq!(surfaces, std::collections::BTreeSet::from([31]));

    let refused = crate::test_support::with_work_refusal(LOOP_SURFACE_REFERENCE, |ctx| {
        super::super::topology_surface_references(ctx, &[record.record()])
    });
    assert!(matches!(
        refused,
        Err(CodecError::ResourceLimit(limit))
            if limit.dimension == ResourceDimension::WorkUnits
                && limit.operation == LOOP_SURFACE_REFERENCE
    ));
}
