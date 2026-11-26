// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` scene decode.

use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_asm::dialect::KernelHeaderRef;
use cadmpeg_container::compound::{CompoundEntry, CompoundSnapshot};
use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::dialect::{DialectLayers, DuplicateDialectLayer, LayerInstance};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::{ProductDefinitionId, UnknownId};
use cadmpeg_ir::products::ExternalDocument;
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::{CadIr, SourceFidelity, UnknownRecord};

use crate::container::BREP_DATA;
use crate::loss::IronCadLossCode;
use crate::tree::{Kernel, KernelRecord};
use crate::{acis, container, dialect, frame, kernel, tree};

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let snapshot = container::open(ctx, root)?;
    let mut layers = dialect::layers();
    let mut losses: Vec<LossNote> = snapshot
        .deviations()
        .map(|deviation| IronCadLossCode::ContainerNoncanonical.note(deviation.description()))
        .collect();
    let mut bodies = Bodies::default();
    if !ctx.container_only() {
        if let Some(records) = records(ctx, &snapshot, &mut losses)? {
            bodies = classify(ctx, &records, &mut layers, &mut losses)?;
        }
        scene_streams(&snapshot, &mut losses);
    }
    let mut ir = CadIr::empty();
    ir.source = Some(SourceMeta::classified(
        layers,
        std::collections::BTreeMap::default(),
    ));
    let digest = Sha256Digest::digest(root.window());
    if bodies.brep.is_none() && bodies.retained.is_empty() {
        return Ok(Decoded {
            ir,
            body: DecodeBody {
                losses,
                ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), false))
            },
            source_fidelity: SourceFidelity::new(digest),
        });
    }
    transfer(ctx, ir, bodies, losses, digest)
}

/// The decoded ACIS graph and the kernel records retained beside it.
#[derive(Default)]
struct Bodies {
    brep: Option<AsmBrep>,
    retained: Vec<UnknownRecord>,
}

/// Moves the decoded ACIS graph and the retained kernel records into the
/// document.
fn transfer(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    bodies: Bodies,
    mut losses: Vec<LossNote>,
    digest: Sha256Digest,
) -> Result<Decoded, CodecError> {
    let Bodies { brep, retained } = bodies;
    let AsmTransferRemainder {
        unknowns,
        stats,
        annotation_records,
        unbounded_faces,
    } = match brep {
        Some(brep) => transfer_into_ir(ctx, &mut ir, cadmpeg_ir::codec_id!("ironcad"), brep)?.1,
        None => AsmTransferRemainder {
            unknowns: Vec::new(),
            stats: cadmpeg_asm::brep::stats::Stats::default(),
            annotation_records: Vec::new(),
            unbounded_faces: Vec::new(),
        },
    };
    for face in &unbounded_faces {
        losses.push(IronCadLossCode::FaceBoundaryUnstated.note(format!(
            "face {} on a {} states no loop: the face is stated partial",
            face.face.as_str(),
            face.surface_kind
        )));
    }
    if stats.unknown_surface_faces() > 0 {
        losses.push(IronCadLossCode::SurfaceUntyped.note(format!(
            "{} face(s) rest on surface constructions without a decoded carrier",
            stats.unknown_surface_faces()
        )));
    }
    if !unknowns.is_empty() {
        losses.push(IronCadLossCode::BodyRecordsRetained.note(format!(
            "{} ACIS record(s) have no typed projection; source records retained",
            unknowns.len()
        )));
    }
    let geometry_transferred = !(ir.geometry.surfaces.is_empty()
        && ir.geometry.points.is_empty()
        && ir.geometry.faces.is_empty());
    let mut annotations = AnnotationBuilder::new();
    for record in annotation_records {
        let stream =
            StreamHandle::new(cadmpeg_ir::stream_name!("ironcad:").with_suffix(&record.stream));
        annotations.note(
            &record.id,
            &stream,
            record.offset,
            Some(record.tag.as_str()),
        );
        for field in record.derived_fields {
            annotations.derived(&record.id, field)?;
        }
    }
    let untyped: Vec<_> = unknowns
        .iter()
        .chain(&retained)
        .map(|record| record.id().clone())
        .collect();
    let mut source_fidelity = SourceFidelity::with_annotations(annotations.build(), digest);
    source_fidelity.attach_native_unknown_records(&mut ir, dialect::FORMAT, unknowns, ctx)?;
    source_fidelity.attach_native_unknown_records(&mut ir, dialect::FORMAT, retained, ctx)?;
    for id in untyped {
        source_fidelity.mark_untyped(id.as_str(), [ArenaClass::Model])?;
    }
    ir.state_root_definition(
        ctx,
        ProductDefinitionId::compose(
            &cadmpeg_ir::identity_namespace!("ironcad", "model", "product-definition"),
            cadmpeg_ir::identity_key!("root"),
        ),
        ExternalDocument::default(),
    )?;
    Ok(Decoded {
        ir,
        body: DecodeBody {
            losses,
            ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), geometry_transferred))
        },
        source_fidelity,
    })
}

