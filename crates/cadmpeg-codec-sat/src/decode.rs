// SPDX-License-Identifier: Apache-2.0
//! Text and binary decode paths for bare ASM streams.

use cadmpeg_asm::acis_header;
use cadmpeg_asm::asm_header;
use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::kernel_header::{BinaryHeader, KernelHeader};
use cadmpeg_asm::{sab, sat};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use std::collections::BTreeMap;

use crate::detect::{classify, header_attributes, StreamKind};
use crate::dialect::{dialect_loss, layers, terminator_line, Family, StreamEvidence, TextEvidence};
use crate::loss::SatLossCode;
use crate::FORMAT;

pub(crate) fn decode(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Decoded, CodecError> {
    match classify(bytes) {
        Some(StreamKind::AsmBinary(header)) => decode_asm_binary(ctx, bytes, &header),
        Some(StreamKind::Text) => decode_text(ctx, bytes),
        Some(StreamKind::AcisBinary(header)) => decode_acis_binary(ctx, bytes, &header),
        None => Err(CodecError::WrongFormat(
            "not an ASM stream: no binary magic and no text header lines".to_string(),
        )),
    }
}

fn decode_asm_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &BinaryHeader,
) -> Result<Decoded, CodecError> {
    if let Some(count) = header.metadata.entity_count {
        ctx.charge_entities(count, "admit SAT header entities")?;
    }
    let width = header.width;
    let stream = crate::dialect::record_stream_start(bytes, Family::Asm, header);
    let Some(stream) = stream else {
        return Err(unsupported_unframed(
            &StreamEvidence::Binary {
                family: Family::Asm,
                header,
                stream: None,
            },
            "ASM binary header has no record stream",
        ));
    };
    let start = stream.offset();
    // A history-bearing stream ends its solved partition at the delta-state
    // boundary; a history-less stream ends at EOF without a terminator tag.
    let framed = match asm_header::solved_record_limit_with_header(bytes, header) {
        Some(limit) => sab::frame(ctx, bytes, start, limit, width),
        None => sab::frame_history(ctx, bytes, start, bytes.len(), width),
    };
    let records = framed.map_err(|failure| {
        failure.into_codec_error(|error| {
            CodecError::malformed(format_args!("SAB framing failed: {error}"))
        })
    })?;
    let brep = decode_with_header(
        ctx,
        &records,
        bytes,
        Some(header.metadata.clone()),
        "stream",
        cadmpeg_asm::asm_format!("sat"),
        DecodePurpose::Model,
    )?;
    let mut attributes = BTreeMap::new();
    header_attributes(&header.metadata, Family::Asm, &mut attributes);
    let evidence = StreamEvidence::Binary {
        family: Family::Asm,
        header,
        stream: Some(stream),
    };
    let (matched, kernel) = layers(&evidence);
    build_result(
        ctx,
        brep,
        attributes,
        &header.metadata,
        None,
        matched,
        &kernel,
    )
}

fn decode_acis_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &BinaryHeader,
) -> Result<Decoded, CodecError> {
    if let Some(count) = header.metadata.entity_count {
        ctx.charge_entities(count, "admit SAT header entities")?;
    }
    let stream = crate::dialect::record_stream_start(bytes, Family::Acis, header);
    let Some(stream) = stream else {
        return Err(unsupported_unframed(
            &StreamEvidence::Binary {
                family: Family::Acis,
                header,
                stream: None,
            },
            "ACIS binary header has no record stream",
        ));
    };
    let start = stream.offset();
    let framed = match acis_header::solved_record_limit_with_header(bytes, header) {
        Some(limit) => sab::frame(
            ctx,
            bytes,
            start,
            limit,
            cadmpeg_asm::kernel_header::RefWidth::Four,
        ),
        None => sab::frame_history(
            ctx,
            bytes,
            start,
            bytes.len(),
            cadmpeg_asm::kernel_header::RefWidth::Four,
        ),
    };
    let records = framed.map_err(|failure| {
        failure.into_codec_error(|error| {
            CodecError::malformed(format_args!("ACIS SAB framing failed: {error}"))
        })
    })?;
    let brep = decode_with_header(
        ctx,
        &records,
        bytes,
        Some(header.metadata.clone()),
        "stream",
        cadmpeg_asm::asm_format!("sat"),
        DecodePurpose::Model,
    )?;
    let mut attributes = BTreeMap::new();
    header_attributes(&header.metadata, Family::Acis, &mut attributes);
    // Every band frames and decodes the same way. Classification states
    // whether the grammar applied is the one the framed stream declares; it
    // gates nothing. Build the admitted evidence only after framing succeeds.
    let evidence = StreamEvidence::Binary {
        family: Family::Acis,
        header,
        stream: Some(stream),
    };
    let (matched, kernel) = layers(&evidence);
    build_result(
        ctx,
        brep,
        attributes,
        &header.metadata,
        None,
        matched,
        &kernel,
    )
}

