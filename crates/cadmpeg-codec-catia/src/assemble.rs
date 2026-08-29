// SPDX-License-Identifier: Apache-2.0
//! Shared emit scaffolding used by two or more family decode routes.
//!
//! Byte-provenance annotation, raw-payload preservation, unresolved-carrier
//! loss accounting, neutral-model admissibility, source metadata, generic
//! vector/range helpers, and the metadata/geometry/container report builders.

use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::geometry::{
    CurveGeometry, PcurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::sha256_hex;
use cadmpeg_ir::ids::{BodyId, RegionId, ShellId, UnknownId};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::report::{DecodeReport, LossNote};
use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::SourceObjectAssociation;
use std::collections::{BTreeMap, HashSet};

use crate::container::{self, ContainerScan};
use crate::loss::CatiaLossCode;

pub(crate) fn cgm_source(kind: &str, tag: u32) -> SourceObjectAssociation {
    cgm_source_key(kind, format!("{tag:06x}"))
}

pub(crate) fn cgm_source_key(kind: &str, key: impl std::fmt::Display) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: "catia".to_string(),
        object_id: format!("cgm-{kind}:{key}"),
        name: None,
        color: None,
        visible: None,
        layer: None,
        instance_path: Vec::new(),
    }
}

pub(crate) fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    stream_name: &str,
    offset: u64,
    tag: impl Into<String>,
    exactness: Exactness,
) {
    let id = id.to_string();
    let stream = annotations.stream(format!("catia:{stream_name}"));
    annotations.note(&id, stream, offset).tag(tag);
    annotations.exactness(id, exactness);
}

/// Judge one candidate neutral model after canonicalizing arena order.
///
/// Matches [`DecodeResult::new`](cadmpeg_ir::codec::DecodeResult::new), which
/// sorts arenas by entity id before a document leaves the codec. Admission uses
/// [`cadmpeg_ir::CATIA_ADMISSION_CHECKS`], not full final-document validation.
pub(crate) fn neutral_model_is_admissible(
    ir: &mut CadIr,
    pending_unknowns: &[UnknownRecord],
) -> bool {
    ir.model.finalize();
    cadmpeg_ir::admit_with_additional_native_identities(
        ir,
        pending_unknowns.iter().map(|record| record.id.as_str()),
        cadmpeg_ir::CATIA_ADMISSION_CHECKS,
        Vec::new(),
    )
    .is_ok()
}

pub(crate) fn unresolved_carrier_counts(ir: &CadIr) -> (usize, usize) {
    let mut resolved_curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| !matches!(curve.geometry, CurveGeometry::Unknown { .. }))
        .map(|curve| curve.id.clone())
        .collect::<HashSet<_>>();
    let mut resolved_surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| !matches!(surface.geometry, SurfaceGeometry::Unknown { .. }))
        .map(|surface| surface.id.clone())
        .collect::<HashSet<_>>();
    loop {
        let mut changed = false;
        for procedural in &ir.model.procedural_surfaces {
            let resolved = match &procedural.definition {
                ProceduralSurfaceDefinition::Exact { .. }
                | ProceduralSurfaceDefinition::Helix { .. }
                | ProceduralSurfaceDefinition::RollingBallJet { .. } => true,
                ProceduralSurfaceDefinition::Offset { support, .. } => {
                    resolved_surfaces.contains(support)
                }
                ProceduralSurfaceDefinition::Revolution { directrix, .. } => {
                    resolved_curves.contains(directrix)
                }
                ProceduralSurfaceDefinition::Extrusion { directrix, .. }
                | ProceduralSurfaceDefinition::LinearSweep { directrix, .. } => {
                    resolved_curves.contains(directrix)
                }
                _ => false,
            };
            if resolved {
                changed |= resolved_surfaces.insert(procedural.surface.clone());
            }
        }
        for procedural in &ir.model.procedural_curves {
            let resolved = match &procedural.definition {
                ProceduralCurveDefinition::Exact | ProceduralCurveDefinition::Helix { .. } => true,
                ProceduralCurveDefinition::Intersection { context, .. } => {
                    context.sides.iter().all(|side| {
                        side.surface
                            .as_ref()
                            .is_some_and(|surface| resolved_surfaces.contains(surface))
                    })
                }
                ProceduralCurveDefinition::SurfaceCurve { context, .. } => {
                    let (has_side, all_resolved) = context
                        .sides
                        .iter()
                        .filter_map(|side| side.surface.as_ref().zip(side.pcurve.as_ref()))
                        .fold((false, true), |(_, all_resolved), (surface, _)| {
                            (true, all_resolved && resolved_surfaces.contains(surface))
                        });
                    has_side && all_resolved
                }
                _ => false,
            };
            if resolved {
                changed |= resolved_curves.insert(procedural.curve.clone());
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
            matches!(curve.geometry, CurveGeometry::Unknown { .. })
                && !resolved_curves.contains(&curve.id)
        })
        .count()
        + ir.model
            .edges
            .iter()
            .filter(|edge| edge.curve.is_none())
            .count();
    let surfaces = ir
        .model
        .surfaces
        .iter()
        .filter(|surface| {
            matches!(surface.geometry, SurfaceGeometry::Unknown { .. })
                && !resolved_surfaces.contains(&surface.id)
        })
        .count();
    (curves, surfaces)
}

