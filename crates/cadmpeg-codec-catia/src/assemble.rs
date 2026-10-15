// SPDX-License-Identifier: Apache-2.0
//! Shared emit scaffolding used by two or more family decode routes.
//!
//! Byte-provenance annotation, raw-payload preservation, unresolved-carrier
//! loss accounting, neutral-model admissibility, source metadata, generic
//! vector/range helpers, and the metadata/geometry/container report builders.

use cadmpeg_core::convert::{f64_from_index, truncate_f64_to_usize};

use cadmpeg_core::dialect::DialectMatch;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::hash::{sha256, LowerHex};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::units::FinitePoint2;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::SourceObjectAssociation;
use std::collections::BTreeMap;

use crate::container::ContainerScan;
use crate::loss::{identity_statement, CatiaLossCode};
use crate::resource;

pub(crate) fn cgm_source(
    kind: &str,
    tag: u32,
) -> Result<SourceObjectAssociation, cadmpeg_core::CodecError> {
    cgm_source_key(kind, format_args!("{tag:06x}"))
}

pub(crate) fn cgm_source_key(
    kind: &str,
    key: impl std::fmt::Display,
) -> Result<SourceObjectAssociation, cadmpeg_core::CodecError> {
    let object_id = format!("cgm-{kind}:{key}");
    let object_id = cadmpeg_core::text::NonBlankString::new(object_id).ok_or_else(|| {
        cadmpeg_core::CodecError::malformed("CATIA CGM source object id is blank")
    })?;
    Ok(SourceObjectAssociation {
        geometry_role: None,
        format: cadmpeg_ir::codec_format!(crate::dialect::FORMAT),
        object_id,
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    })
}

pub(crate) fn annotate(
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    id: impl std::fmt::Display,
    stream_name: &str,
    offset: u64,
    tag: impl std::fmt::Display,
    exactness: Exactness,
) -> Result<(), cadmpeg_core::CodecError> {
    let tag = tag.to_string();
    annotations.annotate(
        id,
        format_args!("catia:{stream_name}"),
        offset,
        &tag,
        exactness,
    )
}

/// Judge one candidate neutral model after canonicalizing arena order.
///
/// Matches [`DecodeResult::new`](cadmpeg_ir::codec::DecodeResult::new), which
/// sorts arenas by entity id before a document leaves the codec. Admission uses
/// [`cadmpeg_ir::CATIA_ADMISSION_CHECKS`], not full final-document validation.
pub(crate) fn neutral_model_is_admissible(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    pending_unknowns: &[UnknownRecord],
) -> Result<bool, cadmpeg_core::CodecError> {
    ir.model.finalize();
    Ok(cadmpeg_ir::admit_with_additional_native_identities(
        ctx,
        ir,
        pending_unknowns.iter().map(|record| record.id().as_str()),
        cadmpeg_ir::CATIA_ADMISSION_CHECKS,
        Vec::new(),
    )?
    .is_ok())
}

/// Identities of the curve and surface carriers the transfer left unresolved.
///
/// A carrier is one record instance, so a report about them names the
/// identities, not only how many there are.
fn unresolved_carrier_ids(ir: &CadIr) -> (Vec<&str>, Vec<&str>) {
    let mut resolved_curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            !matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
                    | CurveGeometry::Procedural { .. }
            )
        })
        .map(|curve| curve.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut resolved_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| {
            !matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                    | SurfaceGeometry::Procedural { .. }
            )
        })
        .map(|surface| surface.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    loop {
        let mut changed = false;
        for procedural in &ir.model.procedural_surfaces {
            let resolved = match procedural.definition() {
                ProceduralSurfaceDefinition::Exact(..)
                | ProceduralSurfaceDefinition::Helix { .. }
                | ProceduralSurfaceDefinition::RollingBallJet(_) => true,
                ProceduralSurfaceDefinition::Offset(definition_payload) => {
                    let support = definition_payload.support();
                    {
                        resolved_surfaces.contains(support.as_str())
                    }
                }
                ProceduralSurfaceDefinition::Revolution(definition_payload) => {
                    let directrix = definition_payload.directrix();
                    {
                        resolved_curves.contains(directrix.as_str())
                    }
                }
                ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
                    let directrix = definition_payload.directrix();
                    {
                        resolved_curves.contains(directrix.as_str())
                    }
                }
                ProceduralSurfaceDefinition::LinearSweep(definition_payload) => {
                    resolved_curves.contains(definition_payload.directrix().as_str())
                }
                _ => false,
            };
            if resolved {
                if let Some(owner) = ir.model.procedural_surface_owner(&procedural.id) {
                    changed |= resolved_surfaces.insert(owner.as_str());
                }
            }
        }
        for procedural in &ir.model.procedural_curves {
            let resolved = match procedural.definition() {
                ProceduralCurveDefinition::Exact { .. } | ProceduralCurveDefinition::Helix(_) => {
                    true
                }
                ProceduralCurveDefinition::Intersection { context, .. } => {
                    context.sides().iter().all(|side| {
                        side.surface
                            .as_ref()
                            .is_some_and(|surface| resolved_surfaces.contains(surface.as_str()))
                    })
                }
                ProceduralCurveDefinition::SurfaceCurve { family } => {
                    let (has_side, all_resolved) = family
                        .context()
                        .sides()
                        .iter()
                        .filter_map(|side| side.surface.as_ref().zip(side.pcurve.as_ref()))
                        .fold((false, true), |(_, all_resolved), (surface, _)| {
                            (
                                true,
                                all_resolved && resolved_surfaces.contains(surface.as_str()),
                            )
                        });
                    has_side && all_resolved
                }
                _ => false,
            };
            if resolved {
                if let Some(owner) = ir.model.procedural_curve_owner(&procedural.id) {
                    changed |= resolved_curves.insert(owner.as_str());
                }
            }
        }
        if !changed {
            break;
        }
    }
    let curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
                    | CurveGeometry::Procedural { .. }
            ) && !resolved_curves.contains(curve.id.as_str())
        })
        .map(|curve| curve.id.as_str())
        .chain(
            ir.model
                .edges
                .iter()
                .filter(|edge| edge.curve().is_none())
                .map(|edge| edge.id.as_str()),
        )
        .collect::<Vec<_>>();
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| {
            matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
                    | SurfaceGeometry::Procedural { .. }
            ) && !resolved_surfaces.contains(surface.id.as_str())
        })
        .map(|surface| surface.id.as_str())
        .collect::<Vec<_>>();
    (curves, surfaces)
}

