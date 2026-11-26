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
use cadmpeg_ir::ids::{BodyId, OccurrenceId, ProductDefinitionId, UnknownId};
use cadmpeg_ir::products::{
    ExternalDocument, Occurrence, ProductDefinition, ProductDefinitionKind, Storage,
};
use cadmpeg_ir::report::decode::DecodeTransfer;
use cadmpeg_ir::report::loss::LossNote;
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
    let mut losses: Vec<LossNote> = snapshot
        .deviations()
        .map(|deviation| IronCadLossCode::ContainerNoncanonical.note(deviation.description()))
        .collect();
    let mut bodies = Bodies::default();
    let mut scene = Scene::default();
    if !ctx.container_only() {
        if let Some(records) = records(ctx, &snapshot, &mut losses)? {
            bodies = classify(ctx, &records, &mut layers, &mut losses)?;
        }
        scene.records = scene_records(ctx, &snapshot, &mut losses)?;
        scene.units = document_units(ctx, &snapshot, &mut losses)?;
        scene_streams(&snapshot, &mut losses);
    }
    let mut ir = CadIr::empty();
    ir.source = Some(SourceMeta::classified(
        layers,
        std::collections::BTreeMap::default(),
    ));
    let digest = Sha256Digest::digest(root.window());
    if bodies.brep.is_none() && bodies.parasolid.is_empty() {
        return Ok(Decoded {
            ir,
            body: DecodeBody {
                losses,
                ..DecodeBody::new(DecodeTransfer::at(ctx.depth(), false))
            },
            source_fidelity: SourceFidelity::new(digest),
        });
    }
    transfer(ctx, ir, bodies, scene, losses, digest)
}

/// The part and assembly records and the scene units.
#[derive(Default)]
struct Scene {
    records: Option<Vec<SceneRecord>>,
    units: Option<UnitSystem>,
}

/// The decoded ACIS graph, the decoded Parasolid bodies, and the kernel
/// records retained beside them.
#[derive(Default)]
struct Bodies {
    brep: Option<AsmBrep>,
    parasolid: Vec<TransmitGeometry>,
    retained: Vec<UnknownRecord>,
    /// The bodies of each part result record, in stored order.
    results: Vec<Vec<BodyId>>,
}

/// Moves the decoded ACIS graph and Parasolid bodies into the document.
fn transfer(
    ctx: &DecodeContext<'_>,
    mut ir: CadIr,
    bodies: Bodies,
    scene: Scene,
    mut losses: Vec<LossNote>,
    digest: Sha256Digest,
) -> Result<Decoded, CodecError> {
    let Bodies {
        brep,
        parasolid,
        retained,
        results,
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
    let placed = match &scene.records {
        Some(records) => place(ctx, &mut ir, results, records, &mut losses)?,
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
    let results = records
        .iter()
        .filter(|record| {
            matches!(record.kernel, Kernel::Acis | Kernel::Parasolid) && record.follows_node
        })
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
                parasolid_body(ctx, record, ordinal, results > 1, &mut bodies, losses)?;
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
                    results > 1,
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
        losses.push(feature_shape(&carrier));
        return retain(ctx, record, ordinal, bodies);
    }
    let stream = format!("{BREP_DATA}/record-{ordinal}");
    let decoded = acis::decode(ctx, record.bytes, header, &stream).and_then(|decoded| {
        let brep = if qualify {
            acis::qualify(ctx, decoded.brep, &format!("record-{ordinal}"))?
        } else {
            decoded.brep
        };
        Ok((brep, decoded.reduced_records))
    });
    let (brep, reduced) = match decoded {
        Ok(decoded) => decoded,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            losses.push(IronCadLossCode::BodyUnreadable.note(format!("{carrier}: {error}")));
            bodies.results.push(Vec::new());
            return retain(ctx, record, ordinal, bodies);
        }
    };
    bodies
        .results
        .push(brep.bodies.iter().map(|body| body.id.clone()).collect());
    match &mut bodies.brep {
        Some(merged) => merged.append(brep),
        None => bodies.brep = Some(brep),
    }
    if reduced > 0 {
        losses.push(IronCadLossCode::BodyExtensionsRetained.note(format!(
            "{carrier}: {reduced} ACIS record(s) store an extended entity prefix whose \
             extension integers have no neutral projection; the kernel record is retained"
        )));
        retain(ctx, record, ordinal, bodies)?;
    }
    Ok(())
}

