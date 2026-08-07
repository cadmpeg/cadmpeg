// SPDX-License-Identifier: Apache-2.0
//! Bounded IGES Fixed ASCII writing.
//!
//! The writer has two deliberately separate paths. An unchanged decode with a
//! verified document baseline replays its retained source image byte for byte.
//! Otherwise the semantic writer emits the current supported neutral profile
//! and refuses a model or native record it cannot represent. A caller never
//! receives a plausible file after an unsupported value was silently dropped.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{EncodeInput, ExportPlan};
use cadmpeg_ir::eval::curve_point;
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, NurbsSurface, Pcurve, PcurveGeometry, SurfaceGeometry,
};
use cadmpeg_ir::hash::{sha256_hex, DOCUMENT_LOCAL_DIGEST_ATTRIBUTE};
use cadmpeg_ir::ids::{PointId, VertexId};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::{
    CensusBasis, EntityCensus, ExportReport, FidelityResolution, LossKind, LossNote, Severity,
    WritePath,
};
use cadmpeg_ir::topology::{BodyKind, Edge, Loop, LoopBoundaryRole, PcurveUse, Sense};
use cadmpeg_ir::{CadIr, SourceFidelity};
use std::collections::BTreeMap;
use std::f64::consts::TAU;
use std::fmt::Write as _;

const ALLOWED_NATIVE_ARENAS: &[&str] = &[
    "cards",
    "copious_data",
    "directions",
    "display_attributes",
    "entities",
    "product_occurrence_expansion",
    "transformations",
];

/// Plan an IGES export, selecting replay only after checking the document
/// baseline and retained source-image integrity.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    options: crate::IgesWriteOptions,
) -> Result<ExportPlan<'_>, CodecError> {
    if let Some(bytes) = replay_bytes(input.ir, input.fidelity, options.version)? {
        return Ok(ExportPlan::buffered(
            report(
                FidelityResolution::Replayed,
                WritePath::VerbatimReplay,
                Vec::new(),
                "preserved source container replayed verbatim",
                counts_for_ir(input.ir),
            ),
            bytes,
        ));
    }

    let source_expected = input
        .ir
        .source
        .as_ref()
        .is_some_and(|source| source.format == "iges");
    let source_available = input
        .fidelity
        .and_then(|fidelity| fidelity.retained_record(crate::SOURCE_IMAGE_ID))
        .is_some();
    let mut losses = Vec::new();
    if source_expected && !source_available {
        losses.push(
            LossNote::new(
                LossKind::PreservedSourceUnavailable,
                "preserved IGES source image is unavailable; semantic regeneration is required",
            )
            .with_severity(Severity::Blocking),
        );
    }
    let synthesis = synthesize(input.ir, options.version)?;
    losses.extend(synthesis.losses.clone());
    let fidelity = if source_expected && !source_available {
        FidelityResolution::Degraded {
            reason: "preserved IGES source image is unavailable".into(),
        }
    } else if input.fidelity.is_some() {
        FidelityResolution::NotConsumed
    } else {
        FidelityResolution::NotProvided
    };
    Ok(ExportPlan::buffered(
        report(
            fidelity,
            WritePath::Synthesized,
            losses,
            "IGES Fixed ASCII container regenerated from supported neutral geometry",
            synthesis.counts,
        ),
        synthesis.bytes,
    ))
}

fn replay_bytes(
    ir: &CadIr,
    fidelity: Option<&SourceFidelity>,
    version: crate::IgesVersion,
) -> Result<Option<Vec<u8>>, CodecError> {
    let Some(expected) = ir
        .source
        .as_ref()
        .filter(|source| source.format == "iges")
        .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE))
    else {
        return Ok(None);
    };
    if ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get("iges_version"))
        .is_none_or(|source_version| source_version != version.name())
    {
        return Ok(None);
    }
    if crate::document_digest(ir) != *expected {
        return Ok(None);
    }
    let Some(record) = fidelity.and_then(|value| value.retained_record(crate::SOURCE_IMAGE_ID))
    else {
        return Ok(None);
    };
    let Some(data) = record.data.as_deref() else {
        return Err(CodecError::Malformed(
            "retained IGES source image has no bytes".into(),
        ));
    };
    if record.byte_len != data.len() as u64 || record.sha256 != sha256_hex(data) {
        return Err(CodecError::Malformed(
            "retained IGES source image failed integrity validation".into(),
        ));
    }
    Ok(Some(data.to_vec()))
}

fn report(
    fidelity: FidelityResolution,
    write_path: WritePath,
    losses: Vec<LossNote>,
    note: &str,
    counts: BTreeMap<String, usize>,
) -> ExportReport {
    ExportReport {
        format: "iges".into(),
        census: EntityCensus {
            basis: CensusBasis::TargetRecords,
            counts,
        },
        fidelity,
        write_path,
        losses,
        notes: vec![note.into()],
    }
}

fn counts_for_ir(ir: &CadIr) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    if let Some(namespace) = ir.native.namespace("iges") {
        if let Some(records) = namespace.arenas.get("entities") {
            for record in records {
                if let Some(entity_type) =
                    record.field("entity_type").and_then(|value| value.as_i64())
                {
                    *counts.entry(format!("{entity_type}_entity")).or_insert(0) += 1;
                }
            }
        }
    }
    if counts.is_empty() {
        counts.insert("116_point".into(), ir.model.points.len());
    }
    counts
}

struct Synthesis {
    bytes: Vec<u8>,
    counts: BTreeMap<String, usize>,
    losses: Vec<LossNote>,
}

fn synthesize(ir: &CadIr, version: crate::IgesVersion) -> Result<Synthesis, CodecError> {
    reject_unsupported_model(ir)?;
    let mut losses = procedural_reduction_losses(ir)?;
    losses.extend(reject_unsupported_native(ir)?);

    let mut entities = if has_brep_topology(ir) {
        brep_entities(ir)?
    } else if has_trimmed_sheet_topology(ir) {
        topology_entities(ir)?
    } else {
        let mut entities = Vec::new();
        let mut consumed_points = std::collections::BTreeSet::new();
        let mut consumed_curves = std::collections::BTreeSet::new();
        let mut surfaces = ir.model.surfaces.iter().collect::<Vec<_>>();
        surfaces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for surface in surfaces {
            append_surface_entities(&mut entities, &surface.geometry)?;
        }
        let mut edges = ir.model.edges.iter().collect::<Vec<_>>();
        edges.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for edge in edges {
            let curve_id = edge.curve.as_ref().ok_or_else(|| {
                CodecError::NotImplemented(format!(
                    "IGES semantic writer does not encode carrier-less edge {}",
                    edge.id
                ))
            })?;
            let curve = ir
                .model
                .curves
                .iter()
                .find(|candidate| candidate.id == *curve_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "IGES edge {} references missing curve {}",
                        edge.id, curve_id
                    ))
                })?;
            let geometry = flatten_curve(&curve.geometry)?;
            let span = edge_span(ir, edge, &geometry)?;
            entities.push(curve_entity(&geometry, Some(&span))?);
            consumed_curves.insert(curve_id.clone());
            consumed_points.insert(vertex_point_id(ir, &edge.start)?);
            consumed_points.insert(vertex_point_id(ir, &edge.end)?);
        }

        let mut curves = ir.model.curves.iter().collect::<Vec<_>>();
        curves.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for curve in curves {
            if consumed_curves.contains(&curve.id) {
                continue;
            }
            let geometry = flatten_curve(&curve.geometry)?;
            entities.push(curve_entity(&geometry, None)?);
        }

        let mut points = ir.model.points.iter().collect::<Vec<_>>();
        points.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
        for point in points {
            if consumed_points.contains(&point.id) {
                continue;
            }
            ensure_finite_point(point.position, &point.id.0)?;
            entities.push(point_entity(point.position));
        }
        entities
    };
    resolve_entity_references(&mut entities)?;

    let counts = entity_counts(&entities);
    Ok(Synthesis {
        bytes: encode_file(&entities, version)?,
        counts,
        losses,
    })
}

fn has_trimmed_sheet_topology(ir: &CadIr) -> bool {
    !ir.model.faces.is_empty()
        || !ir.model.loops.is_empty()
        || !ir.model.coedges.is_empty()
        || !ir.model.pcurves.is_empty()
}

fn has_brep_topology(ir: &CadIr) -> bool {
    if ir
        .model
        .bodies
        .iter()
        .any(|body| !is_decoder_free_geometry_body(body) && body.kind == BodyKind::Solid)
    {
        return true;
    }
    if ir.model.faces.iter().any(|face| {
        face.loops.iter().any(|loop_id| {
            ir.model
                .loops
                .iter()
                .find(|loop_| loop_.id == *loop_id)
                .is_some_and(|loop_| !loop_.vertex_uses.is_empty())
        })
    }) {
        return true;
    }
    let mut edge_use_counts = BTreeMap::new();
    for coedge in &ir.model.coedges {
        *edge_use_counts
            .entry(coedge.edge.as_str())
            .or_insert(0_usize) += 1;
    }
    if edge_use_counts.values().any(|count| *count > 1) {
        return true;
    }
    if ir.model.bodies.iter().any(|body| {
        if is_decoder_free_geometry_body(body) || body.kind != BodyKind::Sheet {
            return false;
        }
        body.regions.iter().any(|region_id| {
            ir.model
                .regions
                .iter()
                .find(|region| region.id == *region_id)
                .is_some_and(|region| region.shells.len() != 1)
        })
    }) {
        return true;
    }
    ir.model.faces.iter().any(|face| {
        ir.model
            .shells
            .iter()
            .find(|shell| shell.faces.iter().any(|face_id| face_id == &face.id))
            .is_some_and(|shell| shell.faces.len() > 1)
    })
}

fn procedural_reduction_losses(ir: &CadIr) -> Result<Vec<LossNote>, CodecError> {
    for procedural in &ir.model.procedural_surfaces {
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == procedural.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES procedural surface {} references missing solved surface {}",
                    procedural.id, procedural.surface
                ))
            })?;
        if !matches!(
            &surface.geometry,
            SurfaceGeometry::Plane { .. }
                | SurfaceGeometry::Nurbs(_)
                | SurfaceGeometry::Cylinder { .. }
                | SurfaceGeometry::Cone { .. }
                | SurfaceGeometry::Sphere { .. }
                | SurfaceGeometry::Torus { .. }
        ) {
            return Err(CodecError::NotImplemented(format!(
                "IGES procedural surface {} has no writable solved carrier",
                procedural.id
            )));
        }
    }
    for procedural in &ir.model.procedural_curves {
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == procedural.curve)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES procedural curve {} references missing solved curve {}",
                    procedural.id, procedural.curve
                ))
            })?;
        let geometry = flatten_curve(&curve.geometry)?;
        if !matches!(
            geometry,
            CurveGeometry::Line { .. }
                | CurveGeometry::Circle { .. }
                | CurveGeometry::Ellipse { .. }
                | CurveGeometry::Parabola { .. }
                | CurveGeometry::Hyperbola { .. }
                | CurveGeometry::Nurbs(_)
                | CurveGeometry::Polyline { .. }
        ) {
            return Err(CodecError::NotImplemented(format!(
                "IGES procedural curve {} has no writable solved carrier",
                procedural.id
            )));
        }
    }
    let surface_count = ir.model.procedural_surfaces.len();
    let curve_count = ir.model.procedural_curves.len();
    if surface_count == 0 && curve_count == 0 {
        return Ok(Vec::new());
    }
    Ok(vec![
        LossNote::new(
            LossKind::ProceduralReduced,
            format!(
                "{surface_count} procedural surface definition(s) and {curve_count} procedural curve definition(s) were reduced to writable solved carriers"
            ),
        )
        .with_severity(Severity::Info),
    ])
}

