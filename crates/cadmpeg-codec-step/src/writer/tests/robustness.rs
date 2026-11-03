// SPDX-License-Identifier: Apache-2.0
//! Writer harness wiring for STEP (#233 item 3, #217 §5.0, #232 §3.3 and §5.2).
//!
//! Injected-error, capacity-helper and `max_entities` runners from
//! `cadmpeg-test-support`; the residual checker with the STEP residual
//! positions; and the `REDUCED_FIELDS` row runner (STEP stores nothing in
//! reduced form, so the row list is empty and the runner is vacuous).

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::reduced::reduced_row_findings;
use cadmpeg_test_support::residual::{residual_findings, ResidualSpec};
use cadmpeg_test_support::robustness::{
    capacity_findings, injected_error_findings, max_entities_findings, standard_injections,
};

use std::io::Cursor;

use crate::StepCodec;

fn seed_golden() -> cadmpeg_ir::CadIr {
    let bytes = include_bytes!("../../../tests/fixtures/ap214_sheet.p21");
    StepCodec::default()
        .decode(&mut Cursor::new(bytes.as_slice()), &DecodeOptions::default())
        .expect("decode the robustness seed")
        .ir()
        .clone()
}

fn request() -> TargetRequest<'static> {
    TargetRequest::Inherit
}

#[test]
fn injected_errors_succeed_and_name_the_record() {
    let seed = seed_golden();
    let findings = injected_error_findings(
        &seed,
        &StepCodec::default(),
        request(),
        &standard_injections(),
    );
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn capacity_helpers_follow_their_rows() {
    // STEP has no fixed-width field; every count chains or splits records, so
    // there is no capacity row and no helper to probe (#233).
    assert!(crate::writer::CAPACITY_FIELDS.is_empty());
    let findings = capacity_findings(&[]);
    assert!(
        findings.is_empty(),
        "capacity findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn max_entities_refuses_an_over_ceiling_plan() {
    let seed = seed_golden();
    let count = u64::try_from(seed.model.entity_count()).expect("entity count fits");
    let findings = max_entities_findings(&seed, &StepCodec::default(), request(), count);
    assert!(
        findings.is_empty(),
        "max_entities findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn reduced_rows_are_empty_for_step() {
    // STEP stores no JSON path in reduced form; every written value
    // round-trips at full precision (#232 §5.2).
    assert!(crate::writer::REDUCED_FIELDS.is_empty());
    let seed = seed_golden();
    let encoder = &StepCodec::default() as &dyn Encoder;
    let redecode = |written: &[u8]| {
        let result = StepCodec::default()
            .decode(&mut Cursor::new(written), &DecodeOptions::default())
            .map_err(|error| error.to_string())?;
        Ok(cadmpeg_test_support::writer_closure::Redecoded {
            members: Vec::new(),
            ir: serde_json::to_value(result.ir()).map_err(|error| error.to_string())?,
            loss_codes: result
                .report()
                .losses
                .iter()
                .map(|loss| loss.code.to_string())
                .collect(),
        })
    };
    let mut findings = Vec::new();
    for row in crate::writer::REDUCED_FIELDS {
        let probe = cadmpeg_test_support::cross_closure::ReducedProbe {
            path: row.path,
            storage: row.storage,
            normalize: None,
        };
        findings.extend(reduced_row_findings(&seed, encoder, request(), &redecode, &probe, None));
    }
    assert!(
        findings.is_empty(),
        "reduced findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn synthesized_records_carry_their_specified_residuals() {
    let seed = seed_golden();
    let plan = StepCodec::default()
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan the residual seed");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write the residual seed");
    let decoded = StepCodec::default()
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode the residual output");
    let after = serde_json::to_value(decoded.ir()).expect("serialize the re-decode");
    let specs = residual_specs(&seed);
    let findings = residual_findings(&after, &report.losses, &specs);
    assert!(
        findings.is_empty(),
        "residual findings:\n{}",
        findings.join("\n")
    );
    assert!(
        !report.losses.iter().any(|loss| loss
            .code
            .local_code()
            == "record.residual-unavailable"),
        "the writer charges no residual-unavailable loss on this seed: {:?}",
        report.losses
    );
}

/// Residual positions the STEP writer synthesizes, from `docs/formats/`.
fn residual_specs(seed: &cadmpeg_ir::CadIr) -> Vec<ResidualSpec> {
    let _ = seed;
    Vec::new()
}