/// Reads the kernel records of `Brep Data`, or charges the reason they are absent.
fn records<'a>(
    ctx: &DecodeContext<'a>,
    snapshot: &CompoundSnapshot<'a>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Vec<KernelRecord<'a>>>, CodecError> {
    let Some(entry) = snapshot.stream(BREP_DATA) else {
        losses.push(IronCadLossCode::FrameUnreadable.note("`Brep Data` is not a stream"));
        return Ok(None);
    };
    let payload = match snapshot
        .open(ctx, entry)
        .and_then(|stream| frame::payload(ctx, stream))
    {
        Ok(payload) => payload,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            losses.push(
                IronCadLossCode::FrameUnreadable
                    .note(format!("`Brep Data` frame could not be read: {error}")),
            );
            return Ok(None);
        }
    };
    match tree::read(payload) {
        Ok(records) => Ok(Some(records)),
        Err(error) => {
            losses.push(IronCadLossCode::RecordTreeUnreadable.note(format!(
                "`Brep Data` record tree could not be read: {}",
                CodecError::from(error)
            )));
            Ok(None)
        }
    }
}

/// Adds a dialect layer for every kernel record, decodes each ACIS body, and
/// charges each Parasolid body as pending.
fn classify(
    ctx: &DecodeContext<'_>,
    records: &[KernelRecord<'_>],
    layers: &mut DialectLayers,
    losses: &mut Vec<LossNote>,
) -> Result<Bodies, CodecError> {
    let acis_count = records
        .iter()
        .filter(|record| record.kernel == Kernel::Acis)
        .count();
    let acis_results = records
        .iter()
        .filter(|record| record.kernel == Kernel::Acis && record.follows_node)
        .count();
    let acis_instance = if acis_count > 1 {
        LayerInstance::Tagged
    } else {
        LayerInstance::Sole
    };
    let mut parasolid = Vec::new();
    let mut bodies = Bodies::default();
    for (ordinal, record) in records.iter().enumerate() {
        let carrier = format!("{BREP_DATA}/record {ordinal}");
        if !record.canonical_header {
            losses.push(
                IronCadLossCode::RecordHeaderUnrecognized
                    .note(format!("{carrier}: header words differ from 1, 1, 1, 0")),
            );
        }
        match record.kernel {
            Kernel::Parasolid => {
                match kernel::parasolid_schema(record.bytes) {
                    Ok(schema) => {
                        parasolid.push((schema, cadmpeg_parasolid::Carrier::new(carrier.clone())));
                    }
                    Err(error) => losses.push(
                        IronCadLossCode::KernelPrologueUnreadable
                            .note(format!("{carrier}: {error}")),
                    ),
                }
                losses.push(IronCadLossCode::BodiesPending.note(format!(
                    "{carrier}: Parasolid body of {} bytes at nesting depth {} is not decoded",
                    record.bytes.window().len(),
                    record.depth
                )));
            }
            Kernel::Acis => {
                let header = match kernel::acis_header(record.bytes) {
                    Ok(header) => header,
                    Err(error) => {
                        losses.push(
                            IronCadLossCode::KernelPrologueUnreadable
                                .note(format!("{carrier}: {error}")),
                        );
                        continue;
                    }
                };
                match cadmpeg_asm::dialect::classify_layer(
                    KernelHeaderRef::Acis(&header),
                    &carrier,
                    acis_instance,
                ) {
                    Ok(matched) => {
                        if let Err(error) = layers.insert(matched) {
                            losses.push(
                                IronCadLossCode::KernelUnverified.note(collision(&error, &carrier)),
                            );
                        }
                    }
                    Err(error) => losses.push(
                        IronCadLossCode::KernelPrologueUnreadable
                            .note(format!("{carrier}: {error}")),
                    ),
                }
                acis_body(
                    ctx,
                    record,
                    ordinal,
                    &header,
                    acis_results > 1,
                    &mut bodies,
                    losses,
                )?;
            }
            Kernel::Other(tag) => {
                losses.push(IronCadLossCode::KernelUnrecognized.note(format!(
                    "{carrier}: kernel tag {tag:#010x} is not ACIS or Parasolid"
                )));
            }
        }
    }
    for message in
        cadmpeg_parasolid::push_extras(layers, cadmpeg_parasolid::extra_layers(parasolid, &[]))
    {
        losses.push(IronCadLossCode::KernelUnverified.note(message));
    }
    for matched in layers.iter() {
        let message = cadmpeg_parasolid::unverified_message(matched)
            .or_else(|| cadmpeg_asm::dialect::unverified_message("ACIS record", matched));
        if let Some(message) = message {
            losses.push(IronCadLossCode::KernelUnverified.note(message));
        }
    }
    Ok(bodies)
}

/// Decodes the ACIS body of a result record, or retains a feature record.
fn acis_body(
    ctx: &DecodeContext<'_>,
    record: &KernelRecord<'_>,
    ordinal: usize,
    header: &cadmpeg_asm::kernel_header::BinaryHeader,
    qualify: bool,
    bodies: &mut Bodies,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let carrier = format!("{BREP_DATA}/record {ordinal}");
    if !record.follows_node {
        losses.push(IronCadLossCode::FeatureShapeRetained.note(format!(
            "{carrier}: the record lies in the node before a part result record and is read \
             as a feature shape of that part; the kernel record is retained"
        )));
        return retain(ctx, record, ordinal, bodies);
    }
    let stream = format!("{BREP_DATA}/record-{ordinal}");
    let decoded = acis::decode(ctx, record.bytes, header, &stream).and_then(|decoded| {
        let brep = if qualify {
            acis::qualify(ctx, decoded.brep, &format!("record-{ordinal}"))?
        } else {
            decoded.brep
        };
        Ok((brep, decoded.reduced_records, decoded.unread))
    });
    let (brep, reduced, unread) = match decoded {
        Ok(decoded) => decoded,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            losses.push(IronCadLossCode::BodyUnreadable.note(format!("{carrier}: {error}")));
            return retain(ctx, record, ordinal, bodies);
        }
    };
    match &mut bodies.brep {
        Some(merged) => merged.append(brep),
        None => bodies.brep = Some(brep),
    }
    if let Some(offset) = unread {
        losses.push(IronCadLossCode::BodyPartitionUnread.note(format!(
            "{carrier}: the ACIS stream holds bytes after its solved records from offset \
             {offset} of the kernel bytes; the kernel record is retained"
        )));
    }
    if reduced > 0 {
        losses.push(IronCadLossCode::BodyExtensionsRetained.note(format!(
            "{carrier}: {reduced} ACIS record(s) store an extended entity prefix whose \
             extension integers have no neutral projection; the kernel record is retained"
        )));
    }
    if unread.is_some() || reduced > 0 {
        retain(ctx, record, ordinal, bodies)?;
    }
    Ok(())
}

