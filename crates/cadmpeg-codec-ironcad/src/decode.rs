// SPDX-License-Identifier: Apache-2.0
//! `IronCAD` scene decode.

use cadmpeg_asm::brep::transfer::{transfer_into_ir, AsmTransferRemainder};
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_asm::dialect::KernelHeaderRef;
use cadmpeg_container::compound::{
    CompoundDeviation, CompoundEntry, CompoundSnapshot, CompoundStreamEntry, CompoundStreamId,
};
use cadmpeg_core::decode::{ArenaClass, DecodeContext, View};
use cadmpeg_core::dialect::{DialectLayers, DuplicateDialectLayer, LayerInstance};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, StreamHandle};
use cadmpeg_ir::codec::{DecodeBody, Decoded};
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::hash::digest::Sha256Digest;
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId, UnknownId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, ProductDefinition, ProductDefinitionKind, Storage,
};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::unknown::part::{Cause, DecodeParts, GivenUpPart, Part};
use cadmpeg_ir::{CadIr, SourceFidelity, UnknownRecord};

use crate::container::{BREP_DATA, DOCUMENT_DATA, PUBLIC_ACCESS};
use crate::loss::IronCadLossCode;
use crate::placement::{PropertyValue, SceneRecord};
use crate::tree::{Kernel, KernelRecord};
use crate::{acis, container, dialect, frame, kernel, parasolid, placement, tree, units};
use cadmpeg_codec_sldprt::transmit::TransmitGeometry;
use cadmpeg_ir::design::units::UnitSystem;
use cadmpeg_ir::ids::MassPropertiesId;
use cadmpeg_ir::physics::{MassProperties, PhysicsTarget};
use cadmpeg_ir::references::InstanceTarget;
use cadmpeg_ir::scalar::PositiveReal;

pub(crate) fn decode(ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
    let snapshot = container::open(ctx, root)?;
    let mut layers = dialect::layers();
    let mut stream_parts = Vec::new();
    let mut failed_opens = Vec::new();
    let mut open = |name| {
        let Some(entry) = snapshot.stream(name) else {
            return Ok(None);
        };
        match ctx.run_part(|| snapshot.open_part(ctx, entry))? {
            Ok(stream) => Ok(Some((entry, stream))),
            Err(error) => {
                let located = snapshot.open_located_part(ctx, entry)?;
                stream_parts.push(give_up_stream(ctx, entry, located, error)?);
                failed_opens.push(entry.id());
                Ok::<_, CodecError>(None)
            }
        }
    };
    let brep = open(BREP_DATA)?;
    let access = open(PUBLIC_ACCESS)?;
    let document = open(DOCUMENT_DATA)?;
    let mut losses = deviation_losses(&snapshot, &failed_opens);
    let mut bodies = Bodies::default();
    let mut scene = Scene::default();
    if !ctx.container_only() {
        if snapshot.stream(BREP_DATA).is_none() {
            losses.push(IronCadLossCode::FrameUnreadable.note("`Brep Data` is not a stream"));
        }
        if let Some((entry, stream)) = brep {
            match ctx
                .run_part(|| tree::read(frame::payload(ctx, stream)?).map_err(CodecError::from))?
            {
                Ok(tree) => {
                    bodies = classify(ctx, &tree.records, &mut layers, &mut losses)?;
                    scene.record_assemblies =
                        tree.records.iter().map(|record| record.assembly).collect();
                    scene.assemblies = tree.assemblies;
                }
                Err(error) => stream_parts.push(give_up_stream(ctx, entry, Some(stream), error)?),
            }
        }
        if snapshot.stream(PUBLIC_ACCESS).is_none() {
            losses.push(
                IronCadLossCode::PlacementUnbound
                    .note("`Public Access` is not a stream; bodies stay in their part frames"),
            );
        }
        if let Some((entry, stream)) = access {
            match ctx.run_part(|| frame::payload(ctx, stream))? {
                Ok(payload) => {
                    let records = placement::records(payload.window());
                    property_losses(&records, &mut losses);
                    scene.records = Some(records);
                }
                Err(error) => stream_parts.push(give_up_stream(ctx, entry, Some(stream), error)?),
            }
        }
        scene.units = document
            .and_then(|(_, stream)| unit_losses(units::scene_units(stream.window()), &mut losses));
        scene_streams(&snapshot, &mut losses);
    }
    bodies.given_up.extend(stream_parts);
    let mut ir = CadIr::empty();
    ir.source = Some(SourceMeta::classified(
        layers,
        std::collections::BTreeMap::default(),
    ));
    let digest = Sha256Digest::digest(root.window());
    if bodies.brep.is_none() && bodies.parasolid.is_empty() && bodies.retained.is_empty() {
        let mut source_fidelity = SourceFidelity::new(digest);
        for part in bodies.given_up {
            losses.push(part.commit(ctx, &mut ir, &mut source_fidelity)?);
        }
        return Ok(Decoded {
            ir,
            body: DecodeBody {
                losses,
                ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), false))
            },
            source_fidelity,
        });
    }
    transfer(ctx, ir, bodies, scene, losses, digest)
}

