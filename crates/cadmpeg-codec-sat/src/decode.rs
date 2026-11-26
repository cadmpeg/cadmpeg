// SPDX-License-Identifier: Apache-2.0
//! Text and binary decode paths for bare ASM streams.

use cadmpeg_asm::acis_header;
use cadmpeg_asm::asm_header;
use cadmpeg_asm::brep::given_up::WithheldRecord;
use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::kernel_header::{BinaryHeader, KernelHeader};
use cadmpeg_asm::{sab, sat};
use cadmpeg_core::decode::ArenaClass;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::{DialectLayers, DialectMatch};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::Identity;
use cadmpeg_ir::unknown::part::{DecodeParts, GivenUpPart, Part};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::detect::{classify, header_attributes, StreamKind};
use crate::dialect::{dialect_loss, layers, terminator_line, Family, StreamEvidence, TextEvidence};
use crate::loss::SatLossCode;
use crate::FORMAT;

/// The part kind of a text stream's bytes from the point where reading
/// stops to the end of the source.
pub(crate) const UNREAD_PART: &str = "stream-remainder";

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
    let (payload, face_children) = if ctx.container_only() {
        (None, BTreeMap::new())
    } else {
        let start = stream.offset();
        // A history-bearing stream ends its solved partition at the delta-state
        // boundary; a history-less stream ends at EOF without a terminator tag.
        let framed = match asm_header::solved_record_limit_with_header(bytes, header) {
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
        (Some(brep), standalone_face_children(&records))
    };
    let mut attributes = BTreeMap::new();
    source_header_attributes(&header.metadata, Family::Asm, &mut attributes);
    let evidence = StreamEvidence::Binary {
        family: Family::Asm,
        header,
        stream: Some(stream),
    };
    let mut result = build_result(
        ctx,
        Sha256Digest::digest(bytes),
        payload.map(|brep| {
            (
                brep,
                Unprojected {
                    stream: bytes,
                    whole_stream: false,
                    face_children,
                },
            )
        }),
        attributes,
        &header.metadata,
        None,
        layers(&evidence),
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
    let (payload, face_children) = if ctx.container_only() {
        (None, BTreeMap::new())
    } else {
        let start = stream.offset();
        let framed = match acis_header::solved_record_limit_with_header(bytes, header) {
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
        (Some(brep), standalone_face_children(&records))
    };
    let mut attributes = BTreeMap::new();
    source_header_attributes(&header.metadata, Family::Acis, &mut attributes);
    // Every band frames and decodes the same way. Classification states
    // whether the grammar applied is the one the framed stream declares; it
    // gates nothing. Build the admitted evidence only after framing succeeds.
    let evidence = StreamEvidence::Binary {
        family: Family::Acis,
        header,
        stream: Some(stream),
    };
    let mut result = build_result(
        ctx,
        Sha256Digest::digest(bytes),
        payload.map(|brep| {
            (
                brep,
                Unprojected {
                    stream: bytes,
                    whole_stream: false,
                    face_children,
                },
            )
        }),
        attributes,
        &header.metadata,
        None,
        layers(&evidence),
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
            let (header, branch) = sat::parse_container(bytes).map_err(|failure| {
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
    let header = text_header.as_kernel_header();
    let mut attributes = BTreeMap::new();
    source_header_attributes(&header, branch.into(), &mut attributes);
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
    let face_children = standalone_face_children(record_list);
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
    let extensions =
        !ctx.container_only() && (1100..10_000).contains(&text_header.save_format_version);
    // A record that holds the whole stream already keeps each part's bytes.
    let whole_stream =
        extensions || concatenated_streams || legacy_context || body_wire || standalone_faces;
    let mut result = build_result(
        ctx,
        Sha256Digest::digest(bytes),
        payload.map(|brep| {
            (
                brep,
                Unprojected {
                    stream: bytes,
                    whole_stream,
                    face_children,
                },
            )
        }),
        attributes,
        &header,
        Some(branch),
        (matched, kernel),
    )?;
    crate::loss::text_stream_losses(
        text_header.diagnostics.iter().chain(&framing),
        unread
            .as_ref()
            .filter(|_| unread_stream_layout)
            .map(|span| span.start),
        &mut result.body.losses,
    );
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
    retain_source(
        ctx,
        &mut result,
        bytes,
        header_span
            .into_iter()
            .chain(extension_span)
            .chain(legacy_span)
            .chain(wire_span)
            .chain(standalone_span)
            .chain(concatenated_span),
    )?;
    if let Some(span) = unread {
        let reason = framing
            .iter()
            .find(|diagnostic| diagnostic.kind == sat::StreamDiagnosticKind::Unread)
            .map_or("the bytes are not read as records", |diagnostic| {
                diagnostic.error.reason.as_str()
            });
        let mut part = Part::at(
            FORMAT,
            UNREAD_PART,
            cadmpeg_core::decode::u64_from_index(span.start),
            &bytes[span],
            [ArenaClass::Model],
        );
        if whole_stream {
            part = part.digest_only();
        }
        let loss = ctx
            .give_up_part(part, CodecError::malformed(reason))?
            .commit(ctx, &mut result.ir, &mut result.source_fidelity)?;
        result.body.losses.push(loss);
    }
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
    let mut untyped = Vec::new();
    for (id, span) in extents {
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(span.len()),
            "SAT recovered source bytes",
        )?;
        if !matches!(
            id,
            "sat:source:header#0" | "sat:source:concatenated-streams#0"
        ) {
            untyped.push(id);
        }
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
        .attach_native_unknown_records(&mut result.ir, FORMAT, retained, ctx)?;
    for id in untyped {
        result
            .source_fidelity
            .mark_untyped(id, [ArenaClass::Model])?;
    }
    Ok(())
}

/// Refusal for bytes whose SAT discriminant matched but whose stream did not
/// frame. Inspection reports the same primary match.
fn unsupported_unframed(evidence: &StreamEvidence<'_>, message: impl Into<String>) -> CodecError {
    let (matched, kernel) = layers(evidence);
    let dialects = match DialectLayers::of(matched).with(kernel) {
        Ok(dialects) => dialects,
        Err(cadmpeg_core::dialect::DuplicateDialectLayer(layer)) => {
            return CodecError::malformed(format_args!(
                "SAT repeated dialect layer key: {layer:?}"
            ));
        }
    };
    CodecError::UnsupportedDialect {
        dialects: Box::new(dialects),
        message: message.into(),
    }
}

fn build_result(
    ctx: &DecodeContext<'_>,
    decode_digest: Sha256Digest,
    payload: Option<(AsmBrep, Unprojected<'_>)>,
    attributes: BTreeMap<String, String>,
    header: &KernelHeader,
    text_dialect: Option<sat::Terminator>,
    (matched, kernel): (DialectMatch, DialectMatch),
) -> Result<Decoded, CodecError> {
    let mut ir = CadIr::decoded(SourceMeta::classified(
        DialectLayers::of(matched).with(kernel.clone()).map_err(
            |cadmpeg_core::dialect::DuplicateDialectLayer(layer)| {
                CodecError::malformed(format_args!("SAT repeated dialect layer key: {layer:?}"))
            },
        )?,
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

    let Some((mut brep, unprojected)) = payload else {
        return Ok(Decoded {
            ir,
            body: DecodeBody {
                losses,
                ..DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {})
            },
            source_fidelity: cadmpeg_ir::SourceFidelity::new(decode_digest),
        });
    };

    let mut given_up = Vec::new();
    let mut kept = Vec::with_capacity(brep.unknowns.len());
    for record in std::mem::take(&mut brep.unknowns) {
        let id = record.id().as_str();
        let (code, message) = if id.contains(":brep:tvertex#") {
            (SatLossCode::VertexToleranceUnresolved,
                "evaluated vertex tolerance is not positive and finite; tolerance left unset; source record retained")
        } else if id.contains(":brep:tedge#") {
            (SatLossCode::EdgeToleranceUnresolved,
                "edge tolerance is not positive and finite; tolerance left unset; source record retained")
        } else if id.contains(":brep:shell#") {
            given_up.push(unprojected.give_up(ctx, &record, "shell", SatLossCode::TopologyShellUnprojected.note(
                "shell has no admissible members or owner; shell omitted from region references"))?);
            continue;
        } else if id.contains(":brep:face#") {
            given_up.push(unprojected.give_up(
                ctx,
                &record,
                "face",
                SatLossCode::TopologyFaceOwnerUnprojected.note(
                    "standalone source face has no shell owner required by the IR; face omitted",
                ),
            )?);
            continue;
        } else if id.contains(":brep:untyped-record#") {
            (SatLossCode::SourceRecordNameUnresolved,
                "record name has no leading component; identity uses its record-table index; source record retained")
        } else {
            kept.push(record);
            continue;
        };
        losses.push(code.note(message));
        kept.push(record);
    }
    brep.unknowns = kept;

    let (
        _,
        AsmTransferRemainder {
            unknowns,
            stats,
            annotation_records,
            unbounded_faces,
            withheld,
        },
    ) = transfer_into_ir(ctx, &mut ir, cadmpeg_ir::codec_id!(FORMAT), brep)?;
    let withheld = withheld
        .iter()
        .map(|record| unprojected.give_up_withheld(ctx, record))
        .collect::<Result<Vec<_>, _>>()?;
    for face in &unbounded_faces {
        losses.push(SatLossCode::TopologyFaceBoundaryUnstated.note(format!(
            "face record {} states no loop on a {}, which no face covers whole: face {} is stated partial",
            face.record_index,
            face.surface_kind,
            face.face.as_str()
        )));
    }

    let geometry_transferred = !(ir.geometry.surfaces.is_empty()
        && ir.geometry.points.is_empty()
        && ir.geometry.faces.is_empty());
    losses.extend(dialect_loss(&kernel));
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
    losses.extend(unmapped_support_chart_losses(&ir));
    let mut coverage = cadmpeg_ir::report::decode::Coverage::default();
    coverage.record(crate::coverage::UNKNOWN_RECORDS, unknowns.len());
    coverage.record(
        crate::coverage::UNKNOWN_SURFACE_FACES,
        stats.unknown_surface_faces(),
    );
    let mut body = DecodeBody {
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
    let untyped = unknowns
        .iter()
        .filter(|record| {
            let id = record.id().as_str();
            !id.contains(":brep:tvertex#") && !id.contains(":brep:tedge#")
        })
        .map(|record| record.id().clone())
        .collect::<Vec<_>>();
    let mut source_fidelity =
        cadmpeg_ir::SourceFidelity::with_annotations(annotations.build(), decode_digest);
    source_fidelity.attach_native_unknown_records(&mut ir, FORMAT, unknowns, ctx)?;
    for id in untyped {
        source_fidelity.mark_untyped(id.as_str(), [ArenaClass::Model])?;
    }
    for (why, part) in given_up {
        body.losses.push(why);
        body.losses
            .push(part.commit(ctx, &mut ir, &mut source_fidelity)?);
    }
    for part in withheld {
        body.losses
            .push(part.commit(ctx, &mut ir, &mut source_fidelity)?);
    }
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

/// How a decode gives up the source records that no neutral topology holds.
struct Unprojected<'a> {
    /// The ASM bytes the decode read.
    stream: &'a [u8],
    /// Whether a retained record holds the whole stream.
    whole_stream: bool,
    /// The loops and coedges of each standalone face, by the face's offset.
    face_children: BTreeMap<u64, Vec<Identity>>,
}

impl Unprojected<'_> {
    /// Gives up `record`, a source record of `kind` that no neutral topology
    /// holds, for the codec loss `why`, as a part over its bytes.
    fn give_up(
        &self,
        ctx: &DecodeContext<'_>,
        record: &cadmpeg_ir::unknown::UnknownRecord,
        kind: &'static str,
        why: cadmpeg_ir::report::loss::LossNote,
    ) -> Result<(cadmpeg_ir::report::loss::LossNote, GivenUpPart), CodecError> {
        let mut part = Part::at(
            FORMAT,
            kind,
            record.offset(),
            record.data().unwrap_or_default(),
            [ArenaClass::Model],
        )
        .dropping(
            self.face_children
                .get(&record.offset())
                .into_iter()
                .flatten()
                .cloned(),
        );
        if self.whole_stream {
            part = part.digest_only();
        }
        let error = CodecError::malformed(format_args!("{}: {}", why.code, why.message));
        Ok((why, ctx.give_up_part(part, error)?))
    }

    /// Gives up `record`, a topology record the shared decode read and left
    /// out, as a part over its bytes.
    fn give_up_withheld(
        &self,
        ctx: &DecodeContext<'_>,
        record: &WithheldRecord,
    ) -> Result<GivenUpPart, CodecError> {
        let mut part = record.part(FORMAT, self.stream)?;
        if self.whole_stream {
            part = part.digest_only();
        }
        ctx.give_up_part(part, record.error())
    }
}

/// The loops and coedges of each standalone face of `records`, by the
/// face's offset. No neutral topology holds them once the face is dropped.
fn standalone_face_children(records: &[sab::Record]) -> BTreeMap<u64, Vec<Identity>> {
    let mut children = BTreeMap::new();
    let standalone = |record: &&sab::Record| record.head() == "face" && record.ref_at(5).is_none();
    if !records.iter().any(|record| standalone(&record)) {
        return children;
    }
    let by_index = records
        .iter()
        .filter_map(|record| Some((i64::try_from(record.index).ok()?, record)))
        .collect::<HashMap<_, _>>();
    let owned = |index: i64, heads: &[&str], slot: usize, owner: i64| {
        by_index
            .get(&index)
            .copied()
            .filter(|record| heads.contains(&record.head()) && record.ref_at(slot) == Some(owner))
    };
    let id = |index| cadmpeg_asm::brep::id(cadmpeg_asm::asm_format!("sat"), index);
    for face in records.iter().filter(standalone) {
        let Ok(face_index) = i64::try_from(face.index) else {
            continue;
        };
        let mut dropped = Vec::new();
        let mut seen = HashSet::new();
        let mut next_loop = face.ref_at(4);
        while let Some(loop_index) = next_loop {
            let Some(lp) = owned(loop_index, &["loop"], 5, face_index) else {
                break;
            };
            if !seen.insert(loop_index) {
                break;
            }
            dropped.push(id(loop_index));
            let mut next_coedge = lp.ref_at(4);
            while let Some(coedge_index) = next_coedge {
                let Some(coedge) = owned(coedge_index, &["coedge", "tcoedge"], 8, loop_index)
                else {
                    break;
                };
                if !seen.insert(coedge_index) {
                    break;
                }
                dropped.push(id(coedge_index));
                next_coedge = coedge.ref_at(3);
            }
            next_loop = lp.ref_at(3);
        }
        children.insert(cadmpeg_core::decode::u64_from_index(face.offset), dropped);
    }
    children
}

fn unmapped_support_chart_losses(ir: &CadIr) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;

    let mut losses = Vec::new();
    for curve in &ir.geometry.procedural_curves {
        let context = match curve.definition() {
            ProceduralCurveDefinition::Intersection { context, .. } => context,
            ProceduralCurveDefinition::SurfaceCurve { family } => family.context(),
            ProceduralCurveDefinition::SurfaceOffset(offset) => offset.context(),
            ProceduralCurveDefinition::Deformable(deformable) => deformable.context(),
            ProceduralCurveDefinition::Spring(spring) => spring.support_context(),
            _ => continue,
        };
        for (side, support) in context.sides().iter().enumerate() {
            if support.surface.is_none() && support.pcurve.is_some() {
                losses.push(SatLossCode::GeometrySupportChartUnresolved.note(format!(
                    "Curve {} support side {side} retains its native pcurve without a neutral surface chart",
                    curve.id
                )));
            }
        }
    }
    losses
}

/// The kernel header facts a decoded document keeps; the declared entity
/// count is a count of source records.
fn source_header_attributes(
    header: &KernelHeader,
    family: Family,
    attributes: &mut BTreeMap<String, String>,
) {
    header_attributes(header, family, attributes);
    attributes.remove("kernel_entity_count");
}

#[cfg(test)]
mod tests;