/// How many curve and surface carriers the transfer left unresolved.
#[cfg(test)]
fn unresolved_carrier_counts(ir: &CadIr) -> (usize, usize) {
    let (curves, surfaces) = unresolved_carrier_ids(ir);
    (curves.len(), surfaces.len())
}

/// The sentence naming one carrier kind, or nothing when none is unresolved.
fn carrier_clause<'a>(kind: &'a str, ids: &'a [&str]) -> impl std::fmt::Display + 'a {
    struct Clause<'a> {
        kind: &'a str,
        ids: &'a [&'a str],
    }
    impl std::fmt::Display for Clause<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            if !self.ids.is_empty() {
                write!(
                    formatter,
                    " {} carriers: {}.",
                    self.kind,
                    identity_statement(self.ids)
                )?;
            }
            Ok(())
        }
    }
    Clause { kind, ids }
}

pub(crate) fn insert_unresolved_carrier_loss(ir: &CadIr, losses: &mut Vec<LossNote>) {
    let (unresolved_curves, unresolved_surfaces) = unresolved_carrier_ids(ir);
    if unresolved_curves.is_empty() && unresolved_surfaces.is_empty() {
        return;
    }
    let statement = format!("The transferred model retains {} unresolved curve carriers and {} unresolved surface carriers without exact procedural constructions.{}{}",
    unresolved_curves.len(),
    unresolved_surfaces.len(),
    carrier_clause("Curve", &unresolved_curves),
    carrier_clause("Surface", &unresolved_surfaces));
    let note = CatiaLossCode::GeometryUnresolvedCarriers.note(statement);

    losses.insert(0, note);
}

pub(crate) fn ordered_range(range: [f64; 2]) -> [f64; 2] {
    if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    }
}
#[derive(Clone, Copy)]
pub(crate) struct CircleParameterRangeFromSurfaceBranchInputs<'input0> {
    pub(crate) surface: &'input0 SurfaceGeometry,
    pub(crate) center: Point3,
    pub(crate) radius: f64,
    pub(crate) axis: Vector3,
    pub(crate) ref_direction: Vector3,
    pub(crate) start: Point3,
    pub(crate) end: Point3,
    pub(crate) pcurve_origin: FinitePoint2,
    pub(crate) pcurve_direction: FinitePoint2,
}

pub(crate) fn circle_parameter_range_from_surface_branch(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    inputs: CircleParameterRangeFromSurfaceBranchInputs<'_>,
) -> Result<Option<[f64; 2]>, cadmpeg_core::decode::ResourceLimit> {
    let CircleParameterRangeFromSurfaceBranchInputs {
        surface,
        center,
        radius,
        axis,
        ref_direction,
        start,
        end,
        pcurve_origin,
        pcurve_direction,
    } = inputs;

    if !center.is_finite()
        || !start.is_finite()
        || !end.is_finite()
        || !axis.is_finite()
        || !ref_direction.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
    {
        return Ok(None);
    }
    let tangent = axis.cross(ref_direction);
    if !tangent.is_finite()
        || tangent.x.hypot(tangent.y).hypot(tangent.z) == 0.0
        || ref_direction
            .x
            .hypot(ref_direction.y)
            .hypot(ref_direction.z)
            == 0.0
    {
        return Ok(None);
    }
    let angle = |point: Point3| {
        let offset = point.vector_from(center);
        offset.dot(tangent).atan2(offset.dot(ref_direction))
    };
    let start = angle(start);
    let end = angle(end);
    if !start.is_finite() || !end.is_finite() {
        return Ok(None);
    }
    let short_end = unwrap_angle(end, start);
    if !short_end.is_finite() {
        return Ok(None);
    }
    let delta = short_end - start;
    if !delta.is_finite() || delta == 0.0 {
        return Ok(None);
    }
    let long_end = short_end - delta.signum() * std::f64::consts::TAU;
    if !long_end.is_finite() {
        return Ok(None);
    }
    let (pcurve_origin, pcurve_direction) = (pcurve_origin.as_raw(), pcurve_direction.as_raw());
    let midpoint_uv = Point2::new(
        pcurve_origin.u + 0.5 * pcurve_direction.u,
        pcurve_origin.v + 0.5 * pcurve_direction.v,
    );
    if !midpoint_uv.is_finite() {
        return Ok(None);
    }
    let Some(surface_midpoint) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::surface_point(ctx, surface, midpoint_uv.u, midpoint_uv.v),
        )?)?
    else {
        return Ok(None);
    };
    let surface_midpoint = surface_midpoint.get();
    let mut candidates = [short_end, long_end].into_iter().filter(|end| {
        let parameter = 0.5 * (start + end);
        if !parameter.is_finite() {
            return false;
        }
        let circle_midpoint = Point3::new(
            center.x + radius * (parameter.cos() * ref_direction.x + parameter.sin() * tangent.x),
            center.y + radius * (parameter.cos() * ref_direction.y + parameter.sin() * tangent.y),
            center.z + radius * (parameter.cos() * ref_direction.z + parameter.sin() * tangent.z),
        );
        if !circle_midpoint.is_finite() {
            return false;
        }
        let distance_squared = circle_midpoint.distance_squared(surface_midpoint);
        distance_squared.is_finite() && distance_squared.sqrt() <= 2e-3
    });
    let Some(end) = candidates.next() else {
        return Ok(None);
    };
    if candidates.next().is_some() {
        return Ok(None);
    }
    Ok((end.is_finite() && end != start).then_some([start, end]))
}

