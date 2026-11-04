// SPDX-License-Identifier: Apache-2.0
//! Writer harness wiring for IGES (#234 item, #217 §5.0, #232 §3.3 and §5.2).
//!
//! Injected-error and `max_entities` runners from `cadmpeg-test-support`;
//! the capacity rows with their helpers; the residual checker (no residual
//! positions are stated in `docs/formats/iges.md`, so the spec list is empty);
//! and the `REDUCED_FIELDS` row runner (IGES stores nothing in reduced form).

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::reduced::reduced_row_findings;
use cadmpeg_test_support::residual::{residual_findings, ResidualSpec};
use cadmpeg_test_support::robustness::{
    injected_error_findings, max_entities_findings, standard_injections,
};

use std::io::Cursor;

use crate::IgesCodec;

fn seed_golden() -> cadmpeg_ir::CadIr {
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "igs",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-iges golden",
    );
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "procedural_and_boolean_solids")
        .expect("commit the robustness seed fixture");
    IgesCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
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
    // The recompute-flag injection flips `needs_recompute` to true, which is
    // legal stored state (#209 §2.5), so it never invalidates the seed and is
    // inapplicable here; the remaining standard injections run.
    let injections: Vec<_> = standard_injections()
        .into_iter()
        .filter(|injection| injection.name != "recompute-flag")
        .collect();
    let findings = injected_error_findings(&seed, &IgesCodec, request(), &injections);
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn capacity_helpers_follow_their_rows() {
    assert_eq!(super::super::CAPACITY_FIELDS.len(), 2);
    assert_eq!(super::super::CAPACITY_FIELDS[0].field, "card.payload");
    assert_eq!(super::super::CAPACITY_FIELDS[0].limit, 72);
    assert_eq!(super::super::CAPACITY_FIELDS[1].field, "card.sequence");
    assert_eq!(super::super::CAPACITY_FIELDS[1].limit, 9_999_999);
    let payload = vec![b' '; super::super::CAPACITY_FIELDS[0].limit as usize];
    assert!(super::super::card_payload(&payload).is_ok());
    let overlong = vec![b' '; super::super::CAPACITY_FIELDS[0].limit as usize + 1];
    assert!(super::super::card_payload(&overlong).is_err());
    let limit = super::super::CAPACITY_FIELDS[1].limit;
    let sequence = u32::try_from(limit).expect("seven-digit limit fits u32");
    assert!(super::super::card_sequence(sequence).is_ok());
    assert!(super::super::card_sequence(sequence + 1).is_err());
}

#[test]
fn max_entities_refuses_an_over_ceiling_plan() {
    let seed = seed_golden();
    let count = u64::try_from(seed.model.entity_count()).expect("entity count fits");
    let findings = max_entities_findings(&seed, &IgesCodec, request(), count);
    assert!(
        findings.is_empty(),
        "max_entities findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn reduced_rows_are_empty_for_iges() {
    // IGES stores no JSON path in reduced form; every written value
    // round-trips at full precision (#232 §5.2).
    assert!(super::super::REDUCED_FIELDS.is_empty());
    let seed = seed_golden();
    let encoder = &IgesCodec as &dyn Encoder;
    let redecode = |written: &[u8]| {
        let result = IgesCodec
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
    for row in super::super::REDUCED_FIELDS {
        let probe = cadmpeg_test_support::cross_closure::ReducedProbe {
            path: row.path,
            storage: row.storage,
            normalize: None,
        };
        findings.extend(reduced_row_findings(
            &seed,
            encoder,
            request(),
            &redecode,
            &probe,
            None,
        ));
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
    let plan = IgesCodec
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan the residual seed");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write the residual seed");
    let decoded = IgesCodec
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
}

/// Residual positions the IGES writer synthesizes, from `docs/formats/`.
///
/// No residual rule is stated in `docs/formats/iges.md`; the first stated
/// rule adds its spec here (#232 §3.3).
fn residual_specs(seed: &cadmpeg_ir::CadIr) -> Vec<ResidualSpec> {
    let _ = seed;
    Vec::new()
}
