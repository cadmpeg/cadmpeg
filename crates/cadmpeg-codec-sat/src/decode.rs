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
            &StreamEvidence::Binary {
                family: Family::Asm,
                header,
                stream: None,
            },
            "ASM binary header has no record stream",
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
    header_attributes(&header.metadata, Family::Asm, &mut attributes);
    let evidence = StreamEvidence::Binary {
        family: Family::Asm,
        header,
        stream: Some(stream),
    };
    let (matched, kernel) = layers(&evidence);
    let mut result = build_result(
        ctx,
        payload,
        attributes,
        &header.metadata,
        None,
        matched,
        &kernel,
    )?;
    if header.metadata.unreadable_product_fields().next().is_some() {
        retain_source(
            ctx,
            &mut result,
            bytes,
            [("sat:source:header#0", 0..stream.offset())],
        )?;
    }
    Ok(result)
}

fn decode_acis_binary(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    header: &BinaryHeader,
) -> Result<Decoded, CodecError> {
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
    let mut result = build_result(
        ctx,
        payload,
        attributes,
        &header.metadata,
        None,
        matched,
        &kernel,
    )?;
    if header.metadata.unreadable_product_fields().next().is_some() {
        retain_source(
            ctx,
            &mut result,
            bytes,
            [("sat:source:header#0", 0..stream.offset())],
        )?;
    }
    Ok(result)
}