fn deviation_losses(
    snapshot: &CompoundSnapshot<'_>,
    failed_opens: &[CompoundStreamId],
) -> Vec<LossNote> {
    snapshot
        .deviations()
        .filter(|deviation| {
            !matches!(deviation, CompoundDeviation::StreamChainBroken(id) if failed_opens.contains(id))
        })
        .map(|deviation| IronCadLossCode::ContainerNoncanonical.note(deviation.description()))
        .collect()
}

/// The part and assembly records, the scene units, and the assemblies of the
/// `Brep Data` tree.
#[derive(Default)]
struct Scene {
    records: Option<Vec<SceneRecord>>,
    units: Option<UnitSystem>,
    /// The assembly nodes of `Brep Data`, in the order they end.
    assemblies: Vec<tree::Assembly>,
    /// The assembly that holds each kernel record's pair, by record ordinal.
    record_assemblies: Vec<Option<usize>>,
}

/// The decoded ACIS graph, the decoded Parasolid bodies, the kernel records
/// retained beside them, and the parts given up, among them the ACIS records
/// the graph leaves out.
#[derive(Default)]
struct Bodies<'a> {
    brep: Option<AsmBrep>,
    parasolid: Vec<TransmitGeometry>,
    retained: Vec<UnknownRecord>,
    given_up: Vec<GivenUpPart>,
    /// The record ordinal and bodies of each part result record, in stored
    /// order once settled.
    results: Vec<(usize, Vec<BodyId>)>,
    /// Decoded part results before their identities settle.
    pending: Vec<(KernelRecord<'a>, usize, PendingBody)>,
}

/// One decoded part result before its identities settle.
enum PendingBody {
    /// An ACIS graph, and whether its kernel record is also retained whole.
    Acis {
        brep: Box<AsmBrep>,
        retained: bool,
    },
    Parasolid(Box<TransmitGeometry>),
}

/// Moves the decoded ACIS graph, the decoded Parasolid bodies, and the
/// retained kernel records into the document.
fn transfer(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    bodies: Bodies<'_>,
    scene: Scene,
    mut losses: Vec<LossNote>,
    digest: Sha256Digest,
) -> Result<Decoded, CodecError> {
    let Bodies {
        brep,
        parasolid,
        retained,
        given_up,
        results,
        pending: _,
    } = bodies;
    let AsmTransferRemainder {
        mut unknowns,
        stats,
        annotation_records,
        unbounded_faces,
        withheld: _,
    } = match brep {
        Some(brep) => transfer_into_ir(ctx, &mut ir, cadmpeg_ir::codec_id!("ironcad"), brep)?.1,
        None => AsmTransferRemainder {
            unknowns: Vec::new(),
            stats: cadmpeg_asm::brep::stats::Stats::default(),
            annotation_records: Vec::new(),
            unbounded_faces: Vec::new(),
            withheld: Vec::new(),
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
    for part in given_up {
        losses.push(part.commit(ctx, &mut ir, &mut source_fidelity)?);
    }
    let placed = match &scene.records {
        Some(records) => place(
            ctx,
            &mut ir,
            results
                .into_iter()
                .map(|(ordinal, ids)| {
                    let assembly = scene.record_assemblies.get(ordinal).copied().flatten();
                    (assembly, ids)
                })
                .collect(),
            records,
            &scene.assemblies,
            &mut losses,
        )?,
        None => Vec::new(),
    };
    if let Some(root) = ir.state_root_definition(
        ctx,
        ProductDefinitionId::compose(
            &cadmpeg_ir::identity_namespace!("ironcad", "model", "product-definition"),
            cadmpeg_ir::identity_key!("root"),
        ),
        ExternalDocument::default(),
    )? {
        root.occurrences.extend(placed);
        root.units = scene.units;
    }
    Ok(Decoded {
        ir,
        body: DecodeBody {
            losses,
            ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), geometry_transferred))
        },
        source_fidelity,
    })
}