pub(crate) fn insert_unresolved_carrier_loss(ir: &CadIr, losses: &mut Vec<LossNote>) {
    let (unresolved_curves, unresolved_surfaces) = unresolved_carrier_counts(ir);
    if unresolved_curves == 0 && unresolved_surfaces == 0 {
        return;
    }
    losses.insert(
        0,
        CatiaLossCode::GeometryUnresolvedCarriers.note(format!(
            "The transferred model retains {unresolved_curves} unresolved curve carriers and {unresolved_surfaces} unresolved surface carriers without exact procedural constructions."
        )),
    );
}

pub(crate) fn attach_free_vertices(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    namespace: &str,
    stream: &str,
) {
    let body_id = BodyId(format!("catia:{namespace}:body#unbound-points"));
    let region_id = RegionId(format!("catia:{namespace}:region#unbound-points"));
    let shell_id = ShellId(format!("catia:{namespace}:shell#unbound-points"));
    for id in [&body_id.0, &region_id.0, &shell_id.0] {
        annotate(
            annotations,
            id,
            stream,
            0,
            "unbound_point_owner",
            Exactness::Inferred,
        );
    }
    ir.model.bodies.push(Body {
        id: body_id.clone(),
        kind: BodyKind::Wire,
        regions: vec![region_id.clone()],
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    ir.model.regions.push(Region {
        id: region_id.clone(),
        body: body_id,
        shells: vec![shell_id.clone()],
    });
    ir.model.shells.push(Shell {
        id: shell_id,
        region: region_id,
        faces: Vec::new(),
        wire_edges: Vec::new(),
        free_vertices: ir
            .model
            .vertices
            .iter()
            .map(|vertex| vertex.id.clone())
            .collect(),
    });
}

pub(crate) fn ordered_range(range: [f64; 2]) -> [f64; 2] {
    if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn circle_parameter_range_from_surface_branch(
    surface: &SurfaceGeometry,
    center: Point3,
    radius: f64,
    axis: Vector3,
    ref_direction: Vector3,
    start: Point3,
    end: Point3,
    pcurve_origin: Point2,
    pcurve_direction: Point2,
) -> Option<[f64; 2]> {
    let finite_point = |point: Point3| [point.x, point.y, point.z].into_iter().all(f64::is_finite);
    let finite_vector = |vector: Vector3| {
        [vector.x, vector.y, vector.z]
            .into_iter()
            .all(f64::is_finite)
    };
    if !finite_point(center)
        || !finite_point(start)
        || !finite_point(end)
        || !finite_vector(axis)
        || !finite_vector(ref_direction)
        || !pcurve_origin.u.is_finite()
        || !pcurve_origin.v.is_finite()
        || !pcurve_direction.u.is_finite()
        || !pcurve_direction.v.is_finite()
        || !radius.is_finite()
        || radius <= 0.0
    {
        return None;
    }
    let tangent = axis.cross(ref_direction);
    if !finite_vector(tangent)
        || tangent.x.hypot(tangent.y).hypot(tangent.z) == 0.0
        || ref_direction
            .x
            .hypot(ref_direction.y)
            .hypot(ref_direction.z)
            == 0.0
    {
        return None;
    }
    let angle = |point: Point3| {
        let offset = point.vector_from(center);
        offset.dot(tangent).atan2(offset.dot(ref_direction))
    };
    let start = angle(start);
    let end = angle(end);
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    let short_end = unwrap_angle(end, start);
    if !short_end.is_finite() {
        return None;
    }
    let delta = short_end - start;
    if !delta.is_finite() || delta == 0.0 {
        return None;
    }
    let long_end = short_end - delta.signum() * std::f64::consts::TAU;
    if !long_end.is_finite() {
        return None;
    }
    let midpoint_uv = Point2::new(
        pcurve_origin.u + 0.5 * pcurve_direction.u,
        pcurve_origin.v + 0.5 * pcurve_direction.v,
    );
    if !midpoint_uv.u.is_finite() || !midpoint_uv.v.is_finite() {
        return None;
    }
    let surface_midpoint = cadmpeg_ir::eval::surface_point(surface, midpoint_uv.u, midpoint_uv.v)?;
    if !finite_point(surface_midpoint) {
        return None;
    }
    let candidates = [short_end, long_end]
        .into_iter()
        .filter(|end| {
            let parameter = 0.5 * (start + end);
            if !parameter.is_finite() {
                return false;
            }
            let circle_midpoint = Point3::new(
                center.x
                    + radius * (parameter.cos() * ref_direction.x + parameter.sin() * tangent.x),
                center.y
                    + radius * (parameter.cos() * ref_direction.y + parameter.sin() * tangent.y),
                center.z
                    + radius * (parameter.cos() * ref_direction.z + parameter.sin() * tangent.z),
            );
            if !finite_point(circle_midpoint) {
                return false;
            }
            let distance_squared = circle_midpoint.distance_squared(surface_midpoint);
            distance_squared.is_finite() && distance_squared.sqrt() <= 2e-3
        })
        .collect::<Vec<_>>();
    let [end] = <[f64; 1]>::try_from(candidates).ok()?;
    (end.is_finite() && end != start).then_some([start, end])
}

pub(crate) fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector.x.hypot(vector.y).hypot(vector.z);
    if !norm.is_finite() || norm == 0.0 {
        return None;
    }
    let unit = vector.scale(1.0 / norm);
    [unit.x, unit.y, unit.z]
        .into_iter()
        .all(f64::is_finite)
        .then_some(unit)
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
            SurfaceGeometry::Plane { .. } => self.plane += 1,
            SurfaceGeometry::Cylinder { .. } => self.cylinder += 1,
            SurfaceGeometry::Cone { .. } => self.cone += 1,
            SurfaceGeometry::Sphere { .. } => self.sphere += 1,
            SurfaceGeometry::Torus { .. } => self.torus += 1,
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

pub(crate) fn source_meta(scan: &ContainerScan) -> SourceMeta {
    let mut attributes = BTreeMap::new();
    attributes.insert("variant".to_string(), scan.variant.token().to_string());
    attributes.insert("file_size".to_string(), scan.data.len().to_string());
    attributes.insert(
        "outer_dir_offset".to_string(),
        scan.outer_dir_offset.to_string(),
    );
    if let Some(dir) = &scan.inner {
        attributes.insert("inner_offset".to_string(), dir.inner.to_string());
        attributes.insert(
            "stream_count".to_string(),
            dir.descriptors.len().to_string(),
        );
    }
    if let Some(brep) = &scan.brep {
        attributes.insert("brep_stream_len".to_string(), brep.len().to_string());
        attributes.insert("brep_stream_sha256".to_string(), sha256_hex(brep));
        attributes.insert("fbb_runs".to_string(), scan.census.fbb_runs.to_string());
        attributes.insert(
            "fbb_face_rows".to_string(),
            scan.census.fbb_face_rows.to_string(),
        );
        attributes.insert(
            "vertex_records".to_string(),
            scan.census.vertex_markers.to_string(),
        );
    }
    attributes.insert("preview_count".to_string(), scan.previews.len().to_string());
    for (index, preview) in scan.previews.iter().enumerate() {
        attributes.insert(format!("preview_{index}_width"), preview.width.to_string());
        attributes.insert(
            format!("preview_{index}_height"),
            preview.height.to_string(),
        );
        attributes.insert(
            format!("preview_{index}_components"),
            preview.components.to_string(),
        );
    }
    if let Some(version) = &scan.last_save_version {
        attributes.insert("catia_version".to_string(), version.version.to_string());
        attributes.insert("catia_release".to_string(), version.release.to_string());
        attributes.insert(
            "catia_service_pack".to_string(),
            version.service_pack.to_string(),
        );
        attributes.insert("catia_hot_fix".to_string(), version.hot_fix.to_string());
        attributes.insert("catia_build_date".to_string(), version.build_date.clone());
    }
    attributes.insert(
        "external_reference_count".to_string(),
        scan.external_references.len().to_string(),
    );
    for (index, reference) in scan.external_references.iter().enumerate() {
        attributes.insert(
            format!("external_reference_{index}"),
            reference.target.clone(),
        );
    }
    attributes.insert(
        "finjpl_segment_count".to_string(),
        scan.finjpl_segments.len().to_string(),
    );
    for (index, segment) in scan.finjpl_segments.iter().enumerate() {
        if let Some(name) = &segment.name {
            attributes.insert(format!("finjpl_segment_{index}_name"), name.clone());
        }
        attributes.insert(
            format!("finjpl_segment_{index}_type"),
            format!("0x{:08x}", segment.type_word),
        );
    }
    SourceMeta {
        format: crate::dialect::FORMAT.to_string(),
        attributes,
        ..Default::default()
    }
}

pub(crate) fn build_geometry_report(
    ir: &CadIr,
    scan: &ContainerScan,
    typed: &TypedCounts,
    plane_faces: usize,
    analytic_record_count: usize,
    report_counts: &GeometryReportCounts,
    topology_failure: Option<&str>,
) -> DecodeReport {
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
    let withheld_face_rows = scan
        .census
        .fbb_face_rows
        .saturating_sub(report_counts.admitted_standard_face_rows);
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

    let invalid_analytic = analytic_record_count.saturating_sub(typed.total() + plane_faces);
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
                  document metadata are not yet transferred.",
        ),
    );

    DecodeReport {
        dialects: Some(
            cadmpeg_core::dialect::DialectLayers::new(crate::dialect::classify(scan), Vec::new())
                .expect("a primary layer without extras is valid"),
        ),
        format: "catia".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: container::summarize(scan).notes,
    }
}

pub(crate) fn build_metadata_ir(
    scan: &ContainerScan,
) -> (CadIr, cadmpeg_ir::Annotations, Vec<UnknownRecord>) {
    let mut ir = CadIr::empty(Units::default());
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    ir.source = Some(source_meta(scan));

    // Preserve the reconstructed BREP stream (or, absent one, the whole file) as
    // an unknown passthrough so no recognized data is silently dropped.
    if let Some(brep) = &scan.brep {
        let id = UnknownId("catia:payload:unknown#brep-stream".to_string());
        annotate(
            &mut annotations,
            &id,
            "MainDataStream+SurfacicReps",
            0,
            scan.variant.token(),
            Exactness::Unknown,
        );
        unknowns.push(UnknownRecord {
            id,
            offset: 0,
            byte_len: brep.len() as u64,
            sha256: sha256_hex(brep),
            data: Some(brep.clone()),
            links: Vec::new(),
        });
    }
    (ir, annotations.build(), unknowns)
}

/// Preserve the native payload for every partial decode.  Typed entities are
/// additive views; unrecovered record families must remain byte-addressable.
pub(crate) fn preserve_raw_payload(
    unknowns: &mut Vec<UnknownRecord>,
    annotations: &mut AnnotationBuilder,
    scan: &ContainerScan,
    id: &str,
) {
    let (bytes, stream) = match scan.brep.as_ref() {
        Some(brep) => (brep.as_slice(), "MainDataStream+SurfacicReps"),
        None => (scan.data.as_ref(), "CATPart"),
    };
    let id = UnknownId(id.to_string());
    annotate(
        annotations,
        &id,
        stream,
        0,
        scan.variant.token(),
        Exactness::Unknown,
    );
    unknowns.push(UnknownRecord {
        id,
        offset: 0,
        byte_len: bytes.len() as u64,
        sha256: sha256_hex(bytes),
        data: Some(bytes.to_vec()),
        links: Vec::new(),
    });
}

/// Attribute typed carrier views to the preserved payload when CATIA's binding
/// layer was not recovered. The raw payload is their byte-backed owner; this
/// avoids inventing topology or procedural relationships.
pub(crate) fn link_payload_carriers(
    ir: &CadIr,
    unknowns: &mut [UnknownRecord],
    annotations: &mut AnnotationBuilder,
) {
    let links = ir
        .model
        .surfaces
        .iter()
        .map(|surface| surface.id.0.clone())
        .chain(ir.model.curves.iter().map(|curve| curve.id.0.clone()))
        .collect::<Vec<_>>();
    if links.is_empty() {
        return;
    }
    let payload = unknowns
        .last_mut()
        .expect("partial CATIA decode preserves its source payload");
    payload.links = links;
    annotations.derived(&payload.id, "links");
}

pub(crate) fn build_container_report(scan: &ContainerScan, container_only: bool) -> DecodeReport {
    let summary = container::summarize(scan);
    let mut losses = vec![CatiaLossCode::GeometryBrepNotTransferred.note(format!(
        "No B-rep geometry was transferred. This file's storage variant is `{}` ({}); the \
         applicable decoded record families transfer geometry in this codec.",
        scan.variant.token(),
        scan.variant.description()
    ))];

    if container_only {
        losses.push(
            CatiaLossCode::ContainerOnlyDecode
                .note("Container-only decode requested; entity decode was not attempted."),
        );
    }

    losses.push(CatiaLossCode::TopologyGraphNotBuilt.note(
        "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built \
                  for this file.",
    ));

    DecodeReport {
        dialects: Some(
            cadmpeg_core::dialect::DialectLayers::new(crate::dialect::classify(scan), Vec::new())
                .expect("a primary layer without extras is valid"),
        ),
        format: "catia".to_string(),
        container_only,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        transfer_ledger: cadmpeg_ir::report::TransferLedger::default(),
        losses,
        notes: summary.notes,
    }
}

pub(crate) fn unwrap_angle(value: f64, reference: f64) -> f64 {
    let delta = value - reference;
    if (-std::f64::consts::PI..std::f64::consts::PI).contains(&delta) {
        value
    } else {
        reference + (delta + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI
    }
}

pub(crate) fn rational_pcurve_arc(
    center: [f64; 2],
    radius: f64,
    range: [f64; 2],
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
    if !segment_count.is_finite() || segment_count > crate::MAX_EXACT_ARC_SPANS as f64 {
        return None;
    }
    let segment_count = (segment_count as usize).max(1);
    let control_count = segment_count.checked_mul(2)?.checked_add(1)?;
    let step = span / segment_count as f64;
    let mut control_points = Vec::with_capacity(control_count);
    let mut weights = Vec::with_capacity(control_count);
    let mut knots = vec![range[0]; 3];
    for index in 0..segment_count {
        let start = range[0] + index as f64 * step;
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
            .all(|point| [point.u, point.v].into_iter().all(f64::is_finite))
        || !weights.iter().copied().all(f64::is_finite)
    {
        return None;
    }
    Some(PcurveGeometry::Nurbs {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    })
}

pub(crate) fn quintic_jet_pcurve(
    degree: u32,
    knots: &[f64],
    points: &[[f64; 2]],
    first: &[[f64; 2]],
    second: &[[f64; 2]],
) -> Option<PcurveGeometry> {
    let (full_knots, controls) =
        crate::nurbs::quintic_jet_bspline(degree, knots, points, first, second)?;
    Some(PcurveGeometry::Nurbs {
        degree,
        knots: full_knots,
        control_points: controls
            .into_iter()
            .map(|point| Point2::new(point[0], point[1]))
            .collect(),
        weights: None,
        periodic: false,
    })
}

#[cfg(test)]
mod route_tests {
    use crate::assemble::{
        circle_parameter_range_from_surface_branch, neutral_model_is_admissible,
        rational_pcurve_arc, unresolved_carrier_counts,
    };

    use cadmpeg_ir::document::CadIr;

    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, PcurveGeometry, ProceduralCurve, ProceduralCurveDefinition,
        ProceduralSurface, ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CurveId, ProceduralCurveId, ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId,
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    use cadmpeg_ir::topology::Shell;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::unknown::UnknownRecord;

    #[test]
    fn rational_pcurve_arc_preserves_tiny_nonzero_sweep() {
        let range = [0.0, 1e-200];
        let pcurve = rational_pcurve_arc([0.0, 0.0], 2.0, range).expect("tiny circular arc");
        let PcurveGeometry::Nurbs {
            knots,
            control_points,
            weights,
            ..
        } = pcurve
        else {
            panic!("rational arc must produce NURBS");
        };
        assert_eq!(knots.first(), Some(&range[0]));
        assert_eq!(knots.last(), Some(&range[1]));
        assert_eq!(control_points.len(), 3);
        assert_eq!(weights, Some(vec![1.0, 1.0, 1.0]));
    }

    #[test]
    fn rational_pcurve_arc_rejects_nonfinite_construction() {
        assert!(rational_pcurve_arc([f64::NAN, 0.0], 1.0, [0.0, 1.0]).is_none());
        assert!(rational_pcurve_arc([0.0, 0.0], f64::MAX, [0.0, 1.0]).is_none());
        assert!(rational_pcurve_arc([0.0, 0.0], 1.0, [1.0, 0.0]).is_none());
    }

    #[test]
    fn surface_circle_branch_preserves_tiny_nonzero_sweep() {
        let sweep = 1e-200_f64;
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let range = circle_parameter_range_from_surface_branch(
            &surface,
            Point3::new(0.0, 0.0, 0.0),
            1.0,
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(sweep.cos(), sweep.sin(), 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, sweep),
        )
        .expect("tiny circle branch");
        assert_eq!(range, [0.0, sweep]);
    }

    #[test]
    fn surface_circle_branch_rejects_nonfinite_or_degenerate_inputs() {
        let surface = SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        };
        let args = || {
            (
                Point3::new(0.0, 0.0, 0.0),
                1.0,
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                Point3::new(1.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point2::new(1.0, 0.0),
                Point2::new(0.0, 1.0),
            )
        };
        let (center, radius, axis, ref_direction, start, end, pcurve_origin, pcurve_direction) =
            args();
        assert!(circle_parameter_range_from_surface_branch(
            &surface,
            Point3::new(f64::NAN, center.y, center.z),
            radius,
            axis,
            ref_direction,
            start,
            end,
            pcurve_origin,
            pcurve_direction,
        )
        .is_none());
        assert!(circle_parameter_range_from_surface_branch(
            &surface,
            center,
            0.0,
            axis,
            ref_direction,
            start,
            end,
            pcurve_origin,
            pcurve_direction,
        )
        .is_none());
        assert!(circle_parameter_range_from_surface_branch(
            &surface,
            center,
            radius,
            axis,
            axis,
            start,
            end,
            pcurve_origin,
            pcurve_direction,
        )
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
    fn unit_vector_preserves_tiny_finite_direction() {
        assert_eq!(
            crate::assemble::unit_vector(cadmpeg_ir::math::Vector3::new(1e-200, 0.0, 0.0)),
            Some(cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0))
        );
        assert_eq!(
            crate::assemble::unit_vector(cadmpeg_ir::math::Vector3::new(0.0, 0.0, 0.0)),
            None
        );
        assert_eq!(
            crate::assemble::unit_vector(cadmpeg_ir::math::Vector3::new(
                f64::from_bits(1),
                0.0,
                0.0,
            )),
            None
        );
    }

    #[test]
    fn neutral_model_admissibility_rejects_invalid_topology() {
        let mut valid = CadIr::empty(Units::default());
        assert!(neutral_model_is_admissible(&mut valid, &[]));

        let mut invalid = CadIr::empty(Units::default());
        invalid.model.shells.push(Shell {
            id: ShellId("catia:test:shell#invalid".into()),
            region: RegionId("catia:test:region#missing".into()),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        assert!(!neutral_model_is_admissible(&mut invalid, &[]));
    }

    /// Phase 5 freeze: shared builders must match the CATIA admission gate.
    #[test]
    fn phase5_freeze_shared_admissibility_fixtures() {
        let mut accepted = cadmpeg_ir::validate::admissibility_freeze::accepted_empty();
        assert!(neutral_model_is_admissible(&mut accepted, &[]));
        let mut rejected =
            cadmpeg_ir::validate::admissibility_freeze::rejected_missing_region("catia:test");
        assert!(!neutral_model_is_admissible(&mut rejected, &[]));
    }

    /// Decimal object-id keys reach the gate in native traversal order, in which
    /// `#10` follows `#9` but precedes it lexicographically. The gate must judge
    /// that arena in the order the pipeline publishes it.
    #[test]
    fn neutral_model_admissibility_canonicalizes_arena_order() {
        let mut ir = CadIr::empty(Units::default());
        for key in [9_u32, 10] {
            ir.model.curves.push(Curve {
                id: CurveId(format!("catia:test:curve#{key}")),
                geometry: CurveGeometry::Line {
                    origin: Point3::new(0.0, 0.0, f64::from(key)),
                    direction: Vector3::new(1.0, 0.0, 0.0),
                },
                source_object: None,
            });
        }
        let unsorted = cadmpeg_ir::validate::validate_neutral_with_additional_native_identities(
            &ir,
            std::iter::empty(),
            Vec::new(),
        );
        assert!(unsorted
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::report::Check::ArenaOrder));

        neutral_model_is_admissible(&mut ir, &[]);

        assert_eq!(
            ir.model
                .curves
                .iter()
                .map(|curve| curve.id.0.clone())
                .collect::<Vec<_>>(),
            ["catia:test:curve#10", "catia:test:curve#9"]
        );
        let sorted = cadmpeg_ir::validate::validate_neutral_with_additional_native_identities(
            &ir,
            std::iter::empty(),
            Vec::new(),
        );
        assert!(!sorted
            .findings
            .iter()
            .any(|finding| finding.check == cadmpeg_ir::report::Check::ArenaOrder));
    }

    #[test]
    fn neutral_model_admissibility_includes_pending_unknown_records() {
        let record_id = UnknownId("catia:test:unknown#0".into());
        let mut ir = CadIr::empty(Units::default());
        let curve_id = CurveId("catia:test:curve#0".into());
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Unknown {
                record: Some(record_id.clone()),
            },
            source_object: None,
        });
        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId("catia:test:procedural-curve#0".into()),
            curve: curve_id,
            definition: ProceduralCurveDefinition::Unknown {
                native_kind: None,
                record: Some(record_id.clone()),
            },
            cache_fit_tolerance: None,
        });
        let unknowns = [UnknownRecord {
            id: record_id,
            offset: 0,
            byte_len: 0,
            sha256: String::new(),
            data: Some(Vec::new()),
            links: Vec::new(),
        }];

        assert!(neutral_model_is_admissible(&mut ir, &unknowns));
    }

    #[test]
    fn unresolved_carrier_accounting_requires_an_exact_construction() {
        let mut ir = CadIr::empty(Units::default());
        let curve_id = CurveId("curve-0".to_string());
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry: CurveGeometry::Unknown { record: None },
            source_object: None,
        });
        let surface_id = SurfaceId("surface-0".to_string());
        ir.model.surfaces.push(Surface {
            id: surface_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        let offset_id = SurfaceId("surface-1".to_string());
        ir.model.surfaces.push(Surface {
            id: offset_id.clone(),
            geometry: SurfaceGeometry::Unknown { record: None },
            source_object: None,
        });
        assert_eq!(unresolved_carrier_counts(&ir), (1, 2));

        ir.model.procedural_curves.push(ProceduralCurve {
            id: ProceduralCurveId("procedural-curve-0".to_string()),
            curve: curve_id,
            definition: ProceduralCurveDefinition::Unknown {
                native_kind: None,
                record: Some(UnknownId("record-0".to_string())),
            },
            cache_fit_tolerance: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId("procedural-surface-0".to_string()),
            surface: surface_id.clone(),
            definition: ProceduralSurfaceDefinition::Unknown {
                record: Some(UnknownId("record-1".to_string())),
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        ir.model.procedural_surfaces.push(ProceduralSurface {
            id: ProceduralSurfaceId("procedural-surface-1".to_string()),
            surface: offset_id,
            definition: ProceduralSurfaceDefinition::Offset {
                support: surface_id,
                distance: 2.0,
                u_sense: Some(1),
                v_sense: Some(1),
                support_extension: None,
                extension_flags: Vec::new(),
                revision_form: None,
            },
            cache_fit_tolerance: None,
            record_bounds: None,
        });
        assert_eq!(unresolved_carrier_counts(&ir), (1, 2));

        ir.model.procedural_curves[0].definition = ProceduralCurveDefinition::Exact;
        ir.model.procedural_surfaces[0].definition = ProceduralSurfaceDefinition::Exact {
            parameters: cadmpeg_ir::geometry::SplineSurfaceParameters::OrderedRanges {
                ranges: [[0.0, 1.0], [0.0, 1.0]],
            },
            extension: 0,
            revision_form: None,
        };
        assert_eq!(unresolved_carrier_counts(&ir), (0, 0));
    }
}
