// SPDX-License-Identifier: Apache-2.0
//! Admit invalid carrier references without refusing unrelated records.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use super::{has_brep_topology, BodyKind, CadIr, IgesLossCode, LossNote};

pub(super) fn edge_carriers(
    ir: &CadIr,
) -> Result<(Cow<'_, CadIr>, Vec<LossNote>), cadmpeg_core::CodecError> {
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| &curve.id)
        .collect::<BTreeSet<_>>();
    let mut invalid = BTreeMap::new();
    for edge in &ir.model.edges {
        let Some(id) = edge.curve() else {
            continue;
        };
        if !curves.contains(id) {
            invalid.insert(edge.id.clone(), "has a dangling curve carrier");
            continue;
        }
        if has_brep_topology(ir) || edge.param_range().is_none() {
            continue;
        }
        let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *id) else {
            continue;
        };
        let Some(solved) = curve.geometry.solved() else {
            continue;
        };
        let Ok(geometry) = super::flatten_curve(solved) else {
            continue;
        };
        match super::checked_edge_span(ir, edge, &geometry) {
            Ok(None) => {
                invalid.insert(
                    edge.id.clone(),
                    "has endpoints that disagree with its curve parameter range",
                );
            }
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) => {
                return Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            }
            _ => {}
        }
    }
    if invalid.is_empty() {
        return Ok((Cow::Borrowed(ir), Vec::new()));
    }
    let mut admitted = ir.clone();
    let mut losses = Vec::new();
    for edge in &mut admitted.model.edges {
        if let Some(reason) = invalid.get(&edge.id) {
            edge.carrier = cadmpeg_ir::topology::EdgeCarrier::new(None, None)
                .expect("an absent curve has no parameter range");
            losses.push(
                IgesLossCode::WriterEntityOmitted.note(format!("IGES edge {} {reason}", edge.id)),
            );
        }
    }
    Ok((Cow::Owned(admitted), losses))
}

/// A loopless NURBS face retains the finite natural parameter domain.
pub(super) fn natural_domain_face(ir: &CadIr, face: &cadmpeg_ir::topology::Face) -> bool {
    face.loops.is_empty()
        && ir
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == face.surface)
            .is_some_and(|surface| {
                matches!(
                    surface.geometry.solved_cache(),
                    Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Nurbs(_))
                )
            })
}

