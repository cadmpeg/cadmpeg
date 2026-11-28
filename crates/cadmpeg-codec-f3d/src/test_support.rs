// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.f3d` ZIP archives and ASM stream payloads.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

pub(crate) mod align;
pub(crate) mod position;
pub(crate) mod scene;
pub(crate) mod shell_slots;

use std::io::Write;

use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::{report::export::WritePath, CadIr, SourceFidelity};

use crate::F3dCodec;

pub(crate) mod act_links;
pub(crate) mod archive_session;
pub(crate) mod canvas;
pub(crate) mod configuration_table;
pub(crate) mod origin_types;
pub(crate) mod parameter_machinery;
pub(crate) mod procedural_support;
pub(crate) mod reverse_normal;
pub(crate) mod sketch_relations;
pub(crate) mod sketch_text;
pub(crate) mod split_face;
pub(crate) mod verification;

/// Whether `loss` gives up the external-reference table.
pub(crate) fn xref_table_given_up(loss: &cadmpeg_ir::report::loss::LossNote) -> bool {
    cadmpeg_ir::unknown::part::gives_up(&loss.code)
        && loss.message.contains("F3D external-reference table")
}

/// Whether `loss` gives up a T-spline control cage.
pub(crate) fn tspline_cage_given_up(loss: &cadmpeg_ir::report::loss::LossNote) -> bool {
    cadmpeg_ir::unknown::part::gives_up(&loss.code)
        && loss.message.contains("T-spline control cage not decoded")
}

/// Whether `loss` gives up a parameter frame that matches no parameter
/// grammar.
pub(crate) fn parameter_frame_given_up(loss: &cadmpeg_ir::report::loss::LossNote) -> bool {
    cadmpeg_ir::unknown::part::gives_up(&loss.code)
        && loss.message.contains("matches no parameter grammar")
}

/// A face of `shell` with no loop, on a surface id the fixture need not hold.
pub(crate) fn bare_face(id: &str, shell: &str) -> cadmpeg_ir::topology::Face {
    cadmpeg_ir::topology::Face {
        id: cadmpeg_ir::ids::FaceId::mint(id).expect("identity grammar"),
        shell: cadmpeg_ir::ids::ShellId::mint(shell).expect("identity grammar"),
        surface: cadmpeg_ir::ids::SurfaceId::mint("test:model:surface#bare")
            .expect("identity grammar"),
        sense: cadmpeg_ir::topology::Sense::Forward,
        loops: cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
        name: None,
        color: None,
        tolerance: None,
        native_ref: None,
    }
}

pub(crate) fn with_decode_context<T>(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    with_decode_policy(&cadmpeg_core::decode::DecodePolicy::service(), f)
}

pub(crate) fn with_decode_policy<T>(
    policy: &cadmpeg_core::decode::DecodePolicy,
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, policy)
        .expect("test decode context");
    f(&ctx)
}

/// Plans a write that names no target, which inherits the dialect of a
/// same-format source, through the sealed encoder and writes its bytes.
pub(crate) fn plan_inherited_write(
    ir: &CadIr,
    fidelity: &SourceFidelity,
    writer: &mut dyn Write,
) -> Result<WritePath, cadmpeg_ir::codec::write::EncodeError> {
    let plan = F3dCodec.plan(EncodeInput::new(ir, Some(fidelity)))?;
    Ok(plan.write_to(writer)?.write_path().clone())
}

/// Write `ir` under a request that names no target, with `fidelity` or
/// source-less, and
/// assert that the file is written, decodes again, and charges a loss whose
/// code is `code` (`namespace/code`) and whose message holds `fragment`.
pub(crate) fn assert_written_with_loss(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    code: &str,
    fragment: &str,
) -> cadmpeg_ir::report::export::ExportReport {
    let report = written_and_decoded(ir, fidelity);
    assert!(
        report
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == code && loss.message.contains(fragment)),
        "expected a {code} loss naming {fragment:?}: {:#?}",
        report.losses
    );
    report
}

/// Writes `ir` with `fidelity` as a synthesis whose cause states that the
/// patch declined for a reason that contains `fragment`, and decodes the
/// file back.
pub(crate) fn assert_patch_declined(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    fragment: &str,
) -> cadmpeg_ir::report::export::ExportReport {
    let report = written_and_decoded(ir, fidelity);
    let Some(cadmpeg_ir::report::export::SynthesisCause::PatchDeclined { reason }) =
        report.synthesis_cause()
    else {
        panic!(
            "the write states no synthesis cause: {:#?}",
            report.write_path()
        );
    };
    assert!(reason.contains(fragment), "{reason:?} names {fragment:?}");
    report
}

