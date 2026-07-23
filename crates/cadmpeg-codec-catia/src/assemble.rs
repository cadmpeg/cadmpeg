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
use cadmpeg_ir::report::{DecodeReport, LossCategory, LossNote, Severity};
use cadmpeg_ir::topology::{Body, BodyKind, Region, Shell};
use cadmpeg_ir::units::Units;
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use cadmpeg_ir::SourceObjectAssociation;
use std::collections::{BTreeMap, HashSet};

use crate::container::{self, ContainerScan};

pub(crate) fn cgm_source(kind: &str, tag: u32) -> SourceObjectAssociation {
    SourceObjectAssociation {
        format: "catia".to_string(),
        object_id: format!("cgm-{kind}:{tag:06x}"),
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

pub(crate) fn neutral_model_is_admissible(ir: &CadIr, pending_unknowns: &[UnknownRecord]) -> bool {
    let mut candidate = ir.clone();
    let native_unknowns = pending_unknowns
        .iter()
        .map(cadmpeg_ir::NativeUnknownRecord::from)
        .collect::<Vec<_>>();
    if candidate
        .set_native_unknowns("catia", &native_unknowns)
        .is_err()
    {
        return false;
    }
    candidate.finalize();
    cadmpeg_ir::validate::validate(&candidate, Vec::new()).is_ok()
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
        LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Blocking,
            message: format!(
                "The transferred model retains {unresolved_curves} unresolved curve carriers and {unresolved_surfaces} unresolved surface carriers without exact procedural constructions."
            ),
            provenance: None,
        },
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
    let tangent = axis.cross(ref_direction);
    let angle = |point: Point3| {
        let offset = point.vector_from(center);
        offset.dot(tangent).atan2(offset.dot(ref_direction))
    };
    let start = angle(start);
    let short_end = unwrap_angle(angle(end), start);
    let delta = short_end - start;
    if delta.abs() <= 1e-9 {
        return None;
    }
    let long_end = short_end - delta.signum() * std::f64::consts::TAU;
    let surface_midpoint = cadmpeg_ir::eval::surface_point(
        surface,
        pcurve_origin.u + 0.5 * pcurve_direction.u,
        pcurve_origin.v + 0.5 * pcurve_direction.v,
    )?;
    let candidates = [short_end, long_end]
        .into_iter()
        .filter(|end| {
            let parameter = 0.5 * (start + end);
            let circle_midpoint = Point3::new(
                center.x
                    + radius * (parameter.cos() * ref_direction.x + parameter.sin() * tangent.x),
                center.y
                    + radius * (parameter.cos() * ref_direction.y + parameter.sin() * tangent.y),
                center.z
                    + radius * (parameter.cos() * ref_direction.z + parameter.sin() * tangent.z),
            );
            circle_midpoint.distance_squared(surface_midpoint).sqrt() <= 2e-3
        })
        .collect::<Vec<_>>();
    <[f64; 1]>::try_from(candidates)
        .ok()
        .map(|[end]| [start, end])
}

pub(crate) fn unit_vector(vector: Vector3) -> Option<Vector3> {
    let norm = vector.norm();
    (norm > f64::EPSILON).then(|| vector.scale(1.0 / norm))
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
        format: "catia".to_string(),
        attributes,
    }
}