/// Faces whose boundary no carrier reaches, with their topology stripped so
/// the carrier surface still writes. A loop coedge whose edge names no model
/// curve, or names one without solved geometry, can never supply a Type 141
/// or Type 142 boundary; the face drops with a loss while its surface writes
/// bare or domain-bounded. Faces the trimmed writer can encode are untouched.
pub(super) fn strip_unusable_face_topology(ir: &CadIr) -> (Option<CadIr>, Vec<LossNote>) {
    if ir.model.faces.is_empty() {
        return (None, Vec::new());
    }
    let invalid_edges_only = has_brep_topology(ir);
    let loops = ir
        .model
        .loops
        .iter()
        .map(|loop_| (loop_.id.as_str(), loop_))
        .collect::<BTreeMap<_, _>>();
    let coedges = ir
        .model
        .coedges
        .iter()
        .map(|coedge| (coedge.id.as_str(), coedge))
        .collect::<BTreeMap<_, _>>();
    let edges = ir
        .model
        .edges
        .iter()
        .map(|edge| (edge.id.as_str(), edge))
        .collect::<BTreeMap<_, _>>();
    let solved = ir
        .model
        .curves
        .iter()
        .filter(|curve| curve.geometry.solved().is_some())
        .map(|curve| curve.id.as_str())
        .collect::<BTreeSet<_>>();
    let mut bad = Vec::new();
    for face in &ir.model.faces {
        let mut missing = Vec::new();
        let mut has_coedge = false;
        for loop_id in face.loops.iter() {
            let Some(loop_) = loops.get(loop_id.as_str()) else {
                continue;
            };
            for coedge_id in loop_.coedges() {
                has_coedge = true;
                let edge = coedges
                    .get(coedge_id.as_str())
                    .and_then(|coedge| edges.get(coedge.edge.as_str()));
                let usable = if invalid_edges_only {
                    edge.is_some()
                } else {
                    edge.and_then(|edge| edge.curve())
                        .is_some_and(|curve_id| solved.contains(curve_id.as_str()))
                };
                if !usable {
                    missing.push(coedge_id.as_str().to_owned());
                }
            }
        }
        if (!invalid_edges_only && !has_coedge && !natural_domain_face(ir, face))
            || !missing.is_empty()
        {
            bad.push((face.id.as_str().to_owned(), missing));
        }
    }
    if bad.is_empty() {
        return (None, Vec::new());
    }
    let bad_faces = bad
        .iter()
        .map(|(face, _)| face.as_str())
        .collect::<BTreeSet<_>>();
    let mut bad_loops = BTreeSet::new();
    for face in &ir.model.faces {
        if bad_faces.contains(face.id.as_str()) {
            bad_loops.extend(face.loops.iter().map(cadmpeg_ir::ids::LoopId::as_str));
        }
    }
    let mut bad_coedges = BTreeSet::new();
    for loop_id in &bad_loops {
        if let Some(loop_) = loops.get(loop_id) {
            bad_coedges.extend(
                loop_
                    .coedges()
                    .iter()
                    .map(cadmpeg_ir::ids::CoedgeId::as_str),
            );
        }
    }
    let affected_bodies = ir
        .model
        .faces
        .iter()
        .filter(|face| bad_faces.contains(face.id.as_str()))
        .filter_map(|face| ir.model.shells.iter().find(|shell| shell.id == face.shell))
        .filter_map(|shell| {
            ir.model
                .regions
                .iter()
                .find(|region| region.id == shell.region)
        })
        .map(|region| region.body.clone())
        .collect::<BTreeSet<_>>();
    let mut document = ir.clone();
    let mut opened_solids = Vec::new();
    for body in &mut document.model.bodies {
        if affected_bodies.contains(&body.id) && body.kind == BodyKind::Solid {
            body.kind = BodyKind::Sheet;
            opened_solids.push(body.id.to_string());
        }
    }
    document
        .model
        .faces
        .retain(|face| !bad_faces.contains(face.id.as_str()));
    document
        .model
        .loops
        .retain(|loop_| !bad_loops.contains(loop_.id.as_str()));
    document
        .model
        .coedges
        .retain(|coedge| !bad_coedges.contains(coedge.id.as_str()));
    let retained_coedges = document
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.id.clone())
        .collect::<BTreeSet<_>>();
    for coedge in &mut document.model.coedges {
        let mut visited = BTreeSet::new();
        while !retained_coedges.contains(&coedge.radial_next) {
            if !visited.insert(coedge.radial_next.clone()) {
                break;
            }
            let Some(removed) = ir
                .model
                .coedges
                .iter()
                .find(|candidate| candidate.id == coedge.radial_next)
            else {
                break;
            };
            coedge.radial_next = removed.radial_next.clone();
        }
    }
    let mut live_shells = BTreeSet::new();
    document.model.shells.retain_mut(|shell| {
        let faces = shell
            .faces()
            .iter()
            .filter(|face| !bad_faces.contains(face.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if shell.replace_faces(faces).is_ok() {
            live_shells.insert(shell.id.as_str().to_owned());
            true
        } else {
            false
        }
    });
    for region in &mut document.model.regions {
        region
            .shells
            .retain(|shell| live_shells.contains(shell.as_str()));
    }
    document
        .model
        .regions
        .retain(|region| !region.shells.is_empty());
    let live_regions = document
        .model
        .regions
        .iter()
        .map(|region| region.id.as_str())
        .collect::<BTreeSet<_>>();
    for body in &mut document.model.bodies {
        body.regions
            .retain(|region| live_regions.contains(region.as_str()));
    }
    document
        .model
        .bodies
        .retain(|body| !body.regions.is_empty());
    let mut losses = bad
        .into_iter()
        .map(|(face, missing)| {
            IgesLossCode::WriterEntityOmitted.note(format!(
                "IGES face {face} has no usable boundary carrier and is omitted: {}",
                missing.join(", ")
            ))
        })
        .collect::<Vec<_>>();
    for id in opened_solids {
        losses.push(IgesLossCode::WriterEntityOmitted.note(format!(
            "IGES solid {id} is reduced to a sheet after an invalid face is removed"
        )));
    }
    for id in bad_loops.into_iter().chain(bad_coedges) {
        losses.push(IgesLossCode::WriterEntityOmitted.note(format!(
            "IGES boundary record {id} is omitted with its unsupported face"
        )));
    }
    (Some(document), losses)
}

/// A horizontal line at spherical latitude ±π/2 maps exactly to one pole.
fn spherical_pole(
    ir: &CadIr,
    edge: &cadmpeg_ir::topology::Edge,
    coedge: &cadmpeg_ir::topology::Coedge,
) -> bool {
    use cadmpeg_ir::geometry::{pcurve::PcurveGeometry, SolvedSurfaceGeometry};
    let Some(boundary) = ir
        .model
        .loops
        .iter()
        .find(|boundary| boundary.id == coedge.owner_loop)
    else {
        return false;
    };
    let Some(face) = ir.model.faces.iter().find(|face| face.id == boundary.face) else {
        return false;
    };
    let Some(SolvedSurfaceGeometry::Sphere(sphere)) = ir
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == face.surface)
        .and_then(|surface| surface.geometry.solved())
    else {
        return false;
    };
    if coedge.pcurves.is_empty() {
        return false;
    }
    let mut sign = None;
    for use_ in &coedge.pcurves {
        let Some(curve) = ir
            .model
            .pcurves
            .iter()
            .find(|curve| curve.id == use_.pcurve)
        else {
            return false;
        };
        if curve
            .support_surface()
            .is_some_and(|surface| *surface != face.surface)
        {
            return false;
        }
        let PcurveGeometry::Line(line) = &curve.geometry else {
            return false;
        };
        let origin = line.origin().get();
        let direction = line.direction().as_raw();
        let current = if origin.v == std::f64::consts::FRAC_PI_2 {
            1.0
        } else if origin.v == -std::f64::consts::FRAC_PI_2 {
            -1.0
        } else {
            return false;
        };
        if direction.v != 0.0 || sign.is_some_and(|previous| previous != current) {
            return false;
        }
        sign = Some(current);
    }
    let Some(vertex) = ir
        .model
        .vertices
        .iter()
        .find(|vertex| vertex.id == edge.start)
    else {
        return false;
    };
    let Some(point) = ir
        .model
        .points
        .iter()
        .find(|point| point.id == vertex.point)
    else {
        return false;
    };
    let actual = point.position().get();
    let center = sphere.center().get();
    let axis = sphere.frame().axis().as_raw();
    let scale = sphere.radius().get() * sign.unwrap_or(0.0);
    let distance = (actual.x - (center.x + scale * axis.x))
        .hypot(actual.y - (center.y + scale * axis.y))
        .hypot(actual.z - (center.z + scale * axis.z));
    distance
        <= vertex
            .tolerance
            .map_or(0.0, cadmpeg_ir::scalar::PositiveReal::get)
}

