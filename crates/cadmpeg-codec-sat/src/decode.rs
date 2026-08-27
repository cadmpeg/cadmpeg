// SPDX-License-Identifier: Apache-2.0
//! Text and binary decode paths for bare ASM streams.

use cadmpeg_asm::acis_header;
use cadmpeg_asm::asm_header;
use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::ids::IdFormat;
use cadmpeg_asm::kernel_header::KernelHeader;
use cadmpeg_asm::{sab, sat};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::{debug_assert_primary_layer, DialectMatch};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::units::{Tolerances, Units};
use std::collections::BTreeMap;

use crate::detect::{classify, header_attributes, StreamKind};
use crate::dialect::{
    classify as classify_dialect, dialect_loss, terminator_line, StreamEvidence, TextEvidence,
};
use crate::loss::SatLossCode;
use crate::FORMAT;

pub(crate) fn decode(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<DecodeResult, CodecError> {
    match classify(bytes) {
        StreamKind::AsmBinary => decode_asm_binary(ctx, bytes),
        StreamKind::Text => decode_text(ctx, bytes),
        StreamKind::AcisBinary => decode_acis_binary(ctx, bytes),
        StreamKind::Unknown => Err(CodecError::Malformed(
            "not an ASM stream: no binary magic and no text header lines".to_string(),
        )),
    }
}

pub(crate) fn decode_asm_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<DecodeResult, CodecError> {
    let header = asm_header::parse(bytes).ok_or_else(|| {
        CodecError::Malformed("ASM binary magic without a parseable header".to_string())
    })?;
    if let Some(count) = header.entity_count {
        ctx.charge_entities(count, "admit SAT header entities")?;
    }
    let width = usize::from(header.width);
    let start = asm_header::record_stream_start(bytes).ok_or_else(|| {
        CodecError::Malformed("ASM binary header without a record stream".to_string())
    })?;
    // A history-bearing stream ends its solved partition at the delta-state
    // boundary; a history-less stream ends at EOF without a terminator tag.
    let framed = match asm_header::solved_record_limit(bytes) {
        Some(limit) => sab::frame(bytes, start, limit, width),
        None => sab::frame_history(bytes, start, bytes.len(), width),
    };
    let records = framed
        .map_err(|error| CodecError::malformed(format_args!("SAB framing failed: {error}")))?;
    let brep = decode_with_header(
        &records,
        bytes,
        Some(header.clone()),
        "stream",
        IdFormat(FORMAT),
        DecodePurpose::Model,
    );
    let mut attributes = BTreeMap::new();
    header_attributes(&header, "asm", &mut attributes);
    attributes.insert("encoding".to_string(), "binary".to_string());
    let matched = classify_dialect(&StreamEvidence::AsmBinary(Some(&header)));
    build_result(ctx, brep, attributes, &header, None, matched)
}

pub(crate) fn decode_acis_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<DecodeResult, CodecError> {
    let header = acis_header::parse(bytes).ok_or_else(|| {
        CodecError::Malformed("ACIS binary magic without a parseable header".to_string())
    })?;
    if let Some(count) = header.entity_count {
        ctx.charge_entities(count, "admit SAT header entities")?;
    }
    // Every band frames and decodes the same way. `classify` states whether the
    // grammar applied is the one the stream's own save format declares; it
    // gates nothing.
    let matched = classify_dialect(&StreamEvidence::AcisBinary(Some(&header)));
    let start = acis_header::record_stream_start(bytes).ok_or_else(|| {
        CodecError::Malformed("ACIS binary header without a record stream".to_string())
    })?;
    let framed = match acis_header::solved_record_limit(bytes) {
        Some(limit) => sab::frame(bytes, start, limit, 4),
        None => sab::frame_history(bytes, start, bytes.len(), 4),
    };
    let records = framed
        .map_err(|error| CodecError::malformed(format_args!("ACIS SAB framing failed: {error}")))?;
    let brep = decode_with_header(
        &records,
        bytes,
        Some(header.clone()),
        "stream",
        IdFormat(FORMAT),
        DecodePurpose::Model,
    );
    let mut attributes = BTreeMap::new();
    header_attributes(&header, "acis", &mut attributes);
    attributes.insert("encoding".to_string(), "binary".to_string());
    build_result(ctx, brep, attributes, &header, None, matched)
}

fn decode_text(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<DecodeResult, CodecError> {
    let stream = sat::parse(bytes).map_err(|error| {
        CodecError::malformed(format_args!("text stream parse failed: {error}"))
    })?;
    let header = stream.header.as_kernel_header();
    let family = match stream.terminator {
        sat::Terminator::Asm => "asm",
        sat::Terminator::Acis => "acis",
    };
    let dialect = terminator_line(stream.terminator);
    let mut attributes = BTreeMap::new();
    header_attributes(&header, family, &mut attributes);
    attributes.insert("encoding".to_string(), "text".to_string());
    attributes.insert("scale".to_string(), format!("{}", stream.header.scale));
    attributes.insert("terminator".to_string(), dialect.to_string());
    // The ACIS branch carries the same save-format band as the ACIS binary
    // stream, so it takes the same admission — literally the same code path,
    // through `classify`. Neither branch gates the record decode on it.
    let matched = classify_dialect(&StreamEvidence::Text(Some(TextEvidence {
        branch: stream.terminator,
        header: &header,
    })));
    let brep = decode_with_header(
        &stream.records,
        bytes,
        Some(header.clone()),
        "stream",
        IdFormat(FORMAT),
        DecodePurpose::Model,
    );
    build_result(ctx, brep, attributes, &header, Some(dialect), matched)
}

/// Mirrors the primary-layer match into [`SourceMeta`], beside the attributes
/// the codec already emits.
///
/// The `encoding`, `scale`, `terminator`, and save-format attribute keys stay.
/// They duplicate the declared keys for now; retiring the ad-hoc attribute keys
/// is a later phase.
fn source_meta(attributes: BTreeMap<String, String>) -> SourceMeta {
    SourceMeta {
        format: FORMAT.to_string(),
        attributes,
        ..Default::default()
    }
}

fn build_result(
    ctx: &DecodeContext<'_>,
    brep: AsmBrep,
    attributes: BTreeMap<String, String>,
    header: &KernelHeader,
    text_dialect: Option<&str>,
    matched: DialectMatch,
) -> Result<DecodeResult, CodecError> {
    let mut ir = CadIr::empty(Units::default());
    ir.source = Some(source_meta(attributes));
    if let (Some(linear), Some(angular)) = (header.linear, header.angular) {
        ir.tolerances = Tolerances { linear, angular };
    }

    let AsmTransferRemainder {
        body_keys: _,
        face_keys: _,
        unknowns,
        stats,
        annotation_records: _,
    } = transfer_into_ir(ctx, &mut ir, FORMAT, 1, brep)?;

    let geometry_transferred =
        !(ir.model.surfaces.is_empty() && ir.model.points.is_empty() && ir.model.faces.is_empty());
    let mut losses = Vec::new();
    losses.extend(dialect_loss(&matched));
    if !geometry_transferred {
        let branch = text_dialect.map_or(String::new(), |dialect| {
            format!(" The stream ends with `{dialect}`.")
        });
        losses.push(SatLossCode::GeometryFramedWithoutCarriers.note(format!(
            "the stream framed but its records decoded no surfaces, points, or faces; its \
             version or branch is outside the ASM decoders' coverage.{branch}"
        )));
    }
    if stats.unknown_surface_faces > 0 {
        losses.push(SatLossCode::GeometryProceduralSurfaceUntyped.note(format!(
            "{} face(s) rest on procedural surface constructions without a decoded carrier",
            stats.unknown_surface_faces
        )));
    }
    let mut coverage = BTreeMap::new();
    coverage.insert("unknown_records".to_string(), unknowns.len());
    coverage.insert(
        "unknown_surface_faces".to_string(),
        stats.unknown_surface_faces,
    );
    let dialects = vec![matched];
    debug_assert_primary_layer(&dialects, FORMAT);
    let report = DecodeReport {
        dialects,
        format: FORMAT.to_string(),
        container_only: false,
        geometry_transferred,
        coverage,
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: Vec::new(),
    };

    let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();
    source_fidelity
        .attach_native_unknown_records(&mut ir, FORMAT, unknowns)
        .map_err(|error| {
            CodecError::malformed(format_args!("unknown-record retention failed: {error}"))
        })?;
    Ok(DecodeResult::new(ir, report, source_fidelity))
}

#[cfg(test)]
mod tests;