/// Decodes the Parasolid body of a result record, or retains a feature record.
fn parasolid_body(
    ctx: &DecodeContext<'_>,
    record: &KernelRecord<'_>,
    ordinal: usize,
    qualify: bool,
    bodies: &mut Bodies,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let carrier = format!("{BREP_DATA}/record {ordinal}");
    if !record.follows_node {
        losses.push(feature_shape(&carrier));
        return retain(ctx, record, ordinal, bodies);
    }
    let stream = format!("{BREP_DATA}/record-{ordinal}");
    let namespace = format!("record-{ordinal}");
    let decoded = parasolid::decode(ctx, record.bytes, &stream).and_then(|decoded| {
        parasolid::qualify(ctx, decoded, qualify.then_some(namespace.as_str()))
    });
    let mut decoded = match decoded {
        Ok(decoded) => decoded,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(error) => {
            losses.push(IronCadLossCode::BodyUnreadable.note(format!("{carrier}: {error}")));
            bodies.results.push(Vec::new());
            return retain(ctx, record, ordinal, bodies);
        }
    };
    bodies.results.push(
        decoded
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect(),
    );
    for loss in std::mem::take(&mut decoded.losses) {
        losses.push(
            IronCadLossCode::ParasolidBodyPartial
                .note(format!("{carrier}: {}: {}", loss.code, loss.message)),
        );
    }
    bodies.parasolid.push(decoded);
    Ok(())
}

/// The loss of a kernel record read as a feature shape.
fn feature_shape(carrier: &str) -> LossNote {
    IronCadLossCode::FeatureShapeRetained.note(format!(
        "{carrier}: the record lies in the node before a part result record and is read as a \
         feature shape of that part; the kernel record is retained"
    ))
}

/// Reads the part and assembly records of `Public Access`, or charges the
/// reason they are absent.
fn scene_records(
    ctx: &DecodeContext<'_>,
    snapshot: &CompoundSnapshot<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<Vec<SceneRecord>>, CodecError> {
    let Some(entry) = snapshot.stream(PUBLIC_ACCESS) else {
        losses.push(
            IronCadLossCode::PlacementUnbound
                .note("`Public Access` is not a stream; bodies stay in their part frames"),
        );
        return Ok(None);
    };
    match snapshot
        .open(ctx, entry)
        .and_then(|stream| frame::payload(ctx, stream))
    {
        Ok(payload) => Ok(Some(placement::records(payload.window()))),
        Err(error @ CodecError::ResourceLimit(_)) => Err(error),
        Err(error) => {
            losses.push(IronCadLossCode::PlacementUnbound.note(format!(
                "`Public Access` frame could not be read: {error}; bodies stay in their part frames"
            )));
            Ok(None)
        }
    }
}

/// Places each part result in the scene by the matrix of its part record.
///
/// Returns the occurrences that the root definition places.
fn place(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    results: Vec<Vec<BodyId>>,
    scene: &[SceneRecord],
    losses: &mut Vec<LossNote>,
) -> Result<Vec<OccurrenceId>, CodecError> {
    let parts: Vec<&SceneRecord> = scene.iter().filter(|record| record.is_part()).collect();
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
         the same rank, and its matrix places the part frame as a row-vector rotation and a \
         translation in metres",
    ));
    let assemblies = scene.len() - parts.len();
    if assemblies > 0 {
        losses.push(IronCadLossCode::AssemblyFlattened.note(format!(
            "{assemblies} assembly record(s) are not transferred; each part is placed in the \
             scene by its own matrix"
        )));
    }
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(2 * parts.len()),
        "IronCAD part definitions and occurrences",
    )?;
    let mut placed = Vec::new();
    for (rank, (part, bodies)) in parts.into_iter().zip(results).enumerate() {
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
                "part record `{}` stores no affine matrix; the part stays in its part frame",
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
        placed.push(occurrence_id);
    }
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

/// Reads the scene units of `Document Data`.
fn document_units(
    ctx: &DecodeContext<'_>,
    snapshot: &CompoundSnapshot<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<Option<UnitSystem>, CodecError> {
    let Some(entry) = snapshot.stream(DOCUMENT_DATA) else {
        return Ok(None);
    };
    let stream = match snapshot.open(ctx, entry) {
        Ok(stream) => stream,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    let units = units::scene_units(stream.window());
    if units.is_some() {
        losses.push(IronCadLossCode::UnitsFirstStated.note(
            "the first stated length, angle, and mass unit of `Document Data` are read as the \
             scene units",
        ));
    }
    Ok(units)
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