/// Lower a carrierless closed edge use to the IGES vertex-use form.
pub(super) fn pole_edges(
    ir: &CadIr,
) -> Result<(Cow<'_, CadIr>, Vec<LossNote>), cadmpeg_core::CodecError> {
    use cadmpeg_ir::topology::{AnchoredVertexUse, LoopBoundary, LoopRing};
    let wire_edges = ir
        .model
        .shells
        .iter()
        .flat_map(cadmpeg_ir::topology::Shell::wire_edges)
        .collect::<BTreeSet<_>>();
    let mut poles = BTreeMap::new();
    for edge in &ir.model.edges {
        if edge.curve().is_some() || edge.start != edge.end || wire_edges.contains(&edge.id) {
            continue;
        }
        let uses = ir
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.edge == edge.id)
            .collect::<Vec<_>>();
        let [coedge] = uses.as_slice() else {
            continue;
        };
        if coedge.use_curve.is_some() || !spherical_pole(ir, edge, coedge) {
            continue;
        }
        let Some(boundary) = ir
            .model
            .loops
            .iter()
            .find(|boundary| boundary.id == coedge.owner_loop)
        else {
            continue;
        };
        let LoopBoundary::Ring(ring) = &boundary.boundary else {
            continue;
        };
        if ring.coedges().len() < 2 || !ring.coedges().contains(&coedge.id) {
            continue;
        }
        poles.insert(
            coedge.id.clone(),
            (edge.id.clone(), edge.start.clone(), coedge.pcurves.clone()),
        );
    }
    if poles.is_empty() {
        return Ok((Cow::Borrowed(ir), Vec::new()));
    }
    let mut admitted = ir.clone();
    let mut removed = BTreeSet::new();
    for boundary in &mut admitted.model.loops {
        let LoopBoundary::Ring(ring) = &boundary.boundary else {
            continue;
        };
        let old = ring.coedges();
        let kept = old
            .iter()
            .filter(|id| !poles.contains_key(*id))
            .cloned()
            .collect::<Vec<_>>();
        if kept.is_empty() {
            continue;
        }
        let mut uses = Vec::new();
        let Some(start) = old.iter().position(|id| !poles.contains_key(id)) else {
            continue;
        };
        let mut previous = old[start].clone();
        for offset in 0..old.len() {
            let id = &old[(start + offset) % old.len()];
            if let Some((edge, vertex, pcurves)) = poles.get(id) {
                uses.push(AnchoredVertexUse {
                    vertex: vertex.clone(),
                    after: previous.clone(),
                    pcurves: pcurves.clone(),
                });
                removed.insert((id.clone(), edge.clone()));
            } else {
                previous = id.clone();
            }
            uses.extend(
                ring.vertex_uses()
                    .iter()
                    .filter(|use_| use_.after == *id)
                    .map(|use_| {
                        let mut use_ = use_.clone();
                        use_.after = previous.clone();
                        use_
                    }),
            );
        }
        boundary.boundary = LoopBoundary::Ring(
            LoopRing::new(kept, uses).map_err(cadmpeg_core::CodecError::malformed)?,
        );
    }
    let removed_coedges = removed
        .iter()
        .map(|(coedge, _)| coedge.clone())
        .collect::<BTreeSet<_>>();
    let removed_edges = removed
        .iter()
        .map(|(_, edge)| edge.clone())
        .collect::<BTreeSet<_>>();
    admitted
        .model
        .coedges
        .retain(|coedge| !removed_coedges.contains(&coedge.id));
    admitted
        .model
        .edges
        .retain(|edge| !removed_edges.contains(&edge.id));
    let mut losses = Vec::new();
    for (coedge, edge) in removed {
        for id in [coedge.to_string(), edge.to_string()] {
            losses.push(IgesLossCode::WriterEntityOmitted.note(format!(
                "IGES record {id} is reduced to a singular vertex use"
            )));
        }
    }
    Ok((Cow::Owned(admitted), losses))
}

