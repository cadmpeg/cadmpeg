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
use crate::{acis, container, dialect, frame, kernel, parasolid, tree};
use cadmpeg_codec_sldprt::transmit::TransmitGeometry;

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
    if bodies.brep.is_none() && bodies.parasolid.is_empty() && bodies.retained.is_empty() {
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

/// The decoded ACIS graph, the decoded Parasolid bodies, and the kernel
/// records retained beside them.
#[derive(Default)]
struct Bodies<'a> {
    brep: Option<AsmBrep>,
    parasolid: Vec<TransmitGeometry>,
    retained: Vec<UnknownRecord>,
    /// Decoded part results before their identities settle.
    pending: Vec<(KernelRecord<'a>, usize, PendingBody)>,
}

/// One decoded part result before its identities settle.
enum PendingBody {
    Acis(Box<AsmBrep>),
    Parasolid(Box<TransmitGeometry>),
}

/// Moves the decoded ACIS graph, the decoded Parasolid bodies, and the
/// retained kernel records into the document.
fn transfer(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    bodies: Bodies<'_>,
    mut losses: Vec<LossNote>,
    digest: Sha256Digest,
) -> Result<Decoded, CodecError> {
    let Bodies {
        brep,
        parasolid,
        retained,
        pending: _,
    } = bodies;
    let AsmTransferRemainder {
        mut unknowns,
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
    for decoded in parasolid {
        let geometry = decoded.geometry;
        ir.geometry.bodies.extend(geometry.bodies);
        ir.geometry.regions.extend(geometry.regions);
        ir.geometry.shells.extend(geometry.shells);
        ir.geometry.faces.extend(geometry.faces);
        ir.geometry.loops.extend(geometry.loops);
        ir.geometry.coedges.extend(geometry.coedges);
        ir.geometry.edges.extend(geometry.edges);
        ir.geometry.vertices.extend(geometry.vertices);
        ir.geometry.points.extend(geometry.points);
        ir.geometry.surfaces.extend(geometry.surfaces);
        ir.geometry
            .procedural_surfaces
            .extend(geometry.procedural_surfaces);
        ir.geometry.curves.extend(geometry.curves);
        ir.geometry
            .procedural_curves
            .extend(geometry.procedural_curves);
        ir.geometry.pcurves.extend(geometry.pcurves);
        ir.geometry.subds.extend(geometry.subds);
        unknowns.extend(decoded.unknowns);
    }
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

/// Adds a dialect layer for every kernel record and decodes the body of each
/// part result record.
fn classify<'a>(
    ctx: &DecodeContext<'_>,
    records: &[KernelRecord<'a>],
    layers: &mut DialectLayers,
    losses: &mut Vec<LossNote>,
) -> Result<Bodies<'a>, CodecError> {
    let acis_count = records
        .iter()
        .filter(|record| record.kernel == Kernel::Acis)
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
                parasolid_body(ctx, record, ordinal, &mut bodies, losses)?;
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
                acis_body(ctx, record, ordinal, &header, &mut bodies, losses)?;
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
    settle(ctx, &mut bodies, losses)?;
    Ok(bodies)
}

/// Decodes the ACIS body of a result record, or retains a feature record.
fn acis_body<'a>(
    ctx: &DecodeContext<'_>,
    record: &KernelRecord<'a>,
    ordinal: usize,
    header: &cadmpeg_asm::kernel_header::BinaryHeader,
    bodies: &mut Bodies<'a>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let carrier = format!("{BREP_DATA}/record {ordinal}");
    if !record.follows_node {
        losses.push(feature_shape(&carrier));
        return retain(ctx, record, ordinal, bodies);
    }
    let stream = format!("{BREP_DATA}/record-{ordinal}");
    let decoded = acis::decode(ctx, record.bytes, header, &stream)
        .map(|decoded| (decoded.brep, decoded.reduced_records, decoded.unread));
    let (brep, reduced, unread) = match decoded {
        Ok(decoded) => decoded,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            losses.push(IronCadLossCode::BodyUnreadable.note(format!("{carrier}: {error}")));
            return retain(ctx, record, ordinal, bodies);
        }
    };
    bodies
        .pending
        .push((*record, ordinal, PendingBody::Acis(Box::new(brep))));
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

/// Decodes the Parasolid body of a result record, or retains a feature record.
fn parasolid_body<'a>(
    ctx: &DecodeContext<'_>,
    record: &KernelRecord<'a>,
    ordinal: usize,
    bodies: &mut Bodies<'a>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let carrier = format!("{BREP_DATA}/record {ordinal}");
    if !record.follows_node {
        losses.push(feature_shape(&carrier));
        return retain(ctx, record, ordinal, bodies);
    }
    let stream = format!("{BREP_DATA}/record-{ordinal}");
    match parasolid::decode(ctx, record.bytes, &stream) {
        Ok(decoded) => {
            bodies
                .pending
                .push((*record, ordinal, PendingBody::Parasolid(Box::new(decoded))));
            Ok(())
        }
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            losses.push(IronCadLossCode::BodyUnreadable.note(format!("{carrier}: {error}")));
            retain(ctx, record, ordinal, bodies)
        }
    }
}

/// Moves the decoded part results into `bodies`, each under its record's
/// namespace when more than one part result decoded.
fn settle(
    ctx: &DecodeContext<'_>,
    bodies: &mut Bodies<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let pending = std::mem::take(&mut bodies.pending);
    let qualify = pending.len() > 1;
    for (record, ordinal, body) in pending {
        let carrier = format!("{BREP_DATA}/record {ordinal}");
        let namespace = format!("record-{ordinal}");
        let namespace = qualify.then_some(namespace.as_str());
        let settled = match body {
            PendingBody::Acis(brep) => match namespace {
                Some(namespace) => acis::qualify(ctx, *brep, namespace)
                    .map(|brep| PendingBody::Acis(Box::new(brep))),
                None => Ok(PendingBody::Acis(brep)),
            },
            PendingBody::Parasolid(decoded) => parasolid::qualify(ctx, *decoded, namespace)
                .map(|decoded| PendingBody::Parasolid(Box::new(decoded))),
        };
        match settled {
            Ok(PendingBody::Acis(brep)) => match &mut bodies.brep {
                Some(merged) => merged.append(*brep),
                None => bodies.brep = Some(*brep),
            },
            Ok(PendingBody::Parasolid(mut decoded)) => {
                let prefix = parasolid::identity_prefix(namespace);
                for loss in std::mem::take(&mut decoded.losses) {
                    losses.push(IronCadLossCode::ParasolidBodyPartial.note(format!(
                        "{carrier}: {}: {}",
                        loss.code,
                        parasolid::rewrite_message(&loss.message, &prefix)
                    )));
                }
                bodies.parasolid.push(*decoded);
            }
            Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
            Err(error) => {
                losses.push(IronCadLossCode::BodyUnreadable.note(format!("{carrier}: {error}")));
                retain(ctx, &record, ordinal, bodies)?;
            }
        }
    }
    Ok(())
}

/// The loss of a kernel record read as a feature shape.
fn feature_shape(carrier: &str) -> LossNote {
    IronCadLossCode::FeatureShapeRetained.note(format!(
        "{carrier}: the record lies in the node before a part result record and is read as a \
         feature shape of that part; the kernel record is retained"
    ))
}

/// Retains the kernel bytes of `record` as a native record.
fn retain(
    ctx: &DecodeContext<'_>,
    record: &KernelRecord<'_>,
    ordinal: usize,
    bodies: &mut Bodies<'_>,
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