/// Retains the kernel bytes of `record` as a native record.
fn retain(
    ctx: &DecodeContext<'_>,
    record: &KernelRecord<'_>,
    ordinal: usize,
    bodies: &mut Bodies,
) -> Result<(), CodecError> {
    let bytes = record.bytes.window();
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "IronCAD kernel record",
    )?;
    bodies.retained.push(UnknownRecord::retained(
        UnknownId::mint(format!("ironcad:source:kernel-record#{ordinal}"))
            .map_err(CodecError::malformed)?,
        cadmpeg_core::decode::u64_from_index(record.bytes.start()),
        bytes.to_vec(),
        Vec::new(),
    ));
    Ok(())
}

fn collision(DuplicateDialectLayer(rejected): &DuplicateDialectLayer, carrier: &str) -> String {
    format!(
        "the scene produced a duplicate {} dialect layer at carrier {carrier}; \
         the later classification was omitted",
        rejected.format()
    )
}

/// Charges every scene stream other than `Brep Data` that holds bytes.
fn scene_streams(snapshot: &CompoundSnapshot<'_>, losses: &mut Vec<LossNote>) {
    for entry in snapshot.entries() {
        let CompoundEntry::Stream(stream) = entry else {
            continue;
        };
        if stream.path().eq_ignore_ascii_case(BREP_DATA) || stream.logical_size() == 0 {
            continue;
        }
        losses.push(IronCadLossCode::SceneStreamsPending.note(format!(
            "stream `{}` of {} bytes is not decoded",
            stream.path(),
            stream.logical_size()
        )));
    }
}

#[cfg(test)]
mod tests;