fn validate_brep_topology(ir: &CadIr) -> Result<(), CodecError> {
    let bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| !is_decoder_free_geometry_body(body))
        .collect::<Vec<_>>();
    if bodies.is_empty() {
        return Err(CodecError::NotImplemented(
            "IGES B-rep writer requires at least one supported body".into(),
        ));
    }
    let supported_body_ids = bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut owned_regions = std::collections::BTreeSet::new();
    let mut owned_shells = std::collections::BTreeSet::new();
    let mut owned_faces = std::collections::BTreeSet::new();
    let mut used_loops = std::collections::BTreeSet::new();
    let mut used_coedges = std::collections::BTreeSet::new();
    let mut used_edges = std::collections::BTreeSet::new();
    let mut used_vertices = std::collections::BTreeSet::new();
    let mut edge_bodies = BTreeMap::<String, (String, BodyKind)>::new();
    let mut edge_coedges = BTreeMap::<String, Vec<String>>::new();

    for body in &bodies {
        if !matches!(body.kind, BodyKind::Solid | BodyKind::Sheet) {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep writer does not encode body kind {:?} ({})",
                body.kind, body.id
            )));
        }
        if body.regions.len() != 1 {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep writer requires one region per body ({})",
                body.id
            )));
        }
        let region_id = &body.regions[0];
        let region = ir
            .model
            .regions
            .iter()
            .find(|region| region.id == *region_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES body {} references missing region {}",
                    body.id, region_id
                ))
            })?;
        if region.body != body.id || region.shells.is_empty() {
            return Err(CodecError::Malformed(format!(
                "IGES region {} is not a nonempty region of body {}",
                region.id, body.id
            )));
        }
        if body.kind == BodyKind::Sheet && region.shells.len() != 1 {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep writer requires one shell for a sheet body ({})",
                body.id
            )));
        }
        if !owned_regions.insert(region.id.as_str().to_owned()) {
            return Err(CodecError::Malformed(format!(
                "IGES region {} is owned more than once",
                region.id
            )));
        }
        for shell_id in &region.shells {
            let shell = ir
                .model
                .shells
                .iter()
                .find(|shell| shell.id == *shell_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "IGES region {} references missing shell {}",
                        region.id, shell_id
                    ))
                })?;
            if shell.region != region.id || shell.faces.is_empty() {
                return Err(CodecError::Malformed(format!(
                    "IGES shell {} is not a nonempty shell of region {}",
                    shell.id, region.id
                )));
            }
            if !shell.wire_edges.is_empty() || !shell.free_vertices.is_empty() {
                return Err(CodecError::NotImplemented(format!(
                    "IGES B-rep writer does not encode wire edges or free vertices in shell {}",
                    shell.id
                )));
            }
            if !owned_shells.insert(shell.id.as_str().to_owned()) {
                return Err(CodecError::Malformed(format!(
                    "IGES shell {} is owned more than once",
                    shell.id
                )));
            }
            for face_id in &shell.faces {
                let face = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES shell {} references missing face {}",
                            shell.id, face_id
                        ))
                    })?;
                if face.shell != shell.id || face.loops.is_empty() {
                    return Err(CodecError::Malformed(format!(
                        "IGES face {} is not a nonempty face of shell {}",
                        face.id, shell.id
                    )));
                }
                if !owned_faces.insert(face.id.as_str().to_owned()) {
                    return Err(CodecError::Malformed(format!(
                        "IGES face {} is owned more than once",
                        face.id
                    )));
                }
                let surface = ir
                    .model
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == face.surface)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES face {} references missing surface {}",
                            face.id, face.surface
                        ))
                    })?;
                surface_entities(&surface.geometry, 0)?;
                for loop_id in &face.loops {
                    let loop_ = ir
                        .model
                        .loops
                        .iter()
                        .find(|loop_| loop_.id == *loop_id)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "IGES face {} references missing loop {}",
                                face.id, loop_id
                            ))
                        })?;
                    if loop_.face != face.id
                        || (loop_.coedges.is_empty() && loop_.vertex_uses.len() != 1)
                    {
                        return Err(CodecError::Malformed(format!(
                            "IGES loop {} is not a valid loop of face {}",
                            loop_.id, face.id
                        )));
                    }
                    if loop_.coedges.is_empty() && loop_.vertex_uses[0].after.is_some() {
                        return Err(CodecError::Malformed(format!(
                            "IGES vertex-only loop {} has a preceding coedge",
                            loop_.id
                        )));
                    }
                    if !used_loops.insert(loop_.id.as_str().to_owned()) {
                        return Err(CodecError::Malformed(format!(
                            "IGES loop {} is used more than once",
                            loop_.id
                        )));
                    }
                    for (index, coedge_id) in loop_.coedges.iter().enumerate() {
                        let coedge = ir
                            .model
                            .coedges
                            .iter()
                            .find(|coedge| coedge.id == *coedge_id)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "IGES loop {} references missing coedge {}",
                                    loop_.id, coedge_id
                                ))
                            })?;
                        let next = &loop_.coedges[(index + 1) % loop_.coedges.len()];
                        let previous =
                            &loop_.coedges[(index + loop_.coedges.len() - 1) % loop_.coedges.len()];
                        if coedge.owner_loop != loop_.id
                            || coedge.next != *next
                            || coedge.previous != *previous
                            || coedge.use_curve.is_some()
                        {
                            return Err(CodecError::Malformed(format!(
                                "IGES coedge {} is not a valid loop use",
                                coedge.id
                            )));
                        }
                        if !used_coedges.insert(coedge.id.as_str().to_owned()) {
                            return Err(CodecError::Malformed(format!(
                                "IGES coedge {} is used more than once",
                                coedge.id
                            )));
                        }
                        let edge = ir
                            .model
                            .edges
                            .iter()
                            .find(|edge| edge.id == coedge.edge)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "IGES coedge {} references missing edge {}",
                                    coedge.id, coedge.edge
                                ))
                            })?;
                        if let Some((owner, _)) = edge_bodies.get(edge.id.as_str()) {
                            if owner != body.id.as_str() {
                                return Err(CodecError::Malformed(format!(
                                    "IGES edge {} is used by multiple bodies",
                                    edge.id
                                )));
                            }
                        } else {
                            edge_bodies.insert(
                                edge.id.as_str().to_owned(),
                                (body.id.as_str().to_owned(), body.kind),
                            );
                        }
                        used_edges.insert(edge.id.as_str().to_owned());
                        edge_coedges
                            .entry(edge.id.as_str().to_owned())
                            .or_default()
                            .push(coedge.id.as_str().to_owned());
                        let curve_id = edge.curve.as_ref().ok_or_else(|| {
                            CodecError::NotImplemented(format!(
                                "IGES B-rep writer does not encode carrier-less edge {}",
                                edge.id
                            ))
                        })?;
                        let curve = ir
                            .model
                            .curves
                            .iter()
                            .find(|curve| curve.id == *curve_id)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "IGES edge {} references missing curve {}",
                                    edge.id, curve_id
                                ))
                            })?;
                        let geometry = flatten_curve(&curve.geometry)?;
                        edge_span(ir, edge, &geometry)?;
                        for vertex_id in [&edge.start, &edge.end] {
                            let vertex = ir
                                .model
                                .vertices
                                .iter()
                                .find(|vertex| vertex.id == *vertex_id)
                                .ok_or_else(|| {
                                    CodecError::Malformed(format!(
                                        "IGES edge {} references missing vertex {}",
                                        edge.id, vertex_id
                                    ))
                                })?;
                            used_vertices.insert(vertex.id.as_str().to_owned());
                            point_position(ir, &vertex.point)?;
                        }
                        validate_brep_pcurve_uses(ir, &coedge.pcurves, coedge.id.as_str())?;
                    }
                    for vertex_use in &loop_.vertex_uses {
                        if !loop_.coedges.is_empty() && vertex_use.after.is_none() {
                            return Err(CodecError::Malformed(format!(
                                "IGES loop {} vertex use has no preceding coedge",
                                loop_.id
                            )));
                        }
                        if vertex_use
                            .after
                            .as_ref()
                            .is_some_and(|coedge_id| !loop_.coedges.contains(coedge_id))
                        {
                            return Err(CodecError::Malformed(format!(
                                "IGES loop {} vertex use references a coedge outside the loop",
                                loop_.id
                            )));
                        }
                        let vertex = ir
                            .model
                            .vertices
                            .iter()
                            .find(|vertex| vertex.id == vertex_use.vertex)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "IGES loop {} references missing vertex {}",
                                    loop_.id, vertex_use.vertex
                                ))
                            })?;
                        used_vertices.insert(vertex.id.as_str().to_owned());
                        point_position(ir, &vertex.point)?;
                        validate_brep_pcurve_uses(ir, &vertex_use.pcurves, loop_.id.as_str())?;
                    }
                }
            }
        }
    }

    let ignored_carriers = ignored_carrier_geometry(ir);
    let mut admitted_edges = used_edges.clone();
    admitted_edges.extend(ignored_carriers.edges.iter().cloned());
    let mut admitted_vertices = used_vertices.clone();
    admitted_vertices.extend(ignored_carriers.vertices.iter().cloned());
    let supported_region_count = ir
        .model
        .regions
        .iter()
        .filter(|region| supported_body_ids.contains(&region.body))
        .count();
    let supported_shell_count = ir
        .model
        .shells
        .iter()
        .filter(|shell| {
            ir.model.regions.iter().any(|region| {
                supported_body_ids.contains(&region.body) && region.shells.contains(&shell.id)
            })
        })
        .count();
    if owned_regions.len() != supported_region_count
        || owned_shells.len() != supported_shell_count
        || owned_faces.len() != ir.model.faces.len()
        || used_loops.len() != ir.model.loops.len()
        || used_coedges.len() != ir.model.coedges.len()
        || admitted_edges.len() != ir.model.edges.len()
        || admitted_vertices.len() != ir.model.vertices.len()
        || used_brep_pcurve_ids(ir).len() != ir.model.pcurves.len()
    {
        return Err(CodecError::NotImplemented(format!(
            "IGES B-rep topology ownership is incomplete: regions {}/{} shells {}/{} faces {}/{} loops {}/{} coedges {}/{} edges {}/{} vertices {}/{} pcurves {}/{}",
            owned_regions.len(),
            supported_region_count,
            owned_shells.len(),
            supported_shell_count,
            owned_faces.len(),
            ir.model.faces.len(),
            used_loops.len(),
            ir.model.loops.len(),
            used_coedges.len(),
            ir.model.coedges.len(),
            admitted_edges.len(),
            ir.model.edges.len(),
            admitted_vertices.len(),
            ir.model.vertices.len(),
            used_brep_pcurve_ids(ir).len(),
            ir.model.pcurves.len()
        )));
    }

    for (edge_id, uses) in &edge_coedges {
        let first = uses.first().ok_or_else(|| {
            CodecError::Malformed(format!("IGES edge {edge_id} has no coedge uses"))
        })?;
        let mut ring = Vec::new();
        let mut current = first.clone();
        loop {
            if ring.contains(&current) {
                if current == *first {
                    break;
                }
                return Err(CodecError::Malformed(format!(
                    "IGES edge {edge_id} has an invalid radial ring"
                )));
            }
            if ring.len() >= uses.len() {
                return Err(CodecError::Malformed(format!(
                    "IGES edge {edge_id} has an invalid radial ring"
                )));
            }
            ring.push(current.clone());
            let coedge = ir
                .model
                .coedges
                .iter()
                .find(|coedge| coedge.id.as_str() == current)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "IGES radial ring references missing coedge {current}"
                    ))
                })?;
            if coedge.edge.as_str() != edge_id {
                return Err(CodecError::Malformed(format!(
                    "IGES radial ring for edge {edge_id} names another edge"
                )));
            }
            current = coedge.radial_next.as_str().to_owned();
        }
        if ring.len() != uses.len() {
            return Err(CodecError::Malformed(format!(
                "IGES edge {edge_id} radial ring does not cover every use"
            )));
        }
        let senses = ring
            .iter()
            .map(|coedge_id| {
                ir.model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id.as_str() == coedge_id)
                    .map(|coedge| coedge.sense)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES radial ring references missing coedge {coedge_id}"
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;
        if senses.len() == 2 && senses[0] == senses[1] {
            return Err(CodecError::Malformed(format!(
                "IGES edge {edge_id} has two coedges with the same sense"
            )));
        }
        if edge_bodies[edge_id].1 == BodyKind::Solid && ring.len() != 2 {
            return Err(CodecError::Malformed(format!(
                "IGES solid edge {edge_id} is not used by exactly two coedges"
            )));
        }
    }
    Ok(())
}

