// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;

use crate::codec::{CadirEncoder, Encoder};
use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::TransferLedger;
use crate::source_fidelity::RetainedSourceRecord;
use crate::validate::validate_neutral;
use crate::CadIr;

use super::*;

#[test]
fn cadir_encoder_streams_the_canonical_json_shape() {
    let ir = unit_cube();
    let mut encoded = Vec::new();
    CadirEncoder
        .plan(crate::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let mut canonical = ir.to_canonical_json().unwrap();
    canonical.push('\n');
    assert_eq!(encoded, canonical.as_bytes());
}

#[test]
fn cadir_encoder_census_matches_validation_counts() {
    let ir = directed_subd_sum();
    let validation_counts = validate_neutral(&ir, Vec::new()).entity_counts;
    let plan = CadirEncoder
        .plan(crate::codec::EncodeInput {
            ir: &ir,
            fidelity: None,
        })
        .expect("plan CADIR export");

    assert_eq!(plan.report().census.counts, validation_counts);
}

fn decode_result(ir: CadIr) -> DecodeResult {
    DecodeResult::new(
        ir,
        DecodeReport {
            format: "test".into(),
            container_only: false,
            geometry_transferred: true,
            coverage: BTreeMap::new(),
            losses: Vec::new(),
            notes: Vec::new(),
            transfer_ledger: TransferLedger::default(),
        },
        SourceFidelity::default(),
    )
}

fn retained_record(id: &str, offset: u64) -> RetainedSourceRecord {
    RetainedSourceRecord {
        id: id.into(),
        stream: "test".into(),
        offset,
        byte_len: 0,
        sha256: String::new(),
        data: None,
    }
}

#[test]
fn decode_result_edit_guards_restore_finalization() {
    let mut result = decode_result(unit_cube());
    {
        let mut ir = result.ir_mut();
        ir.model.points.reverse();
    }
    assert!(result
        .ir()
        .model
        .points
        .windows(2)
        .all(|pair| pair[0].id < pair[1].id));

    {
        let mut fidelity = result.source_fidelity_mut();
        fidelity
            .retained_records
            .extend([retained_record("b", 2), retained_record("a", 1)]);
    }
    assert_eq!(
        result
            .source_fidelity()
            .retained_records
            .iter()
            .map(|record| record.id.as_str())
            .collect::<Vec<_>>(),
        ["a", "b"]
    );
}

#[test]
fn decode_result_edit_guard_finalizes_during_unwind() {
    let mut result = decode_result(unit_cube());
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut ir = result.ir_mut();
        ir.model.points.reverse();
        panic!("abort edit");
    }));
    assert!(unwind.is_err());
    assert!(result
        .ir()
        .model
        .points
        .windows(2)
        .all(|pair| pair[0].id < pair[1].id));
}
