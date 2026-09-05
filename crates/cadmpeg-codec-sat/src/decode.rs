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
use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use std::collections::BTreeMap;

use crate::detect::{classify, header_attributes, StreamKind};
use crate::dialect::{dialect_loss, layers, terminator_line, Family, StreamEvidence, TextEvidence};
use crate::loss::SatLossCode;
use crate::FORMAT;

pub(crate) fn decode(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Decoded, CodecError> {
    match classify(bytes) {
        Some(StreamKind::AsmBinary) => decode_asm_binary(ctx, bytes),
        Some(StreamKind::Text) => decode_text(ctx, bytes),
        Some(StreamKind::AcisBinary) => decode_acis_binary(ctx, bytes),
        None => Err(CodecError::WrongFormat(
            "not an ASM stream: no binary magic and no text header lines".to_string(),
        )),
    }
}

pub(crate) fn decode_asm_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Decoded, CodecError> {
    let header =
        asm_header::parse(bytes).expect("StreamKind::AsmBinary guarantees the ASM header magic");
    if let Some(count) = header.entity_count {
        ctx.charge_entities(count, "admit SAT header entities")?;
    }
    let width = header.width;
    let start = asm_header::record_stream_start_with_header(bytes, &header).ok_or_else(|| {
        unsupported_unframed(
            &StreamEvidence::Binary {
                family: Family::Asm,
                header: &header,
                framed: false,
            },
            "ASM binary header has no record stream",
        )
    })?;
    // A history-bearing stream ends its solved partition at the delta-state
    // boundary; a history-less stream ends at EOF without a terminator tag.
    let framed = match asm_header::solved_record_limit_with_header(bytes, &header) {
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
    header_attributes(&header, Family::Asm, &mut attributes);
    let evidence = StreamEvidence::Binary {
        family: Family::Asm,
        header: &header,
        framed: true,
    };
    let (matched, kernel) = layers(&evidence);
    build_result(ctx, brep, attributes, &header, None, matched, &kernel)
}

pub(crate) fn decode_acis_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Decoded, CodecError> {
    let header =
        acis_header::parse(bytes).expect("StreamKind::AcisBinary guarantees the ACIS header magic");
    if let Some(count) = header.entity_count {
        ctx.charge_entities(count, "admit SAT header entities")?;
    }
    let start = acis_header::record_stream_start_with_header(bytes, &header).ok_or_else(|| {
        unsupported_unframed(
            &StreamEvidence::Binary {
                family: Family::Acis,
                header: &header,
                framed: false,
            },
            "ACIS binary header has no record stream",
        )
    })?;
    let framed = match acis_header::solved_record_limit_with_header(bytes, &header) {
        Some(limit) => sab::frame(
            bytes,
            start,
            limit,
            cadmpeg_asm::kernel_header::RefWidth::Four,
        ),
        None => sab::frame_history(
            bytes,
            start,
            bytes.len(),
            cadmpeg_asm::kernel_header::RefWidth::Four,
        ),
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
    header_attributes(&header, Family::Acis, &mut attributes);
    // Every band frames and decodes the same way. Classification states
    // whether the grammar applied is the one the framed stream declares; it
    // gates nothing. Build the admitted evidence only after framing succeeds.
    let evidence = StreamEvidence::Binary {
        family: Family::Acis,
        header: &header,
        framed: true,
    };
    let (matched, kernel) = layers(&evidence);
    build_result(ctx, brep, attributes, &header, None, matched, &kernel)
}

fn decode_text(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Decoded, CodecError> {
    let stream = sat::parse(bytes).map_err(|error| {
        unsupported_unframed(
            &StreamEvidence::Text(None),
            format!("text stream does not frame: {error}"),
        )
    })?;
    let header = stream.header.as_kernel_header();
    let mut attributes = BTreeMap::new();
    header_attributes(&header, stream.terminator.into(), &mut attributes);
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
        Some(stream.terminator),
        matched,
        &kernel,
    )
}

/// Refusal for bytes whose SAT discriminant matched but whose stream did not
/// frame. Inspection reports the same primary match.
fn unsupported_unframed(evidence: &StreamEvidence<'_>, message: impl Into<String>) -> CodecError {
    let (matched, kernel) = layers(evidence);
    CodecError::UnsupportedDialect {
        dialects: Box::new(DialectLayers::of(matched).with(kernel)),
        message: message.into(),
    }
}

fn build_result(
    ctx: &DecodeContext<'_>,
    brep: AsmBrep,
    attributes: BTreeMap<String, String>,
    header: &KernelHeader,
    text_dialect: Option<sat::Terminator>,
    matched: DialectMatch,
    kernel: &DialectMatch,
) -> Result<Decoded, CodecError> {
    let mut ir = CadIr::decoded(SourceMeta::classified(
        DialectLayers::of(matched).with(kernel.clone()),
        attributes,
    ));
    if let Some(linear) = header.linear {
        ir.tolerances.linear = linear;
    }
    if let Some(angular) = header.angular {
        ir.tolerances.angular = angular;
    }

    let AsmTransferRemainder {
        body_keys: _,
        face_keys: _,
        unknowns,
        stats,
        annotation_records: _,
    } = transfer_into_ir(ctx, &mut ir, FORMAT, std::num::NonZeroU32::MIN, brep)?;

    let geometry_transferred =
        !(ir.model.surfaces.is_empty() && ir.model.points.is_empty() && ir.model.faces.is_empty());
    let mut losses = Vec::new();
    losses.extend(dialect_loss(kernel));
    if !geometry_transferred {
        let branch = text_dialect.map_or(String::new(), |dialect| {
            format!(" The stream ends with `{}`.", terminator_line(dialect))
        });
        losses.push(SatLossCode::GeometryFramedWithoutCarriers.note(format!(
            "the stream framed but its records decoded no surfaces, points, or faces; its \
             version or branch is outside the ASM decoders' coverage.{branch}"
        )));
    }
    if stats.unknown_surface_faces() > 0 {
        losses.push(SatLossCode::GeometryProceduralSurfaceUntyped.note(format!(
            "{} face(s) rest on procedural surface constructions without a decoded carrier",
            stats.unknown_surface_faces()
        )));
    }
    let mut coverage = cadmpeg_ir::Coverage::default();
    coverage.record(crate::coverage::UNKNOWN_RECORDS, unknowns.len());
    coverage.record(
        crate::coverage::UNKNOWN_SURFACE_FACES,
        stats.unknown_surface_faces(),
    );
    let body = DecodeBody {
        coverage,
        losses,
        ..DecodeBody::new(geometry_transferred)
    };

    let mut source_fidelity = cadmpeg_ir::SourceFidelity::default();
    source_fidelity
        .attach_native_unknown_records(&mut ir, FORMAT, unknowns)
        .map_err(|error| {
            CodecError::malformed(format_args!("unknown-record retention failed: {error}"))
        })?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

#[cfg(test)]
mod tests;