fn brep_entities(ir: &CadIr) -> Result<Vec<Entity>, CodecError> {
    validate_brep_topology(ir)?;
    let ignored_carriers = ignored_carrier_geometry(ir);
    let mut topology_point_ids = std::collections::BTreeSet::new();
    for coedge in &ir.model.coedges {
        let Some(edge) = ir.model.edges.iter().find(|edge| edge.id == coedge.edge) else {
            continue;
        };
        for vertex_id in [&edge.start, &edge.end] {
            if let Some(vertex) = ir
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id == *vertex_id)
            {
                topology_point_ids.insert(vertex.point.as_str().to_owned());
            }
        }
    }
    for loop_ in &ir.model.loops {
        for vertex_use in &loop_.vertex_uses {
            if let Some(vertex) = ir
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id == vertex_use.vertex)
            {
                topology_point_ids.insert(vertex.point.as_str().to_owned());
            }
        }
    }
    let bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| !is_decoder_free_geometry_body(body))
        .collect::<Vec<_>>();
    let mut entities = Vec::new();
    let mut surface_indices = BTreeMap::new();
    let mut surfaces = ir.model.surfaces.iter().collect::<Vec<_>>();
    surfaces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for surface in surfaces {
        let index = append_surface_entities(&mut entities, &surface.geometry)?;
        surface_indices.insert(surface.id.as_str().to_owned(), index);
    }

    let mut topology_edge_ids = std::collections::BTreeSet::new();
    for coedge in &ir.model.coedges {
        topology_edge_ids.insert(coedge.edge.as_str().to_owned());
    }
    let mut edge_curve_indices = BTreeMap::new();
    let mut edges = topology_edge_ids.iter().collect::<Vec<_>>();
    edges.sort();
    for edge_id in edges {
        let edge = ir
            .model
            .edges
            .iter()
            .find(|candidate| candidate.id.as_str() == edge_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!("IGES topology references missing edge {edge_id}"))
            })?;
        let curve_id = edge.curve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES B-rep writer does not encode carrier-less edge {}",
                edge.id
            ))
        })?;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|candidate| candidate.id == *curve_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES edge {} references missing curve {}",
                    edge.id, curve_id
                ))
            })?;
        let geometry = flatten_curve(&curve.geometry)?;
        let span = edge_span(ir, edge, &geometry)?;
        let index = entities.len();
        entities.push(curve_entity(&geometry, Some(&span))?);
        edge_curve_indices.insert(edge.id.as_str().to_owned(), index);
    }

    let consumed_curve_ids = ir
        .model
        .edges
        .iter()
        .filter(|edge| topology_edge_ids.contains(edge.id.as_str()))
        .filter_map(|edge| edge.curve.as_ref().map(|curve| curve.as_str().to_owned()))
        .collect::<std::collections::BTreeSet<_>>();
    let mut curves = ir.model.curves.iter().collect::<Vec<_>>();
    curves.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for curve in curves {
        if consumed_curve_ids.contains(curve.id.as_str())
            || ignored_carriers.curves.contains(curve.id.as_str())
        {
            continue;
        }
        let geometry = flatten_curve(&curve.geometry)?;
        entities.push(curve_entity(&geometry, None)?);
    }

    let used_pcurve_ids = used_brep_pcurve_ids(ir);
    let mut pcurve_indices = BTreeMap::new();
    for pcurve_id in used_pcurve_ids {
        let pcurve = ir
            .model
            .pcurves
            .iter()
            .find(|candidate| candidate.id.as_str() == pcurve_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES topology references missing pcurve {pcurve_id}"
                ))
            })?;
        let index = entities.len();
        entities.push(pcurve_entity(pcurve)?);
        pcurve_indices.insert(pcurve_id, index);
    }

    let mut body_ids = bodies
        .iter()
        .map(|body| body.id.as_str().to_owned())
        .collect::<Vec<_>>();
    body_ids.sort();
    for body_id in body_ids {
        let body = bodies
            .iter()
            .find(|body| body.id.as_str() == body_id)
            .ok_or_else(|| CodecError::Malformed(format!("IGES body {body_id} is missing")))?;
        let region = ir
            .model
            .regions
            .iter()
            .find(|region| region.body.as_str() == body_id)
            .ok_or_else(|| CodecError::Malformed(format!("IGES body {body_id} has no region")))?;
        let shells = region
            .shells
            .iter()
            .map(|shell_id| {
                ir.model
                    .shells
                    .iter()
                    .find(|shell| shell.id == *shell_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES region {} references missing shell {}",
                            region.id, shell_id
                        ))
                    })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let mut body_edge_ids = std::collections::BTreeSet::new();
        let mut body_vertex_ids = std::collections::BTreeSet::new();
        let mut body_loop_ids = Vec::new();
        let mut body_face_ids = Vec::new();
        for shell in &shells {
            for face_id in &shell.faces {
                let face = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES shell {} references missing face {}",
                            shell.id, face_id
                        ))
                    })?;
                body_face_ids.push(face.id.clone());
                for loop_id in &face.loops {
                    let loop_ = ir
                        .model
                        .loops
                        .iter()
                        .find(|loop_| loop_.id == *loop_id)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "IGES face {} references missing loop {}",
                                face.id, loop_id
                            ))
                        })?;
                    body_loop_ids.push(loop_.id.clone());
                    for coedge_id in &loop_.coedges {
                        let coedge = ir
                            .model
                            .coedges
                            .iter()
                            .find(|coedge| coedge.id == *coedge_id)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "IGES loop {} references missing coedge {}",
                                    loop_.id, coedge_id
                                ))
                            })?;
                        body_edge_ids.insert(coedge.edge.as_str().to_owned());
                        let edge = ir
                            .model
                            .edges
                            .iter()
                            .find(|edge| edge.id == coedge.edge)
                            .ok_or_else(|| {
                                CodecError::Malformed(format!(
                                    "IGES coedge {} references missing edge {}",
                                    coedge.id, coedge.edge
                                ))
                            })?;
                        body_vertex_ids.insert(edge.start.as_str().to_owned());
                        body_vertex_ids.insert(edge.end.as_str().to_owned());
                    }
                    for vertex_use in &loop_.vertex_uses {
                        body_vertex_ids.insert(vertex_use.vertex.as_str().to_owned());
                    }
                }
            }
        }

        let mut vertex_ids = body_vertex_ids.into_iter().collect::<Vec<_>>();
        vertex_ids.sort();
        let mut vertex_indices = BTreeMap::new();
        for (index, vertex_id) in vertex_ids.iter().enumerate() {
            vertex_indices.insert(vertex_id.clone(), index);
        }
        let mut parameters = format!("502,{}", vertex_ids.len());
        for vertex_id in &vertex_ids {
            let vertex = ir
                .model
                .vertices
                .iter()
                .find(|vertex| vertex.id.as_str() == vertex_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "IGES B-rep references missing vertex {vertex_id}"
                    ))
                })?;
            let point = point_position(ir, &vertex.point)?;
            for value in [point.x, point.y, point.z] {
                parameters.push(',');
                parameters.push_str(&number(value));
            }
        }
        parameters.push(';');
        let vertex_list_index = entities.len();
        entities.push(Entity {
            type_code: 502,
            form: 1,
            label: "VERTICES",
            status: "00010000",
            parameters: parameters.into_bytes(),
            transform: None,
        });

        let mut edge_ids = body_edge_ids.into_iter().collect::<Vec<_>>();
        edge_ids.sort();
        let mut edge_indices = BTreeMap::new();
        for (index, edge_id) in edge_ids.iter().enumerate() {
            edge_indices.insert(edge_id.clone(), index);
        }
        let edge_list_index = if edge_ids.is_empty() {
            None
        } else {
            let mut parameters = format!("504,{}", edge_ids.len());
            for edge_id in &edge_ids {
                let edge = ir
                    .model
                    .edges
                    .iter()
                    .find(|edge| edge.id.as_str() == edge_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!("IGES B-rep edge {edge_id} is missing"))
                    })?;
                let curve_index = edge_curve_indices[edge_id];
                let start_index = vertex_indices[edge.start.as_str()];
                let end_index = vertex_indices[edge.end.as_str()];
                let _ = write!(
                    parameters,
                    ",{},{},{},{},{}",
                    reference_marker(curve_index),
                    reference_marker(vertex_list_index),
                    start_index + 1,
                    reference_marker(vertex_list_index),
                    end_index + 1
                );
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 504,
                form: 1,
                label: "EDGES",
                status: "00010001",
                parameters: parameters.into_bytes(),
                transform: None,
            });
            Some(index)
        };

        let mut loop_indices = BTreeMap::new();
        for loop_id in &body_loop_ids {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|loop_| loop_.id == *loop_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("IGES B-rep loop {loop_id} is missing"))
                })?;
            let use_count = loop_
                .coedges
                .len()
                .checked_add(loop_.vertex_uses.len())
                .ok_or_else(|| CodecError::Malformed("IGES loop use count overflows".into()))?;
            let mut parameters = format!("508,{use_count}");
            for coedge_id in &loop_.coedges {
                let coedge = ir
                    .model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == *coedge_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES loop {} references missing coedge {}",
                            loop_.id, coedge_id
                        ))
                    })?;
                let edge_index = edge_indices[coedge.edge.as_str()];
                let edge_list_index = edge_list_index.ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "IGES loop {} has a coedge but no edge list",
                        loop_.id
                    ))
                })?;
                let sense = brep_sense(coedge.sense);
                let _ = write!(
                    parameters,
                    ",0,{},{},{},{}",
                    reference_marker(edge_list_index),
                    edge_index + 1,
                    sense,
                    coedge.pcurves.len()
                );
                for pcurve_use in &coedge.pcurves {
                    let _ = write!(
                        parameters,
                        ",{},{}",
                        i32::from(pcurve_use.isoparametric.unwrap_or(false)),
                        reference_marker(pcurve_indices[pcurve_use.pcurve.as_str()])
                    );
                }
                for vertex_use in loop_
                    .vertex_uses
                    .iter()
                    .filter(|vertex_use| vertex_use.after.as_ref() == Some(&coedge.id))
                {
                    let vertex_index = vertex_indices[vertex_use.vertex.as_str()];
                    let _ = write!(
                        parameters,
                        ",1,{},{},{},{}",
                        reference_marker(vertex_list_index),
                        vertex_index + 1,
                        0,
                        vertex_use.pcurves.len()
                    );
                    for pcurve_use in &vertex_use.pcurves {
                        let _ = write!(
                            parameters,
                            ",{},{}",
                            i32::from(pcurve_use.isoparametric.unwrap_or(false)),
                            reference_marker(pcurve_indices[pcurve_use.pcurve.as_str()])
                        );
                    }
                }
            }
            if loop_.coedges.is_empty() {
                for vertex_use in &loop_.vertex_uses {
                    let vertex_index = vertex_indices[vertex_use.vertex.as_str()];
                    let _ = write!(
                        parameters,
                        ",1,{},{},{},{}",
                        reference_marker(vertex_list_index),
                        vertex_index + 1,
                        0,
                        vertex_use.pcurves.len()
                    );
                    for pcurve_use in &vertex_use.pcurves {
                        let _ = write!(
                            parameters,
                            ",{},{}",
                            i32::from(pcurve_use.isoparametric.unwrap_or(false)),
                            reference_marker(pcurve_indices[pcurve_use.pcurve.as_str()])
                        );
                    }
                }
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 508,
                form: 1,
                label: "LOOP",
                status: "00010000",
                parameters: parameters.into_bytes(),
                transform: None,
            });
            loop_indices.insert(loop_id.as_str().to_owned(), index);
        }

        let mut face_indices = BTreeMap::new();
        for face_id in &body_face_ids {
            let face = ir
                .model
                .faces
                .iter()
                .find(|face| face.id == *face_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!("IGES B-rep face {face_id} is missing"))
                })?;
            let has_outer = face.loops.iter().any(|loop_id| {
                ir.model
                    .loops
                    .iter()
                    .find(|loop_| loop_.id == *loop_id)
                    .is_some_and(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
            });
            let mut parameters = format!(
                "510,{},{},{}",
                reference_marker(surface_indices[face.surface.as_str()]),
                face.loops.len(),
                i32::from(has_outer)
            );
            for loop_id in &face.loops {
                let _ = write!(
                    parameters,
                    ",{}",
                    reference_marker(loop_indices[loop_id.as_str()])
                );
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 510,
                form: 1,
                label: "FACE",
                status: "00010000",
                parameters: parameters.into_bytes(),
                transform: None,
            });
            face_indices.insert(face_id.as_str().to_owned(), index);
        }

        let mut shell_indices = Vec::new();
        for shell in &shells {
            let mut parameters = format!("514,{}", shell.faces.len());
            for face_id in &shell.faces {
                let face = ir
                    .model
                    .faces
                    .iter()
                    .find(|face| face.id == *face_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES shell {} references missing face {}",
                            shell.id, face_id
                        ))
                    })?;
                let _ = write!(
                    parameters,
                    ",{},{}",
                    reference_marker(face_indices[face.id.as_str()]),
                    brep_sense(face.sense)
                );
            }
            parameters.push(';');
            let index = entities.len();
            entities.push(Entity {
                type_code: 514,
                form: if body.kind == BodyKind::Solid { 1 } else { 2 },
                label: "SHELL",
                status: if body.kind == BodyKind::Solid {
                    "00010000"
                } else {
                    "00000000"
                },
                parameters: parameters.into_bytes(),
                transform: None,
            });
            shell_indices.push(index);
        }
        if body.kind == BodyKind::Solid {
            let mut parameters = format!(
                "186,{},1,{}",
                reference_marker(shell_indices[0]),
                shell_indices.len() - 1
            );
            for shell_index in shell_indices.iter().skip(1) {
                let _ = write!(parameters, ",{},1", reference_marker(*shell_index));
            }
            parameters.push(';');
            entities.push(Entity {
                type_code: 186,
                form: 0,
                label: "SOLID",
                status: "00000000",
                parameters: parameters.into_bytes(),
                transform: None,
            });
        }
    }
    let mut points = ir.model.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for point in points {
        if topology_point_ids.contains(point.id.as_str())
            || ignored_carriers.points.contains(point.id.as_str())
        {
            continue;
        }
        ensure_finite_point(point.position, &point.id.0)?;
        entities.push(point_entity(point.position));
    }
    Ok(entities)
}

fn brep_sense(sense: Sense) -> i32 {
    match sense {
        Sense::Forward => 1,
        Sense::Reversed => 0,
    }
}

fn used_brep_pcurve_ids(ir: &CadIr) -> std::collections::BTreeSet<String> {
    let mut ids = ir
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter())
        .map(|use_| use_.pcurve.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    ids.extend(
        ir.model
            .loops
            .iter()
            .flat_map(|loop_| loop_.vertex_uses.iter())
            .flat_map(|vertex_use| vertex_use.pcurves.iter())
            .map(|use_| use_.pcurve.as_str().to_owned()),
    );
    ids
}

struct IgnoredCarrierGeometry {
    edges: std::collections::BTreeSet<String>,
    curves: std::collections::BTreeSet<String>,
    vertices: std::collections::BTreeSet<String>,
    points: std::collections::BTreeSet<String>,
}

fn ignored_carrier_geometry(ir: &CadIr) -> IgnoredCarrierGeometry {
    let topology_edge_ids = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let topology_edges = ir
        .model
        .edges
        .iter()
        .filter(|edge| topology_edge_ids.contains(edge.id.as_str()))
        .collect::<Vec<_>>();
    let mut ignored = IgnoredCarrierGeometry {
        edges: std::collections::BTreeSet::new(),
        curves: std::collections::BTreeSet::new(),
        vertices: std::collections::BTreeSet::new(),
        points: std::collections::BTreeSet::new(),
    };
    for body in &ir.model.bodies {
        if !is_decoder_free_geometry_body(body) {
            continue;
        }
        for region_id in &body.regions {
            let Some(region) = ir
                .model
                .regions
                .iter()
                .find(|region| region.id == *region_id)
            else {
                continue;
            };
            for shell_id in &region.shells {
                let Some(shell) = ir.model.shells.iter().find(|shell| shell.id == *shell_id) else {
                    continue;
                };
                ignored.vertices.extend(
                    shell
                        .free_vertices
                        .iter()
                        .map(|vertex| vertex.as_str().to_owned()),
                );
            }
        }
    }
    for edge in &ir.model.edges {
        if topology_edge_ids.contains(edge.id.as_str()) {
            continue;
        }
        let Some(curve_id) = &edge.curve else {
            continue;
        };
        let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *curve_id) else {
            continue;
        };
        let is_model_carrier = topology_edges.iter().any(|topology_edge| {
            topology_edge.curve.as_ref() == Some(curve_id)
                && topology_edge.param_range.zip(edge.param_range).is_some_and(
                    |(topology_range, edge_range)| same_range(topology_range, edge_range),
                )
                && vertex_position(ir, &topology_edge.start)
                    .zip(vertex_position(ir, &edge.start))
                    .is_some_and(|(topology_start, edge_start)| {
                        same_point(topology_start, edge_start)
                    })
                && vertex_position(ir, &topology_edge.end)
                    .zip(vertex_position(ir, &edge.end))
                    .is_some_and(|(topology_end, edge_end)| same_point(topology_end, edge_end))
        });
        let is_pcurve_carrier = ir.model.pcurves.iter().any(|pcurve| {
            pcurve.parameter_range.is_some_and(|range| {
                curve_matches_pcurve(&curve.geometry, range, pcurve)
                    && edge
                        .param_range
                        .is_some_and(|edge_range| same_range(edge_range, range))
                    && vertex_position(ir, &edge.start)
                        .zip(curve_point(&curve.geometry, range[0]))
                        .is_some_and(|(start, evaluated)| same_point(start, evaluated))
                    && vertex_position(ir, &edge.end)
                        .zip(curve_point(&curve.geometry, range[1]))
                        .is_some_and(|(end, evaluated)| same_point(end, evaluated))
            })
        });
        if is_model_carrier || is_pcurve_carrier {
            ignored.edges.insert(edge.id.as_str().to_owned());
            ignored.curves.insert(curve_id.as_str().to_owned());
            ignored.vertices.insert(edge.start.as_str().to_owned());
            ignored.vertices.insert(edge.end.as_str().to_owned());
        }
    }
    for vertex in &ir.model.vertices {
        if ignored.vertices.contains(vertex.id.as_str()) {
            ignored.points.insert(vertex.point.as_str().to_owned());
        }
    }
    ignored
}

