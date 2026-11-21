// SPDX-License-Identifier: Apache-2.0
//! Content the semantic writer cannot hold (§3.4, "Content the target cannot
//! hold"): each item is dropped or degraded with a loss naming it, and the
//! rest of the document is written.

use std::collections::HashMap;

use cadmpeg_core::decode::ResourceLimit;
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{edit_dropped, output_empty, record_invalid};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::curve_point_solved;
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::report::check::ValidationReport;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity;
use cadmpeg_ir::schema::EntityKind;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::{Annotations, Exactness};

use crate::loss::SldprtLossCode;

/// The linear resolution within which a vertex fixes an edge end on its
/// carrier when the vertex states no larger tolerance (`sldprt.md`, B-rep
/// edge ranges), in millimetres.
const LINEAR_RESOLUTION_MM: f64 = 1.0e-5;
/// Relative roundoff within which a range end equals a NURBS domain end.
const DOMAIN_END_ROUNDOFF_RELATIVE: f64 = 1.0e-9;

/// The writer bounds every edge by its vertices. A range the decode derives
/// from those vertices is stated by them; any other range is not written.
pub(super) fn edge_ranges_omitted(
    ir: &CadIr,
    annotations: &Annotations,
) -> Result<Option<LossNote>, ResourceLimit> {
    let points = ir
        .geometry
        .points
        .iter()
        .map(|point| (point.id.as_str(), point.position().get()))
        .collect::<HashMap<_, _>>();
    let vertices = ir
        .geometry
        .vertices
        .iter()
        .filter_map(|vertex| {
            let bound = vertex.tolerance.map_or(LINEAR_RESOLUTION_MM, |tolerance| {
                tolerance.get().max(LINEAR_RESOLUTION_MM)
            });
            Some((
                vertex.id.as_str(),
                (*points.get(vertex.point.as_str())?, bound),
            ))
        })
        .collect::<HashMap<_, _>>();
    let curves = ir
        .geometry
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    let mut omitted = Vec::new();
    for edge in &ir.geometry.edges {
        if edge.param_range().is_none() || derived_range(annotations, edge) {
            continue;
        }
        let Some(curve) = edge.curve() else {
            omitted.push(edge.id.as_str());
            continue;
        };
        let stated = match curves.get(curve.as_str()) {
            Some(CurveGeometry::Solved(geometry)) => {
                range_stated_by_vertices(edge, geometry, &vertices)?
            }
            _ => false,
        };
        if !stated {
            omitted.push(edge.id.as_str());
        }
    }
    Ok((!omitted.is_empty()).then(|| {
        SldprtLossCode::WriterEdgeRangesOmitted.note(format!(
            "{} edge(s) carry a parameter range their vertices do not fix on the curve; each is \
             written bounded by its vertices, without its range: {}",
            omitted.len(),
            omitted.join(", ")
        ))
    }))
}

fn derived_range(annotations: &Annotations, edge: &Edge) -> bool {
    annotations
        .exactness()
        .get(edge.id.as_str())
        .is_some_and(|note| {
            note.entity() == Exactness::Derived
                || note.fields().get("carrier.param_range") == Some(&Exactness::Derived)
        })
}

/// The curve at each range end lies within its vertex's bound. A NURBS
/// carrier that closes on itself gives its closure vertex two parameters, so
/// only its whole domain is stated, by a closed edge.
fn range_stated_by_vertices(
    edge: &Edge,
    geometry: &SolvedCurveGeometry,
    vertices: &HashMap<&str, (Point3, f64)>,
) -> Result<bool, ResourceLimit> {
    let (Some(range), Some(ends)) = (edge.param_range(), edge.ends.as_ref()) else {
        return Ok(false);
    };
    let [start_t, end_t] = range.get();
    let (Some(&(start, start_bound)), Some(&(end, end_bound))) = (
        vertices.get(ends.start.as_str()),
        vertices.get(ends.end.as_str()),
    ) else {
        return Ok(false);
    };
    // A point the carrier states no finite value for fixes no range end.
    let at =
        |parameter| match curve_point_solved(EvaluationAdmission::Standard, geometry, parameter) {
            Ok(point) => Ok(Some(point.get())),
            Err(failure) => failure.non_finite().map(|_| None),
        };
    let (Some(at_start), Some(at_end)) = (at(start_t)?, at(end_t)?) else {
        return Ok(false);
    };
    if Point3::distance(at_start, start) > start_bound || Point3::distance(at_end, end) > end_bound
    {
        return Ok(false);
    }
    let SolvedCurveGeometry::Nurbs(nurbs) = geometry else {
        return Ok(true);
    };
    let Some(domain) = cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs) else {
        return Ok(false);
    };
    let [low, high] = domain.endpoints();
    let (Some(first), Some(last)) = (at(low)?, at(high)?) else {
        return Ok(false);
    };
    if Point3::distance(first, last) > start_bound.max(end_bound) {
        return Ok(true);
    }
    let roundoff = (high - low) * DOMAIN_END_ROUNDOFF_RELATIVE;
    Ok(ends.start == ends.end
        && (start_t - low).abs() <= roundoff
        && (end_t - high).abs() <= roundoff)
}