fn give_up_stream(
    ctx: &DecodeContext<'_>,
    entry: &CompoundStreamEntry,
    source: Option<View<'_>>,
    error: CodecError,
) -> Result<GivenUpPart, CodecError> {
    let path = entry.path();
    let bytes = source.as_ref().map_or(&[][..], |view| view.window());
    let mut part = Part::at("ironcad", "stream", 0, bytes, [ArenaClass::Model])
        .named(path)
        .in_stream(cadmpeg_ir::stream_name!("ironcad:").with_suffix(path))
        .digest_only();
    if path.eq_ignore_ascii_case(DOCUMENT_DATA) {
        part = part.costing("the root definition states no units");
    } else if path.eq_ignore_ascii_case(PUBLIC_ACCESS) {
        part = part.costing("bodies stay in their part frames");
    }
    ctx.give_up_part(part, Cause::Unreadable, error)
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
                        let part = kernel_part(record, ordinal);
                        bodies.given_up.push(ctx.give_up_part(
                            part,
                            Cause::Undecoded,
                            CodecError::malformed(format_args!("{carrier}: {error}")),
                        )?);
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
    let mut cause = Cause::Undecoded;
    let decoded = ctx.run_part(|| {
        acis::decode(ctx, record.bytes, header, &stream)
            .map(|decoded| (decoded.brep, decoded.reduced_records, decoded.unread))
            .map_err(|failure| {
                cause = failure.cause;
                failure.error
            })
    })?;
    let (brep, reduced, unread) = match decoded {
        Ok(decoded) => decoded,
        Err(error) => {
            bodies.results.push((ordinal, Vec::new()));
            let part = kernel_part(record, ordinal);
            bodies.given_up.push(ctx.give_up_part(part, cause, error)?);
            return Ok(());
        }
    };
    let retained = unread.is_some() || reduced > 0;
    bodies.pending.push((
        *record,
        ordinal,
        PendingBody::Acis {
            brep: Box::new(brep),
            retained,
        },
    ));
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
    if retained {
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
    match ctx.run_part(|| parasolid::decode(ctx, record.bytes, &stream))? {
        Ok(decoded) => {
            bodies
                .pending
                .push((*record, ordinal, PendingBody::Parasolid(Box::new(decoded))));
        }
        Err(error) => {
            bodies.results.push((ordinal, Vec::new()));
            let part = kernel_part(record, ordinal);
            bodies
                .given_up
                .push(ctx.give_up_part(part, Cause::Undecoded, error)?);
        }
    }
    Ok(())
}

/// Moves the decoded part results into `bodies`, each under its record's
/// namespace when more than one part result decoded, and gives up each ACIS
/// record a graph leaves out as a part of its kernel record.
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
        let settled = ctx.run_part(|| match body {
            PendingBody::Acis { brep, retained } => match namespace {
                Some(namespace) => {
                    acis::qualify(ctx, *brep, namespace).map(|brep| PendingBody::Acis {
                        brep: Box::new(brep),
                        retained,
                    })
                }
                None => Ok(PendingBody::Acis { brep, retained }),
            },
            PendingBody::Parasolid(decoded) => parasolid::qualify(ctx, *decoded, namespace)
                .map(|decoded| PendingBody::Parasolid(Box::new(decoded))),
        })?;
        match settled {
            Ok(PendingBody::Acis { mut brep, retained }) => {
                let stream = cadmpeg_ir::stream_name!("ironcad:")
                    .with_suffix(format!("{BREP_DATA}/record-{ordinal}"));
                for withheld in std::mem::take(&mut brep.withheld) {
                    let mut part = withheld.part(dialect::FORMAT).in_stream(stream.clone());
                    if retained {
                        part = part.digest_only();
                    }
                    bodies.given_up.push(ctx.give_up_part(
                        part,
                        withheld.cause,
                        withheld.error(),
                    )?);
                }
                bodies.results.push((
                    ordinal,
                    brep.bodies.iter().map(|body| body.id.clone()).collect(),
                ));
                match &mut bodies.brep {
                    Some(merged) => merged.append(*brep),
                    None => bodies.brep = Some(*brep),
                }
            }
            Ok(PendingBody::Parasolid(mut decoded)) => {
                let prefix = parasolid::identity_prefix(namespace);
                for loss in std::mem::take(&mut decoded.losses) {
                    losses.push(IronCadLossCode::ParasolidBodyPartial.note(format!(
                        "{carrier}: {}: {}",
                        loss.code,
                        parasolid::rewrite_message(&loss.message, &prefix)
                    )));
                }
                bodies.results.push((
                    ordinal,
                    decoded
                        .geometry
                        .bodies
                        .iter()
                        .map(|body| body.id.clone())
                        .collect(),
                ));
                bodies.parasolid.push(*decoded);
            }
            Err(error) => {
                bodies.results.push((ordinal, Vec::new()));
                let part = kernel_part(&record, ordinal);
                bodies
                    .given_up
                    .push(ctx.give_up_part(part, Cause::Undecoded, error)?);
            }
        }
    }
    bodies.results.sort_by_key(|(ordinal, _)| *ordinal);
    Ok(())
}

