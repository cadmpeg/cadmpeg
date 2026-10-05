// SPDX-License-Identifier: Apache-2.0
//! Historical transition source admission tests.

#![allow(clippy::unwrap_used)]

use crate::history::historical_transition;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};

#[test]
fn historical_transition_source_scans_refuse_work() {
    let state = |state_id, versions: &[(i64, i64)]| AsmDeltaState {
        id: format!("state-{state_id}"),
        parent: "history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: versions
            .iter()
            .map(|&(entity_ref, record_ref)| AsmEntityVersion {
                entity_ref,
                record_ref,
            })
            .collect(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(AsmHistoricalTopology {
            bodies: vec![1],
            ..AsmHistoricalTopology::default()
        }),
        transition: None,
    };
    let refuse = |operation: &str| {
        crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            operation,
            0,
            |ctx| {
                let current = state(11, &[(1, 10), (2, 20)]);
                let previous = state(10, &[(1, 11), (3, 30)]);
                historical_transition(ctx, &current, Some(&previous)).map(|_| ())
            },
        )
    };
    for operation in [
        "scan F3D current transition version keys",
        "scan F3D previous transition version keys",
        "scan F3D inserted transition entities",
        "scan F3D deleted transition entities",
        "scan F3D shared transition entities",
    ] {
        let error = refuse(operation);
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
    for skip in 0..=1 {
        let operation = "collect F3D transition version keys";
        let error = crate::test_support::resource_refusal_at(
            cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
            operation,
            skip,
            |ctx| {
                let current = state(11, &[(1, 10), (2, 20)]);
                let previous = state(10, &[(1, 11), (3, 30)]);
                historical_transition(ctx, &current, Some(&previous)).map(|_| ())
            },
        );
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
        ));
    }
    let operation = "collect F3D historical transitions";
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::MaterializedBytes,
        operation,
        0,
        |ctx| {
            let mut states = vec![state(11, &[(1, 10)])];
            super::super::bind_historical_transitions(ctx, &mut states)
        },
    );
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::ResourceLimit(limit) if limit.operation == operation
    ));
}
