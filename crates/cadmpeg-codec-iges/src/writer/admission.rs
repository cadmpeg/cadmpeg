// SPDX-License-Identifier: Apache-2.0
//! Admit invalid carrier references without refusing unrelated records.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use super::{has_brep_topology, BodyKind, CadIr, IgesLossCode, LossNote};

pub(super) fn edge_carriers(ir: &CadIr) -> (Cow<'_, CadIr>, Vec<LossNote>) {
    let curves = ir
        .model
        .curves
        .iter()
        .map(|curve| &curve.id)
        .collect::<BTreeSet<_>>();
    let invalid = ir
        .model
        .edges
        .iter()
        .filter(|edge| edge.curve().is_some_and(|curve| !curves.contains(curve)))
        .map(|edge| edge.id.clone())
        .collect::<BTreeSet<_>>();
    if invalid.is_empty() {
        return (Cow::Borrowed(ir), Vec::new());
    }
    let mut admitted = ir.clone();
    let mut losses = Vec::new();
    for edge in &mut admitted.model.edges {
        if invalid.contains(&edge.id) {
            edge.carrier = cadmpeg_ir::topology::EdgeCarrier::new(None, None)
                .expect("an absent curve has no parameter range");
            losses.push(IgesLossCode::WriterEntityOmitted.note(format!(
                "IGES edge {} has a dangling curve carrier",
                edge.id
            )));
        }
    }
    (Cow::Owned(admitted), losses)
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
        if (!invalid_edges_only && !has_coedge) || !missing.is_empty() {
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
            bad_loops.extend(face.loops.iter().map(|loop_| loop_.as_str()));
        }
    }
    let mut bad_coedges = BTreeSet::new();
    for loop_id in &bad_loops {
        if let Some(loop_) = loops.get(loop_id) {
            bad_coedges.extend(loop_.coedges().iter().map(|coedge| coedge.as_str()));
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
