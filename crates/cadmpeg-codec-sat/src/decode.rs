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
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::DecodeResult;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::report::DecodeReport;
use cadmpeg_ir::units::{Tolerances, Units};
use std::collections::BTreeMap;

use crate::detect::{classify, header_attributes, StreamKind};
use crate::dialect::{dialect_loss, layers, terminator_line, StreamEvidence, TextEvidence};
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
        unsupported_unframed(
            &StreamEvidence::AsmBinary(None),
            "ASM binary magic has no parseable header",
        )
    })?;
    if let Some(count) = header.entity_count {
        ctx.charge_entities(count, "admit SAT header entities")?;
    }
    let width = usize::from(header.width);
    let start = asm_header::record_stream_start(bytes).ok_or_else(|| {
        unsupported_unframed(
            &StreamEvidence::UnframedAsmBinary(&header),
            "ASM binary header has no record stream",
        )
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
    let evidence = StreamEvidence::AsmBinary(Some(&header));
    let (matched, kernel) = layers(&evidence);
    build_result(ctx, brep, attributes, &header, None, matched, kernel)
}

pub(crate) fn decode_acis_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<DecodeResult, CodecError> {
    let header = acis_header::parse(bytes).ok_or_else(|| {
        unsupported_unframed(
            &StreamEvidence::AcisBinary(None),
            "ACIS binary magic has no parseable header",
        )
    })?;
    if let Some(count) = header.entity_count {
        ctx.charge_entities(count, "admit SAT header entities")?;
    }
    // Every band frames and decodes the same way. `classify` states whether the
    // grammar applied is the one the stream's own save format declares; it
    // gates nothing.
    let evidence = StreamEvidence::AcisBinary(Some(&header));
    let (matched, kernel) = layers(&evidence);
    let start = acis_header::record_stream_start(bytes).ok_or_else(|| {
        unsupported_unframed(
            &StreamEvidence::UnframedAcisBinary(&header),
            "ACIS binary header has no record stream",
        )
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
    build_result(ctx, brep, attributes, &header, None, matched, kernel)
}

fn decode_text(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<DecodeResult, CodecError> {
    let stream = sat::parse(bytes).map_err(|error| {
        unsupported_unframed(
            &StreamEvidence::Text(None),
            format!("text stream does not frame: {error}"),
        )
    })?;
    let header = stream.header.as_kernel_header();
    let family = match stream.terminator {
        sat::Terminator::Asm => "asm",
        sat::Terminator::Acis => "acis",
    };
    let dialect = terminator_line(stream.terminator);
    let mut attributes = BTreeMap::new();
    header_attributes(&header, family, &mut attributes);
    attributes.insert("scale".to_string(), format!("{}", stream.header.scale));
    // The ACIS branch carries the same save-format band as the ACIS binary
    // stream, so it takes the same admission — literally the same code path,
    // through `classify`. Neither branch gates the record decode on it.
    let evidence = StreamEvidence::Text(Some(TextEvidence {
        branch: stream.terminator,
        header: &header,
    }));
    let (matched, kernel) = layers(&evidence);
    let brep = decode_with_header(
        &stream.records,
        bytes,
        Some(header.clone()),
        "stream",
        IdFormat(FORMAT),
        DecodePurpose::Model,
    );
    build_result(
        ctx,
        brep,
        attributes,
        &header,
        Some(dialect),
        matched,
        kernel,
    )
}

/// Refusal for bytes whose SAT discriminant matched but whose stream did not
/// frame. Inspection reports the same primary match.
fn unsupported_unframed(evidence: &StreamEvidence<'_>, message: impl Into<String>) -> CodecError {
    let (matched, _) = layers(evidence);
    CodecError::UnsupportedDialect {
        format: FORMAT.into(),
        dialect_match: Box::new(matched),
        message: message.into(),
    }
}

/// Builds source metadata from non-dialect stream attributes.
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
    kernel: DialectMatch,
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
    losses.extend(dialect_loss(&kernel));
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
    let report = DecodeReport::classified(
        cadmpeg_core::dialect::DialectLayers::new(matched, vec![kernel])
            .expect("the ACIS kernel layer differs from the SAT primary"),
        false,
        geometry_transferred,
        coverage,
        losses,
        Vec::new(),
        cadmpeg_ir::report::TransferLedger::default(),
    );

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
