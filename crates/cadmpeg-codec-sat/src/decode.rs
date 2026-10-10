// SPDX-License-Identifier: Apache-2.0
//! Text and binary decode paths for bare ASM streams.

use cadmpeg_asm::acis_header;
use cadmpeg_asm::asm_header;
use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::kernel_header::{BinaryHeader, KernelHeader};
use cadmpeg_asm::{sab, sat};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use std::collections::BTreeMap;

use crate::detect::{classify, header_attributes, StreamKind};
use crate::dialect::{
    dialect_layers, dialect_loss, layers, terminator_line, Family, StreamEvidence, TextEvidence,
};
use crate::loss::SatLossCode;
use crate::FORMAT;

pub(crate) fn decode(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Decoded, CodecError> {
    match classify(ctx, bytes)? {
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
    let width = header.width;
    let stream = crate::dialect::record_stream_start(bytes, Family::Asm, header);
    let Some(stream) = stream else {
        return Err(unsupported_unframed(
            ctx,
            &StreamEvidence::Binary {
                family: Family::Asm,
                header,
                stream: None,
            },
            format_args!("ASM binary header has no record stream"),
        ));
    };
    let payload = if ctx.container_only() {
        None
    } else {
        let start = stream.offset();
        // A history-bearing stream ends its solved partition at the delta-state
        // boundary; a history-less stream ends at EOF without a terminator tag.
        let framed = match asm_header::solved_record_limit_with_header(ctx, bytes, header)? {
            Some(limit) => sab::frame(
                ctx,
                bytes,
                start,
                limit,
                width,
                header.metadata.entity_count,
            ),
            None => sab::frame_history(
                ctx,
                bytes,
                start,
                bytes.len(),
                width,
                header.metadata.entity_count,
            ),
        };
        let records = framed.map_err(|failure| {
            failure.into_codec_error(ctx, |error| {
                CodecError::malformed(format_args!("SAB framing failed: {error}"))
            })
        })?;
        let brep = decode_with_header(
            ctx,
            &records,
            bytes,
            Some(&header.metadata),
            "stream",
            cadmpeg_asm::asm_format!("sat"),
            DecodePurpose::Model,
        )?;
        Some(brep)
    };
    let mut attributes = BTreeMap::new();
    header_attributes(ctx, &header.metadata, Family::Asm, &mut attributes)?;
    let evidence = StreamEvidence::Binary {
        family: Family::Asm,
        header,
        stream: Some(stream),
    };
    let (matched, kernel) = layers(ctx, &evidence)?;
    build_result(
        ctx,
        payload,
        attributes,
        &header.metadata,
        None,
        matched,
        kernel,
    )
}

fn decode_acis_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &BinaryHeader,
) -> Result<Decoded, CodecError> {
    let stream = crate::dialect::record_stream_start(bytes, Family::Acis, header);
    let Some(stream) = stream else {
        return Err(unsupported_unframed(
            ctx,
            &StreamEvidence::Binary {
                family: Family::Acis,
                header,
                stream: None,
            },
            format_args!("ACIS binary header has no record stream"),
        ));
    };
    let payload = if ctx.container_only() {
        None
    } else {
        let start = stream.offset();
        let framed = match acis_header::solved_record_limit_with_header(ctx, bytes, header)? {
            Some(limit) => sab::frame(
                ctx,
                bytes,
                start,
                limit,
                cadmpeg_asm::kernel_header::RefWidth::Four,
                header.metadata.entity_count,
            ),
            None => sab::frame_history(
                ctx,
                bytes,
                start,
                bytes.len(),
                cadmpeg_asm::kernel_header::RefWidth::Four,
                header.metadata.entity_count,
            ),
        };
        let records = framed.map_err(|failure| {
            failure.into_codec_error(ctx, |error| {
                CodecError::malformed(format_args!("ACIS SAB framing failed: {error}"))
            })
        })?;
        let brep = decode_with_header(
            ctx,
            &records,
            bytes,
            Some(&header.metadata),
            "stream",
            cadmpeg_asm::asm_format!("sat"),
            DecodePurpose::Model,
        )?;
        Some(brep)
    };
    let mut attributes = BTreeMap::new();
    header_attributes(ctx, &header.metadata, Family::Acis, &mut attributes)?;
    // Every band frames and decodes the same way. Classification states
    // whether the grammar applied is the one the framed stream declares; it
    // gates nothing. Build the admitted evidence only after framing succeeds.
    let evidence = StreamEvidence::Binary {
        family: Family::Acis,
        header,
        stream: Some(stream),
    };
    let (matched, kernel) = layers(ctx, &evidence)?;
    build_result(
        ctx,
        payload,
        attributes,
        &header.metadata,
        None,
        matched,
        kernel,
    )
}

fn decode_text(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Decoded, CodecError> {
    let (text_header, branch, records) = if ctx.container_only() {
        let (header, branch) = sat::parse_container(ctx, bytes).map_err(|failure| {
            failure.into_codec_error(ctx, |error| {
                unsupported_unframed(
                    ctx,
                    &StreamEvidence::Text(None),
                    format_args!("text container does not frame: {error}"),
                )
            })
        })?;
        (header, branch, None)
    } else {
        let stream = sat::parse(ctx, bytes).map_err(|failure| {
            failure.into_codec_error(ctx, |error| {
                unsupported_unframed(
                    ctx,
                    &StreamEvidence::Text(None),
                    format_args!("text stream does not frame: {error}"),
                )
            })
        })?;
        (stream.header, stream.terminator, Some(stream.records))
    };
    let header = text_header.as_kernel_header(ctx)?;
    let mut attributes = BTreeMap::new();
    header_attributes(ctx, &header, branch.into(), &mut attributes)?;
    let key = ctx.copy_retained_text("scale", "retain SAT scale attribute key")?;
    let value = ctx.format_retained(
        format_args!("{}", text_header.scale().get()),
        "retain SAT scale attribute",
    )?;
    ctx.insert_btree_map(&mut attributes, key, value, "collect SAT scale attribute")?;
    // The ACIS branch carries the same save-format band as the ACIS binary
    // stream, so it takes the same admission — literally the same code path,
    // through `classify`. Neither branch gates the record decode on it.
    let evidence = StreamEvidence::Text(Some(TextEvidence {
        branch,
        header: &header,
    }));
    let (matched, kernel) = layers(ctx, &evidence)?;
    let payload = match records {
        Some(records) => Some(decode_with_header(
            ctx,
            &records,
            bytes,
            Some(&header),
            "stream",
            cadmpeg_asm::asm_format!("sat"),
            DecodePurpose::Model,
        )?),
        None => None,
    };
    build_result(
        ctx,
        payload,
        attributes,
        &header,
        Some(branch),
        matched,
        kernel,
    )
}

/// Refusal for bytes whose SAT discriminant matched but whose stream did not
/// frame. Inspection reports the same primary match.
fn unsupported_unframed(
    ctx: &DecodeContext<'_>,
    evidence: &StreamEvidence<'_>,
    message: std::fmt::Arguments<'_>,
) -> CodecError {
    let (matched, kernel) = match layers(ctx, evidence) {
        Ok(layers) => layers,
        Err(error) => return error,
    };
    let dialects = match dialect_layers(ctx, matched, kernel) {
        Ok(dialects) => dialects,
        Err(error) => return error,
    };
    CodecError::UnsupportedDialect {
        dialects: Box::new(dialects),
        message: match ctx.format_retained(message, "SAT unframed stream refusal") {
            Ok(message) => message,
            Err(error) => return error,
        },
    }
}

fn build_result(
    ctx: &DecodeContext<'_>,
    payload: Option<AsmBrep>,
    attributes: BTreeMap<String, String>,
    header: &KernelHeader,
    text_dialect: Option<sat::Terminator>,
    matched: DialectMatch,
    kernel: DialectMatch,
) -> Result<Decoded, CodecError> {
    // The recovery loss reads the kernel match, which then moves into the
    // layers. A container-only result reports no transfer and no recovery.
    let recovery = match payload {
        Some(_) => dialect_loss(ctx, &kernel)?,
        None => None,
    };
    let mut ir = CadIr::decoded(SourceMeta::classified(
        dialect_layers(ctx, matched, kernel)?,
        cadmpeg_core::text::named_entries_for_decode(ctx, "the acis header", attributes)?,
    ));
    let mut losses = Vec::new();
    let mut unresolved_tolerance = |name: &str, value: f64| -> Result<(), CodecError> {
        let message = ctx.format_retained(format_args!(
            "header {name} tolerance {value} does not yield a positive finite IR value; keeping the default"
        ), "SAT unresolved tolerance loss")?;
        ctx.push_vec(
            &mut losses,
            SatLossCode::HeaderToleranceUnresolved.note(message),
            "SAT loss notes",
        )
    };
    if let Some(value) = header.linear {
        let linear_mm = value * 10.0;
        match cadmpeg_ir::scalar::PositiveLength::new(linear_mm) {
            Some(linear) => ir.tolerances.linear = linear,
            None => unresolved_tolerance("linear", value)?,
        }
    }
    if let Some(value) = header.angular {
        match cadmpeg_ir::scalar::PositiveAngle::new(value) {
            Some(angular) => ir.tolerances.angular = angular,
            None => unresolved_tolerance("angular", value)?,
        }
    }

    let Some(brep) = payload else {
        return Ok(Decoded {
            ir,
            body: DecodeBody {
                losses,
                ..DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {})
            },
            source_fidelity: cadmpeg_ir::SourceFidelity::default(),
        });
    };

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
    ctx.extend_vec(&mut losses, recovery, "SAT dialect loss notes")?;
    if !geometry_transferred {
        let message = match text_dialect {
            Some(dialect) => ctx.format_retained(
                format_args!(
                    "{UNTRANSFERRED_GEOMETRY} The stream ends with `{}`.",
                    terminator_line(dialect)
                ),
                "SAT untransferred geometry loss",
            )?,
            None => {
                ctx.copy_retained_text(UNTRANSFERRED_GEOMETRY, "SAT untransferred geometry loss")?
            }
        };
        ctx.push_vec(
            &mut losses,
            SatLossCode::GeometryFramedWithoutCarriers.note(message),
            "SAT loss notes",
        )?;
    }
    if stats.unknown_surface_faces() > 0 {
        let message = ctx.format_retained(
            format_args!(
                "{} face(s) rest on procedural surface constructions without a decoded carrier",
                stats.unknown_surface_faces()
            ),
            "SAT procedural surface loss",
        )?;
        ctx.push_vec(
            &mut losses,
            SatLossCode::GeometryProceduralSurfaceUntyped.note(message),
            "SAT loss notes",
        )?;
    }
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    coverage.record(ctx, crate::coverage::UNKNOWN_RECORDS, unknowns.len())?;
    coverage.record(
        ctx,
        crate::coverage::UNKNOWN_SURFACE_FACES,
        stats.unknown_surface_faces(),
    )?;
    let body = DecodeBody {
        coverage,
        losses,
        ..DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::full(
            geometry_transferred,
        ))
    };

    let mut annotations = AnnotationBuilder::new();
    // Every record of one stream shares one handle; a record names a new
    // stream only where its stream name differs from the previous record's.
    let mut current: Option<(String, StreamHandle)> = None;
    for record in ctx.admit_iter(annotation_records, "scan SAT annotation records")? {
        let named = match current.take() {
            Some((name, handle))
                if ctx.equal_bytes(
                    name.as_bytes(),
                    record.stream.as_bytes(),
                    "compare SAT annotation stream",
                )? =>
            {
                (name, handle)
            }
            _ => {
                let handle = StreamHandle::new(
                    ctx,
                    cadmpeg_ir::stream_name!("sat:").with_suffix(
                        ctx,
                        &record.stream,
                        "compose annotation stream name",
                    )?,
                    "allocate annotation stream handle",
                )?;
                (record.stream, handle)
            }
        };
        let stream = &current.insert(named).1;
        for field in ctx.admit_iter(&record.derived_fields, "scan SAT derived fields")? {
            annotations
                .derived(ctx, &record.id, field)
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        annotations.note_owned(
            ctx,
            record.id,
            stream,
            record.offset,
            Some(record.tag.as_str()),
        )?;
    }
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations.build());
    source_fidelity.attach_native_unknown_records(&mut ir, FORMAT, unknowns, ctx)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

/// The untransferred-geometry loss text; a text stream appends its terminator.
const UNTRANSFERRED_GEOMETRY: &str = "the stream framed but its records decoded no surfaces, \
    points, or faces; its version or branch is outside the ASM decoders' coverage.";

#[cfg(test)]
mod tests;