/// The report of writing `ir` with `fidelity`, where the written file
/// decodes back ([`assert_decodes_back`]).
fn written_and_decoded(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
) -> cadmpeg_ir::report::export::ExportReport {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let mut bytes = Vec::new();
    let report = F3dCodec
        .plan(EncodeInput::new(ir, fidelity))
        .and_then(|plan| plan.write_to(&mut bytes))
        .expect("the writer writes what it can encode");
    let decoded = F3dCodec
        .decode(&mut std::io::Cursor::new(&bytes), &DecodeOptions::default())
        .expect("the written file decodes");
    assert_decodes_back(&report, decoded.report());
    report
}

/// The written file's B-rep frames, and a write that charged no
/// `export:output.empty` decodes with no blocking loss. A write that charged
/// `writer.geometry-omitted` for a design with no body decodes with the
/// blocking losses of empty body maps.
pub(crate) fn assert_decodes_back(
    written: &cadmpeg_ir::report::export::ExportReport,
    decoded: &cadmpeg_ir::report::decode::DecodeReport,
) {
    let charged = |code: &str| {
        written
            .losses
            .iter()
            .any(|loss| loss.code.to_string() == code)
    };
    let empty = charged("export/output.empty");
    let body_less = charged(
        &crate::loss::F3dLossCode::WriterGeometryOmitted
            .kind()
            .to_string(),
    );
    for loss in &decoded.losses {
        assert!(
            !loss.message.contains("not a decodable"),
            "the written B-rep frames: {}",
            loss.message
        );
        let empty_maps = body_less
            && (loss
                .message
                .starts_with("No physical model BREP was selected")
                || loss
                    .message
                    .starts_with("No physical B-rep topology was selected"));
        assert!(
            empty || empty_maps || loss.severity < cadmpeg_ir::report::Severity::Blocking,
            "the written file decodes without a blocking loss: {:#?}",
            decoded.losses
        );
    }
}

/// A finite test value as the checked scalar a record holds.
pub(crate) fn real(value: f64) -> cadmpeg_ir::scalar::FiniteReal {
    cadmpeg_ir::scalar::FiniteReal::new(value).expect("a finite test value")
}

/// Finite test values as the checked lane a record holds.
pub(crate) fn reals<const N: usize>(values: [f64; N]) -> [cadmpeg_ir::scalar::FiniteReal; N] {
    values.map(real)
}

/// Write a length-prefixed ASCII string: a little-endian `u32` byte count then
/// the bytes.
pub(crate) fn lp_ascii(out: &mut Vec<u8>, value: &str) {
    out.extend_from_slice(
        &u32::try_from(value.len())
            .expect("test ASCII string length")
            .to_le_bytes(),
    );
    out.extend_from_slice(value.as_bytes());
}

/// Write a length-prefixed UTF-16 string: a little-endian `u32` code-unit count
/// then the little-endian units.
pub(crate) fn lp_utf16(out: &mut Vec<u8>, value: &str) {
    let units = value.encode_utf16().collect::<Vec<_>>();
    out.extend_from_slice(
        &u32::try_from(units.len())
            .expect("test UTF-16 unit count")
            .to_le_bytes(),
    );
    for unit in units {
        out.extend_from_slice(&unit.to_le_bytes());
    }
}

/// Write a same-document reference to a `u64` target: the presence byte, the
/// target, then the two zero bytes that close the reference.
pub(crate) fn push_reference_u64(out: &mut Vec<u8>, target: u64) {
    out.push(1);
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&[0, 0]);
}

/// A same-document reference record to `target`.
pub(crate) fn local_reference(target: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    push_reference_u64(&mut bytes, target);
    bytes
}

/// One legacy target reference to `target` with source occurrence identity
/// `identity`.
pub(crate) fn legacy_occurrence_reference(target: u64, identity: u64) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&identity.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&36_u32.to_le_bytes());
    bytes.extend_from_slice(b"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");
    bytes.push(0);
    bytes
}

