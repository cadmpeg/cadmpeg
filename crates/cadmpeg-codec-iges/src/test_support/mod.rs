// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic IGES byte-fixture builders for crate tests.
#![allow(clippy::unwrap_used)]

pub(crate) mod test_cards;
pub(crate) mod test_curves_and_surfaces;
pub(crate) mod test_drawing_and_trimming;
pub(crate) mod test_owned;
pub(crate) mod test_procedural_surfaces;
pub(crate) mod test_solids_and_structure;
pub(crate) mod test_surface_fixtures;
pub(crate) mod test_tabulated_surfaces;

/// Parses a scanned Global section with a test-owned decode session.
pub(crate) fn parse_global(
    scan: &crate::card::CardScan<'_>,
) -> Result<
    (
        crate::global::ResolvedGlobal,
        Vec<cadmpeg_ir::report::loss::LossNote>,
    ),
    cadmpeg_core::CodecError,
> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)?;
    crate::global::parse(scan, &ctx)
}

/// Plans a write at one Fixed ASCII target, the request the command line
/// builds for an explicit `--to`.
///
/// The tests here assert what the writer produces at a version, not how the
/// request that names it is spelled, so the spelling lives in one place.
pub(crate) fn plan_at(
    version: crate::IgesVersion,
    ir: &cadmpeg_ir::CadIr,
    fidelity: Option<&cadmpeg_ir::SourceFidelity>,
) -> Result<cadmpeg_ir::codec::write::ExportPlan, cadmpeg_core::CodecError> {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};

    crate::IgesCodec.plan(
        EncodeInput { ir, fidelity },
        TargetRequest::Explicit(version.descriptor().id.as_str()),
    )
}

/// Decode options in strict mode.
pub(crate) fn strict_options() -> cadmpeg_ir::codec::DecodeOptions {
    let mut options = cadmpeg_ir::codec::DecodeOptions::default();
    options.policy.mode = cadmpeg_core::decode::DecodeMode::Strict;
    options
}

/// Decodes a synthesized stream with default options.
pub(crate) fn decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::codec::Codec as _;

    crate::IgesCodec
        .decode(
            &mut std::io::Cursor::new(bytes),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .unwrap()
}

/// Decodes a synthesized stream that must also detect at high confidence.
pub(crate) fn detect_and_decode(bytes: Vec<u8>) -> cadmpeg_ir::codec::DecodeResult {
    use cadmpeg_ir::codec::Codec as _;

    assert_eq!(
        crate::IgesCodec.detect(&bytes),
        cadmpeg_ir::codec::Confidence::High
    );
    crate::IgesCodec
        .decode(
            &mut std::io::Cursor::new(bytes),
            &cadmpeg_ir::codec::DecodeOptions::default(),
        )
        .expect("synthesized IGES stream should decode")
}

/// Count of report losses carrying one loss code.
pub(crate) fn code_count(
    report: &cadmpeg_ir::report::decode::DecodeReport,
    code: crate::loss::IgesLossCode,
) -> usize {
    report
        .losses
        .iter()
        .filter(|loss| loss.code == code.kind())
        .count()
}

/// A Directory Entry whose fields are zero apart from the two named here.
pub(crate) fn directory_target(
    sequence: u32,
    entity_type: i64,
) -> crate::directory::DirectoryEntry {
    crate::directory::DirectoryEntry {
        source_offset: 0,
        sequence,
        entity_type,
        parameter_start: 1,
        structure: 0,
        line_font: 0,
        level: 0,
        view: 0,
        transform: 0,
        label_display: 0,
        status: crate::directory::SourceStatus::from_codes([0, 0, 0, 0]),
        line_weight: 0,
        color: 0,
        parameter_line_count: 1,
        form: 0,
        reserved: [[b' '; 8]; 2],
        label: [b' '; 8],
        subscript: 0,
    }
}

/// A 26-field Global record carrying `version_flag` in field 23.
///
/// Field 23 is the version flag of IGES 5.3 Table 1. An empty string omits the
/// field, which is the specification's own default case.
pub(crate) fn global_with_version_flag(version_flag: &str) -> Vec<u8> {
    format!(
        "1H,,1H;,7Hproduct,8Hpart.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,\
         2HMM,1,1.0,15H20260714.000000,0.001,1000.0,6Hauthor,3Horg,{version_flag},0,0H,0H;"
    )
    .into_bytes()
}

/// The one dialect match of a report. The primary-layer invariant makes it the
/// primary layer, so no consumer here indexes by position for any other reason.
pub(crate) fn only_match(
    dialects: Option<&cadmpeg_core::dialect::DialectLayers>,
) -> &cadmpeg_core::dialect::DialectMatch {
    let layers = dialects.expect("IGES reports dialect layers");
    assert_eq!(layers.iter().count(), 1, "{dialects:#?}");
    assert_eq!(layers.primary().format(), "iges");
    layers.primary()
}