/// The kernel record at `ordinal` of `Brep Data`, as a part to give up.
fn kernel_part<'r>(record: &'r KernelRecord<'_>, ordinal: usize) -> Part<'r> {
    Part::at(
        "ironcad",
        "kernel-record",
        0,
        record.bytes.window(),
        [ArenaClass::Model],
    )
    .in_stream(
        cadmpeg_ir::stream_name!("ironcad:").with_suffix(format!("{BREP_DATA}/record-{ordinal}")),
    )
}

/// The loss of a kernel record read as a feature shape.
fn feature_shape(carrier: &str) -> LossNote {
    IronCadLossCode::FeatureShapeRetained.note(format!(
        "{carrier}: the record lies in the node before a part result record and is read as a \
         feature shape of that part; the kernel record is retained"
    ))
}

/// Charges each record that holds a property the reader does not read.
fn property_losses(records: &[SceneRecord], losses: &mut Vec<LossNote>) {
    for record in records {
        if let Some(name) = &record.unread_property {
            losses.push(IronCadLossCode::PropertyUnread.note(format!(
                "record `{}`: property `{name}` stores a value type that is not read; it and \
                 the properties after it are not transferred",
                record.name
            )));
        }
    }
}

/// Places each part result by the matrix of its part record, and each
/// assembly of the `Brep Data` tree by the matrix of its assembly record.
///
/// `results` holds the bodies of each part result record and the assembly
/// that holds its pair. Returns the occurrences that the root definition
/// places.
fn place(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    results: Vec<(Option<usize>, Vec<BodyId>)>,
    scene: &[SceneRecord],
    tree_assemblies: &[tree::Assembly],
    losses: &mut Vec<LossNote>,
) -> Result<Vec<OccurrenceId>, CodecError> {
    let parts: Vec<&SceneRecord> = scene.iter().filter(|record| record.is_part()).collect();
    let assembly_records: Vec<&SceneRecord> =
        scene.iter().filter(|record| !record.is_part()).collect();
    if parts.len() != results.len() {
        losses.push(IronCadLossCode::PlacementUnbound.note(format!(
            "`Public Access` holds {} part record(s) and `Brep Data` holds {} part result \
             record(s); bodies stay in their part frames",
            parts.len(),
            results.len()
        )));
        return Ok(Vec::new());
    }
    if parts.is_empty() {
        return Ok(Vec::new());
    }
    losses.push(IronCadLossCode::PlacementBoundByOrder.note(
        "each `Public Access` part record is bound to the `Brep Data` part result record of \
         the same rank, and its matrix places the part frame in its assembly as a row-vector \
         rotation and a translation in metres",
    ));
    let nested = assembly_records.len() == tree_assemblies.len();
    if nested && !assembly_records.is_empty() {
        losses.push(IronCadLossCode::AssemblyBoundByOrder.note(format!(
            "{} `Public Access` assembly record(s) are bound to the `Brep Data` nodes that \
             hold pairs, in the order the nodes end",
            assembly_records.len()
        )));
    } else if !nested {
        losses.push(IronCadLossCode::AssemblyFlattened.note(format!(
            "`Public Access` holds {} assembly record(s) and `Brep Data` holds {} assembly \
             node(s); the assemblies are not transferred, and each part is placed in the \
             scene by its own matrix",
            assembly_records.len(),
            tree_assemblies.len()
        )));
    }
    let assembly_count = if nested { assembly_records.len() } else { 0 };
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(2 * (parts.len() + assembly_count)),
        "IronCAD part and assembly definitions and occurrences",
    )?;
    let mut assemblies = Vec::with_capacity(assembly_count);
    for (index, record) in assembly_records.iter().take(assembly_count).enumerate() {
        let key = cadmpeg_core::decode::u64_from_index(index);
        let definition_id = ProductDefinitionId::compose(
            &cadmpeg_ir::identity_namespace!("ironcad", "model", "assembly-definition"),
            key,
        );
        let mut definition = ProductDefinition::new(definition_id, Storage::Embedded);
        definition.kind = Some(ProductDefinitionKind::Assembly);
        definition.source_name = Some(record.name.clone());
        part_properties(record, &mut definition);
        if let Some(mass) = mass_properties(
            record,
            &definition.id,
            cadmpeg_core::decode::u64_from_index(parts.len() + index),
        ) {
            ir.model.mass_properties.push(mass);
        }
        assemblies.push(definition);
    }
    let mut placed = Vec::new();
    for (rank, (part, (assembly, bodies))) in parts.into_iter().zip(results).enumerate() {
        let rank = cadmpeg_core::decode::u64_from_index(rank);
        let definition_id = ProductDefinitionId::compose(
            &cadmpeg_ir::identity_namespace!("ironcad", "model", "product-definition"),
            rank,
        );
        let mut definition = ProductDefinition::new(definition_id.clone(), Storage::Embedded);
        definition.kind = Some(ProductDefinitionKind::Part);
        definition.source_name = Some(part.name.clone());
        definition.bodies = bodies;
        part_properties(part, &mut definition);
        if let Some(mass) = mass_properties(part, &definition_id, rank) {
            ir.model.mass_properties.push(mass);
        }
        ir.model.product_definitions.push(definition);
        if part.transform.is_none() {
            losses.push(IronCadLossCode::PlacementUnbound.note(format!(
                "part record `{}` stores no affine matrix; the part frame is placed at its \
                 parent origin",
                part.name
            )));
        }
        let occurrence_id = OccurrenceId::compose(
            &cadmpeg_ir::identity_namespace!("ironcad", "model", "occurrence"),
            rank,
        );
        let mut occurrence = Occurrence::new(occurrence_id.clone(), definition_id, part.transform);
        occurrence.name = Some(part.name.clone());
        ir.model.occurrences.push(occurrence);
        match assembly.and_then(|index| assemblies.get_mut(index)) {
            Some(owner) => owner.occurrences.push(occurrence_id),
            None => placed.push(occurrence_id),
        }
    }
    for (index, record) in assembly_records.iter().take(assembly_count).enumerate() {
        if record.transform.is_none() {
            losses.push(IronCadLossCode::PlacementUnbound.note(format!(
                "assembly record `{}` stores no affine matrix; the assembly frame is placed \
                 at its parent origin",
                record.name
            )));
        }
        let occurrence_id = OccurrenceId::compose(
            &cadmpeg_ir::identity_namespace!("ironcad", "model", "assembly-occurrence"),
            cadmpeg_core::decode::u64_from_index(index),
        );
        let mut occurrence = Occurrence::new(
            occurrence_id.clone(),
            assemblies[index].id.clone(),
            record.transform,
        );
        occurrence.name = Some(record.name.clone());
        ir.model.occurrences.push(occurrence);
        match tree_assemblies[index].parent {
            Some(parent) => assemblies[parent].occurrences.push(occurrence_id),
            None => placed.push(occurrence_id),
        }
    }
    ir.model.product_definitions.extend(assemblies);
    Ok(placed)
}