fn decode_text(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Decoded, CodecError> {
    let (text_header, branch, records, framing, unread, concatenated_streams, unread_stream_layout) =
        if ctx.container_only() {
            let (header, branch) = sat::parse_container(ctx, bytes).map_err(|failure| {
                failure.into_codec_error(ctx, |error| {
                    unsupported_unframed(
                        &StreamEvidence::Text(None),
                        format!("text container does not frame: {error}"),
                    )
                })
            })?;
            (header, branch, None, Vec::new(), None, false, false)
        } else {
            let stream = sat::parse(ctx, bytes).map_err(|failure| {
                failure.into_codec_error(ctx, |error| {
                    unsupported_unframed(
                        &StreamEvidence::Text(None),
                        format!("text stream does not frame: {error}"),
                    )
                })
            })?;
            (
                stream.header,
                stream.terminator,
                Some(stream.records),
                stream.framing,
                stream.unread,
                stream.concatenated_streams,
                stream.unread_stream_layout,
            )
        };
    let header = text_header.as_kernel_header(ctx)?;
    let mut attributes = BTreeMap::new();
    header_attributes(&header, branch.into(), &mut attributes);
    if let sat::TextUnits::Declared(scale) = text_header.units() {
        attributes.insert("scale".to_owned(), scale.get().to_string());
    }
    // The ACIS branch carries the same save-format band as the ACIS binary
    // stream, so it takes the same admission — literally the same code path,
    // through `classify`. Neither branch gates the record decode on it.
    let evidence = StreamEvidence::Text(Some(TextEvidence {
        branch,
        header: &header,
    }));
    let (matched, kernel) = layers(&evidence);
    let mut body_wire = false;
    let mut standalone_faces = false;
    let record_list = records.as_deref().unwrap_or_default();
    for record in record_list {
        standalone_faces |= record.head() == "face" && record.ref_at(5).is_none();
        if record.head() == "wire"
            && record
                .ref_at(5)
                .and_then(|owner| usize::try_from(owner).ok())
                .and_then(|owner| record_list.get(owner))
                .is_some_and(|owner| owner.head() == "body")
        {
            body_wire = true;
        }
    }
    let mut legacy_context = false;
    if text_header.save_format_version < 700 {
        for record in records.as_deref().unwrap_or_default() {
            if !matches!(record.head(), "spline" | "intcurve") {
                continue;
            }
            legacy_context |= record.tokens.windows(2).enumerate().any(|(index, tokens)| {
                let [sab::Token::SubtypeOpen, sab::Token::Ident(name)] = tokens else {
                    return false;
                };
                if !matches!(name.as_str(), "exactcur" | "surfintcur" | "exactsur") {
                    return false;
                }
                let block = match record.tokens.get(index + 2) {
                    Some(sab::Token::Enum(0)) => record.tokens.get(index + 3),
                    block => block,
                };
                matches!(block, Some(sab::Token::Ident(name)) if matches!(name.as_str(), "nubs" | "nurbs"))
            });
        }
    }
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
    let mut result = build_result(
        ctx,
        payload,
        attributes,
        &header,
        Some(branch),
        matched,
        &kernel,
    )?;
    crate::loss::text_stream_losses(
        text_header.diagnostics.iter().chain(&framing),
        unread
            .as_ref()
            .filter(|_| unread_stream_layout)
            .map(|span| span.start),
        &mut result.body.losses,
    );
    let extensions =
        !ctx.container_only() && (1100..10_000).contains(&text_header.save_format_version);
    if extensions {
        result.body.losses.push(SatLossCode::SourceRecordExtensionsUnprojected.note(
            "ACIS base extension integers and class-specific tails have no neutral projection; shared entity fields decoded; complete stream retained"));
    }
    if concatenated_streams {
        result.body.losses.push(SatLossCode::SourceConcatenatedStreamsRecovered.note(
            "independently headed SAT streams use separate entity and subtype tables; references rebased; complete source retained"));
    }
    let concatenated_span =
        concatenated_streams.then_some(("sat:source:concatenated-streams#0", 0..bytes.len()));
    let extension_span = extensions.then_some(("sat:source:record-extensions#0", 0..bytes.len()));
    if legacy_context {
        result.body.losses.push(SatLossCode::SourceRecordExtensionsUnprojected.note(
            "Legacy spline construction context has no complete native projection; solved B-spline blocks decoded; complete stream retained"));
    }
    let legacy_span = legacy_context.then_some(("sat:source:legacy-context#0", 0..bytes.len()));
    if body_wire {
        result
            .body
            .losses
            .push(SatLossCode::TopologyWireOwnerUnprojected.note(
            "Body-owned wires have no neutral ownership representation; complete stream retained",
        ));
    }
    let wire_span = body_wire.then_some(("sat:source:body-wire#0", 0..bytes.len()));
    // A dropped standalone face can own loops, trims and carriers that no
    // emitted topology reaches. Retain their complete source with the face.
    let standalone_span = (standalone_faces
        && !(extensions || concatenated_streams || legacy_context || body_wire))
        .then_some(("sat:source:standalone-faces#0", 0..bytes.len()));
    let header_span = (!text_header.diagnostics.is_empty())
        .then_some(("sat:source:header#0", text_header.source_span));
    let unread_span = unread.map(|span| ("sat:source:unread#0", span));
    retain_source(
        ctx,
        &mut result,
        bytes,
        header_span
            .into_iter()
            .chain(unread_span)
            .chain(extension_span)
            .chain(legacy_span)
            .chain(wire_span)
            .chain(standalone_span)
            .chain(concatenated_span),
    )?;
    Ok(result)
}

/// Retain recovered source extents that no framed record carries.
fn retain_source<'a>(
    ctx: &DecodeContext<'_>,
    result: &mut Decoded,
    bytes: &[u8],
    extents: impl IntoIterator<Item = (&'a str, std::ops::Range<usize>)>,
) -> Result<(), CodecError> {
    let mut retained = Vec::new();
    for (id, span) in extents {
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(span.len()),
            "SAT recovered source bytes",
        )?;
        retained.push(cadmpeg_ir::UnknownRecord::retained(
            cadmpeg_ir::ids::UnknownId::mint(id.to_owned()).map_err(CodecError::malformed)?,
            cadmpeg_core::decode::u64_from_index(span.start),
            bytes[span].to_vec(),
            Vec::new(),
        ));
    }
    if retained.is_empty() {
        return Ok(());
    }
    result
        .source_fidelity
        .attach_native_unknown_records(&mut result.ir, FORMAT, retained, ctx)
}