/// Counts of each typed analytic surface kind decoded.
#[derive(Debug, Default)]
pub(crate) struct TypedCounts {
    pub(crate) plane: usize,
    pub(crate) cylinder: usize,
    pub(crate) cone: usize,
    pub(crate) sphere: usize,
    pub(crate) torus: usize,
}

impl TypedCounts {
    pub(crate) fn record(&mut self, g: &SurfaceGeometry) {
        match g {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => self.plane += 1,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => self.cylinder += 1,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => self.cone += 1,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => self.sphere += 1,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => self.torus += 1,
            _ => {}
        }
    }

    pub(crate) fn total(&self) -> usize {
        self.plane + self.cylinder + self.cone + self.sphere + self.torus
    }
}

/// Counts used to account for decoded geometry and topology populations.
pub(crate) struct GeometryReportCounts {
    pub(crate) face_local_freeform: usize,
    pub(crate) unbound_revolution: usize,
    pub(crate) admitted_standard_face_rows: usize,
}

pub(crate) fn source_meta(
    scan: &ContainerScan,
    matched: &DialectMatch,
) -> Result<SourceMeta, cadmpeg_core::CodecError> {
    let mut attributes = BTreeMap::new();
    resource::source_attribute(
        &mut attributes,
        format_args!("file_size"),
        format_args!("{}", scan.data.len()),
    )?;
    resource::source_attribute(
        &mut attributes,
        format_args!("outer_dir_offset"),
        format_args!("{}", scan.outer_dir_offset),
    )?;
    if let Some(dir) = &scan.inner {
        resource::source_attribute(
            &mut attributes,
            format_args!("inner_offset"),
            format_args!("{}", dir.inner),
        )?;
        resource::source_attribute(
            &mut attributes,
            format_args!("stream_count"),
            format_args!("{}", dir.descriptors.len()),
        )?;
    }
    if let Some(brep) = &scan.brep {
        resource::source_attribute(
            &mut attributes,
            format_args!("brep_stream_len"),
            format_args!("{}", brep.len()),
        )?;
        let digest = sha256(brep);
        resource::source_attribute(
            &mut attributes,
            format_args!("brep_stream_sha256"),
            format_args!("{}", LowerHex(&digest)),
        )?;
        resource::source_attribute(
            &mut attributes,
            format_args!("fbb_runs"),
            format_args!("{}", scan.census.fbb_runs),
        )?;
        resource::source_attribute(
            &mut attributes,
            format_args!("fbb_face_rows"),
            format_args!("{}", scan.census.fbb_face_rows),
        )?;
        resource::source_attribute(
            &mut attributes,
            format_args!("vertex_records"),
            format_args!("{}", scan.census.vertex_markers),
        )?;
    }
    resource::source_attribute(
        &mut attributes,
        format_args!("preview_count"),
        format_args!("{}", scan.previews.len()),
    )?;
    for (index, preview) in scan.previews.iter().enumerate() {
        resource::source_attribute(
            &mut attributes,
            format_args!("preview_{index}_width"),
            format_args!("{}", preview.width),
        )?;
        resource::source_attribute(
            &mut attributes,
            format_args!("preview_{index}_height"),
            format_args!("{}", preview.height),
        )?;
        resource::source_attribute(
            &mut attributes,
            format_args!("preview_{index}_components"),
            format_args!("{}", preview.components),
        )?;
    }
    resource::source_attribute(
        &mut attributes,
        format_args!("external_reference_count"),
        format_args!("{}", scan.external_references.len()),
    )?;
    for (index, reference) in scan.external_references.iter().enumerate() {
        resource::source_attribute(
            &mut attributes,
            format_args!("external_reference_{index}"),
            format_args!("{}", reference.target),
        )?;
    }
    resource::source_attribute(
        &mut attributes,
        format_args!("finjpl_segment_count"),
        format_args!("{}", scan.finjpl_segments.len()),
    )?;
    for (index, segment) in scan.finjpl_segments.iter().enumerate() {
        if let Some(name) = &segment.name {
            resource::source_attribute(
                &mut attributes,
                format_args!("finjpl_segment_{index}_name"),
                format_args!("{name}"),
            )?;
        }
        resource::source_attribute(
            &mut attributes,
            format_args!("finjpl_segment_{index}_type"),
            format_args!("0x{:08x}", segment.type_word),
        )?;
    }
    Ok(SourceMeta::classified(
        cadmpeg_core::dialect::DialectLayers::of(matched.clone()),
        attributes,
    ))
}