/// Remove the content the `SolidWorks` form has no field for from the document
/// the writer writes, and name each record.
pub(super) fn unwritable_content(ir: &mut CadIr) -> Result<Vec<LossNote>, CodecError> {
    let mut losses = Vec::new();
    let mut named = Vec::new();
    for face in &mut ir.geometry.faces {
        if face.name.take().is_some() {
            named.push(face.id.as_str().to_owned());
        }
    }
    losses.extend(note(
        SldprtLossCode::WriterFaceNamesOmitted,
        "face(s) have a name the SLDPRT B-rep has no field for; each is written unnamed",
        &named,
    ));
    let mut named = Vec::new();
    for body in &mut ir.geometry.bodies {
        if body.color.is_none() && body.name.take().is_some() {
            named.push(body.id.as_str().to_owned());
        }
    }
    losses.extend(note(
        SldprtLossCode::WriterBodyNamesOmitted,
        "body or bodies have a name and no material, and the SLDPRT writer stores a body name \
         only as its material's; each is written unnamed",
        &named,
    ));
    let mut named = Vec::new();
    for appearance in &mut ir.model.appearances {
        let fields = [
            ("asset_guid", appearance.asset_guid.take().is_some()),
            ("visual_guid", appearance.visual_guid.take().is_some()),
            ("physical_token", appearance.physical_token.take().is_some()),
            ("category", appearance.category.take().is_some()),
            (
                "properties",
                !std::mem::take(&mut appearance.properties).is_empty(),
            ),
        ];
        let dropped = fields
            .iter()
            .filter(|(_, dropped)| *dropped)
            .map(|(field, _)| *field)
            .collect::<Vec<_>>();
        if !dropped.is_empty() {
            named.push(format!(
                "{} ({})",
                appearance.id.as_str(),
                dropped.join(", ")
            ));
        }
    }
    losses.extend(note(
        SldprtLossCode::WriterAppearanceFieldsOmitted,
        "appearance(s) have fields beside a name and base colour that the SLDPRT material \
         record has no field for; each is written with its name and base colour",
        &named,
    ));
    losses.extend(note(
        SldprtLossCode::WriterSubdOmitted,
        "subdivision surface(s) have no SLDPRT form and are not written",
        &std::mem::take(&mut ir.geometry.subds)
            .iter()
            .map(|subd| subd.id.as_str().to_owned())
            .collect::<Vec<_>>(),
    ));
    losses.extend(pmi(ir)?);
    losses.extend(active_configuration(ir));
    Ok(losses)
}

/// PMI with no SWIFT baseline comes from another format: the writer has no
/// PMI encoder, and the arena coverage (`Omitted` off the patched path)
/// charges it. PMI that differs from its baseline is an edit the retained
/// SWIFT records do not hold.
fn pmi(ir: &mut CadIr) -> Result<Option<LossNote>, CodecError> {
    let baseline = ir
        .source
        .as_ref()
        .and_then(|source| source.attributes.get(super::PMI_LOCAL_DIGEST_ATTRIBUTE))
        .cloned();
    let Some(baseline) = baseline else {
        ir.model.pmi.clear();
        return Ok(None);
    };
    if baseline == super::pmi_local_sha256(ir)? {
        return Ok(None);
    }
    let mut ids = ir
        .model
        .pmi
        .iter()
        .map(|pmi| pmi.id.as_str().to_owned())
        .collect::<Vec<_>>();
    if ids.is_empty() {
        ids.push("PMISemanticDataDB".to_owned());
    }
    Ok(edit_dropped(
        EntityKind::PmiAnnotation,
        "SLDPRT",
        "has no encoder for an edit of the retained SWIFT PMI and holds the PMI as the source \
         file stores it; the PMI digest does not isolate the edited record among these",
        ids,
    ))
}