/// Properties of a part record that the definition states directly.
const DEFINITION_PROPERTIES: [&str; 4] = ["PartName", "Part Number", "Description", "ModelGUID"];

/// Properties of a part record that its mass record states.
const MASS_PROPERTIES: [&str; 2] = ["MassDensity", "PartMass"];

/// Kilograms per cubic metre to kilograms per cubic millimetre.
const KG_PER_M3_TO_KG_PER_MM3: f64 = 1.0e-9;

/// Moves the part number, description, GUID, and remaining set properties
/// of `part` into `definition`.
fn part_properties(part: &SceneRecord, definition: &mut ProductDefinition) {
    let text = |name: &str| match part.property(name) {
        Some(PropertyValue::Text(text)) => Some(text.clone()),
        _ => None,
    };
    definition.label = text("PartName");
    definition.part_number = text("Part Number");
    definition.description = text("Description");
    definition.native_ref = text("ModelGUID");
    for (name, value) in &part.properties {
        if DEFINITION_PROPERTIES.contains(&name.as_str())
            || MASS_PROPERTIES.contains(&name.as_str())
        {
            continue;
        }
        let Some(key) = cadmpeg_core::text::NonBlankString::new(name.clone()) else {
            continue;
        };
        let value = match value {
            PropertyValue::Real(value) => value.to_string(),
            PropertyValue::Text(text) => text.clone(),
        };
        definition.bom_properties.insert(key, value);
    }
}