/// A solid with a single non-closed or non-manifold shell can retain its faces as a sheet.
pub(super) fn open_incomplete_solids(ir: &CadIr) -> (Cow<'_, CadIr>, Vec<LossNote>) {
    let mut open = BTreeSet::new();
    for body in &ir.model.bodies {
        if body.kind != BodyKind::Solid || body.regions.len() != 1 {
            continue;
        }
        let Some(region) = ir
            .model
            .regions
            .iter()
            .find(|region| region.id == body.regions[0])
        else {
            continue;
        };
        if region.shells.len() != 1 {
            continue;
        }
        let shell = &region.shells[0];
        let faces = ir
            .model
            .faces
            .iter()
            .filter(|face| face.shell == *shell)
            .map(|face| &face.id)
            .collect::<BTreeSet<_>>();
        let loops = ir
            .model
            .loops
            .iter()
            .filter(|boundary| faces.contains(&boundary.face))
            .map(|boundary| &boundary.id)
            .collect::<BTreeSet<_>>();
        let mut edges = BTreeMap::new();
        for coedge in ir
            .model
            .coedges
            .iter()
            .filter(|coedge| loops.contains(&coedge.owner_loop))
        {
            *edges.entry(&coedge.edge).or_insert(0_usize) += 1;
        }
        if edges.values().any(|count| *count != 2) {
            open.insert(body.id.clone());
        }
    }
    if open.is_empty() {
        return (Cow::Borrowed(ir), Vec::new());
    }
    let mut document = ir.clone();
    let mut losses = Vec::new();
    for body in &mut document.model.bodies {
        if open.contains(&body.id) {
            body.kind = BodyKind::Sheet;
            losses.push(IgesLossCode::WriterEntityOmitted.note(format!(
                "IGES solid {} has an open shell or non-manifold edge and is written as a sheet",
                body.id
            )));
        }
    }
    (Cow::Owned(document), losses)
}