fn curve_matches_pcurve(curve: &CurveGeometry, range: [f64; 2], pcurve: &Pcurve) -> bool {
    let CurveGeometry::Nurbs(curve) = curve else {
        return false;
    };
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &pcurve.geometry
    else {
        return false;
    };
    curve.degree == *degree
        && curve.periodic == *periodic
        && curve.knots.len() == knots.len()
        && curve
            .knots
            .iter()
            .zip(knots)
            .all(|(left, right)| same_float(*left, *right))
        && curve.control_points.len() == control_points.len()
        && curve
            .control_points
            .iter()
            .zip(control_points)
            .all(|(left, right)| {
                same_float(left.x, right.u)
                    && same_float(left.y, right.v)
                    && same_float(left.z, 0.0)
            })
        && match (&curve.weights, weights) {
            (None, None) => true,
            (Some(left), Some(right)) if left.len() == right.len() => left
                .iter()
                .zip(right)
                .all(|(left, right)| same_float(*left, *right)),
            _ => false,
        }
        && pcurve
            .parameter_range
            .is_some_and(|candidate| same_range(candidate, range))
}

fn same_float(left: f64, right: f64) -> bool {
    (left - right).abs() <= left.abs().max(right.abs()).max(1.0) * 1.0e-10
}

fn topology_entities(ir: &CadIr) -> Result<Vec<Entity>, CodecError> {
    validate_trimmed_sheet_topology(ir)?;
    let ignored_carriers = ignored_carrier_geometry(ir);
    let topology_edge_ids = ir
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.as_str().to_owned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut entities = Vec::new();
    let mut surface_indices = BTreeMap::new();
    let mut surfaces = ir.model.surfaces.iter().collect::<Vec<_>>();
    surfaces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for surface in surfaces {
        let index = append_surface_entities(&mut entities, &surface.geometry)?;
        surface_indices.insert(surface.id.as_str().to_owned(), index);
    }

    let mut edge_indices = BTreeMap::new();
    let mut consumed_curves = std::collections::BTreeSet::new();
    let mut consumed_points = std::collections::BTreeSet::new();
    let mut edges = ir.model.edges.iter().collect::<Vec<_>>();
    edges.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for edge in edges {
        if !topology_edge_ids.contains(edge.id.as_str()) {
            continue;
        }
        let curve_id = edge.curve.as_ref().ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES semantic writer does not encode carrier-less edge {}",
                edge.id
            ))
        })?;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|candidate| candidate.id == *curve_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES edge {} references missing curve {}",
                    edge.id, curve_id
                ))
            })?;
        let geometry = flatten_curve(&curve.geometry)?;
        let span = edge_span(ir, edge, &geometry)?;
        let index = entities.len();
        entities.push(curve_entity(&geometry, Some(&span))?);
        edge_indices.insert(edge.id.as_str().to_owned(), index);
        consumed_curves.insert(curve_id.as_str().to_owned());
        consumed_points.insert(vertex_point_id(ir, &edge.start)?.as_str().to_owned());
        consumed_points.insert(vertex_point_id(ir, &edge.end)?.as_str().to_owned());
    }

    let mut curves = ir.model.curves.iter().collect::<Vec<_>>();
    curves.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for curve in curves {
        if consumed_curves.contains(curve.id.as_str())
            || ignored_carriers.curves.contains(curve.id.as_str())
        {
            continue;
        }
        let geometry = flatten_curve(&curve.geometry)?;
        entities.push(curve_entity(&geometry, None)?);
    }

    let mut pcurve_ids = std::collections::BTreeSet::new();
    for face in &ir.model.faces {
        for loop_id in &face.loops {
            let loop_ = ir
                .model
                .loops
                .iter()
                .find(|candidate| candidate.id == *loop_id)
                .expect("validated loop reference");
            for coedge_id in &loop_.coedges {
                let coedge = ir
                    .model
                    .coedges
                    .iter()
                    .find(|candidate| candidate.id == *coedge_id)
                    .expect("validated coedge reference");
                pcurve_ids.extend(
                    coedge
                        .pcurves
                        .iter()
                        .map(|use_| use_.pcurve.as_str().to_owned()),
                );
            }
        }
    }
    let mut pcurve_indices = BTreeMap::new();
    for pcurve_id in pcurve_ids {
        let pcurve = ir
            .model
            .pcurves
            .iter()
            .find(|candidate| candidate.id.as_str() == pcurve_id)
            .expect("validated pcurve reference");
        let index = entities.len();
        entities.push(pcurve_entity(pcurve)?);
        pcurve_indices.insert(pcurve_id, index);
    }

    let mut boundary_indices = BTreeMap::new();
    let mut faces = ir.model.faces.iter().collect::<Vec<_>>();
    faces.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for face in &faces {
        let surface_index = *surface_indices
            .get(face.surface.as_str())
            .expect("validated face surface reference");
        for loop_ in face_loop_order(ir, face)? {
            let index = entities.len();
            entities.push(boundary_entity(
                ir,
                loop_,
                surface_index,
                &edge_indices,
                &pcurve_indices,
            )?);
            boundary_indices.insert(loop_.id.as_str().to_owned(), index);
        }
    }

    for face in &faces {
        let surface_index = *surface_indices
            .get(face.surface.as_str())
            .expect("validated face surface reference");
        let loops = face_loop_order(ir, face)?;
        let outer = loops
            .first()
            .filter(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer);
        let inner = if outer.is_some() {
            &loops[1..]
        } else {
            &loops[..]
        };
        let mut parameters = format!(
            "144,{},{},{},{}",
            reference_marker(surface_index),
            i32::from(outer.is_some()),
            inner.len(),
            outer.map_or_else(
                || "0".into(),
                |loop_| reference_marker(boundary_indices[loop_.id.as_str()]),
            )
        );
        for loop_ in inner {
            parameters.push(',');
            parameters.push_str(&reference_marker(boundary_indices[loop_.id.as_str()]));
        }
        parameters.push(';');
        entities.push(Entity {
            type_code: 144,
            form: 0,
            label: "TRIMMED",
            status: "00000000",
            parameters: parameters.into_bytes(),
            transform: None,
        });
    }

    let mut points = ir.model.points.iter().collect::<Vec<_>>();
    points.sort_by(|left, right| left.id.as_str().cmp(right.id.as_str()));
    for point in points {
        if consumed_points.contains(point.id.as_str())
            || ignored_carriers.points.contains(point.id.as_str())
        {
            continue;
        }
        ensure_finite_point(point.position, &point.id.0)?;
        entities.push(point_entity(point.position));
    }
    Ok(entities)
}

fn validate_trimmed_sheet_topology(ir: &CadIr) -> Result<(), CodecError> {
    if ir.model.faces.is_empty() {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer requires at least one face for topology output".into(),
        ));
    }
    let supported_bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| !is_decoder_free_geometry_body(body))
        .collect::<Vec<_>>();
    let supported_body_ids = supported_bodies
        .iter()
        .map(|body| body.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let supported_region_ids = ir
        .model
        .regions
        .iter()
        .filter(|region| supported_body_ids.contains(&region.body))
        .map(|region| region.id.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let supported_shell_count = ir
        .model
        .shells
        .iter()
        .filter(|shell| supported_region_ids.contains(&shell.region))
        .count();
    if supported_bodies.len() != ir.model.faces.len()
        || supported_region_ids.len() != ir.model.faces.len()
        || supported_shell_count != ir.model.faces.len()
    {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer currently encodes one trimmed sheet face per body".into(),
        ));
    }
    let mut owned_faces = std::collections::BTreeSet::new();
    for body in &ir.model.bodies {
        if is_decoder_free_geometry_body(body) {
            continue;
        }
        if body.kind != BodyKind::Sheet
            || body.regions.len() != 1
            || body
                .transform
                .is_some_and(|transform| transform != cadmpeg_ir::transform::Transform::identity())
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer only encodes identity single-face sheet body {}",
                body.id
            )));
        }
        let region_id = &body.regions[0];
        let region = ir
            .model
            .regions
            .iter()
            .find(|candidate| candidate.id == *region_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES body {} references missing region {}",
                    body.id, region_id
                ))
            })?;
        if region.body != body.id || region.shells.len() != 1 {
            return Err(CodecError::Malformed(format!(
                "IGES region {} is not owned by body {} with one shell",
                region.id, body.id
            )));
        }
        let shell_id = &region.shells[0];
        let shell = ir
            .model
            .shells
            .iter()
            .find(|candidate| candidate.id == *shell_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES region {} references missing shell {}",
                    region.id, shell_id
                ))
            })?;
        if shell.region != region.id
            || shell.faces.len() != 1
            || !shell.wire_edges.is_empty()
            || !shell.free_vertices.is_empty()
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer only encodes a single face in shell {}",
                shell.id
            )));
        }
        let face = ir
            .model
            .faces
            .iter()
            .find(|candidate| candidate.id == shell.faces[0])
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES shell {} references missing face {}",
                    shell.id, shell.faces[0]
                ))
            })?;
        if face.shell != shell.id {
            return Err(CodecError::Malformed(format!(
                "IGES face {} is not owned by shell {}",
                face.id, shell.id
            )));
        }
        if !owned_faces.insert(face.id.as_str().to_owned()) {
            return Err(CodecError::Malformed(format!(
                "IGES face {} is owned by more than one sheet shell",
                face.id
            )));
        }
    }
    if owned_faces.len() != ir.model.faces.len() {
        return Err(CodecError::Malformed(
            "IGES sheet body hierarchy does not own every face exactly once".into(),
        ));
    }

    let mut used_loops = std::collections::BTreeSet::new();
    let mut used_coedges = std::collections::BTreeSet::new();
    let mut used_edges = std::collections::BTreeSet::new();
    let mut used_vertices = std::collections::BTreeSet::new();
    let mut used_pcurves = std::collections::BTreeSet::new();
    let ignored_carriers = ignored_carrier_geometry(ir);
    for face in &ir.model.faces {
        if face.sense != Sense::Forward {
            return Err(CodecError::NotImplemented(format!(
                "IGES Type 144 output cannot encode reversed face sense {}",
                face.id
            )));
        }
        let surface = ir
            .model
            .surfaces
            .iter()
            .find(|candidate| candidate.id == face.surface)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES face {} references missing surface {}",
                    face.id, face.surface
                ))
            })?;
        surface_entities(&surface.geometry, 0)?;
        for loop_ in face_loop_order(ir, face)? {
            if !used_loops.insert(loop_.id.as_str().to_owned()) {
                return Err(CodecError::Malformed(format!(
                    "IGES face {} uses loop {} more than once",
                    face.id, loop_.id
                )));
            }
            if loop_.coedges.is_empty() || !loop_.vertex_uses.is_empty() {
                return Err(CodecError::NotImplemented(format!(
                    "IGES semantic writer only encodes edge loops without pole vertices ({})",
                    loop_.id
                )));
            }
            let first_pcurve_count = loop_.coedges.first().and_then(|coedge_id| {
                ir.model
                    .coedges
                    .iter()
                    .find(|coedge| coedge.id == *coedge_id)
                    .map(|coedge| coedge.pcurves.len())
            });
            let Some(first_pcurve_count) = first_pcurve_count else {
                return Err(CodecError::Malformed(format!(
                    "IGES loop {} references a missing first coedge",
                    loop_.id
                )));
            };
            for (index, coedge_id) in loop_.coedges.iter().enumerate() {
                let coedge = ir
                    .model
                    .coedges
                    .iter()
                    .find(|candidate| candidate.id == *coedge_id)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES loop {} references missing coedge {}",
                            loop_.id, coedge_id
                        ))
                    })?;
                if !used_coedges.insert(coedge.id.as_str().to_owned()) {
                    return Err(CodecError::Malformed(format!(
                        "IGES coedge {} is used more than once",
                        coedge.id
                    )));
                }
                let next = &loop_.coedges[(index + 1) % loop_.coedges.len()];
                let previous =
                    &loop_.coedges[(index + loop_.coedges.len() - 1) % loop_.coedges.len()];
                if coedge.owner_loop != loop_.id
                    || coedge.next != *next
                    || coedge.previous != *previous
                    || coedge.radial_next != coedge.id
                    || coedge.use_curve.is_some()
                {
                    return Err(CodecError::Malformed(format!(
                        "IGES coedge {} is not a simple laminar loop use",
                        coedge.id
                    )));
                }
                if coedge.pcurves.is_empty() != (first_pcurve_count == 0) {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES Type 141 requires one pcurve representation per loop ({})",
                        loop_.id
                    )));
                }
                let edge = ir
                    .model
                    .edges
                    .iter()
                    .find(|candidate| candidate.id == coedge.edge)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES coedge {} references missing edge {}",
                            coedge.id, coedge.edge
                        ))
                    })?;
                if !used_edges.insert(edge.id.as_str().to_owned()) {
                    return Err(CodecError::NotImplemented(format!(
                        "IGES semantic writer does not yet encode a shared or seam edge {}",
                        edge.id
                    )));
                }
                let curve_id = edge.curve.as_ref().ok_or_else(|| {
                    CodecError::NotImplemented(format!(
                        "IGES semantic writer does not encode carrier-less edge {}",
                        edge.id
                    ))
                })?;
                if !ir.model.curves.iter().any(|curve| curve.id == *curve_id) {
                    return Err(CodecError::Malformed(format!(
                        "IGES edge {} references missing curve {}",
                        edge.id, curve_id
                    )));
                }
                edge_span(
                    ir,
                    edge,
                    &flatten_curve(
                        &ir.model
                            .curves
                            .iter()
                            .find(|curve| curve.id == *curve_id)
                            .expect("curve existence checked")
                            .geometry,
                    )?,
                )?;
                let start_vertex = ir
                    .model
                    .vertices
                    .iter()
                    .find(|vertex| vertex.id == edge.start)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES edge {} references missing start vertex {}",
                            edge.id, edge.start
                        ))
                    })?;
                let end_vertex = ir
                    .model
                    .vertices
                    .iter()
                    .find(|vertex| vertex.id == edge.end)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "IGES edge {} references missing end vertex {}",
                            edge.id, edge.end
                        ))
                    })?;
                if !ir
                    .model
                    .points
                    .iter()
                    .any(|point| point.id == start_vertex.point)
                    || !ir
                        .model
                        .points
                        .iter()
                        .any(|point| point.id == end_vertex.point)
                {
                    return Err(CodecError::Malformed(format!(
                        "IGES edge {} references a vertex with a missing point",
                        edge.id
                    )));
                }
                used_vertices.insert(edge.start.as_str().to_owned());
                used_vertices.insert(edge.end.as_str().to_owned());
                for pcurve_use in &coedge.pcurves {
                    if pcurve_use.isoparametric == Some(true) {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer does not encode isoparametric pcurve use {}",
                            pcurve_use.pcurve
                        )));
                    }
                    let pcurve = ir
                        .model
                        .pcurves
                        .iter()
                        .find(|candidate| candidate.id == pcurve_use.pcurve)
                        .ok_or_else(|| {
                            CodecError::Malformed(format!(
                                "IGES coedge {} references missing pcurve {}",
                                coedge.id, pcurve_use.pcurve
                            ))
                        })?;
                    if pcurve.wrapper_reversed.is_some() || pcurve.native_tail_flags.is_some() {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer does not encode pcurve wrapper metadata {}",
                            pcurve.id
                        )));
                    }
                    let Some(parameter_range) = pcurve.parameter_range else {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer requires a parameter range for pcurve {}",
                            pcurve.id
                        )));
                    };
                    if pcurve_use
                        .parameter_range
                        .is_some_and(|range| !same_range(range, parameter_range))
                    {
                        return Err(CodecError::NotImplemented(format!(
                            "IGES semantic writer cannot restrict pcurve use {}",
                            pcurve_use.pcurve
                        )));
                    }
                    pcurve_entity(pcurve)?;
                    used_pcurves.insert(pcurve.id.as_str().to_owned());
                }
            }
        }
    }
    let mut admitted_edges = used_edges.clone();
    admitted_edges.extend(ignored_carriers.edges.iter().cloned());
    let mut admitted_vertices = used_vertices.clone();
    admitted_vertices.extend(ignored_carriers.vertices.iter().cloned());
    if used_loops.len() != ir.model.loops.len()
        || used_coedges.len() != ir.model.coedges.len()
        || admitted_edges.len() != ir.model.edges.len()
        || admitted_vertices.len() != ir.model.vertices.len()
        || used_pcurves.len() != ir.model.pcurves.len()
    {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer requires every topology arena entry to belong to a supported sheet face".into(),
        ));
    }
    Ok(())
}