/// Refusal for bytes whose SAT discriminant matched but whose stream did not
/// frame. Inspection reports the same primary match.
fn unsupported_unframed(evidence: &StreamEvidence<'_>, message: impl Into<String>) -> CodecError {
    let (matched, kernel) = layers(evidence);
    let dialects = match DialectLayers::of(matched).with(kernel) {
        Ok(dialects) => dialects,
        Err(rejected) => {
            return match rejected {
                cadmpeg_core::dialect::DialectLayerError::Duplicate(layer) => {
                    CodecError::malformed(format_args!("SAT repeated dialect layer key: {layer:?}"))
                }
            };
        }
    };
    CodecError::UnsupportedDialect {
        dialects: Box::new(dialects),
        message: message.into(),
    }
}

fn build_result(
    ctx: &DecodeContext<'_>,
    payload: Option<AsmBrep>,
    attributes: BTreeMap<String, String>,
    header: &KernelHeader,
    text_dialect: Option<sat::Terminator>,
    matched: DialectMatch,
    kernel: &DialectMatch,
) -> Result<Decoded, CodecError> {
    let mut ir = CadIr::decoded(SourceMeta::classified(
        DialectLayers::of(matched)
            .with(kernel.clone())
            .map_err(|rejected| match rejected {
                cadmpeg_core::dialect::DialectLayerError::Duplicate(layer) => {
                    CodecError::malformed(format_args!("SAT repeated dialect layer key: {layer:?}"))
                }
            })?,
        cadmpeg_core::text::named_entries("the acis header", attributes)?,
    ));
    let mut losses = Vec::new();
    if text_dialect.is_none() {
        crate::loss::binary_header_losses(header, &mut losses);
    }
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

    for record in &brep.unknowns {
        let code = if record.id().as_str().contains(":brep:tvertex#") {
            Some((SatLossCode::VertexToleranceUnresolved,
                "evaluated vertex tolerance is not positive and finite; tolerance left unset; source record retained"))
        } else if record.id().as_str().contains(":brep:tedge#") {
            Some((SatLossCode::EdgeToleranceUnresolved,
                "edge tolerance is not positive and finite; tolerance left unset; source record retained"))
        } else if record.id().as_str().contains(":brep:shell#") {
            Some((SatLossCode::TopologyShellUnprojected,
                "shell has no admissible members or owner; shell omitted from region references; source record retained"))
        } else if record.id().as_str().contains(":brep:face#") {
            Some((SatLossCode::TopologyFaceOwnerUnprojected,
                "standalone source face has no shell owner required by the IR; face omitted; source record retained"))
        } else if record.id().as_str().contains(":brep:untyped-record#") {
            Some((SatLossCode::SourceRecordNameUnresolved,
                "record name has no leading component; identity uses its record-table index; source record retained"))
        } else {
            None
        };
        if let Some((code, message)) = code {
            losses.push(code.note(message));
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
    if let Some(count) = stats
        .other_record_kinds
        .get("cached-procedural-surface-untyped")
    {
        losses.push(SatLossCode::GeometryProceduralSurfaceUntyped.note(format!(
            "{count} surface construction(s) are untyped; solved caches and source records retained"
        )));
    }
    if stats.invalid_use_curve_intervals() > 0 {
        losses.push(SatLossCode::GeometryUseCurveIntervalInvalid.note(format!(
            "{} tolerant-coedge use curve(s) have decreasing carrier endpoints; use curves omitted; coedges and native intervals retained",
            stats.invalid_use_curve_intervals()
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
        annotations.note(
            &record.id,
            &stream,
            record.offset,
            Some(record.tag.as_str()),
        );
        for field in record.derived_fields {
            annotations
                .derived(&record.id, field)
                .map_err(cadmpeg_core::CodecError::from)?;
        }
    }
    let mut source_fidelity = cadmpeg_ir::SourceFidelity::with_annotations(annotations.build());
    source_fidelity.attach_native_unknown_records(&mut ir, FORMAT, unknowns, ctx)?;
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

#[cfg(test)]
mod tests;