pub(crate) fn build_geometry_report(
    ir: &CadIr,
    scan: &ContainerScan,
    typed: &TypedCounts,
    plane_faces: usize,
    analytic_record_count: usize,
    freeform_record_count: usize,
    topology_attached: bool,
) -> DecodeReport {
    let mut losses = Vec::new();

    losses.push(LossNote {
        code: cadmpeg_ir::report::LossCode::CarrierSummary,
        category: LossCategory::Geometry,
        severity: Severity::Info,
        message: format!(
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
        ),
        provenance: None,
    });

    if !topology_attached {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
            category: LossCategory::Topology,
            severity: Severity::Blocking,
            message: format!(
                "The B-rep boundary graph was not emitted: {} face outer-bound run(s) were \
                 detected, but a complete trim/spine/support-table parse and unique \
                 surface-constrained logical-vertex assignment were not all available.",
                scan.census.fbb_runs
            ),
            provenance: None,
        });
    }

    if plane_faces > 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{plane_faces} plane surface record(s) were located but not decoded because their \
                 tag-bridged parameter records were absent or invalid."
            ),
            provenance: None,
        });
    }

    let invalid_analytic = analytic_record_count.saturating_sub(typed.total() + plane_faces);
    if invalid_analytic > 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{invalid_analytic} analytic surface record(s) had a non-finite or out-of-range \
                 inline payload and were not decoded."
            ),
            provenance: None,
        });
    }
    if freeform_record_count > 0 {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
            category: LossCategory::Geometry,
            severity: Severity::Warning,
            message: format!(
                "{freeform_record_count} face-local free-form carrier record(s) retain their \
                 tag, bounds, and orientation, but their aliased surface geometry is not yet \
                 transferred."
            ),
            provenance: None,
        });
    }

    insert_unresolved_carrier_loss(ir, &mut losses);

    losses.push(LossNote {
        code: cadmpeg_ir::report::LossCode::AttributesNotTransferred,
        category: LossCategory::Attribute,
        severity: Severity::Warning,
        message: "Standard circles with an exact adjacent-carrier section normal, plane-plane \
                  lines, and same-surface cylinder or cone generators are transferred as curves. \
                  Standard spline edges retain exact two-surface intersection constructions and \
                  their identity-bound support pcurves when present, but their serialized 3D \
                  NURBS caches, materials, and document metadata are not yet transferred."
            .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "catia".to_string(),
        container_only: false,
        geometry_transferred: true,
        coverage: std::collections::BTreeMap::new(),
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
        None => (scan.data.as_slice(), "CATPart"),
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
    let mut losses = vec![LossNote {
        code: cadmpeg_ir::report::LossCode::GeometryNotTransferred,
        category: LossCategory::Geometry,
        severity: Severity::Blocking,
        message: format!(
            "No B-rep geometry was transferred. This file's storage variant is `{}` ({}); the \
             applicable decoded record families transfer geometry in this codec.",
            scan.variant.token(),
            scan.variant.description()
        ),
        provenance: None,
    }];

    if container_only {
        losses.push(LossNote {
            code: cadmpeg_ir::report::LossCode::ContainerOnly,
            category: LossCategory::Geometry,
            severity: Severity::Info,
            message: "Container-only decode requested; entity decode was not attempted."
                .to_string(),
            provenance: None,
        });
    }

    losses.push(LossNote {
        code: cadmpeg_ir::report::LossCode::TopologyNotTransferred,
        category: LossCategory::Topology,
        severity: Severity::Blocking,
        message:
            "B-rep topology graph (body/region/shell/face/loop/coedge/edge/vertex) was not built \
                  for this file."
                .to_string(),
        provenance: None,
    });

    DecodeReport {
        format: "catia".to_string(),
        container_only,
        geometry_transferred: false,
        coverage: std::collections::BTreeMap::new(),
        losses,
        notes: summary.notes,
    }
}

pub(crate) fn unwrap_angle(value: f64, reference: f64) -> f64 {
    reference + (value - reference + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
        - std::f64::consts::PI
}

pub(crate) fn rational_pcurve_arc(
    center: [f64; 2],
    radius: f64,
    range: [f64; 2],
) -> Option<PcurveGeometry> {
    let span = range[1] - range[0];
    if !radius.is_finite() || radius <= 0.0 || !span.is_finite() || span.abs() <= 1e-12 {
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
        if middle_weight.abs() <= 1e-12 {
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
    use crate::assemble::{neutral_model_is_admissible, unresolved_carrier_counts};

    use cadmpeg_ir::document::CadIr;

    use cadmpeg_ir::geometry::{
        Curve, CurveGeometry, ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface,
        ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{
        CurveId, ProceduralCurveId, ProceduralSurfaceId, RegionId, ShellId, SurfaceId, UnknownId,
    };

    use cadmpeg_ir::topology::Shell;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::unknown::UnknownRecord;

    #[test]
    fn neutral_model_admissibility_rejects_invalid_topology() {
        let valid = CadIr::empty(Units::default());
        assert!(neutral_model_is_admissible(&valid, &[]));

        let mut invalid = CadIr::empty(Units::default());
        invalid.model.shells.push(Shell {
            id: ShellId("catia:test:shell#invalid".into()),
            region: RegionId("catia:test:region#missing".into()),
            faces: Vec::new(),
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        assert!(!neutral_model_is_admissible(&invalid, &[]));
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

        assert!(neutral_model_is_admissible(&ir, &unknowns));
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