pub(crate) fn build_geometry_report(
    ir: &CadIr,
    scan: &ContainerScan,
    typed: &TypedCounts,
    (plane_faces, analytic_record_count): (usize, usize),
    report_counts: &GeometryReportCounts,
    topology_failure: Option<&str>,
) -> DecodeBody {
    let mut losses = Vec::new();

    losses.push(CatiaLossCode::GeometryCarrierSummary.note(format!(
        "{} vertex point(s) were decoded verbatim from `05 08 01` records (3×f32 \
         LE, millimetres, identity world placement) and {} analytic surface carrier(s) were \
         decoded from `SurfacicReps` `00 33` records: {} plane, {} cylinder, {} cone, {} \
         sphere, {} torus.",
        ir.model.vertices.len(),
        typed.total(),
        typed.plane,
        typed.cylinder,
        typed.cone,
        typed.sphere,
        typed.torus
    )));

    if let Some(topology_failure) = topology_failure {
        losses.push(CatiaLossCode::TopologyBoundaryGraphNotEmitted.note(format!(
            "The B-rep boundary graph was not emitted: {} face outer-bound row(s) in {} \
             group(s) were detected, but {topology_failure}.",
            scan.census.fbb_face_rows, scan.census.fbb_runs,
        )));
    }
    let withheld_face_rows = scan.census.fbb_face_rows
        - scan
            .census
            .fbb_face_rows
            .min(report_counts.admitted_standard_face_rows);
    if topology_failure.is_none() && scan.census.fbb_runs > 1 && withheld_face_rows > 0 {
        losses.push(CatiaLossCode::TopologyFbbRowsWithheld.note(format!(
            "{withheld_face_rows} candidate FBB face row(s) in {} marker group(s) were not admitted to the standard topology population; only {} row(s) have a source-closed edge, vertex, trim, and topology binding, and cross-group ownership remains unresolved.",
            scan.census.fbb_runs,
            report_counts.admitted_standard_face_rows,
        )));
    }

    if plane_faces > 0 {
        losses.push(CatiaLossCode::GeometryPlaneParametersInvalid.note(format!(
            "{plane_faces} plane surface record(s) were located but not decoded because their \
             tag-bridged parameter records were absent or invalid."
        )));
    }

    let invalid_analytic = typed
        .total()
        .checked_add(plane_faces)
        .and_then(|decoded| analytic_record_count.checked_sub(decoded))
        .unwrap_or(0);
    if invalid_analytic > 0 {
        losses.push(CatiaLossCode::GeometryAnalyticPayloadInvalid.note(format!(
            "{invalid_analytic} analytic surface record(s) had a non-finite or out-of-range \
             inline payload and were not decoded."
        )));
    }
    if report_counts.face_local_freeform > 0 {
        losses.push(
            CatiaLossCode::GeometryFaceLocalFreeformNotTransferred.note(format!(
                "{} face-local free-form carrier record(s) retain their tag, bounds, and \
                 orientation, but their aliased surface geometry is not yet transferred.",
                report_counts.face_local_freeform,
            )),
        );
    }
    if report_counts.unbound_revolution > 0 {
        losses.push(
            CatiaLossCode::GeometryRevolutionProfileUnbound.note(format!(
                "{} consolidated surface-of-revolution record(s) retain their profile identity, \
             orthonormal axis frame, angular chart, and profile interval, but the profile \
             identities are not yet bound to directrix curves.",
                report_counts.unbound_revolution,
            )),
        );
    }

    insert_unresolved_carrier_loss(ir, &mut losses);

    losses.push(
        CatiaLossCode::AttributesMaterialsMetadataNotTransferred.note(
            "Standard circles with an exact adjacent-carrier section normal or two \
                  non-collinear endpoint radii, plane-plane lines, and same-surface cylinder or \
                  cone generators are transferred as curves. Standard spline edges retain exact \
                  two-surface intersection constructions and their identity-bound support \
                  pcurves when present, but unbound serialized 3D NURBS caches, materials, and \
                  document metadata are not yet transferred."
                .to_string(),
        ),
    );

    DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses,
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    }
}

pub(crate) fn build_metadata_fallback(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<(CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>), cadmpeg_core::CodecError> {
    let ir = CadIr::empty();
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();

    // Preserve the reconstructed BREP stream, or the whole file when the scan
    // recovered no stream, as an unknown passthrough so no source byte is
    // dropped. A container-only decode transfers no model, so this record is
    // the only place the payload survives.
    let (bytes, stream, key) = match scan.brep.as_ref() {
        Some(brep) => (
            brep.as_slice(),
            "MainDataStream+SurfacicReps",
            cadmpeg_ir::identity_key!("brep-stream"),
        ),
        None => (
            scan.data.as_ref(),
            "CATPart",
            cadmpeg_ir::identity_key!("container"),
        ),
    };
    let id = UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "payload", "unknown"),
        key,
    );
    ctx.charge_entities(1, "admit CATIA retained source record")?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "retain CATIA raw payload",
    )?;
    let bytes = bytes.to_vec();
    annotate(
        &mut annotations,
        &id,
        stream,
        0,
        scan.variant.id(),
        Exactness::Unknown,
    )?;
    unknowns.push(UnknownRecord::retained(id, 0, bytes, Vec::new()));
    Ok((ir, annotations.build(), unknowns))
}

/// Preserve the native payload for every partial decode.  Typed entities are
/// additive views; unrecovered record families must remain byte-addressable.
/// Returns the index of the preserved payload record in `unknowns`.
pub(crate) fn preserve_raw_payload(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    unknowns: &mut Vec<UnknownRecord>,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    scan: &ContainerScan,
    id: UnknownId,
) -> Result<usize, cadmpeg_core::CodecError> {
    let (bytes, stream) = match scan.brep.as_ref() {
        Some(brep) => (brep.as_slice(), "MainDataStream+SurfacicReps"),
        None => (scan.data.as_ref(), "CATPart"),
    };
    ctx.charge_entities(1, "admit CATIA retained source record")?;
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(bytes.len()),
        "retain CATIA raw payload",
    )?;
    let bytes = bytes.to_vec();
    annotate(
        annotations,
        &id,
        stream,
        0,
        scan.variant.id(),
        Exactness::Unknown,
    )?;
    unknowns.push(UnknownRecord::retained(id, 0, bytes, Vec::new()));
    Ok(unknowns.len() - 1)
}

