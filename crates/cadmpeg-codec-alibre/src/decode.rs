// SPDX-License-Identifier: Apache-2.0
//! Decode of an Alibre part: the ACIS body of `SAB_DATA`.

use std::collections::BTreeMap;

use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::{decode_with_header, AsmBrep, DecodePurpose};
use cadmpeg_asm::kernel_header::{KernelHeader, RefWidth};
use cadmpeg_asm::{acis_header, sab};
use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::report::loss::LossNote;

use crate::container::{AlibreContainer, SAB_STREAM, STEP_STREAM};
use crate::dialect::{classify, kernel_stream, Classification, KernelStream};
use crate::history::History;
use crate::loss::AlibreLossCode;
use crate::FORMAT;

/// Millimetres per ASM model-space length unit, which the header scale of an
/// Alibre stream declares.
const HEADER_SCALE_TO_MM: f64 = 10.0;

pub(crate) fn decode<'a>(ctx: &DecodeContext<'a>, root: View<'a>) -> Result<Decoded, CodecError> {
    let digest = Sha256Digest::digest(root.window());
    let container = AlibreContainer::open(ctx, root)?;
    let sab = container.stream(ctx, SAB_STREAM)?;
    let bytes = sab.window();
    let kernel = kernel_stream(bytes);
    let classification = classify(kernel.as_ref());
    let mut losses = container.deviation_losses();

    if ctx.container_only() {
        losses.extend(classification.kernel_loss());
        let ir = new_ir(classification, kernel.as_ref())?;
        return Ok(Decoded {
            ir,
            body: DecodeBody {
                losses,
                ..DecodeBody::new(cadmpeg_ir::report::decode::DecodeTransfer::ContainerOnly {})
            },
            source_fidelity: cadmpeg_ir::SourceFidelity::new(digest),
        });
    }

    let Some(kernel) = kernel else {
        let message = "the SAB_DATA stream is not an ACIS binary stream with a record stream";
        return Err(CodecError::UnsupportedDialect {
            dialects: Box::new(classification.layers()?),
            message: message.into(),
        });
    };
    let (brep, removed) = decode_kernel_stream(ctx, bytes, &kernel)?;
    losses.extend(extensions_loss(removed));
    losses.extend(container.undecoded_streams_loss());
    let step = container.stream(ctx, STEP_STREAM)?;
    let history =
        match read_history(ctx, step.window()) {
            Ok(Some(history)) => Some(history),
            Ok(None) => {
                losses.push(AlibreLossCode::HistoryStreamUnread.note(
                    "the STEP_DATA stream names another schema or holds no part state context",
                ));
                None
            }
            Err(CodecError::Malformed(reason)) => {
                losses.push(AlibreLossCode::HistoryStreamUnread.note(format!(
                    "the STEP_DATA stream is not read as a feature history: {reason}"
                )));
                None
            }
            Err(error) => return Err(error),
        };
    from_kernel(
        ctx,
        classification,
        &kernel,
        brep,
        losses,
        digest,
        history.as_ref(),
    )
}

fn read_history(ctx: &DecodeContext<'_>, bytes: &[u8]) -> Result<Option<History>, CodecError> {
    crate::history::read(&crate::part21::parse(ctx, bytes)?)
}

pub(crate) fn decode_kernel_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    kernel: &KernelStream,
) -> Result<(AsmBrep, sab::ExtendedBaseRemoval), CodecError> {
    let header = &kernel.header;
    let limit = acis_header::solved_record_limit_with_header(bytes, header);
    let framed = sab::frame_for_header(
        ctx,
        bytes,
        kernel.records_at,
        limit.unwrap_or(bytes.len()),
        RefWidth::Four,
        &header.metadata,
        limit.is_none(),
    );
    let mut records = framed.map_err(|failure| {
        failure.into_codec_error(ctx, |error| {
            CodecError::malformed(format_args!("ACIS SAB framing failed: {error}"))
        })
    })?;
    let removed = sab::normalize_extended_base(&mut records, header.metadata.save_format_version);
    let brep = decode_with_header(
        ctx,
        &records,
        bytes,
        Some(&header.metadata),
        SAB_STREAM,
        cadmpeg_asm::asm_format!("alibre"),
        DecodePurpose::Model,
    )?;
    Ok((brep, removed))
}