fn is_decoder_free_geometry_body(body: &cadmpeg_ir::topology::Body) -> bool {
    body.id.as_str() == "iges:model:body#free-geometry"
        && body.kind == BodyKind::Wire
        && body.name.as_deref() == Some("IGES free geometry")
}

fn face_loop_order<'a>(
    ir: &'a CadIr,
    face: &cadmpeg_ir::topology::Face,
) -> Result<Vec<&'a Loop>, CodecError> {
    let mut loops = Vec::with_capacity(face.loops.len());
    for loop_id in &face.loops {
        let loop_ = ir
            .model
            .loops
            .iter()
            .find(|candidate| candidate.id == *loop_id)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES face {} references missing loop {}",
                    face.id, loop_id
                ))
            })?;
        loops.push(loop_);
    }
    if loops
        .iter()
        .filter(|loop_| loop_.boundary_role == LoopBoundaryRole::Outer)
        .count()
        > 1
    {
        return Err(CodecError::NotImplemented(format!(
            "IGES Type 144 supports one explicit outer loop per face ({})",
            face.id
        )));
    }
    loops.sort_by_key(|loop_| usize::from(loop_.boundary_role != LoopBoundaryRole::Outer));
    Ok(loops)
}

fn boundary_entity(
    ir: &CadIr,
    loop_: &Loop,
    surface_index: usize,
    edge_indices: &BTreeMap<String, usize>,
    pcurve_indices: &BTreeMap<String, usize>,
) -> Result<Entity, CodecError> {
    let coedges = loop_
        .coedges
        .iter()
        .map(|coedge_id| {
            ir.model
                .coedges
                .iter()
                .find(|coedge| coedge.id == *coedge_id)
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "IGES loop {} references missing coedge {}",
                        loop_.id, coedge_id
                    ))
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    let has_pcurves = coedges
        .first()
        .is_some_and(|coedge| !coedge.pcurves.is_empty());
    let representation = i32::from(has_pcurves);
    let mut parameters = format!(
        "141,{representation},0,{},{}",
        reference_marker(surface_index),
        loop_.coedges.len()
    );
    for coedge in coedges {
        let edge_index = edge_indices
            .get(coedge.edge.as_str())
            .copied()
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES boundary loop {} references missing edge entity {}",
                    loop_.id, coedge.edge
                ))
            })?;
        let sense = match coedge.sense {
            Sense::Forward => 1,
            Sense::Reversed => 2,
        };
        parameters.push(',');
        parameters.push_str(&reference_marker(edge_index));
        let _ = write!(parameters, ",{sense},{}", coedge.pcurves.len());
        for pcurve_use in &coedge.pcurves {
            let pcurve_index = pcurve_indices
                .get(pcurve_use.pcurve.as_str())
                .copied()
                .ok_or_else(|| {
                    CodecError::Malformed(format!(
                        "IGES boundary loop {} references missing pcurve entity {}",
                        loop_.id, pcurve_use.pcurve
                    ))
                })?;
            parameters.push(',');
            parameters.push_str(&reference_marker(pcurve_index));
        }
    }
    parameters.push(';');
    Ok(Entity {
        type_code: 141,
        form: 0,
        label: "BOUNDARY",
        status: "00000000",
        parameters: parameters.into_bytes(),
        transform: None,
    })
}

fn pcurve_entity(pcurve: &Pcurve) -> Result<Entity, CodecError> {
    let range = pcurve.parameter_range.ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES semantic writer requires a parameter range for pcurve {}",
            pcurve.id
        ))
    })?;
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = &pcurve.geometry
    else {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer only encodes NURBS pcurves ({})",
            pcurve.id
        )));
    };
    let control_points = control_points
        .iter()
        .map(|point| Point3::new(point.u, point.v, 0.0))
        .collect();
    encode_nurbs(
        &NurbsCurve {
            degree: *degree,
            knots: knots.clone(),
            control_points,
            weights: weights.clone(),
            periodic: *periodic,
        },
        range,
        "PCURVE",
    )
}

fn reference_marker(index: usize) -> String {
    format!("@R{index}@")
}

fn resolve_entity_references(entities: &mut [Entity]) -> Result<(), CodecError> {
    let mut directory_sequences = Vec::with_capacity(entities.len());
    let mut expanded_index = 0_u32;
    for entity in entities.iter() {
        if entity.transform.is_some() {
            expanded_index = expanded_index
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("IGES entity sequence overflows".into()))?;
        }
        let sequence = expanded_index
            .checked_mul(2)
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::Malformed("IGES entity sequence overflows".into()))?;
        directory_sequences.push(sequence);
        expanded_index = expanded_index
            .checked_add(1)
            .ok_or_else(|| CodecError::Malformed("IGES entity sequence overflows".into()))?;
    }
    for entity in entities {
        let mut resolved = Vec::with_capacity(entity.parameters.len());
        let mut index = 0;
        while index < entity.parameters.len() {
            if entity.parameters[index] == b'@' && entity.parameters.get(index + 1) == Some(&b'R') {
                let Some(end_offset) = entity.parameters[index + 2..]
                    .iter()
                    .position(|byte| *byte == b'@')
                else {
                    return Err(CodecError::Malformed(
                        "IGES entity contains an unterminated pointer reference".into(),
                    ));
                };
                let end = index + 2 + end_offset;
                let target = std::str::from_utf8(&entity.parameters[index + 2..end])
                    .ok()
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or_else(|| {
                        CodecError::Malformed(
                            "IGES entity contains an invalid pointer reference".into(),
                        )
                    })?;
                let sequence = directory_sequences.get(target).copied().ok_or_else(|| {
                    CodecError::Malformed("IGES pointer reference targets no entity".into())
                })?;
                resolved.extend_from_slice(sequence.to_string().as_bytes());
                index = end + 1;
            } else {
                resolved.push(entity.parameters[index]);
                index += 1;
            }
        }
        entity.parameters = resolved;
    }
    Ok(())
}

fn same_range(left: [f64; 2], right: [f64; 2]) -> bool {
    left.into_iter()
        .zip(right)
        .all(|(left, right)| (left - right).abs() <= left.abs().max(right.abs()).max(1.0) * 1.0e-10)
}

fn validate_brep_pcurve_uses(
    ir: &CadIr,
    uses: &[PcurveUse],
    owner: &str,
) -> Result<(), CodecError> {
    for pcurve_use in uses {
        let pcurve = ir
            .model
            .pcurves
            .iter()
            .find(|pcurve| pcurve.id == pcurve_use.pcurve)
            .ok_or_else(|| {
                CodecError::Malformed(format!(
                    "IGES B-rep {owner} references missing pcurve {}",
                    pcurve_use.pcurve
                ))
            })?;
        let range = pcurve.parameter_range.ok_or_else(|| {
            CodecError::NotImplemented(format!(
                "IGES B-rep requires a parameter range for pcurve {}",
                pcurve.id
            ))
        })?;
        if pcurve_use
            .parameter_range
            .is_some_and(|use_range| !same_range(use_range, range))
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep cannot restrict pcurve use {}",
                pcurve_use.pcurve
            )));
        }
        if pcurve.wrapper_reversed.is_some() || pcurve.native_tail_flags.is_some() {
            return Err(CodecError::NotImplemented(format!(
                "IGES B-rep does not encode pcurve wrapper metadata {}",
                pcurve.id
            )));
        }
        pcurve_entity(pcurve)?;
    }
    Ok(())
}

fn same_point(left: Point3, right: Point3) -> bool {
    same_float(left.x, right.x) && same_float(left.y, right.y) && same_float(left.z, right.z)
}

fn entity_counts(entities: &[Entity]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for entity in entities {
        let name = match entity.type_code {
            100 => "100_circular_arc",
            104 => "104_conic_arc",
            108 => "108_plane",
            190 => "190_pointer_plane",
            192 => "192_cylinder",
            194 => "194_cone",
            196 => "196_sphere",
            198 => "198_torus",
            110 => "110_line",
            116 => "116_point",
            126 => "126_nurbs_curve",
            128 => "128_nurbs_surface",
            124 => "124_transformation",
            106 => "106_copious_data",
            102 => "102_composite_curve",
            141 => "141_boundary",
            142 => "142_curve_on_parametric_surface",
            143 => "143_bounded_surface",
            144 => "144_trimmed_surface",
            186 => "186_manifold_solid_brep",
            502 => "502_vertex_list",
            504 => "504_edge_list",
            508 => "508_loop",
            510 => "510_face",
            514 => "514_shell",
            _ => "unknown_entity",
        };
        *counts.entry(name.into()).or_insert(0) += 1;
    }
    counts
}

fn reject_unsupported_model(ir: &CadIr) -> Result<(), CodecError> {
    let unsupported = [
        ("subds", !ir.model.subds.is_empty()),
        ("assets", !ir.model.assets.is_empty()),
        ("features", !ir.model.features.is_empty()),
        (
            "feature_input_topologies",
            !ir.model.feature_input_topologies.is_empty(),
        ),
        (
            "feature_result_topologies",
            !ir.model.feature_result_topologies.is_empty(),
        ),
        ("configurations", !ir.model.configurations.is_empty()),
        ("parameters", !ir.model.parameters.is_empty()),
        ("sketches", !ir.model.sketches.is_empty()),
        ("sketch_entities", !ir.model.sketch_entities.is_empty()),
        (
            "sketch_constraints",
            !ir.model.sketch_constraints.is_empty(),
        ),
        ("spatial_sketches", !ir.model.spatial_sketches.is_empty()),
        (
            "spatial_sketch_entities",
            !ir.model.spatial_sketch_entities.is_empty(),
        ),
        (
            "spatial_sketch_constraints",
            !ir.model.spatial_sketch_constraints.is_empty(),
        ),
        ("spreadsheets", !ir.model.spreadsheets.is_empty()),
        (
            "product_definitions",
            !ir.model.product_definitions.is_empty(),
        ),
        ("occurrences", !ir.model.occurrences.is_empty()),
        ("assembly_joints", !ir.model.assembly_joints.is_empty()),
        ("drawings", !ir.model.drawings.is_empty()),
        (
            "semantic_annotations",
            !ir.model.semantic_annotations.is_empty(),
        ),
        (
            "presentation_documents",
            !ir.model.presentation_documents.is_empty(),
        ),
        (
            "view_presentations",
            !ir.model.view_presentations.is_empty(),
        ),
        ("tessellations", !ir.model.tessellations.is_empty()),
        ("appearances", !ir.model.appearances.is_empty()),
        (
            "appearance_bindings",
            !ir.model.appearance_bindings.is_empty(),
        ),
        ("attributes", !ir.model.attributes.is_empty()),
        ("pmi", !ir.model.pmi.is_empty()),
        (
            "presentation_layers",
            !ir.model.presentation_layers.is_empty(),
        ),
    ];
    if let Some((arena, _)) = unsupported.into_iter().find(|(_, present)| *present) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode model arena {arena}"
        )));
    }
    for body in &ir.model.bodies {
        if body
            .transform
            .is_some_and(|transform| transform != cadmpeg_ir::transform::Transform::identity())
        {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer does not apply body transform {}",
                body.id
            )));
        }
    }
    for vertex in &ir.model.vertices {
        if !ir.model.points.iter().any(|point| point.id == vertex.point) {
            return Err(CodecError::Malformed(format!(
                "IGES vertex {} references missing point {}",
                vertex.id, vertex.point
            )));
        }
    }
    Ok(())
}

