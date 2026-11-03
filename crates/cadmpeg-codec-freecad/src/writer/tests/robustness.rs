// SPDX-License-Identifier: Apache-2.0
//! Writer harness wiring for FreeCAD (#238 item 1, #217 §5.0, #232 §3.3 and §5.2).
//!
//! Injected-error, capacity-helper and `max_entities` runners from
//! `cadmpeg-test-support`; the residual checker (no residual positions are
//! stated in `docs/formats/freecad_fcstd.md` yet, so the spec list is empty
//! and the runner asserts no `residual-unavailable` loss); and the
//! `REDUCED_FIELDS` row runner (empty until the §5.5 normalization lines
//! land, each with its loss).

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::reduced::reduced_row_findings;
use cadmpeg_test_support::residual::{residual_findings, ResidualSpec};
use cadmpeg_test_support::robustness::{
    capacity_findings, injected_error_findings, max_entities_findings, standard_injections,
};

use std::io::Cursor;

use crate::test_support::test_archive::CORE_DESIGN_PRODUCT;
use crate::FcstdCodec;

fn seed_golden() -> cadmpeg_ir::CadIr {
    FcstdCodec
        .decode(
            &mut Cursor::new(CORE_DESIGN_PRODUCT),
            &DecodeOptions::default(),
        )
        .expect("decode the robustness seed")
        .ir()
        .clone()
}

fn request() -> TargetRequest<'static> {
    TargetRequest::Explicit("fcstd:schema-4")
}

#[test]
fn injected_errors_succeed_and_name_the_record() {
    let seed = seed_golden();
    // The standard recompute-flag injection flips `needs_recompute` to
    // true, which is legal stored state (#209 §2.5: staleness suppresses
    // cache agreement instead of failing it), so it never invalidates any
    // seed and is inapplicable here; the remaining standard injections run.
    let injections: Vec<_> = standard_injections()
        .into_iter()
        .filter(|injection| injection.name != "recompute-flag")
        .collect();
    let findings = injected_error_findings(&seed, &FcstdCodec, request(), &injections);
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn capacity_helpers_follow_their_rows() {
    // FreeCAD has no fixed-width field; every count is an XML attribute or a
    // ZIP member, so there is no capacity row and no helper to probe (#238).
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
    let findings = max_entities_findings(&seed, &FcstdCodec, request(), count);
    assert!(
        findings.is_empty(),
        "max_entities findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn reduced_rows_are_empty_until_normalizations_land() {
    // Each §5.5 normalization row lands here with its loss as its writer
    // line lands (#238, #232 §5.2); the runner below then probes each row.
    assert!(crate::writer::REDUCED_FIELDS.is_empty());
    let seed = seed_golden();
    let encoder = &FcstdCodec as &dyn Encoder;
    let redecode = |written: &[u8]| {
        let result = FcstdCodec
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
    let plan = FcstdCodec
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan the residual seed");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write the residual seed");
    let decoded = FcstdCodec
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
        !report
            .losses
            .iter()
            .any(|loss| loss.code.local_code() == "record.residual-unavailable"),
        "the writer charges no residual-unavailable loss on this seed: {:?}",
        report.losses
    );
}

/// Residual positions the FreeCAD writer synthesizes, from `docs/formats/`.
///
/// No residual rule is stated in `docs/formats/freecad_fcstd.md` yet; the
/// first stated rule adds its spec here (#232 §3.3).
fn residual_specs(seed: &cadmpeg_ir::CadIr) -> Vec<ResidualSpec> {
    let _ = seed;
    Vec::new()
}

#[test]
fn admission_is_silent_on_clean_fixture_decodes() {
    // The admission scan names only dropped records (#232 §5.1(a)). A clean
    // decode drops nothing, so every public fixture charges nothing here;
    // any finding names a false positive in the scan itself.
    let dir = format!(
        "{}/../../corpus/freecad_fcstd/fixtures",
        env!("CARGO_MANIFEST_DIR")
    );
    let mut names: Vec<_> = std::fs::read_dir(&dir)
        .expect("fixtures dir")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "FCStd"))
        .collect();
    names.sort();
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let ctx = cadmpeg_core::decode::DecodeContext::for_encode(&arena, &policy);
    let mut findings = Vec::new();
    for path in &names {
        let bytes = std::fs::read(path).expect("fixture bytes");
        let Ok(decoded) = FcstdCodec.decode(&mut Cursor::new(&bytes), &DecodeOptions::default())
        else {
            continue;
        };
        for loss in
            super::super::synthesis_graph::admission(decoded.ir(), &ctx).expect("admission runs")
        {
            findings.push(format!(
                "{}: {} :: {}",
                path.file_name().expect("name").to_string_lossy(),
                loss.code,
                loss.message
            ));
        }
    }
    assert!(
        findings.is_empty(),
        "admission findings on clean decodes:\n{}",
        findings.join("\n")
    );
}