/// The one note of a stream whose records stored the extended entity base. The
/// decoder reads the shared entity fields and drops the integers the base adds.
fn extensions_loss(removed: sab::ExtendedBaseRemoval) -> Option<LossNote> {
    (!removed.is_empty()).then(|| {
        AlibreLossCode::SourceRecordExtensionsUnprojected.note(format!(
            "the extended entity base stores an integer in {} record(s), and a body record stores one more in {}; the source states neither, and the decoder drops them",
            removed.base_integers, removed.body_integers
        ))
    })
}

fn header_attributes(header: &KernelHeader) -> BTreeMap<String, String> {
    let mut attributes = BTreeMap::new();
    for (key, value) in [
        (
            "acis_save_format_version",
            header.save_format_version.map(|value| value.to_string()),
        ),
        ("product_family", header.product_family.clone()),
        ("product_version", header.product_version.clone()),
        ("kernel_family", Some("acis".to_owned())),
    ] {
        if let Some(value) = value {
            attributes.insert(key.to_owned(), value);
        }
    }
    attributes
}

fn new_ir(
    classification: Classification,
    kernel: Option<&KernelStream>,
) -> Result<CadIr, CodecError> {
    let attributes = kernel
        .map(|kernel| header_attributes(&kernel.header.metadata))
        .unwrap_or_default();
    Ok(CadIr::decoded(SourceMeta::classified(
        classification.layers()?,
        cadmpeg_core::text::named_entries("the acis header", attributes)?,
    )))
}