fn reject_unsupported_native(ir: &CadIr) -> Result<Vec<LossNote>, CodecError> {
    let Some(namespace) = ir.native.namespace("iges") else {
        return Ok(Vec::new());
    };
    if let Some((arena, _)) = namespace.arenas.iter().find(|(arena, records)| {
        !records.is_empty() && !ALLOWED_NATIVE_ARENAS.contains(&arena.as_str())
    }) {
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer cannot preserve native arena {arena}"
        )));
    }
    if let Some(record) = namespace
        .arenas
        .get("entities")
        .into_iter()
        .flatten()
        .find(|record| {
            !matches!(
                record.field("entity_type").and_then(|value| value.as_i64()),
                Some(
                    100 | 102
                        | 104
                        | 106
                        | 108
                        | 110
                        | 112
                        | 114
                        | 116
                        | 118
                        | 120
                        | 122
                        | 123
                        | 124
                        | 126
                        | 128
                        | 130
                        | 141
                        | 142
                        | 143
                        | 144
                        | 140
                        | 190
                        | 192
                        | 194
                        | 196
                        | 198
                        | 186
                        | 502
                        | 504
                        | 508
                        | 510
                        | 514,
                )
            )
        })
    {
        let entity_type = record
            .field("entity_type")
            .and_then(|value| value.as_i64())
            .unwrap_or_default();
        return Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode native entity type {entity_type}"
        )));
    }
    let mut native_entities = namespace.arenas.get("entities").into_iter().flatten();
    for record in native_entities.clone().filter(|record| {
        let entity_type = record.field("entity_type").and_then(|value| value.as_i64());
        let form = record.field("form").and_then(|value| value.as_i64());
        matches!(entity_type, Some(100 | 102 | 104 | 110 | 112 | 126 | 130))
            || (entity_type == Some(106) && matches!(form, Some(1..=3 | 11..=13 | 63)))
    }) {
        let Some(sequence) = record
            .field("directory_sequence")
            .and_then(|value| value.as_i64())
            .and_then(|value| u32::try_from(value).ok())
        else {
            return Err(CodecError::Malformed(
                "IGES native curve entity has no directory sequence".into(),
            ));
        };
        let object_id = format!("D{sequence}");
        if !ir.model.curves.iter().any(|curve| {
            curve
                .source_object
                .as_ref()
                .is_some_and(|source| source.format == "iges" && source.object_id == object_id)
        }) {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot preserve native curve entity {object_id} without neutral geometry"
            )));
        }
    }
    let has_native_surface = native_entities.clone().any(|record| {
        matches!(
            record.field("entity_type").and_then(|value| value.as_i64()),
            Some(108 | 114 | 118 | 120 | 122 | 128 | 140 | 190 | 192 | 194 | 196 | 198)
        )
    });
    let has_native_topology = native_entities.any(|record| {
        matches!(
            record.field("entity_type").and_then(|value| value.as_i64()),
            Some(141..=144 | 186 | 502 | 504 | 508 | 510 | 514)
        )
    });
    if has_native_surface && ir.model.surfaces.is_empty() {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer cannot preserve a native support surface without neutral geometry".into(),
        ));
    }
    if has_native_topology && !has_trimmed_sheet_topology(ir) {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer cannot preserve native trimming without neutral topology".into(),
        ));
    }
    let mut losses = Vec::new();
    for (arena, records) in &namespace.arenas {
        if records.is_empty() {
            continue;
        }
        let message = match arena.as_str() {
            "directions" => {
                "IGES direction records are regenerated only for required analytic-surface support; native direction identities and unrelated records are omitted"
            }
            "display_attributes" => {
                "IGES display attributes are not regenerated by the bounded semantic writer"
            }
            "product_occurrence_expansion" => {
                "IGES product occurrence expansion is not regenerated by the bounded semantic writer"
            }
            _ => continue,
        };
        losses.push(
            LossNote::new(LossKind::PassthroughRecordOmitted, message)
                .with_severity(Severity::Warning),
        );
    }
    Ok(losses)
}

#[derive(Clone, Copy)]
struct Placement {
    rows: [[f64; 4]; 3],
}

struct CurveSpan {
    range: [f64; 2],
    start: Point3,
    end: Point3,
}

fn point_entity(position: Point3) -> Entity {
    point_entity_with_status(position, "00000000")
}

fn point_entity_with_status(position: Point3, status: &'static str) -> Entity {
    Entity {
        type_code: 116,
        form: 0,
        label: "POINT",
        status,
        parameters: format!(
            "116,{},{},{};",
            number(position.x),
            number(position.y),
            number(position.z)
        )
        .into_bytes(),
        transform: None,
    }
}

fn direction_entity(direction: Vector3) -> Result<Entity, CodecError> {
    let direction = unit(direction, "analytic surface direction")?;
    Ok(Entity {
        type_code: 123,
        form: 0,
        label: "DIRECTN",
        status: "00010000",
        parameters: format!(
            "123,{},{},{};",
            number(direction.x),
            number(direction.y),
            number(direction.z)
        )
        .into_bytes(),
        transform: None,
    })
}

fn pointer_surface_support(
    base_index: usize,
    location: Point3,
    axis: Vector3,
    reference: Vector3,
) -> Result<(Vec<Entity>, usize, usize, usize), CodecError> {
    ensure_finite_point(location, "analytic surface location")?;
    let axis = unit(axis, "analytic surface axis")?;
    let reference = unit(reference, "analytic surface reference direction")?;
    if axis.dot(reference).abs() > 1.0e-10 {
        return Err(CodecError::Malformed(
            "analytic surface axis and reference direction are not orthogonal".into(),
        ));
    }
    let location_index = base_index;
    let axis_index = base_index
        .checked_add(1)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    let reference_index = base_index
        .checked_add(2)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    Ok((
        vec![
            point_entity_with_status(location, "00010000"),
            direction_entity(axis)?,
            direction_entity(reference)?,
        ],
        location_index,
        axis_index,
        reference_index,
    ))
}

fn append_surface_entities(
    entities: &mut Vec<Entity>,
    geometry: &SurfaceGeometry,
) -> Result<usize, CodecError> {
    let base_index = entities.len();
    let additions = surface_entities(geometry, base_index)?;
    let surface_offset = additions
        .len()
        .checked_sub(1)
        .ok_or_else(|| CodecError::Malformed("IGES surface encoder produced no entity".into()))?;
    let surface_index = base_index
        .checked_add(surface_offset)
        .ok_or_else(|| CodecError::Malformed("IGES entity index overflows".into()))?;
    entities.extend(additions);
    Ok(surface_index)
}

fn surface_entities(
    geometry: &SurfaceGeometry,
    base_index: usize,
) -> Result<Vec<Entity>, CodecError> {
    match geometry {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            ensure_finite_point(*origin, "plane origin")?;
            let normal = unit(*normal, "plane normal")?;
            let u_axis = unit(*u_axis, "plane u axis")?;
            let projected = u_axis - normal.scale(normal.dot(u_axis));
            let u_axis = unit(projected, "plane u axis")?;
            let v_axis = normal.cross(u_axis);
            let placement = placement(*origin, u_axis, v_axis, normal)?;
            Ok(vec![Entity {
                type_code: 108,
                form: 0,
                label: "PLANE",
                status: "00000000",
                parameters: b"108,0,0,1,0,0;".to_vec(),
                transform: Some(placement),
            }])
        }
        SurfaceGeometry::Nurbs(nurbs) => Ok(vec![encode_nurbs_surface(nurbs)?]),
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            radius,
        } => {
            if !radius.is_finite() || *radius <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES cylinder radius must be positive and finite".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *origin, *axis, *ref_direction)?;
            let surface = Entity {
                type_code: 192,
                form: 1,
                label: "CYLINDER",
                status: "00000000",
                parameters: format!(
                    "192,{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(*radius),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            radius,
            ratio,
            half_angle,
        } => {
            if !same_float(*ratio, 1.0) {
                return Err(CodecError::NotImplemented(
                    "IGES analytic cone writer only encodes circular cones".into(),
                ));
            }
            if !radius.is_finite() || *radius < 0.0 {
                return Err(CodecError::Malformed(
                    "IGES cone radius must be finite and non-negative".into(),
                ));
            }
            if !half_angle.is_finite()
                || *half_angle <= 0.0
                || *half_angle >= std::f64::consts::FRAC_PI_2
            {
                return Err(CodecError::Malformed(
                    "IGES cone semi-angle must be in (0, 90) degrees".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *origin, *axis, *ref_direction)?;
            let surface = Entity {
                type_code: 194,
                form: 1,
                label: "CONE",
                status: "00000000",
                parameters: format!(
                    "194,{},{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(*radius),
                    number(half_angle.to_degrees()),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            if !radius.is_finite() || *radius <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES sphere radius must be positive and finite".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *center, *axis, *ref_direction)?;
            let surface = Entity {
                type_code: 196,
                form: 1,
                label: "SPHERE",
                status: "00000000",
                parameters: format!(
                    "196,{},{},{},{};",
                    reference_marker(location),
                    number(*radius),
                    reference_marker(axis),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            if !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *minor_radius <= 0.0
                || *minor_radius >= *major_radius
            {
                return Err(CodecError::Malformed(
                    "IGES torus radii must satisfy 0 < minor < major".into(),
                ));
            }
            let (mut entities, location, axis, reference) =
                pointer_surface_support(base_index, *center, *axis, *ref_direction)?;
            let surface = Entity {
                type_code: 198,
                form: 1,
                label: "TORUS",
                status: "00000000",
                parameters: format!(
                    "198,{},{},{},{},{};",
                    reference_marker(location),
                    reference_marker(axis),
                    number(*major_radius),
                    number(*minor_radius),
                    reference_marker(reference)
                )
                .into_bytes(),
                transform: None,
            };
            entities.push(surface);
            Ok(entities)
        }
        other => Err(CodecError::NotImplemented(format!(
            "IGES semantic writer does not encode surface geometry {other:?}"
        ))),
    }
}

fn encode_nurbs_surface(nurbs: &NurbsSurface) -> Result<Entity, CodecError> {
    let u_count = usize::try_from(nurbs.u_count)
        .map_err(|_| CodecError::Malformed("IGES surface u count overflows usize".into()))?;
    let v_count = usize::try_from(nurbs.v_count)
        .map_err(|_| CodecError::Malformed("IGES surface v count overflows usize".into()))?;
    let u_degree = usize::try_from(nurbs.u_degree)
        .map_err(|_| CodecError::Malformed("IGES surface u degree overflows usize".into()))?;
    let v_degree = usize::try_from(nurbs.v_degree)
        .map_err(|_| CodecError::Malformed("IGES surface v degree overflows usize".into()))?;
    let pole_count = u_count.checked_mul(v_count).ok_or_else(|| {
        CodecError::Malformed("IGES surface control-point count overflows".into())
    })?;
    let u_knot_count = u_count
        .checked_add(u_degree)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| CodecError::Malformed("IGES surface u knot count overflows".into()))?;
    let v_knot_count = v_count
        .checked_add(v_degree)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(|| CodecError::Malformed("IGES surface v knot count overflows".into()))?;
    if u_count == 0
        || v_count == 0
        || u_degree == 0
        || v_degree == 0
        || u_degree >= u_count
        || v_degree >= v_count
        || nurbs.control_points.len() != pole_count
        || nurbs.u_knots.len() != u_knot_count
        || nurbs.v_knots.len() != v_knot_count
        || nurbs.u_knots.iter().any(|value| !value.is_finite())
        || nurbs.v_knots.iter().any(|value| !value.is_finite())
        || nurbs.u_knots.windows(2).any(|pair| pair[0] > pair[1])
        || nurbs.v_knots.windows(2).any(|pair| pair[0] > pair[1])
        || nurbs.control_points.iter().any(|point| {
            [point.x, point.y, point.z]
                .iter()
                .any(|value| !value.is_finite())
        })
    {
        return Err(CodecError::Malformed(
            "IGES NURBS surface dimensions, knots, or poles are invalid".into(),
        ));
    }
    let weights = match &nurbs.weights {
        Some(weights) if weights.len() == pole_count => {
            if weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
            {
                return Err(CodecError::Malformed(
                    "IGES NURBS surface weights must be finite and positive".into(),
                ));
            }
            weights.clone()
        }
        Some(_) => {
            return Err(CodecError::Malformed(
                "IGES NURBS surface weight count does not match poles".into(),
            ));
        }
        None => vec![1.0; pole_count],
    };
    let u_range = [nurbs.u_knots[u_degree], nurbs.u_knots[u_count]];
    let v_range = [nurbs.v_knots[v_degree], nurbs.v_knots[v_count]];
    if u_range[0] >= u_range[1] || v_range[0] >= v_range[1] {
        return Err(CodecError::Malformed(
            "IGES NURBS surface has an empty parameter domain".into(),
        ));
    }
    let closed_u = nurbs.u_periodic || nurbs_surface_closed_u(nurbs, u_range, v_range);
    let closed_v = nurbs.v_periodic || nurbs_surface_closed_v(nurbs, u_range, v_range);
    let mut parameters = format!(
        "128,{},{},{},{},{},{},{},{},{}",
        u_count - 1,
        v_count - 1,
        nurbs.u_degree,
        nurbs.v_degree,
        i32::from(closed_u),
        i32::from(closed_v),
        i32::from(nurbs.weights.is_none()),
        i32::from(nurbs.u_periodic),
        i32::from(nurbs.v_periodic)
    );
    for value in &nurbs.u_knots {
        parameters.push(',');
        parameters.push_str(&number(*value));
    }
    for value in &nurbs.v_knots {
        parameters.push(',');
        parameters.push_str(&number(*value));
    }
    for v in 0..v_count {
        for u in 0..u_count {
            parameters.push(',');
            parameters.push_str(&number(weights[u * v_count + v]));
        }
    }
    for v in 0..v_count {
        for u in 0..u_count {
            let point = nurbs.control_points[u * v_count + v];
            for value in [point.x, point.y, point.z] {
                parameters.push(',');
                parameters.push_str(&number(value));
            }
        }
    }
    for value in [u_range[0], u_range[1], v_range[0], v_range[1]] {
        parameters.push(',');
        parameters.push_str(&number(value));
    }
    parameters.push(';');
    Ok(Entity {
        type_code: 128,
        form: 0,
        label: "NURBS",
        status: "00000000",
        parameters: parameters.into_bytes(),
        transform: None,
    })
}

fn nurbs_surface_closed_u(nurbs: &NurbsSurface, u_range: [f64; 2], v_range: [f64; 2]) -> bool {
    [v_range[0], v_range[0].midpoint(v_range[1]), v_range[1]]
        .into_iter()
        .all(|v| {
            let Some(start) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u_range[0], v) else {
                return false;
            };
            let Some(end) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u_range[1], v) else {
                return false;
            };
            close_point(start, end)
        })
}

fn nurbs_surface_closed_v(nurbs: &NurbsSurface, u_range: [f64; 2], v_range: [f64; 2]) -> bool {
    [u_range[0], u_range[0].midpoint(u_range[1]), u_range[1]]
        .into_iter()
        .all(|u| {
            let Some(start) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u, v_range[0]) else {
                return false;
            };
            let Some(end) = cadmpeg_ir::eval::nurbs_surface_point(nurbs, u, v_range[1]) else {
                return false;
            };
            close_point(start, end)
        })
}

fn vertex_point_id(ir: &CadIr, vertex_id: &VertexId) -> Result<PointId, CodecError> {
    let vertex = ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *vertex_id)
        .ok_or_else(|| {
            CodecError::Malformed(format!("IGES edge references missing vertex {vertex_id}"))
        })?;
    Ok(vertex.point.clone())
}

fn point_position(ir: &CadIr, point_id: &PointId) -> Result<Point3, CodecError> {
    ir.model
        .points
        .iter()
        .find(|point| point.id == *point_id)
        .map(|point| point.position)
        .ok_or_else(|| {
            CodecError::Malformed(format!("IGES topology references missing point {point_id}"))
        })
}

fn vertex_position(ir: &CadIr, vertex_id: &VertexId) -> Option<Point3> {
    let point_id = ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == *vertex_id)?
        .point
        .clone();
    ir.model
        .points
        .iter()
        .find(|point| point.id == point_id)
        .map(|point| point.position)
}