/// One legacy typed placement envelope.
pub(crate) fn legacy_occurrence_record(
    role: &str,
    entity_id: u64,
    transform: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&entity_id.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(legacy_occurrence_reference(3, 0x0102_0304_0506_0708));
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes("11111111-2222-3333-4444-555555555555")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend(
        crate::bytes::lp_utf16_bytes("66666666-7777-8888-9999-aaaaaaaaaaaa")
            .expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.push(0);
    bytes.extend(legacy_occurrence_reference(3, 0x0102_0304_0506_0708));
    match transform {
        Some(transform) => {
            bytes.push(0);
            for value in transform.into_iter().flatten() {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        None => bytes.push(1),
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes.extend(
        crate::bytes::lp_utf16_bytes(role).expect("fixture UTF-16 code-unit count fits u32"),
    );
    bytes.extend_from_slice(&[0, 1, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

pub(crate) fn legacy_attributed_external_placement(
    role: &str,
    attributes: bool,
    matrix: Option<[[f64; 4]; 4]>,
) -> Vec<u8> {
    let target = || {
        let mut reference = vec![1];
        reference.extend_from_slice(&3_u64.to_le_bytes());
        reference.push(1);
        reference.extend_from_slice(&0x0102_0304_0506_0708_u64.to_le_bytes());
        reference.push(0);
        lp_ascii(&mut reference, "E03784ED-5E19-4E14-B9F2-3B07017018CD");
        reference.push(0);
        reference
    };
    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "380");
    bytes.extend_from_slice(&10_u64.to_le_bytes());
    lp_ascii(&mut bytes, "");
    bytes.push(1);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend(target());
    bytes.extend_from_slice(&9_u32.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&(if attributes { 3_u32 } else { 0_u32 }).to_le_bytes());
    if attributes {
        lp_ascii(&mut bytes, "GatedByParent");
        lp_ascii(&mut bytes, "IntrinsicMetaTypebool");
        bytes.push(0);
        lp_ascii(&mut bytes, "ResolutionTime");
        lp_ascii(&mut bytes, "IntrinsicMetaTypeHString");
        lp_utf16(&mut bytes, "");
        lp_ascii(&mut bytes, "SelfGated");
        lp_ascii(&mut bytes, "IntrinsicMetaTypebool");
        bytes.push(0);
    }
    bytes.extend_from_slice(&0_u16.to_le_bytes());
    bytes.extend_from_slice(&2_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    lp_utf16(&mut bytes, "66666666-7777-8888-9999-aaaaaaaaaaaa");
    bytes.push(1);
    bytes.extend_from_slice(&71_u64.to_le_bytes());
    bytes.extend_from_slice(&[0, 1]);
    bytes.extend_from_slice(&5_u32.to_le_bytes());
    bytes.push(0);
    bytes.extend(target());
    bytes.push(u8::from(matrix.is_none()));
    if let Some(matrix) = matrix {
        for value in matrix.into_iter().flatten() {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bytes, role);
    bytes.push(0);
    bytes.extend(local_reference(7));
    bytes
}

/// A cross-document reference record: the presence byte, the target, the
/// cross-document marker, the source type GUID, the link type GUID, and the
/// link name.
pub(crate) fn cross_document_reference(target: u64, link_name: &str) -> Vec<u8> {
    typed_cross_document_reference(target, "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee", link_name)
}

/// A cross-document reference whose target has type `type_guid`.
pub(crate) fn typed_cross_document_reference(
    target: u64,
    type_guid: &str,
    link_name: &str,
) -> Vec<u8> {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.push(1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    lp_utf16(&mut bytes, "11111111-2222-3333-4444-555555555555");
    bytes.push(0);
    lp_ascii(&mut bytes, type_guid);
    lp_utf16(&mut bytes, link_name);
    bytes.push(0);
    bytes
}

/// Append an indexed record header: the three-byte class tag behind its
/// little-endian `u32` length, then the little-endian record index.
pub(crate) fn indexed_header(bytes: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&class_tag);
    bytes.extend_from_slice(&record_index.to_le_bytes());
}

/// Write an indexed record header over the first eleven bytes of `bytes`.
fn write_indexed_header(bytes: &mut [u8], class_tag: [u8; 3], record_index: u32) {
    bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
    bytes[4..7].copy_from_slice(&class_tag);
    bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
}

/// Write a present marked reference to `record_index` at `at`: the presence
/// byte then the little-endian record index.
pub(crate) fn write_marked_reference(bytes: &mut [u8], at: usize, record_index: u32) {
    bytes[at] = 1;
    bytes[at + 1..at + 5].copy_from_slice(&record_index.to_le_bytes());
}

/// Append a present marked reference to `record_index`: the presence byte, the
/// little-endian record index, then the six zero bytes that close the mark.
pub(crate) fn push_marked_reference(bytes: &mut Vec<u8>, record_index: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&record_index.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
}

pub(crate) mod tokens_fixture;

pub(crate) mod smbh_header_fixture;

pub(crate) mod smbh_blocks_fixture;

pub(crate) mod sketch_region_fixture;
pub(crate) mod smbh_geometry_fixture;

pub(crate) mod smbh_pcurves_fixture;

pub(crate) mod smbh_curves_fixture;

pub(crate) mod smbh_surfaces_fixture;

pub(crate) mod smbh_revision_fixture;

pub(crate) mod smbh_blends_fixture;

pub(crate) mod smbh_bf4_fixture;

pub(crate) mod native_fixture;

pub(crate) mod manifest_fixture;

pub(crate) mod protein_fixture;

pub(crate) mod empty_attribute_occurrences;
pub(crate) mod streams_fixture;

pub(crate) mod zip_fixture;

pub(crate) mod assembly_fixture;

pub(crate) mod procedural_fixture;

pub(crate) mod history;

pub(crate) mod history_stream;

pub(crate) mod body_map_fixture;

pub(crate) mod component_insert_fixture;

pub(crate) mod lost_edge_fixture;

pub(crate) mod step_conversion;

pub(crate) mod body_reference_tail_fixture;

pub(crate) mod placement_construction;

pub(crate) mod pipe;

pub(crate) mod work_plane;

pub(crate) mod component_body_fixture;
pub(crate) mod component_design;

pub(crate) mod scan_counts;

pub(crate) mod multiple_profiles;

/// The loss codes of `losses`, in order.
pub(crate) fn loss_codes(
    losses: &[cadmpeg_ir::report::loss::LossNote],
) -> Vec<cadmpeg_ir::report::loss::LossKind> {
    losses.iter().map(|loss| loss.code.clone()).collect()
}

pub(crate) mod move_face;

pub(crate) mod typed_reference_scopes;

/// The one give-up loss for `cause` of `losses` whose message starts with
/// `head`, after checking that it names a stub of the F3D `unknowns` arena of
/// `ir` and that its dropped list is `dropped`.
pub(crate) fn charged_part<'a>(
    ir: &CadIr,
    losses: &'a [cadmpeg_ir::report::loss::LossNote],
    (cause, head): (cadmpeg_ir::unknown::part::Cause, &str),
    dropped: &str,
) -> &'a cadmpeg_ir::report::loss::LossNote {
    let charges = losses
        .iter()
        .filter(|loss| loss.code == cause.kind() && loss.message.starts_with(head))
        .collect::<Vec<_>>();
    assert_eq!(charges.len(), 1, "{head}: {losses:#?}");
    let charge = charges[0];
    assert!(
        charge.message.contains(&format!("; dropped=[{dropped}]")),
        "{}",
        charge.message
    );
    let stubs = ir.native_unknowns("f3d").expect("the F3D unknowns arena");
    assert!(
        stubs
            .iter()
            .any(|stub| charge.message.contains(&format!("record={}:", stub.id))),
        "the charge names no stub: {}",
        charge.message
    );
    charge
}

/// Asserts that a pinned decode of `bytes` refuses through a
/// `record.unreadable` loss whose message starts with `head`.
pub(crate) fn assert_pinned_refusal(bytes: Vec<u8>, head: &str) {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    let failure = F3dCodec
        .decode(&mut std::io::Cursor::new(bytes), &options)
        .expect_err("a pinned decode refuses the given-up record");
    let cadmpeg_ir::DecodeFailure::StrictRejected { rejection } = &failure else {
        panic!("expected a strict rejection, got {failure:?}");
    };
    assert_eq!(
        rejection.loss().code,
        cadmpeg_ir::unknown::part::record_unreadable_kind()
    );
    assert!(
        rejection.loss().message.starts_with(head),
        "{}",
        rejection.loss().message
    );
}

/// A pinned decode of `bytes` accepts its `record.undecoded` give-ups.
pub(crate) fn assert_pinned_acceptance(bytes: Vec<u8>) {
    use cadmpeg_ir::codec::{Codec, DecodeOptions};
    let mut options = DecodeOptions::default();
    options.strictness.pinned = true;
    F3dCodec
        .decode(&mut std::io::Cursor::new(bytes), &options)
        .expect("a pinned decode accepts the undecoded give-ups");
}