/// Attribute typed carrier views to the preserved payload when CATIA's binding
/// layer was not recovered. The raw payload is their byte-backed owner; this
/// avoids inventing topology or procedural relationships.
pub(crate) fn link_payload_carriers(
    ir: &CadIr,
    payload: &mut UnknownRecord,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut links = Vec::new();
    for id in ir
        .model
        .surfaces
        .iter()
        .map(|surface| surface.id.as_str())
        .chain(ir.model.curves.iter().map(|curve| curve.id.as_str()))
    {
        let id = id.to_owned();
        links.push(id);
    }
    if links.is_empty() {
        return Ok(());
    }
    *payload.links_mut() = links;
    annotations.derived(payload.id().as_str(), "links")?;
    Ok(())
}

pub(crate) fn build_container_report(scan: &ContainerScan) -> DecodeBody {
    let mut losses = Vec::new();
    losses.push(CatiaLossCode::GeometryBrepNotTransferred.note(format!(
        "No B-rep geometry was transferred. This file's storage variant is `{}` ({}); the \
         applicable decoded record families transfer geometry in this codec.",
        scan.variant.id(),
        scan.variant.description()
    )));

    losses.push(
        CatiaLossCode::TopologyGraphNotBuilt.note(
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built \
                  for this file."
                .to_string(),
        ),
    );

    DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(false),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses,
        notes: Vec::new(),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    }
}

pub(crate) fn unwrap_angle(value: f64, reference: f64) -> f64 {
    let delta = value - reference;
    if (-std::f64::consts::PI..std::f64::consts::PI).contains(&delta) {
        value
    } else {
        let delta = if delta.is_finite() {
            delta
        } else {
            value.rem_euclid(std::f64::consts::TAU) - reference.rem_euclid(std::f64::consts::TAU)
        };
        reference + (delta + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI
    }
}

pub(crate) fn rational_pcurve_arc(
    center: [f64; 2],
    radius: f64,
    range: [f64; 2],
    refusal: &mut crate::nurbs::LaneRefusals,
    record: &str,
) -> Option<PcurveGeometry> {
    let span = range[1] - range[0];
    if !center.into_iter().all(f64::is_finite)
        || !range.into_iter().all(f64::is_finite)
        || range[0] >= range[1]
        || !radius.is_finite()
        || radius <= 0.0
        || !span.is_finite()
    {
        return None;
    }
    let segment_count = (span.abs() / std::f64::consts::FRAC_PI_2).ceil();
    if !segment_count.is_finite() || segment_count > crate::MAX_EXACT_ARC_SPANS {
        return None;
    }
    // `ceil` answers zero only for an angular span of exactly zero: an arc that
    // sweeps no angle states no span, which this route refuses as it refuses
    // every other degeneracy.
    let segment_count =
        truncate_f64_to_usize(segment_count).and_then(std::num::NonZeroUsize::new)?;
    let segment_count = segment_count.get();
    let step = span / f64_from_index(segment_count)?;
    let mut control_points = Vec::new();

    let mut weights = Vec::new();

    let mut knots = Vec::new();

    knots.extend([range[0]; 3]);
    for index in 0..segment_count {
        let start = range[0] + f64_from_index(index)? * step;
        let end = start + step;
        let middle = (start + end) * 0.5;
        let middle_weight = (step * 0.5).cos();
        if !middle_weight.is_finite() || middle_weight == 0.0 {
            return None;
        }
        if index == 0 {
            control_points.push(Point2::new(
                center[0] + radius * start.cos(),
                center[1] + radius * start.sin(),
            ));
            weights.push(1.0);
        }
        control_points.push(Point2::new(
            center[0] + radius / middle_weight * middle.cos(),
            center[1] + radius / middle_weight * middle.sin(),
        ));
        control_points.push(Point2::new(
            center[0] + radius * end.cos(),
            center[1] + radius * end.sin(),
        ));
        weights.extend([middle_weight, 1.0]);
        if index + 1 < segment_count {
            knots.extend([end; 2]);
        }
    }
    knots.extend([range[1]; 3]);
    if !knots.iter().copied().all(f64::is_finite)
        || !control_points
            .iter()
            .copied()
            .all(|point| point.is_finite())
        || !weights.iter().copied().all(f64::is_finite)
    {
        return None;
    }
    crate::nurbs::note_refusal(
        cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            2,
            knots,
            control_points,
            Some(weights),
            false,
        ),
        refusal,
        record,
    )
    .map(|nurbs| PcurveGeometry::Nurbs { nurbs })
}

pub(crate) fn quintic_jet_pcurve(
    degree: u32,
    knots: &[f64],
    points: &[[f64; 2]],
    (first, second): (&[[f64; 2]], &[[f64; 2]]),
    refusal: &mut crate::nurbs::LaneRefusals,
    record: impl std::fmt::Display,
) -> Option<PcurveGeometry> {
    let (full_knots, controls) =
        crate::nurbs::quintic_jet_bspline(degree, knots, points, first, second)?;
    let mut control_points = Vec::new();

    control_points.extend(
        controls
            .into_iter()
            .map(|point| Point2::new(point[0], point[1])),
    );
    crate::nurbs::note_refusal(
        cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
            degree,
            full_knots,
            control_points,
            None,
            false,
        ),
        refusal,
        record,
    )
    .map(|nurbs| PcurveGeometry::Nurbs { nurbs })
}

#[cfg(test)]
mod route_tests {
    use crate::assemble::source_meta;
    use crate::assemble::{
        circle_parameter_range_from_surface_branch, neutral_model_is_admissible,
        rational_pcurve_arc, unresolved_carrier_counts,
    };

    use cadmpeg_ir::document::CadIr;