fn decode_text(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Decoded, CodecError> {
    let stream = sat::parse(ctx, bytes).map_err(|failure| {
        failure.into_codec_error(|error| {
            unsupported_unframed(
                &StreamEvidence::Text(None),
                format!("text stream does not frame: {error}"),
            )
        })
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
        ctx,
        &stream.records,
        bytes,
        Some(header.clone()),
        "stream",
        cadmpeg_asm::asm_format!("sat"),
        DecodePurpose::Model,
    )?;
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
    let dialects = match DialectLayers::of(matched).with(kernel) {
        Ok(dialects) => dialects,
        Err(rejected) => {
            return CodecError::malformed(format!("SAT repeated dialect layer key: {rejected:?}"));
        }
    };
    CodecError::UnsupportedDialect {
        dialects: Box::new(dialects),
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
        DialectLayers::of(matched)
            .with(kernel.clone())
            .map_err(|rejected| {
                CodecError::malformed(format!("SAT repeated dialect layer key: {rejected:?}"))
            })?,
        cadmpeg_core::text::named_entries("the acis header", attributes)?,
    ));
    let mut losses = Vec::new();
    let mut unresolved_tolerance = |name: &str, value: f64| {
        losses.push(SatLossCode::HeaderToleranceUnresolved.note(format!(
            "header {name} tolerance {value} does not yield a positive finite IR value; keeping the default"
        )));
    };
    if let Some(value) = header.linear {
        let linear_mm = value * 10.0;
        match cadmpeg_ir::scalar::PositiveLength::new(linear_mm) {
            Some(linear) => ir.tolerances.linear = linear,
            None => unresolved_tolerance("linear", value),
        }
    }
    if let Some(value) = header.angular {
        match cadmpeg_ir::scalar::PositiveAngle::new(value) {
            Some(angular) => ir.tolerances.angular = angular,
            None => unresolved_tolerance("angular", value),
        }
    }

    let (
        _,
        AsmTransferRemainder {
            unknowns,
            stats,
            annotation_records,
        },
    ) = transfer_into_ir(ctx, &mut ir, FORMAT, brep)?;

    let geometry_transferred =
        !(ir.model.surfaces.is_empty() && ir.model.points.is_empty() && ir.model.faces.is_empty());
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
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    coverage.record(crate::coverage::UNKNOWN_RECORDS, unknowns.len());
    coverage.record(
        crate::coverage::UNKNOWN_SURFACE_FACES,
        stats.unknown_surface_faces(),
    );
    let body = DecodeBody {
        coverage,
        losses,
        ..DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::full(
            geometry_transferred,
        ))
    };

    let mut annotations = AnnotationBuilder::new();
    for record in annotation_records {
        let stream =
            StreamHandle::new(cadmpeg_ir::stream_name!("sat:").with_suffix(&record.stream));
        annotations
            .note(&record.id, &stream, record.offset)
            .tag(record.tag.as_str());
        for field in record.derived_fields {
            annotations
                .derived(&record.id, field)
                .map_err(CodecError::malformed)?;
        }
    }
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations.build());
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
