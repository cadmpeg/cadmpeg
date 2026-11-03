// SPDX-License-Identifier: Apache-2.0
//! Writer harness wiring for SolidWorks (#237 item, #217 §5.0, #232 §3.3 and §5.2).
//!
//! Injected-error, capacity-helper and `max_entities` runners from
//! `cadmpeg-test-support`; the residual checker over the §3.3 statement list;
//! and the `REDUCED_FIELDS` row runner.

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_test_support::residual::{residual_findings, ResidualSpec};
use cadmpeg_test_support::robustness::{
    injected_error_findings, max_entities_findings, standard_injections,
};

use std::io::Cursor;

use crate::SldprtCodec;

fn seed_golden() -> cadmpeg_ir::CadIr {
    let harness = cadmpeg_test_support::golden::Harness::new(
        env!("CARGO_MANIFEST_DIR"),
        "sldprt",
        "UPDATE_GOLDEN=1 cargo test -p cadmpeg-codec-sldprt golden",
    );
    let (_, bytes) = harness
        .fixture_inputs()
        .into_iter()
        .find(|(name, _)| name == "authored_complete_cylinder")
        .expect("commit the robustness seed fixture");
    SldprtCodec
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
    let findings = injected_error_findings(&seed, &SldprtCodec, request(), &injections);
    assert!(
        findings.is_empty(),
        "injected-error findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn capacity_helpers_follow_their_rows() {
    assert_eq!(crate::writer::CAPACITY_FIELDS.len(), 2);
    let name = "n".repeat(crate::writer::CAPACITY_FIELDS[0].limit as usize);
    assert!(crate::writer::material_name_len(&name).is_ok());
    let overlong = "n".repeat(crate::writer::CAPACITY_FIELDS[0].limit as usize + 1);
    assert!(crate::writer::material_name_len(&overlong).is_err());
    let limit = crate::writer::CAPACITY_FIELDS[1].limit;
    let index = usize::try_from(limit).expect("type-id limit fits usize");
    assert!(crate::writer::section_type_id_at(index).is_ok());
    assert!(crate::writer::section_type_id_at(index + 1).is_err());
}

#[test]
fn max_entities_refuses_an_over_ceiling_plan() {
    let seed = seed_golden();
    let count = u64::try_from(seed.model.entity_count()).expect("entity count fits");
    let findings = max_entities_findings(&seed, &SldprtCodec, request(), count);
    assert!(
        findings.is_empty(),
        "max_entities findings:\n{}",
        findings.join("\n")
    );
}

#[test]
fn reduced_rows_hold_their_storage() {
    // The declared rows feed the cross-closure comparison, which applies
    // them positionally. The probe harness addresses records by identity,
    // which synthesis regenerates, so the storage itself is pinned here:
    // colours quantize to 8-bit channels and tessellation scalars to f32.
    let rows = crate::writer::REDUCED_FIELDS;
    assert_eq!(
        rows.iter().map(|row| row.path).collect::<Vec<_>>(),
        [
            "model.tessellations[].vertices[]",
            "model.tessellations[].vertex_normals[]",
            "model.appearances[].base_color",
        ]
    );
    assert_eq!(
        rows.iter().map(|row| row.storage).collect::<Vec<_>>(),
        [
            cadmpeg_ir::codec::write::ReducedStorage::F32,
            cadmpeg_ir::codec::write::ReducedStorage::F32,
            cadmpeg_ir::codec::write::ReducedStorage::U8Channel,
        ]
    );
    let payload = crate::writer::material_payload(
        "probe",
        cadmpeg_ir::topology::Color::new(0.25, 0.5, 0.75, 1.0).expect("probe colour admits"),
    )
    .expect("probe material encodes");
    let tag = b"moVisualProperties_c";
    let at = payload
        .windows(tag.len())
        .position(|window| window == tag)
        .expect("material tag opens the payload");
    assert_eq!(
        payload[at + tag.len()..at + tag.len() + 4],
        [0x40, 0x80, 0xBF, 0x00],
        "8-bit channels quantize 0.25/0.5/0.75 to 64/128/191"
    );
    assert_eq!(
        crate::writer::tessellation_f32(0.1, "probe").expect("f32 admits 0.1"),
        0.1f32,
        "tessellation scalars store as f32"
    );
}

#[test]
fn synthesized_records_carry_their_specified_residuals() {
    let seed = seed_golden();
    let plan = SldprtCodec
        .plan(EncodeInput::new(&seed, None), request())
        .expect("plan the residual seed");
    let mut bytes = Vec::new();
    let report = plan.write_to(&mut bytes).expect("write the residual seed");
    let decoded = SldprtCodec
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

/// Residual positions the SolidWorks writer synthesizes, from `docs/formats/`.
///
/// No residual rule is stated in `docs/formats/sldprt.md` yet; the first
/// stated rule adds its spec here (#232 §3.3).
fn residual_specs(seed: &cadmpeg_ir::CadIr) -> Vec<ResidualSpec> {
    let _ = seed;
    Vec::new()
}