    use cadmpeg_ir::geometry::{
        pcurve::PcurveGeometry, Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition,
        ProceduralSurface, ProceduralSurfaceDefinition, SolvedCurveGeometry, SolvedSurfaceGeometry,
        Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{CurveId, ProceduralCurveId, ProceduralSurfaceId, SurfaceId, UnknownId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::units::FinitePoint2;

    use cadmpeg_ir::unknown::UnknownRecord;

    #[test]
    fn geometry_report_records_summary_loss() {
        let scan = crate::test_support::with_service_context(|ctx| {
            crate::container::scan_bytes(
                ctx,
                [crate::container::OUTER_MAGIC.as_slice(), &[0; 8]].concat(),
            )
        })
        .expect("service resource budget");
        let ir = CadIr::empty();
        let counts = super::GeometryReportCounts {
            face_local_freeform: 0,
            unbound_revolution: 0,
            admitted_standard_face_rows: 0,
        };
        let report = super::build_geometry_report(
            &ir,
            &scan,
            &super::TypedCounts::default(),
            (0, 0),
            &counts,
            None,
        );
        assert!(!report.losses.is_empty());
    }

    #[test]
    fn rational_pcurve_arc_preserves_tiny_nonzero_sweep() {
        let range = [0.0, 1e-200];
        let pcurve = rational_pcurve_arc(
            [0.0, 0.0],
            2.0,
            range,
            &mut crate::nurbs::LaneRefusals::new(),
            "test record",
        )
        .expect("tiny circular arc");
        let PcurveGeometry::Nurbs { nurbs } = pcurve else {
            panic!("rational arc must produce NURBS");
        };
        assert_eq!(nurbs.knots().first(), Some(&range[0]));
        assert_eq!(nurbs.knots().last(), Some(&range[1]));
        assert_eq!(nurbs.control_points().len(), 3);
        assert_eq!(nurbs.pole_rows().weights(), Some(vec![1.0, 1.0, 1.0]));
    }

    #[test]
    fn rational_pcurve_arc_rejects_nonfinite_construction() {
        assert!(rational_pcurve_arc(
            [f64::NAN, 0.0],
            1.0,
            [0.0, 1.0],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());
        assert!(rational_pcurve_arc(
            [0.0, 0.0],
            f64::MAX,
            [0.0, 1.0],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());
        assert!(rational_pcurve_arc(
            [0.0, 0.0],
            1.0,
            [1.0, 0.0],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());
    }

    #[test]
    fn surface_circle_branch_preserves_tiny_nonzero_sweep() {
        let sweep = 1e-200_f64;
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ));
        let range = crate::test_support::with_service_context(|ctx| {
            circle_parameter_range_from_surface_branch(
                ctx,
                crate::assemble::CircleParameterRangeFromSurfaceBranchInputs {
                    surface: &surface,
                    center: Point3::new(0.0, 0.0, 0.0),
                    radius: 1.0,
                    axis: Vector3::new(0.0, 0.0, 1.0),
                    ref_direction: Vector3::new(1.0, 0.0, 0.0),
                    start: Point3::new(1.0, 0.0, 0.0),
                    end: Point3::new(sweep.cos(), sweep.sin(), 0.0),
                    pcurve_origin: FinitePoint2::new(Point2::new(1.0, 0.0))
                        .expect("finite pcurve origin"),
                    pcurve_direction: FinitePoint2::new(Point2::new(0.0, sweep))
                        .expect("finite pcurve direction"),
                },
            )
        })
        .expect("circle evaluation resources")
        .expect("tiny circle branch");
        assert_eq!(range, [0.0, sweep]);
    }

    #[test]
    fn surface_circle_branch_rejects_nonfinite_or_degenerate_inputs() {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ));
        let args = || {
            (
                Point3::new(0.0, 0.0, 0.0),
                1.0,
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                FinitePoint2::new(Point2::new(1.0, 0.0)).expect("finite pcurve origin"),
                FinitePoint2::new(Point2::new(0.0, 1.0)).expect("finite pcurve direction"),
            )
        };
        let (center, radius, axis, ref_direction, start, end, pcurve_origin, pcurve_direction) =
            args();
        assert!(crate::test_support::with_service_context(|ctx| {
            circle_parameter_range_from_surface_branch(
                ctx,
                crate::assemble::CircleParameterRangeFromSurfaceBranchInputs {
                    surface: &surface,
                    center: Point3::new(f64::NAN, center.y, center.z),
                    radius,
                    axis,
                    ref_direction,
                    start,
                    end,
                    pcurve_origin,
                    pcurve_direction,
                },
            )
        })
        .expect("circle evaluation resources")
        .is_none());
        assert!(crate::test_support::with_service_context(|ctx| {
            circle_parameter_range_from_surface_branch(
                ctx,
                crate::assemble::CircleParameterRangeFromSurfaceBranchInputs {
                    surface: &surface,
                    center,
                    radius: 0.0,
                    axis,
                    ref_direction,
                    start,
                    end,
                    pcurve_origin,
                    pcurve_direction,
                },
            )
        })
        .expect("circle evaluation resources")
        .is_none());
        assert!(crate::test_support::with_service_context(|ctx| {
            circle_parameter_range_from_surface_branch(
                ctx,
                crate::assemble::CircleParameterRangeFromSurfaceBranchInputs {
                    surface: &surface,
                    center,
                    radius,
                    axis,
                    ref_direction: axis,
                    start,
                    end,
                    pcurve_origin,
                    pcurve_direction,
                },
            )
        })
        .expect("circle evaluation resources")
        .is_none());
    }

    #[test]
    fn angle_unwrap_preserves_tiny_principal_differences() {
        let tiny = 1e-200;
        assert_eq!(crate::assemble::unwrap_angle(tiny, 0.0), tiny);
        assert_eq!(crate::assemble::unwrap_angle(-tiny, 0.0), -tiny);
        assert_eq!(
            crate::assemble::unwrap_angle(std::f64::consts::PI, 0.0),
            -std::f64::consts::PI
        );
    }

    #[test]
    fn angle_unwrap_keeps_finite_branches_when_endpoint_difference_overflows() {
        assert_eq!(
            crate::assemble::unwrap_angle(f64::MAX, -f64::MAX),
            -f64::MAX
        );
        assert_eq!(crate::assemble::unwrap_angle(-f64::MAX, f64::MAX), f64::MAX);
    }

    #[test]
    fn neutral_model_admissibility_rejects_invalid_topology() {
        let mut valid = CadIr::empty();
        assert!(neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut valid,
            &[]
        )
        .expect("resource allocation did not fail"));

        let mut invalid =
            cadmpeg_test_support::admissibility::rejected_missing_region("catia:test")
                .expect("fixture identities are valid");
        assert!(!neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut invalid,
            &[]
        )
        .expect("resource allocation did not fail"));
    }

    /// Phase 5 freeze: shared builders must match the CATIA admission gate.
    #[test]
    fn phase5_freeze_shared_admissibility_fixtures() {
        let mut accepted = cadmpeg_test_support::admissibility::accepted_empty();
        assert!(neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut accepted,
            &[]
        )
        .expect("resource allocation did not fail"));
        let mut rejected =
            cadmpeg_test_support::admissibility::rejected_missing_region("catia:test")
                .expect("fixture identities are valid");
        assert!(!neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut rejected,
            &[]
        )
        .expect("resource allocation did not fail"));
    }

    /// Decimal object-id keys reach the gate in native traversal order, in which
    /// `#10` follows `#9` but precedes it lexicographically. The gate must judge
    /// that arena in the order the pipeline publishes it.
    #[test]
    fn neutral_model_admissibility_canonicalizes_arena_order() {
        let mut ir = CadIr::empty();
        for key in [9_u32, 10] {
            ir.model.curves.push(Curve {
                parameter_range: None,
                id: CurveId::mint(format!("catia:test:curve#{key}")).expect("identity grammar"),
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Line(
                    cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                        Point3::new(0.0, 0.0, f64::from(key)),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid LineCurve fixture"),
                )),
                source_object: None,
            });
        }
        let unsorted = cadmpeg_ir::validate::validate_neutral_with_additional_native_identities(
            &ir,
            std::iter::empty(),
            Vec::new(),
        )
        .expect("resource allocation did not fail");
        assert!(unsorted
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::report::check::Check::ArenaOrder));

        neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut ir,
            &[],
        )
        .expect("resource allocation did not fail");

        assert_eq!(
            ir.model
                .curves
                .iter()
                .map(|curve| curve.id.as_str().to_owned())
                .collect::<Vec<_>>(),
            ["catia:test:curve#10", "catia:test:curve#9"]
        );
        let sorted = cadmpeg_ir::validate::validate_neutral_with_additional_native_identities(
            &ir,
            std::iter::empty(),
            Vec::new(),
        )
        .expect("resource allocation did not fail");
        assert!(!sorted
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::report::check::Check::ArenaOrder));
    }

    #[test]
    // These checked constructors must accept the explicit test fixtures.
    #[allow(clippy::unwrap_used)]
    fn neutral_model_admissibility_includes_pending_unknown_records() {
        let record_id = UnknownId::mint("catia:test:unknown#0").expect("identity grammar");
        let mut ir = CadIr::empty();
        let curve_id = CurveId::mint("catia:test:curve#0").expect("identity grammar");
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: Some(record_id.clone()),
            }),
            source_object: None,
        });
        ir.model
            .add_procedural_curve(
                &curve_id,
                ProceduralCurve::new(
                    ProceduralCurveId::mint("catia:test:procedural-curve#0")
                        .expect("identity grammar"),
                    ProceduralCurveDefinition::Unknown {
                        native_kind: None,
                        record: Some(record_id.clone()),
                        cache: None,
                    },
                ),
            )
            .unwrap();
        let unknowns = [UnknownRecord::retained(
            record_id,
            0,
            Vec::new(),
            Vec::new(),
        )];

        assert!(neutral_model_is_admissible(
            &cadmpeg_test_support::service_decode_context(),
            &mut ir,
            &unknowns
        )
        .expect("resource allocation did not fail"));
    }

    #[test]
    fn unresolved_carrier_accounting_requires_an_exact_construction() {
        let mut ir = CadIr::empty();
        let curve_id =
            CurveId::mint("catia:test:curve#curve-0".to_string()).expect("identity grammar");
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: curve_id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        let surface_id =
            SurfaceId::mint("catia:test:surface#surface-0".to_string()).expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        let offset_id =
            SurfaceId::mint("catia:test:surface#surface-1".to_string()).expect("identity grammar");
        ir.model.surfaces.push(Surface {
            id: offset_id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { record: None }),
            source_object: None,
        });
        assert_eq!(unresolved_carrier_counts(&ir), (1, 2));

        ir.model
            .add_procedural_curve(
                &curve_id,
                ProceduralCurve::new(
                    ProceduralCurveId::mint(
                        "catia:test:proceduralcurve#procedural-curve-0".to_string(),
                    )
                    .expect("identity grammar"),
                    ProceduralCurveDefinition::Unknown {
                        native_kind: None,
                        record: Some(
                            UnknownId::mint("catia:test:unknown#record-0".to_string())
                                .expect("identity grammar"),
                        ),
                        cache: None,
                    },
                ),
            )
            .expect("attach construction to its fixture carrier");
        ir.model
            .add_procedural_surface(
                &surface_id,
                ProceduralSurface::new(
                    ProceduralSurfaceId::mint(
                        "catia:test:proceduralsurface#procedural-surface-0".to_string(),
                    )
                    .expect("identity grammar"),
                    ProceduralSurfaceDefinition::Unknown {
                        record: Some(
                            UnknownId::mint("catia:test:unknown#record-1".to_string())
                                .expect("identity grammar"),
                        ),
                        cache: None,
                    },
                    None,
                ),
            )
            .expect("attach construction to its fixture carrier");
        ir.model
            .add_procedural_surface(
                &offset_id,
                cadmpeg_ir::geometry::surface_payloads::OffsetSurfaceConstruction::try_new(
                    surface_id,
                    2.0,
                    Some(1),
                    Some(1),
                    false,
                    cadmpeg_ir::geometry::OffsetExtension::Legacy {
                        flags: cadmpeg_ir::geometry::LegacyExtensionFlags::Absent {},
                        cache: None,
                    },
                )
                .map(|admitted_payload| {
                    ProceduralSurface::new(
                        ProceduralSurfaceId::mint(
                            "catia:test:proceduralsurface#procedural-surface-1".to_string(),
                        )
                        .expect("identity grammar"),
                        ProceduralSurfaceDefinition::Offset(admitted_payload),
                        None,
                    )
                })
                .expect("valid ProceduralSurface fixture"),
            )
            .expect("attach construction to its fixture carrier");
        assert_eq!(unresolved_carrier_counts(&ir), (1, 2));

        ir.model.procedural_curves[0]
            .replace_definition(ProceduralCurveDefinition::Exact { cache: None });
        ir.model.procedural_surfaces[0].edit_definition(|definition| {
            *definition = ProceduralSurfaceDefinition::Exact(
                cadmpeg_ir::geometry::surface_payloads::ExactSurfacePayload::try_new(
                    cadmpeg_ir::geometry::ExactSpline::Legacy {
                        ranges: [[0.0, 1.0], [0.0, 1.0]],
                        extension: 0,
                        cache: None,
                    },
                )
                .expect("finite ordered exact-spline fixture ranges"),
            );
        });
        assert_eq!(unresolved_carrier_counts(&ir), (0, 0));
    }

    #[test]
    fn annotation_result() {
        let annotations = {
            let mut builder = cadmpeg_ir::AnnotationBuilder::new();
            super::annotate(
                &mut builder,
                "catia:test:curve#0",
                "CATPart",
                17,
                format_args!("record:{:08x}", 7),
                cadmpeg_ir::Exactness::Derived,
            )
            .expect("service profile admits annotation");
            builder.build()
        };
        let note = &annotations.provenance["catia:test:curve#0"];
        assert_eq!(note.stream(), "catia:CATPart");
        assert_eq!(note.offset, 17);
        assert_eq!(note.tag.as_deref(), Some("record:00000007"));
    }

    #[test]
    fn container_report_losses_result() {
        let scan = crate::test_support::with_service_context(|ctx| {
            crate::container::scan_bytes(
                ctx,
                crate::test_support::test_container::standard_catpart(),
            )
        })
        .expect("service profile admits container scan");
        let report = super::build_container_report(&scan);
        assert_eq!(report.losses.len(), 2);
        assert!(report.losses[0]
            .message
            .contains("No B-rep geometry was transferred"));
        assert!(report.losses[1].message.contains("topology graph"));
    }

    #[test]
    fn payload_carrier_links_result() {
        let mut ir = CadIr::empty();
        let id = CurveId::mint("catia:test:curve#link".to_string()).expect("identity grammar");
        ir.model.curves.push(Curve {
            parameter_range: None,
            id: id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        let run = |_: &cadmpeg_core::decode::DecodeContext<'_>| {
            let mut payload = UnknownRecord::retained(
                UnknownId::mint("catia:test:unknown#payload".to_string())
                    .expect("identity grammar"),
                0,
                Vec::new(),
                Vec::new(),
            );
            let mut annotations = cadmpeg_ir::AnnotationBuilder::new();
            super::link_payload_carriers(&ir, &mut payload, &mut annotations)?;
            Ok::<_, cadmpeg_core::CodecError>(payload.links().to_vec())
        };
        assert_eq!(
            crate::test_support::with_service_context(run).expect("service resource budget"),
            [id.as_str()]
        );
    }

    #[test]
    fn unresolved_carrier_inventory_and_note_result() {
        let mut ir = CadIr::empty();
        let id =
            CurveId::mint("catia:test:curve#unresolved".to_string()).expect("identity grammar");
        ir.model.curves.push(Curve {
            parameter_range: None,
            id,
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
            source_object: None,
        });
        let mut losses = Vec::new();
        super::insert_unresolved_carrier_loss(&ir, &mut losses);
        assert_eq!(losses.len(), 1);
    }

    #[test]
    fn source_metadata_result() {
        let scan = crate::test_support::with_service_context(|ctx| {
            crate::container::scan_bytes(
                ctx,
                crate::test_support::test_container::standard_catpart(),
            )
        })
        .expect("service budget admits container scan");
        let matched =
            crate::dialect::classify(&scan).expect("service budget admits dialect classification");
        let source = source_meta(&scan, &matched).expect("service budget admits source metadata");
        assert_eq!(source.attributes["file_size"], scan.data.len().to_string());
    }

    #[test]
    fn cgm_source_object_identity_result() {
        let source =
            super::cgm_source("surface", 0x1234).expect("service profile admits source identity");
        assert_eq!(source.object_id.as_str(), "cgm-surface:001234");
        let key = super::cgm_source_key("frame", format_args!("{:010}", 23))
            .expect("service profile admits frame identity");
        assert_eq!(key.object_id.as_str(), "cgm-frame:0000000023");
    }
}