fn from_kernel(
    ctx: &DecodeContext<'_>,
    classification: Classification,
    kernel: &KernelStream,
    brep: AsmBrep,
    mut losses: Vec<LossNote>,
    digest: Sha256Digest,
    history: Option<&History>,
) -> Result<Decoded, CodecError> {
    let header = &kernel.header.metadata;
    losses.extend(classification.kernel_loss());
    let mut ir = new_ir(classification, Some(kernel))?;
    if let Some(value) = header.linear {
        let linear_mm = value * HEADER_SCALE_TO_MM;
        match cadmpeg_ir::scalar::PositiveLength::new(linear_mm) {
            Some(linear) => ir.tolerances.linear = linear,
            None => losses.push(unresolved_tolerance("linear", value)),
        }
    }
    if let Some(value) = header.angular {
        match cadmpeg_ir::scalar::PositiveAngle::new(value) {
            Some(angular) => ir.tolerances.angular = angular,
            None => losses.push(unresolved_tolerance("angular", value)),
        }
    }
    for record in &brep.unknowns {
        if let Some(note) = unknown_record_loss(record.id().as_str()) {
            losses.push(note);
        }
    }
    let (
        _,
        AsmTransferRemainder {
            unknowns,
            stats,
            annotation_records,
        },
    ) = transfer_into_ir(ctx, &mut ir, cadmpeg_ir::codec_id!(FORMAT), brep)?;
    let geometry_transferred = !(ir.geometry.surfaces.is_empty()
        && ir.geometry.points.is_empty()
        && ir.geometry.faces.is_empty());
    if !geometry_transferred {
        losses.push(
            AlibreLossCode::GeometryFramedWithoutCarriers
                .note("the stream framed but its records decoded no surfaces, points, or faces"),
        );
    }
    if stats.unknown_surface_faces() > 0 {
        losses.push(
            AlibreLossCode::GeometryProceduralSurfaceUntyped.note(format!(
                "{} face(s) rest on procedural surface constructions without a decoded carrier",
                stats.unknown_surface_faces()
            )),
        );
    }
    if let Some(count) = stats
        .other_record_kinds
        .get("cached-procedural-surface-untyped")
    {
        losses.push(
            AlibreLossCode::GeometryProceduralSurfaceUntyped.note(format!(
            "{count} surface construction(s) are untyped; solved caches and source records retained"
        )),
        );
    }
    if stats.undecoded_pcurve_refs() > 0 {
        let kinds = stats
            .undecoded_pcurve_kinds
            .iter()
            .map(|(kind, count)| format!("{kind}={count}"))
            .collect::<Vec<_>>()
            .join(", ");
        losses.push(AlibreLossCode::GeometryPcurveUndecoded.note(format!(
            "{} coedge(s) name a pcurve record with no decodable 2D carrier; those coedges carry no pcurve. Source kinds: {kinds}",
            stats.undecoded_pcurve_refs()
        )));
    }
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
            StreamHandle::new(cadmpeg_ir::stream_name!("alibre:").with_suffix(&record.stream));
        annotations.note(
            &record.id,
            &stream,
            record.offset,
            Some(record.tag.as_str()),
        );
        for field in record.derived_fields {
            annotations
                .derived(&record.id, field)
                .map_err(CodecError::from)?;
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
        cadmpeg_ir::SourceFidelity::with_annotations(annotations.build(), digest);
    source_fidelity.attach_native_unknown_records(&mut ir, FORMAT, unknowns, ctx)?;
    for id in untyped {
        source_fidelity.mark_untyped(id.as_str(), [ArenaClass::Model])?;
    }
    let root = cadmpeg_ir::ids::ProductDefinitionId::compose(
        &cadmpeg_ir::identity_namespace!("alibre", "model", "product-definition"),
        cadmpeg_ir::identity_key!("root"),
    );
    ir.state_root_definition(
        ctx,
        root.clone(),
        cadmpeg_ir::products::ExternalDocument::default(),
    )?;
    if let Some(history) = history {
        match crate::design::project(ctx, &mut ir, history, &root, &mut body.losses) {
            Ok(()) => {}
            Err(CodecError::Malformed(reason)) => {
                let message =
                    format!("the feature history is not projected into the design layer: {reason}");
                body.losses
                    .push(AlibreLossCode::HistoryStreamUnread.note(message));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(Decoded {
        ir,
        body,
        source_fidelity,
    })
}

/// The loss a retained source record states, by the class of the record the
/// shared decoder could not project.
fn unknown_record_loss(id: &str) -> Option<LossNote> {
    let (code, message) = if id.contains(":brep:tvertex#") {
        (AlibreLossCode::VertexToleranceUnresolved,
            "evaluated vertex tolerance is not positive and finite; tolerance left unset; source record retained")
    } else if id.contains(":brep:tedge#") {
        (AlibreLossCode::EdgeToleranceUnresolved,
            "edge tolerance is not positive and finite; tolerance left unset; source record retained")
    } else if id.contains(":brep:shell#") {
        (AlibreLossCode::TopologyShellUnprojected,
            "shell has no admissible members or owner; shell omitted from region references; source record retained")
    } else if id.contains(":brep:face#") {
        (AlibreLossCode::TopologyFaceOwnerUnprojected,
            "standalone source face has no shell owner required by the IR; face omitted; source record retained")
    } else if id.contains(":brep:untyped-record#") {
        (AlibreLossCode::SourceRecordNameUnresolved,
            "record name has no leading component; identity uses its record-table index; source record retained")
    } else {
        return None;
    };
    Some(code.note(message))
}

fn unresolved_tolerance(name: &str, value: f64) -> LossNote {
    AlibreLossCode::HeaderToleranceUnresolved.note(format!(
        "header {name} tolerance {value} does not yield a positive finite IR value; keeping the default"
    ))
}

#[cfg(test)]
mod tests {
    use super::unknown_record_loss;
    use crate::loss::AlibreLossCode;

    /// Each retained-record class the shared decoder reports maps to its own
    /// code, and any other record states no loss.
    #[test]
    fn retained_records_map_to_their_loss_codes() {
        for (id, code) in [
            (
                "alibre:brep:tvertex#4",
                AlibreLossCode::VertexToleranceUnresolved,
            ),
            (
                "alibre:brep:tedge#7",
                AlibreLossCode::EdgeToleranceUnresolved,
            ),
            (
                "alibre:brep:shell#3",
                AlibreLossCode::TopologyShellUnprojected,
            ),
            (
                "alibre:brep:face#9",
                AlibreLossCode::TopologyFaceOwnerUnprojected,
            ),
            (
                "alibre:brep:untyped-record#12",
                AlibreLossCode::SourceRecordNameUnresolved,
            ),
        ] {
            let note = unknown_record_loss(id).expect(id);
            assert_eq!(note.code, code.kind(), "{id}");
            assert_eq!(note.severity, code.note("").severity, "{id}");
        }
        assert!(unknown_record_loss("alibre:brep:lump#2").is_none());
    }
}