fn edge_span(ir: &CadIr, edge: &Edge, geometry: &CurveGeometry) -> Result<CurveSpan, CodecError> {
    let range = edge.param_range.ok_or_else(|| {
        CodecError::NotImplemented(format!(
            "IGES semantic writer requires a parameter range for edge {}",
            edge.id
        ))
    })?;
    if range.iter().any(|value| !value.is_finite()) || range[0] > range[1] {
        return Err(CodecError::Malformed(format!(
            "IGES edge {} has an invalid parameter range",
            edge.id
        )));
    }
    let start = point_position(ir, &vertex_point_id(ir, &edge.start)?)?;
    let end = point_position(ir, &vertex_point_id(ir, &edge.end)?)?;
    ensure_finite_point(start, &format!("edge {} start", edge.id))?;
    ensure_finite_point(end, &format!("edge {} end", edge.id))?;
    if matches!(
        geometry,
        CurveGeometry::Circle { .. }
            | CurveGeometry::Ellipse { .. }
            | CurveGeometry::Parabola { .. }
            | CurveGeometry::Hyperbola { .. }
            | CurveGeometry::Nurbs(_)
            | CurveGeometry::Polyline { .. }
    ) {
        let evaluated_start = curve_point(geometry, range[0]).ok_or_else(|| {
            CodecError::Malformed(format!(
                "IGES edge {} start cannot be evaluated on its curve",
                edge.id
            ))
        })?;
        let evaluated_end = curve_point(geometry, range[1]).ok_or_else(|| {
            CodecError::Malformed(format!(
                "IGES edge {} end cannot be evaluated on its curve",
                edge.id
            ))
        })?;
        if !close_point(start, evaluated_start) || !close_point(end, evaluated_end) {
            return Err(CodecError::Malformed(format!(
                "IGES edge {} endpoints disagree with its curve parameter range",
                edge.id
            )));
        }
    }
    Ok(CurveSpan { range, start, end })
}

fn default_range(geometry: &CurveGeometry) -> Result<[f64; 2], CodecError> {
    match geometry {
        CurveGeometry::Circle { .. } | CurveGeometry::Ellipse { .. } => Ok([0.0, TAU]),
        CurveGeometry::Nurbs(nurbs) => nurbs_domain(nurbs),
        CurveGeometry::Polyline {
            points, parameters, ..
        } => {
            if points.len() < 2 {
                return Err(CodecError::NotImplemented(
                    "IGES semantic writer requires at least two polyline points".into(),
                ));
            }
            let values = polyline_parameters(points.len(), parameters.as_deref())?;
            Ok([values[0], *values.last().expect("polyline has points")])
        }
        CurveGeometry::Line { .. }
        | CurveGeometry::Parabola { .. }
        | CurveGeometry::Hyperbola { .. }
        | CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer requires a finite curve parameter range".into(),
        )),
        CurveGeometry::Transformed { .. } => Err(CodecError::Malformed(
            "IGES transformed curve was not flattened before encoding".into(),
        )),
    }
}

fn curve_entity(geometry: &CurveGeometry, span: Option<&CurveSpan>) -> Result<Entity, CodecError> {
    let range = span.map_or_else(|| default_range(geometry), |span| Ok(span.range))?;
    if range.iter().any(|value| !value.is_finite()) || range[0] > range[1] {
        return Err(CodecError::Malformed(
            "IGES curve parameter range is invalid".into(),
        ));
    }
    match geometry {
        CurveGeometry::Line { .. } => {
            let span = span.ok_or_else(|| {
                CodecError::NotImplemented(
                    "IGES semantic writer cannot bound an unreferenced line".into(),
                )
            })?;
            Ok(Entity {
                type_code: 110,
                form: 0,
                label: "LINE",
                status: "00000000",
                parameters: format!(
                    "110,{},{},{},{},{},{};",
                    number(span.start.x),
                    number(span.start.y),
                    number(span.start.z),
                    number(span.end.x),
                    number(span.end.y),
                    number(span.end.z)
                )
                .into_bytes(),
                transform: None,
            })
        }
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => {
            let axis = unit(*axis, "circle axis")?;
            let reference = unit(*ref_direction, "circle reference direction")?;
            if axis.dot(reference).abs() > 1.0e-10 || !radius.is_finite() || *radius <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES circle basis is not an orthonormal positive-radius frame".into(),
                ));
            }
            let y_axis = axis.cross(reference);
            let end = effective_arc_end(range, true)?;
            let start_xy = [radius * range[0].cos(), radius * range[0].sin()];
            let end_xy = [radius * end.cos(), radius * end.sin()];
            Ok(Entity {
                type_code: 100,
                form: 0,
                label: "ARC",
                status: "00000000",
                parameters: format!(
                    "100,0,0,0,{},{},{},{};",
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, reference, y_axis, axis)?),
            })
        }
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            let axis = unit(*axis, "ellipse axis")?;
            let major = unit(*major_direction, "ellipse major direction")?;
            if axis.dot(major).abs() > 1.0e-10
                || !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *major_radius <= 0.0
                || *minor_radius <= 0.0
            {
                return Err(CodecError::Malformed(
                    "IGES ellipse basis or radii are invalid".into(),
                ));
            }
            let y_axis = axis.cross(major);
            let end = effective_arc_end(range, true)?;
            let start_xy = [major_radius * range[0].cos(), minor_radius * range[0].sin()];
            let end_xy = [major_radius * end.cos(), minor_radius * end.sin()];
            Ok(Entity {
                type_code: 104,
                form: 0,
                label: "CONIC",
                status: "00000000",
                parameters: format!(
                    "104,{},0,{},0,0,-1,0,{},{},{},{};",
                    number(1.0 / (major_radius * major_radius)),
                    number(1.0 / (minor_radius * minor_radius)),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, major, y_axis, axis)?),
            })
        }
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => {
            if range[0] == range[1] || !focal_distance.is_finite() || *focal_distance <= 0.0 {
                return Err(CodecError::Malformed(
                    "IGES parabola requires a finite non-zero parameter span".into(),
                ));
            }
            let axis = unit(*axis, "parabola axis")?;
            let major = unit(*major_direction, "parabola major direction")?;
            if axis.dot(major).abs() > 1.0e-10 {
                return Err(CodecError::Malformed(
                    "IGES parabola basis is not orthogonal".into(),
                ));
            }
            let x_axis = major.cross(axis);
            let start_xy = parabola_point(*focal_distance, range[0])?;
            let end_xy = parabola_point(*focal_distance, range[1])?;
            Ok(Entity {
                type_code: 104,
                form: 3,
                label: "CONIC",
                status: "00000000",
                parameters: format!(
                    "104,1,0,0,0,{},0,0,{},{},{},{};",
                    number(-4.0 * focal_distance),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*vertex, x_axis, major, axis)?),
            })
        }
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => {
            if range[0] == range[1]
                || !major_radius.is_finite()
                || !minor_radius.is_finite()
                || *major_radius <= 0.0
                || *minor_radius <= 0.0
            {
                return Err(CodecError::Malformed(
                    "IGES hyperbola requires positive radii and a finite span".into(),
                ));
            }
            let axis = unit(*axis, "hyperbola axis")?;
            let major = unit(*major_direction, "hyperbola major direction")?;
            if axis.dot(major).abs() > 1.0e-10 {
                return Err(CodecError::Malformed(
                    "IGES hyperbola basis is not orthogonal".into(),
                ));
            }
            let y_axis = axis.cross(major);
            let start_xy = hyperbola_point(*major_radius, *minor_radius, range[0])?;
            let end_xy = hyperbola_point(*major_radius, *minor_radius, range[1])?;
            Ok(Entity {
                type_code: 104,
                form: 2,
                label: "CONIC",
                status: "00000000",
                parameters: format!(
                    "104,{},0,{},0,0,-1,0,{},{},{},{};",
                    number(1.0 / (major_radius * major_radius)),
                    number(-1.0 / (minor_radius * minor_radius)),
                    number(start_xy[0]),
                    number(start_xy[1]),
                    number(end_xy[0]),
                    number(end_xy[1])
                )
                .into_bytes(),
                transform: Some(placement(*center, major, y_axis, axis)?),
            })
        }
        CurveGeometry::Nurbs(nurbs) => encode_nurbs(nurbs, range, "NURBS"),
        CurveGeometry::Polyline {
            points, parameters, ..
        } => {
            let values = polyline_parameters(points.len(), parameters.as_deref())?;
            let nurbs = NurbsCurve {
                degree: 1,
                knots: polyline_knots(&values),
                control_points: points.clone(),
                weights: None,
                periodic: false,
            };
            encode_nurbs(&nurbs, range, "POLYLINE")
        }
        CurveGeometry::Degenerate { .. }
        | CurveGeometry::Composite { .. }
        | CurveGeometry::Procedural { .. }
        | CurveGeometry::Unknown { .. }
        | CurveGeometry::Transformed { .. } => Err(CodecError::NotImplemented(
            "IGES semantic writer does not encode this curve geometry".into(),
        )),
    }
}

fn encode_nurbs(
    nurbs: &NurbsCurve,
    range: [f64; 2],
    label: &'static str,
) -> Result<Entity, CodecError> {
    let control_count = nurbs.control_points.len();
    let degree = usize::try_from(nurbs.degree)
        .map_err(|_| CodecError::Malformed("IGES NURBS degree overflows usize".into()))?;
    if control_count == 0
        || degree == 0
        || degree >= control_count
        || nurbs.knots.len() != control_count + degree + 1
        || range[0] > range[1]
        || range.iter().any(|value| !value.is_finite())
        || nurbs.knots.iter().any(|value| !value.is_finite())
        || nurbs.knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Err(CodecError::Malformed(
            "IGES NURBS degree, knot vector, or parameter range is invalid".into(),
        ));
    }
    let domain = [nurbs.knots[degree], nurbs.knots[control_count]];
    if range[0] < domain[0] || range[1] > domain[1] {
        return Err(CodecError::Malformed(
            "IGES NURBS parameter range lies outside its knot domain".into(),
        ));
    }
    if nurbs
        .control_points
        .iter()
        .any(|point| !point.x.is_finite() || !point.y.is_finite() || !point.z.is_finite())
    {
        return Err(CodecError::Malformed(
            "IGES NURBS control point is non-finite".into(),
        ));
    }
    let weights = match &nurbs.weights {
        Some(weights) if weights.len() == control_count => {
            if weights
                .iter()
                .any(|weight| !weight.is_finite() || *weight <= 0.0)
            {
                return Err(CodecError::Malformed(
                    "IGES NURBS weights must be finite and positive".into(),
                ));
            }
            weights.clone()
        }
        Some(_) => {
            return Err(CodecError::Malformed(
                "IGES NURBS weight count does not match control points".into(),
            ));
        }
        None => vec![1.0; control_count],
    };
    let polynomial = nurbs.weights.is_none();
    let planar = nurbs_control_polygon_is_planar(&nurbs.control_points);
    let closed = nurbs_is_closed(nurbs, &weights, domain);
    let k = control_count - 1;
    let mut parameters = format!(
        "126,{k},{},{},{},{},{}",
        nurbs.degree,
        i32::from(planar),
        i32::from(closed),
        i32::from(polynomial),
        i32::from(nurbs.periodic)
    );
    for value in &nurbs.knots {
        parameters.push(',');
        parameters.push_str(&number(*value));
    }
    for weight in weights {
        parameters.push(',');
        parameters.push_str(&number(weight));
    }
    for point in &nurbs.control_points {
        for value in [point.x, point.y, point.z] {
            parameters.push(',');
            parameters.push_str(&number(value));
        }
    }
    parameters.push(',');
    parameters.push_str(&number(range[0]));
    parameters.push(',');
    parameters.push_str(&number(range[1]));
    parameters.push(';');
    let status = if label == "PCURVE" {
        "00010500"
    } else {
        "00000000"
    };
    Ok(Entity {
        type_code: 126,
        form: 0,
        label,
        status,
        parameters: parameters.into_bytes(),
        transform: None,
    })
}

fn nurbs_control_polygon_is_planar(points: &[Point3]) -> bool {
    let Some(origin) = points.first().copied() else {
        return true;
    };
    let distances = points
        .iter()
        .map(|point| point.distance(origin))
        .collect::<Vec<_>>();
    if distances.iter().any(|distance| !distance.is_finite()) {
        return false;
    }
    let scale = distances.into_iter().fold(1.0, f64::max);
    let tolerance = 1.0e-10 * scale;
    let mut first_direction = None;
    for point in points.iter().skip(1) {
        let direction = point.vector_from(origin);
        let length = direction.norm();
        if !length.is_finite() {
            return false;
        }
        if length > tolerance {
            first_direction = Some(direction);
            break;
        }
    }
    let Some(first_direction) = first_direction else {
        return true;
    };
    let normal_threshold = 1.0e-12 * scale * first_direction.norm();
    let mut normal = None;
    for point in points.iter().skip(1) {
        let candidate = first_direction.cross(point.vector_from(origin));
        let length = candidate.norm();
        if !length.is_finite() {
            return false;
        }
        if length > normal_threshold {
            normal = Some(candidate);
            break;
        }
    }
    let Some(normal) = normal else {
        return true;
    };
    let normal_length = normal.norm();
    if !normal_length.is_finite() || normal_length <= f64::EPSILON {
        return false;
    }
    let unit_normal = normal.scale(1.0 / normal_length);
    points
        .iter()
        .all(|point| unit_normal.dot(point.vector_from(origin)).abs() <= tolerance)
}

