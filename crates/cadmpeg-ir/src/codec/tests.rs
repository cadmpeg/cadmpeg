// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use std::collections::BTreeMap;
use std::io::Cursor;

use cadmpeg_core::dialect::{Admission, DialectId, DialectMatch};

use crate::codec::{CadirEncoder, Encoder};
use crate::examples::{directed_subd_sum, unit_cube};
use crate::report::{LossKind, LossNote, LossTaxonomy, TransferLedger};
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
            dialects: Vec::new(),
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

struct RejectFloorCodec;

fn reject_floor_kind() -> LossKind {
    LossKind::shared(LossTaxonomy::TopologyNotTransferred)
}

impl CodecBackend for RejectFloorCodec {
    fn id(&self) -> &'static str {
        "reject-floor"
    }

    fn detect(&self, _prefix: &[u8]) -> Confidence {
        Confidence::No
    }

    fn inspect_impl(
        &self,
        _ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        panic!("the strict gate tests do not inspect")
    }

    fn decode_impl(
        &self,
        ctx: &DecodeContext<'_>,
        _root: View<'_>,
    ) -> Result<DecodeResult, CodecError> {
        let mut result = decode_result(unit_cube());
        let report = result.report_mut();
        // Deliberately lie in both directions; the wrapper owns this field.
        report.container_only = !ctx.container_only();
        report
            .losses
            .push(LossNote::new(reject_floor_kind(), "synthetic reject floor"));
        Ok(result)
    }
}

fn strict_options(container_only: bool) -> DecodeOptions {
    let mut options = DecodeOptions {
        container_only,
        ..DecodeOptions::default()
    };
    options.policy.mode = DecodeMode::Strict;
    options
}

#[test]
fn the_strict_gate_refuses_a_full_decode_on_a_reject_floor_loss() {
    let error = RejectFloorCodec
        .decode(&mut Cursor::new(vec![1u8, 2, 3, 4]), &strict_options(false))
        .unwrap_err();

    match error {
        CodecError::StrictRefusal { loss_code, .. } => {
            assert_eq!(loss_code, reject_floor_kind().to_string());
        }
        other => panic!("expected a strict refusal, got {other:?}"),
    }
}

#[test]
fn a_container_only_strict_decode_keeps_its_losses_and_is_admitted() {
    let result = RejectFloorCodec
        .decode(&mut Cursor::new(vec![1u8, 2, 3, 4]), &strict_options(true))
        .unwrap();

    assert!(result.report().container_only);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| loss.code == reject_floor_kind())
            .count(),
        1
    );
}

/// The primary layer needs no marker field: exactly one entry names the
/// report's own format, and `DecodeResult::new` is the construction path that
/// says so.
#[test]
fn a_decode_result_accepts_dialects_with_one_primary_layer() {
    let mut ir = unit_cube();
    ir.source = None;
    let mut report = DecodeReport {
        dialects: vec![
            dialect_layer("test", "test:only"),
            dialect_layer("acis", "acis:save-format-217"),
        ],
        format: "test".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: BTreeMap::new(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: TransferLedger::default(),
    };
    let result = DecodeResult::new(ir.clone(), report.clone(), SourceFidelity::default());

    assert_eq!(result.report().dialects.len(), 2);

    // An empty list is the staged state and stays admissible.
    report.dialects.clear();
    let staged = DecodeResult::new(ir, report, SourceFidelity::default());
    assert!(staged.report().dialects.is_empty());
}

#[test]
#[should_panic(expected = "must contain exactly one entry naming it")]
fn a_decode_result_refuses_dialects_with_no_primary_layer() {
    let report = DecodeReport {
        dialects: vec![dialect_layer("acis", "acis:save-format-217")],
        format: "test".into(),
        container_only: false,
        geometry_transferred: true,
        coverage: BTreeMap::new(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: TransferLedger::default(),
    };

    DecodeResult::new(unit_cube(), report, SourceFidelity::default());
}

fn dialect_layer(format: &str, id: &'static str) -> DialectMatch {
    DialectMatch {
        format: format.into(),
        dialect: Some(DialectId::pinned(id)),
        declared: BTreeMap::new(),
        admission: Admission::Admitted,
    }
}