/// `SolidWorks` stores one active configuration. Where the document states none
/// or several, the first by ordinal among the candidates is written active.
fn active_configuration(ir: &mut CadIr) -> Option<LossNote> {
    let configurations = &mut ir.model.configurations;
    let active = configurations
        .iter()
        .filter(|configuration| configuration.active)
        .count();
    if configurations.is_empty() || active == 1 {
        return None;
    }
    let chosen = configurations
        .iter()
        .filter(|configuration| active == 0 || configuration.active)
        .min_by_key(|configuration| configuration.ordinal)?
        .id
        .clone();
    let stated = configurations
        .iter()
        .filter(|configuration| configuration.active)
        .map(|configuration| configuration.id.as_str().to_owned())
        .collect::<Vec<_>>();
    for configuration in configurations.iter_mut() {
        configuration.active = configuration.id == chosen;
    }
    Some(
        SldprtLossCode::WriterActiveConfigurationAssumed.note(format!(
        "the document states {active} active configuration(s) ({}), and the SLDPRT writer stores \
         exactly one; it writes {} active, the first by ordinal",
        stated.join(", "),
        chosen.as_str()
    )),
    )
}

fn note(code: SldprtLossCode, reason: &str, ids: &[String]) -> Option<LossNote> {
    (!ids.is_empty()).then(|| code.note(format!("{} {reason}: {}", ids.len(), ids.join(", "))))
}

/// A document with no face writes partitions that hold no body.
pub(super) fn empty_brep(ir: &CadIr) -> Vec<LossNote> {
    let mut losses = Vec::new();
    losses.extend(note(
        SldprtLossCode::WriterBodiesOmitted,
        "body or bodies hold no face, and the SLDPRT partition is written with no body",
        &ir.geometry
            .bodies
            .iter()
            .map(|body| body.id.as_str().to_owned())
            .collect::<Vec<_>>(),
    ));
    if ir.model.tessellations.is_empty() {
        losses.push(output_empty(
            "the SLDPRT partition holds no body: the document holds no face and no tessellation",
        ));
    }
    losses
}

/// Each Error the written document's validation finds is written from the
/// canonical side (§3.4, Refusals). A finding whose record is in no arena
/// listed here is reported by the caller's own validation of the document.
pub(super) fn invalid_records(ir: &CadIr, validation: &ValidationReport) -> Vec<LossNote> {
    let kinds = record_kinds(ir);
    let mut grouped = Vec::<(EntityKind, Vec<String>)>::new();
    for finding in &validation.findings {
        if finding.severity < Severity::Error {
            continue;
        }
        let Some((id, kind)) = finding
            .entity
            .as_deref()
            .and_then(|id| Some((id, *kinds.get(id)?)))
        else {
            continue;
        };
        let entry = format!("{id} ({})", finding.message);
        match grouped.iter_mut().find(|(known, _)| *known == kind) {
            Some((_, entries)) => entries.push(entry),
            None => grouped.push((kind, vec![entry])),
        }
    }
    grouped
        .into_iter()
        .map(|(kind, entries)| {
            record_invalid(
                kind,
                format!(
                    "{} record(s) of the written document fail its validation and are written \
                     from the canonical side: {}",
                    entries.len(),
                    entries.join("; ")
                ),
            )
        })
        .collect()
}

fn record_kinds(ir: &CadIr) -> HashMap<&str, EntityKind> {
    let geometry = &ir.geometry;
    let model = &ir.model;
    let mut kinds = HashMap::new();
    kinds.extend(
        geometry
            .bodies
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Body)),
    );
    kinds.extend(
        geometry
            .regions
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Region)),
    );
    kinds.extend(
        geometry
            .shells
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Shell)),
    );
    kinds.extend(
        geometry
            .faces
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Face)),
    );
    kinds.extend(
        geometry
            .loops
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Loop)),
    );
    kinds.extend(
        geometry
            .coedges
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Coedge)),
    );
    kinds.extend(
        geometry
            .edges
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Edge)),
    );
    kinds.extend(
        geometry
            .vertices
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Vertex)),
    );
    kinds.extend(
        geometry
            .points
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Point)),
    );
    kinds.extend(
        geometry
            .surfaces
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Surface)),
    );
    kinds.extend(
        geometry
            .curves
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Curve)),
    );
    kinds.extend(
        geometry
            .pcurves
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Pcurve)),
    );
    kinds.extend(
        model
            .features
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Feature)),
    );
    kinds.extend(
        model
            .configurations
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Configuration)),
    );
    kinds.extend(
        model
            .parameters
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Parameter)),
    );
    kinds.extend(
        model
            .sketches
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Sketch)),
    );
    kinds.extend(
        model
            .sketch_constraints
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::SketchConstraint)),
    );
    kinds.extend(
        model
            .appearances
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Appearance)),
    );
    kinds.extend(
        model
            .appearance_bindings
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::AppearanceBinding)),
    );
    kinds.extend(
        model
            .tessellations
            .iter()
            .map(|row| (row.id.as_str(), EntityKind::Tessellation)),
    );
    kinds
}