fn nurbs_is_closed(nurbs: &NurbsCurve, weights: &[f64], domain: [f64; 2]) -> bool {
    if nurbs.periodic {
        return true;
    }
    let Some(start) = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        Some(weights),
        domain[0],
    ) else {
        return false;
    };
    let Some(end) = cadmpeg_ir::eval::nurbs_curve_point(
        nurbs.degree,
        &nurbs.knots,
        &nurbs.control_points,
        Some(weights),
        domain[1],
    ) else {
        return false;
    };
    let scale = nurbs
        .control_points
        .iter()
        .map(|point| point.distance(start))
        .filter(|distance| distance.is_finite())
        .fold(1.0, f64::max);
    start.distance(end) <= 1.0e-10 * scale
}

fn flatten_curve(geometry: &CurveGeometry) -> Result<CurveGeometry, CodecError> {
    match geometry {
        CurveGeometry::Transformed { basis, transform } => {
            if !transform.is_proper_rigid() {
                return Err(CodecError::NotImplemented(
                    "IGES semantic writer only applies proper-rigid curve transforms".into(),
                ));
            }
            let basis = flatten_curve(basis)?;
            apply_rigid_transform(basis, *transform)
        }
        _ => Ok(geometry.clone()),
    }
}

fn apply_rigid_transform(
    geometry: CurveGeometry,
    transform: cadmpeg_ir::transform::Transform,
) -> Result<CurveGeometry, CodecError> {
    let point = |value: Point3| transform.apply_point(value);
    let vector = |value: Vector3, label: &str| unit(transform.apply_vector(value), label);
    Ok(match geometry {
        CurveGeometry::Line { origin, direction } => CurveGeometry::Line {
            origin: point(origin),
            direction: vector(direction, "transformed line direction")?,
        },
        CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        } => CurveGeometry::Circle {
            center: point(center),
            axis: vector(axis, "transformed circle axis")?,
            ref_direction: vector(ref_direction, "transformed circle reference")?,
            radius,
        },
        CurveGeometry::Ellipse {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Ellipse {
            center: point(center),
            axis: vector(axis, "transformed ellipse axis")?,
            major_direction: vector(major_direction, "transformed ellipse major")?,
            major_radius,
            minor_radius,
        },
        CurveGeometry::Parabola {
            vertex,
            axis,
            major_direction,
            focal_distance,
        } => CurveGeometry::Parabola {
            vertex: point(vertex),
            axis: vector(axis, "transformed parabola axis")?,
            major_direction: vector(major_direction, "transformed parabola major")?,
            focal_distance,
        },
        CurveGeometry::Hyperbola {
            center,
            axis,
            major_direction,
            major_radius,
            minor_radius,
        } => CurveGeometry::Hyperbola {
            center: point(center),
            axis: vector(axis, "transformed hyperbola axis")?,
            major_direction: vector(major_direction, "transformed hyperbola major")?,
            major_radius,
            minor_radius,
        },
        CurveGeometry::Degenerate { point: value } => CurveGeometry::Degenerate {
            point: point(value),
        },
        CurveGeometry::Nurbs(mut nurbs) => {
            nurbs.control_points = nurbs.control_points.into_iter().map(point).collect();
            CurveGeometry::Nurbs(nurbs)
        }
        CurveGeometry::Polyline {
            points,
            parameters,
            chordal_deflection,
        } => CurveGeometry::Polyline {
            points: points.into_iter().map(point).collect(),
            parameters,
            chordal_deflection,
        },
        other => {
            return Err(CodecError::NotImplemented(format!(
                "IGES semantic writer cannot flatten curve geometry {other:?}"
            )))
        }
    })
}

fn unit(vector: Vector3, label: &str) -> Result<Vector3, CodecError> {
    let norm = vector.norm();
    if !norm.is_finite() || norm <= f64::EPSILON {
        return Err(CodecError::Malformed(format!("IGES {label} is degenerate")));
    }
    Ok(vector.scale(1.0 / norm))
}

fn placement(
    origin: Point3,
    x_axis: Vector3,
    y_axis: Vector3,
    z_axis: Vector3,
) -> Result<Placement, CodecError> {
    ensure_finite_point(origin, "placement origin")?;
    let x_axis = unit(x_axis, "placement x axis")?;
    let y_axis = unit(y_axis, "placement y axis")?;
    let z_axis = unit(z_axis, "placement z axis")?;
    if x_axis.dot(y_axis).abs() > 1.0e-10
        || x_axis.dot(z_axis).abs() > 1.0e-10
        || y_axis.dot(z_axis).abs() > 1.0e-10
        || x_axis.cross(y_axis).dot(z_axis) < 1.0 - 1.0e-10
    {
        return Err(CodecError::Malformed(
            "IGES placement axes are not a right-handed orthonormal frame".into(),
        ));
    }
    Ok(Placement {
        rows: [
            [x_axis.x, y_axis.x, z_axis.x, origin.x],
            [x_axis.y, y_axis.y, z_axis.y, origin.y],
            [x_axis.z, y_axis.z, z_axis.z, origin.z],
        ],
    })
}

fn effective_arc_end(range: [f64; 2], closed: bool) -> Result<f64, CodecError> {
    let sweep = range[1] - range[0];
    if !(0.0..=TAU + 1.0e-10).contains(&sweep) {
        return Err(CodecError::NotImplemented(
            "IGES conic writer requires an ordered span no larger than one revolution".into(),
        ));
    }
    if closed && sweep <= 1.0e-14 {
        Ok(range[0] + TAU)
    } else {
        Ok(range[1])
    }
}

fn parabola_point(focal_distance: f64, parameter: f64) -> Result<[f64; 2], CodecError> {
    let point = [
        -2.0 * focal_distance * parameter,
        focal_distance * parameter * parameter,
    ];
    point
        .iter()
        .all(|value| value.is_finite())
        .then_some(point)
        .ok_or_else(|| CodecError::Malformed("IGES parabola endpoint is non-finite".into()))
}

fn hyperbola_point(
    major_radius: f64,
    minor_radius: f64,
    parameter: f64,
) -> Result<[f64; 2], CodecError> {
    let point = [
        major_radius * parameter.cosh(),
        minor_radius * parameter.sinh(),
    ];
    point
        .iter()
        .all(|value| value.is_finite())
        .then_some(point)
        .ok_or_else(|| CodecError::Malformed("IGES hyperbola endpoint is non-finite".into()))
}

fn nurbs_domain(nurbs: &NurbsCurve) -> Result<[f64; 2], CodecError> {
    let degree = usize::try_from(nurbs.degree)
        .map_err(|_| CodecError::Malformed("IGES NURBS degree overflows usize".into()))?;
    let end = nurbs.control_points.len();
    if nurbs.knots.len() <= end || degree >= nurbs.knots.len() {
        return Err(CodecError::Malformed(
            "IGES NURBS knot vector cannot provide a domain".into(),
        ));
    }
    Ok([nurbs.knots[degree], nurbs.knots[end]])
}

fn polyline_parameters(count: usize, parameters: Option<&[f64]>) -> Result<Vec<f64>, CodecError> {
    if count < 2 {
        return Err(CodecError::NotImplemented(
            "IGES semantic writer requires at least two polyline points".into(),
        ));
    }
    let values = parameters.map_or_else(
        || (0..count).map(|value| value as f64).collect(),
        <[f64]>::to_vec,
    );
    if values.len() != count
        || values.iter().any(|value| !value.is_finite())
        || values.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err(CodecError::Malformed(
            "IGES polyline parameters must be finite and strictly increasing".into(),
        ));
    }
    Ok(values)
}

fn polyline_knots(parameters: &[f64]) -> Vec<f64> {
    let mut knots = Vec::with_capacity(parameters.len() + 2);
    knots.extend([parameters[0], parameters[0]]);
    knots.extend_from_slice(&parameters[1..parameters.len() - 1]);
    knots.extend([*parameters.last().expect("polyline has points"); 2]);
    knots
}

fn close_point(left: Point3, right: Point3) -> bool {
    let scale = left
        .x
        .abs()
        .max(left.y.abs())
        .max(left.z.abs())
        .max(right.x.abs())
        .max(right.y.abs())
        .max(right.z.abs())
        .max(1.0);
    let tolerance = scale * 1.0e-8;
    (left.x - right.x).abs() <= tolerance
        && (left.y - right.y).abs() <= tolerance
        && (left.z - right.z).abs() <= tolerance
}

fn ensure_finite_point(point: Point3, label: &str) -> Result<(), CodecError> {
    if [point.x, point.y, point.z]
        .iter()
        .all(|value| value.is_finite())
    {
        Ok(())
    } else {
        Err(CodecError::Malformed(format!(
            "IGES point {label} has non-finite coordinates"
        )))
    }
}

#[derive(Clone)]
struct Entity {
    type_code: u32,
    form: i64,
    label: &'static str,
    status: &'static str,
    parameters: Vec<u8>,
    transform: Option<Placement>,
}

fn encode_file(entities: &[Entity], version: crate::IgesVersion) -> Result<Vec<u8>, CodecError> {
    let global = format!(
        "1H,,1H;,7Hcadmpeg,13Hgenerated.igs,7Hcadmpeg,3H0.1,32,38,6,308,15,0H,1.0,2,2HMM,1,1.0,15H20260807.000000,0.001,1000.0,6Hauthor,7Hcadmpeg,{},0,0H,0H;",
        version.global_flag()
    )
    .into_bytes();
    let global_count = global.len().div_ceil(72);
    let mut expanded = Vec::with_capacity(entities.len() * 2);
    for entity in entities {
        if let Some(placement) = entity.transform {
            let transform_parameters = placement
                .rows
                .iter()
                .flatten()
                .map(|value| number(*value))
                .collect::<Vec<_>>()
                .join(",");
            expanded.push((
                Entity {
                    type_code: 124,
                    form: 0,
                    label: "XFORM",
                    status: "00000000",
                    parameters: format!("124,{transform_parameters};").into_bytes(),
                    transform: None,
                },
                0_u32,
            ));
            let transform_sequence = u32::try_from(expanded.len())
                .ok()
                .and_then(|value| value.checked_mul(2).and_then(|value| value.checked_sub(1)))
                .ok_or_else(|| {
                    CodecError::Malformed("IGES transformation sequence overflows".into())
                })?;
            let mut entity = entity.clone();
            entity.transform = None;
            expanded.push((entity, transform_sequence));
        } else {
            expanded.push((entity.clone(), 0));
        }
    }
    let mut parameter_sequence = 1_u32;
    let mut directory = Vec::with_capacity(expanded.len() * 2);
    let mut parameters = Vec::new();
    for (index, (entity, transform_sequence)) in expanded.iter().enumerate() {
        let directory_sequence = u32::try_from(index)
            .ok()
            .and_then(|value| value.checked_mul(2))
            .and_then(|value| value.checked_add(1))
            .ok_or_else(|| CodecError::Malformed("IGES directory sequence overflows".into()))?;
        let parameter_count = entity.parameters.len().div_ceil(64);
        let parameter_count = u32::try_from(parameter_count)
            .map_err(|_| CodecError::Malformed("IGES parameter count overflows".into()))?;
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                parameter_sequence.to_string(),
                "0".into(),
                "0".into(),
                "0".into(),
                "0".into(),
                transform_sequence.to_string(),
                "0".into(),
                entity.status.into(),
            ],
            directory_sequence,
        )?);
        directory.push(directory_card(
            [
                entity.type_code.to_string(),
                "0".into(),
                "0".into(),
                parameter_count.to_string(),
                entity.form.to_string(),
                String::new(),
                String::new(),
                entity.label.to_owned(),
                "0".into(),
            ],
            directory_sequence + 1,
        )?);
        for chunk in entity.parameters.chunks(64) {
            parameters.push(parameter_card(
                chunk,
                directory_sequence,
                parameter_sequence,
            )?);
            parameter_sequence = parameter_sequence
                .checked_add(1)
                .ok_or_else(|| CodecError::Malformed("IGES parameter sequence overflows".into()))?;
        }
    }
    let mut bytes = Vec::new();
    bytes.extend(card(b"Generated by cadmpeg", b'S', 1)?);
    for (index, chunk) in global.chunks(72).enumerate() {
        bytes.extend(card(
            chunk,
            b'G',
            u32::try_from(index + 1).unwrap_or(u32::MAX),
        )?);
    }
    for card_bytes in directory {
        bytes.extend(card_bytes);
    }
    for card_bytes in parameters {
        bytes.extend(card_bytes);
    }
    let directory_count = expanded
        .len()
        .checked_mul(2)
        .ok_or_else(|| CodecError::Malformed("IGES directory count overflows".into()))?;
    let directory_count = u32::try_from(directory_count)
        .map_err(|_| CodecError::Malformed("IGES directory count overflows".into()))?;
    let parameter_count = parameter_sequence - 1;
    let terminate = format!(
        "S{start_count:07}G{global_count:07}D{directory_count:07}P{parameter_count:07}",
        start_count = 1
    );
    bytes.extend(card(terminate.as_bytes(), b'T', 1)?);
    Ok(bytes)
}

fn directory_card(fields: [String; 9], sequence: u32) -> Result<Vec<u8>, CodecError> {
    let mut payload = Vec::with_capacity(72);
    for field in fields {
        if field.len() > 8 {
            return Err(CodecError::Malformed(format!(
                "IGES Directory field is wider than eight bytes: {field}"
            )));
        }
        payload.extend_from_slice(format!("{field:>8}").as_bytes());
    }
    card(&payload, b'D', sequence)
}

fn parameter_card(
    data: &[u8],
    directory_sequence: u32,
    sequence: u32,
) -> Result<Vec<u8>, CodecError> {
    if data.len() > 64 {
        return Err(CodecError::Malformed(
            "IGES Parameter Data payload exceeds 64 bytes".into(),
        ));
    }
    let mut payload = vec![b' '; 72];
    payload[..data.len()].copy_from_slice(data);
    let pointer = format!("{directory_sequence:>8}");
    payload[64..].copy_from_slice(pointer.as_bytes());
    card(&payload, b'P', sequence)
}

fn card(data: &[u8], section: u8, sequence: u32) -> Result<Vec<u8>, CodecError> {
    let width = 72;
    if data.len() > width {
        return Err(CodecError::Malformed(
            "IGES card payload exceeds 72 bytes".into(),
        ));
    }
    let mut payload = vec![b' '; 80];
    payload[..data.len()].copy_from_slice(data);
    payload[72] = section;
    let sequence = format!("{sequence:>7}");
    payload[73..].copy_from_slice(sequence.as_bytes());
    payload.push(b'\n');
    Ok(payload)
}

fn number(value: f64) -> String {
    if value == 0.0 {
        "0".into()
    } else {
        format!("{value:.17}")
    }
}