/// The mass record of a part whose record states a mass or a density.
fn mass_properties(
    part: &SceneRecord,
    definition: &ProductDefinitionId,
    rank: u64,
) -> Option<MassProperties> {
    let real = |name: &str| match part.property(name) {
        Some(PropertyValue::Real(value)) => Some(*value),
        _ => None,
    };
    let mass = real("PartMass").and_then(PositiveReal::new);
    let density = real("MassDensity")
        .map(|value| value * KG_PER_M3_TO_KG_PER_MM3)
        .and_then(PositiveReal::new);
    if mass.is_none() && density.is_none() {
        return None;
    }
    Some(MassProperties {
        id: MassPropertiesId::compose(
            &cadmpeg_ir::identity_namespace!("ironcad", "model", "mass-properties"),
            rank,
        ),
        target: InstanceTarget::new(PhysicsTarget::Definition {
            definition: definition.clone(),
        }),
        mass,
        density,
        center_of_mass: None,
        inertia: None,
        material: None,
        native_ref: None,
    })
}

/// Charges the notes of `scene` and returns its unit system.
fn unit_losses(scene: units::SceneUnits, losses: &mut Vec<LossNote>) -> Option<UnitSystem> {
    for note in scene.unrecognized {
        losses.push(IronCadLossCode::UnitsUnrecognized.note(format!("`{DOCUMENT_DATA}`: {note}")));
    }
    for note in scene.conflicts {
        losses.push(IronCadLossCode::UnitsScaleConflict.note(format!("`{DOCUMENT_DATA}`: {note}")));
    }
    scene.units
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
        let read = [BREP_DATA, PUBLIC_ACCESS, DOCUMENT_DATA]
            .iter()
            .any(|name| stream.path().eq_ignore_ascii_case(name));
        if read || stream.logical_size() == 0 || stream.chain_error().is_some() {
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