/// A body whose topology names a record the document does not hold cannot be
/// encoded, and is not written either. The compact analytic records hold circular cones with an acute positive
/// half-angle, spheres of positive radius and tori with major > minor > 0. A
/// carrier the writer cannot generate (`Unknown`) is written only from a
/// retained partition; `opaque_unwritable` states that the write has none. A body
/// with a face or edge on such a carrier is not written.
pub(super) fn unwritable_carriers(
    ir: &mut CadIr,
    opaque_unwritable: bool,
) -> Result<Vec<LossNote>, CodecError> {
    let surfaces = ir
        .geometry
        .surfaces
        .iter()
        .filter_map(|surface| {
            Some((
                surface.id.as_str(),
                unwritable_surface(&surface.geometry, opaque_unwritable)?,
            ))
        })
        .collect::<HashMap<_, _>>();
    let curves = ir
        .geometry
        .curves
        .iter()
        .filter(|curve| {
            opaque_unwritable
                && matches!(
                    curve.geometry,
                    CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
                )
        })
        .map(|curve| curve.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let dangling = dangling_references(&ir.geometry);
    if surfaces.is_empty() && curves.is_empty() && dangling.is_empty() {
        return Ok(Vec::new());
    }
    let shell_bodies = ir
        .geometry
        .regions
        .iter()
        .flat_map(|region| {
            region
                .shells
                .iter()
                .map(move |shell| (shell.as_str(), region.body.as_str()))
        })
        .collect::<HashMap<_, _>>();
    let loop_faces = ir
        .geometry
        .faces
        .iter()
        .flat_map(|face| face.loops.iter().map(move |loop_| (loop_.as_str(), face)))
        .collect::<HashMap<_, _>>();
    let edges = ir
        .geometry
        .edges
        .iter()
        .filter(|edge| {
            edge.curve()
                .is_some_and(|curve| curves.contains(curve.as_str()))
        })
        .map(|edge| edge.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    let mut reasons = std::collections::BTreeMap::<String, Vec<String>>::new();
    let faces = ir
        .geometry
        .faces
        .iter()
        .map(|face| (face.id.as_str(), face))
        .collect::<HashMap<_, _>>();
    for (face, reason) in dangling {
        if let Some(body) = faces
            .get(face)
            .and_then(|face| shell_bodies.get(face.shell.as_str()))
        {
            reasons
                .entry((*body).to_owned())
                .or_default()
                .push(format!("face {face} ({reason})"));
        }
    }
    for face in &ir.geometry.faces {
        if let (Some(reason), Some(body)) = (
            surfaces.get(face.surface.as_str()),
            shell_bodies.get(face.shell.as_str()),
        ) {
            reasons
                .entry((*body).to_owned())
                .or_default()
                .push(format!("face {} ({reason})", face.id.as_str()));
        }
    }
    for coedge in &ir.geometry.coedges {
        if !edges.contains(coedge.edge.as_str()) {
            continue;
        }
        let Some(face) = loop_faces.get(coedge.owner_loop.as_str()) else {
            continue;
        };
        if let Some(body) = shell_bodies.get(face.shell.as_str()) {
            reasons.entry((*body).to_owned()).or_default().push(format!(
                "edge {} (a curve the writer has no record for)",
                coedge.edge.as_str()
            ));
        }
    }
    let kept = ir
        .geometry
        .bodies
        .iter()
        .filter(|body| !reasons.contains_key(body.id.as_str()))
        .map(|body| body.id.clone())
        .collect::<Vec<_>>();
    let mut subset = super::body_subset(ir, &kept)?;
    for configuration in &mut subset.model.configurations {
        if let Some(bodies) = &mut configuration.bodies {
            bodies.retain(|body| kept.contains(body));
        }
    }
    *ir = subset;
    let entries = reasons
        .into_iter()
        .map(|(body, mut reasons)| {
            reasons.sort();
            reasons.dedup();
            format!("{body}: {}", reasons.join(", "))
        })
        .collect::<Vec<_>>();
    Ok(note(
        SldprtLossCode::WriterBodiesOmitted,
        "body or bodies have a face or edge on a carrier the SLDPRT writer has no record for, \
         and are not written",
        &entries,
    )
    .into_iter()
    .collect())
}

/// Faces whose topology names a record the document does not hold, and the
/// record. The B-rep encoder resolves every such reference.
fn dangling_references(geometry: &cadmpeg_ir::document::Topology) -> Vec<(&str, String)> {
    use std::collections::HashSet;
    let surfaces = geometry
        .surfaces
        .iter()
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    let curves = geometry
        .curves
        .iter()
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    let points = geometry
        .points
        .iter()
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    let coedges = geometry
        .coedges
        .iter()
        .map(|row| row.id.as_str())
        .collect::<HashSet<_>>();
    let vertices = geometry
        .vertices
        .iter()
        .map(|row| (row.id.as_str(), row.point.as_str()))
        .collect::<HashMap<_, _>>();
    let edges = geometry
        .edges
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect::<HashMap<_, _>>();
    let loops = geometry
        .loops
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect::<HashMap<_, _>>();
    let loop_faces = geometry
        .faces
        .iter()
        .flat_map(|face| {
            face.loops
                .iter()
                .map(move |loop_| (loop_.as_str(), face.id.as_str()))
        })
        .collect::<HashMap<_, _>>();
    let mut dangling = Vec::new();
    for face in &geometry.faces {
        if !surfaces.contains(face.surface.as_str()) {
            dangling.push((
                face.id.as_str(),
                format!("missing surface {}", face.surface.as_str()),
            ));
        }
        for loop_ in &face.loops {
            match loops.get(loop_.as_str()) {
                None => {
                    dangling.push((face.id.as_str(), format!("missing loop {}", loop_.as_str())));
                }
                Some(row) => {
                    for coedge in row.coedges() {
                        if !coedges.contains(coedge.as_str()) {
                            dangling.push((
                                face.id.as_str(),
                                format!("missing coedge {}", coedge.as_str()),
                            ));
                        }
                    }
                }
            }
        }
    }
    for coedge in &geometry.coedges {
        let Some(&face) = loop_faces.get(coedge.owner_loop.as_str()) else {
            continue;
        };
        let Some(edge) = edges.get(coedge.edge.as_str()) else {
            dangling.push((face, format!("missing edge {}", coedge.edge.as_str())));
            continue;
        };
        if let Some(curve) = edge
            .curve()
            .filter(|curve| !curves.contains(curve.as_str()))
        {
            dangling.push((face, format!("missing curve {}", curve.as_str())));
        }
        for vertex in edge.vertices() {
            match vertices.get(vertex.as_str()) {
                None => dangling.push((face, format!("missing vertex {}", vertex.as_str()))),
                Some(point) if !points.contains(point) => {
                    dangling.push((face, format!("missing point {point}")));
                }
                Some(_) => {}
            }
        }
    }
    dangling
}

fn unwritable_surface(geometry: &SurfaceGeometry, opaque_unwritable: bool) -> Option<String> {
    match geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone)) => {
            let ratio = cone.ratio().get();
            let half_angle = cone.half_angle().get();
            if ratio != 1.0 {
                Some(format!("elliptical cone ratio {ratio}"))
            } else if !(half_angle > 0.0 && half_angle < std::f64::consts::FRAC_PI_2) {
                Some(format!("cone half-angle {half_angle}"))
            } else {
                None
            }
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere))
            if sphere.radius().get() < 0.0 =>
        {
            Some(format!("signed sphere radius {}", sphere.radius().get()))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus)) => {
            let major = torus.major_radius().get();
            let minor = torus.minor_radius().get();
            (!(major > minor && minor > 0.0)).then(|| format!("torus radii ({major}, {minor})"))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. }) if opaque_unwritable => {
            Some("a surface the writer has no record for".to_owned())
        }
        _ => None,
    }
}
