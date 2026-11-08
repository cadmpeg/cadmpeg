// SPDX-License-Identifier: Apache-2.0
//! Admit independent body graphs so one unsupported body cannot block others.

use std::collections::BTreeSet;

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::report::loss::LossNote;

use crate::loss::F3dLossCode;

pub(super) fn repair_body_regions(
    target: &mut CadIr,
    losses: &mut Vec<LossNote>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<(), CodecError> {
    let regions = &target.model.regions;
    let mut dropped_body = false;
    for body in &mut target.model.bodies {
        let invalid = body
            .regions
            .iter()
            .filter(|id| {
                !regions
                    .iter()
                    .any(|region| &region.id == *id && region.body == body.id)
            })
            .count();
        if invalid == 0 {
            continue;
        }
        let candidates = regions
            .iter()
            .filter(|region| region.body == body.id && !body.regions.contains(&region.id))
            .map(|region| region.id.clone())
            .collect::<Vec<_>>();
        if invalid == 1 && candidates.len() == 1 {
            for id in &mut body.regions {
                if !regions
                    .iter()
                    .any(|region| region.id == *id && region.body == body.id)
                {
                    id.clone_from(&candidates[0]);
                }
            }
        } else {
            body.regions.retain(|id| {
                regions
                    .iter()
                    .any(|region| region.id == *id && region.body == body.id)
            });
        }
        losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
            "body {} has an unresolved region reference",
            body.id
        )));
        dropped_body |= body.regions.is_empty();
    }
    if dropped_body {
        let retained = target
            .model
            .bodies
            .iter()
            .filter(|body| !body.regions.is_empty())
            .map(|body| body.id.clone())
            .collect();
        *target = body_graph(target, &retained, limits)?;
    }
    Ok(())
}

pub(super) fn omit_ambiguous_points(
    target: &mut CadIr,
    losses: &mut Vec<LossNote>,
) -> Result<(), CodecError> {
    let mut seen = BTreeSet::new();
    let duplicates = target
        .model
        .points
        .iter()
        .filter_map(|point| (!seen.insert(point.id.clone())).then_some(point.id.clone()))
        .collect::<BTreeSet<_>>();
    if duplicates.is_empty() {
        return Ok(());
    }
    for id in &duplicates {
        losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
            "point {id} has a duplicate identity and is omitted"
        )));
    }
    target
        .model
        .points
        .retain(|point| !duplicates.contains(&point.id));
    let points = target
        .model
        .points
        .iter()
        .map(|point| point.id.clone())
        .collect::<BTreeSet<_>>();
    target.model.vertices.retain(|vertex| {
        if points.contains(&vertex.point) {
            true
        } else {
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "vertex {} has an unresolved point and is omitted",
                vertex.id
            )));
            false
        }
    });
    let vertices = target
        .model
        .vertices
        .iter()
        .map(|vertex| vertex.id.clone())
        .collect::<BTreeSet<_>>();
    target.model.edges.retain(|edge| {
        if vertices.contains(&edge.start) && vertices.contains(&edge.end) {
            true
        } else {
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "edge {} has an unresolved vertex and is omitted",
                edge.id
            )));
            false
        }
    });
    let edges = target
        .model
        .edges
        .iter()
        .map(|edge| edge.id.clone())
        .collect::<BTreeSet<_>>();
    let mut shells = Vec::new();
    let mut omitted_shells = BTreeSet::new();
    for shell in &target.model.shells {
        let wire_edges = shell
            .wire_edges()
            .iter()
            .filter(|id| edges.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let free_vertices = shell
            .free_vertices()
            .iter()
            .filter(|id| vertices.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        if shell.faces().is_empty() && wire_edges.is_empty() && free_vertices.is_empty() {
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "shell {} has no remaining members and is omitted",
                shell.id
            )));
            omitted_shells.insert(shell.id.clone());
            continue;
        }
        let mut rebuilt = cadmpeg_ir::topology::Shell::new(
            shell.id.clone(),
            shell.region.clone(),
            shell.faces().to_vec(),
            wire_edges,
            free_vertices,
        )
        .map_err(CodecError::malformed)?;
        rebuilt.native_ref.clone_from(&shell.native_ref);
        shells.push(rebuilt);
    }
    target.model.shells = shells;
    let mut omitted_regions = BTreeSet::new();
    for region in &mut target.model.regions {
        let before = region.shells.len();
        region.shells.retain(|id| !omitted_shells.contains(id));
        if before != region.shells.len() && region.shells.is_empty() {
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "region {} has no remaining shells and is omitted",
                region.id
            )));
            omitted_regions.insert(region.id.clone());
        }
    }
    target
        .model
        .regions
        .retain(|region| !omitted_regions.contains(&region.id));
    target.model.bodies.retain_mut(|body| {
        let before = body.regions.len();
        body.regions.retain(|id| !omitted_regions.contains(id));
        if before != body.regions.len() && body.regions.is_empty() {
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "body {} has no remaining regions and is omitted",
                body.id
            )));
            false
        } else {
            true
        }
    });
    Ok(())
}

pub(super) fn omit_unresolved_coedge_faces(
    target: &mut CadIr,
    losses: &mut Vec<LossNote>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<(), CodecError> {
    let edges: BTreeSet<_> = target.model.edges.iter().map(|edge| &edge.id).collect();
    let loops: BTreeSet<_> = target
        .model
        .loops
        .iter()
        .map(|boundary| &boundary.id)
        .collect();
    let coedges: BTreeSet<_> = target
        .model
        .coedges
        .iter()
        .map(|coedge| &coedge.id)
        .collect();
    let mut broken = BTreeSet::new();
    for coedge in &target.model.coedges {
        if !edges.contains(&coedge.edge)
            || !loops.contains(&coedge.owner_loop)
            || !coedges.contains(&coedge.radial_next)
        {
            losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                "coedge {} has an unresolved topology reference and is omitted",
                coedge.id
            )));
            broken.insert(coedge.id.clone());
        }
    }
    if broken.is_empty() {
        return Ok(());
    }
    let affected: BTreeSet<_> = target
        .model
        .loops
        .iter()
        .filter(|boundary| boundary.coedges().iter().any(|id| broken.contains(id)))
        .map(|boundary| boundary.face.clone())
        .collect();
    let shells = target
        .model
        .shells
        .iter()
        .map(|shell| shell.id.clone())
        .collect();
    let faces = target
        .model
        .faces
        .iter()
        .filter(|face| !affected.contains(&face.id))
        .map(|face| face.id.clone())
        .collect();
    for id in affected {
        losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
            "face {id} requires an omitted coedge and is omitted"
        )));
    }
    *target = face_graph(target, &shells, &faces, limits)?;
    Ok(())
}

pub(super) fn admit_bodies(
    source: &CadIr,
    losses: &mut Vec<LossNote>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<CadIr, CodecError> {
    let mut admitted = BTreeSet::new();
    for body in &source.model.bodies {
        let mut candidate_ids = admitted.clone();
        candidate_ids.insert(body.id.clone());
        let candidate = body_graph(source, &candidate_ids, limits)?;
        match super::super::generate::write_with_limits(&candidate, &mut Vec::new(), limits) {
            Ok(_) => admitted = candidate_ids,
            Err(error) if super::declines(&error) => {
                // F3D can encode an open shell. Admit independently encodable
                // faces before considering omission of their containing body.
                let reduced = admit_faces(&candidate, &body.id, losses, limits)?;
                if reduced
                    .model
                    .bodies
                    .iter()
                    .any(|record| record.id == body.id)
                {
                    match super::super::generate::write_with_limits(
                        &reduced,
                        &mut Vec::new(),
                        limits,
                    ) {
                        Ok(_) => {
                            admitted = candidate_ids;
                            return admit_bodies_after_reduction(
                                source, reduced, &admitted, losses, limits,
                            );
                        }
                        Err(error) if super::declines(&error) => {}
                        Err(error) => return Err(error),
                    }
                }
                losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                    "body {} has no supported F3D encoding and is omitted: {error}",
                    body.id,
                )));
            }
            Err(error) => return Err(error),
        }
    }
    body_graph(source, &admitted, limits)
}

fn admit_bodies_after_reduction(
    source: &CadIr,
    mut reduced: CadIr,
    admitted: &BTreeSet<BodyId>,
    losses: &mut Vec<LossNote>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<CadIr, CodecError> {
    let remaining = source
        .model
        .bodies
        .iter()
        .filter(|body| !admitted.contains(&body.id))
        .map(|body| body.id.clone())
        .collect();
    let tail = admit_bodies(&body_graph(source, &remaining, limits)?, losses, limits)?;
    // Merge only geometry arenas: auxiliary and design content were already
    // prepared once and are shared by all admitted body candidates.
    macro_rules! append { ($($field:ident),*) => {$({
        for record in &tail.model.$field {
            if !reduced.model.$field.iter().any(|existing| existing.id == record.id) {
                reduced.model.$field.push(record.clone());
            }
        }
    })*}; }
    append!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        surfaces,
        curves,
        pcurves,
        procedural_surfaces,
        procedural_curves
    );
    Ok(reduced)
}

fn admit_faces(
    source: &CadIr,
    body: &BodyId,
    losses: &mut Vec<LossNote>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<CadIr, CodecError> {
    let shells: BTreeSet<_> = source
        .model
        .shells
        .iter()
        .filter(|shell| {
            source
                .model
                .regions
                .iter()
                .any(|region| region.id == shell.region && &region.body == body)
        })
        .map(|shell| shell.id.clone())
        .collect();
    let mut admitted = BTreeSet::new();
    for face in source
        .model
        .faces
        .iter()
        .filter(|face| shells.contains(&face.shell))
    {
        let mut candidate_ids = admitted.clone();
        candidate_ids.insert(face.id.clone());
        let candidate = face_graph(source, &shells, &candidate_ids, limits)?;
        match super::super::generate::write_with_limits(&candidate, &mut Vec::new(), limits) {
            Ok(_) => admitted = candidate_ids,
            Err(error) if super::declines(&error) => {
                losses.push(F3dLossCode::WriterGeometryOmitted.note(format!(
                    "face {} is omitted from its open shell: {error}",
                    face.id
                )));
            }
            Err(error) => return Err(error),
        }
    }
    face_graph(source, &shells, &admitted, limits)
}

fn face_graph(
    source: &CadIr,
    shells: &BTreeSet<cadmpeg_ir::ids::ShellId>,
    faces: &BTreeSet<cadmpeg_ir::ids::FaceId>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<CadIr, CodecError> {
    let mut result = source.clone();
    result
        .model
        .faces
        .retain(|face| !shells.contains(&face.shell) || faces.contains(&face.id));
    let bodies = result
        .model
        .bodies
        .iter()
        .map(|body| body.id.clone())
        .collect();
    let mut result = body_graph(&result, &bodies, limits)?;
    let affected = source
        .model
        .regions
        .iter()
        .filter(|region| region.shells.iter().any(|id| shells.contains(id)))
        .map(|region| region.body.clone())
        .collect::<BTreeSet<_>>();
    let kinds = result
        .model
        .bodies
        .iter()
        .filter(|body| affected.contains(&body.id))
        .map(|body| {
            let body_shells: BTreeSet<_> = result
                .model
                .regions
                .iter()
                .filter(|region| region.body == body.id)
                .flat_map(|region| &region.shells)
                .collect();
            let body_faces: BTreeSet<_> = result
                .model
                .shells
                .iter()
                .filter(|shell| body_shells.contains(&shell.id))
                .flat_map(cadmpeg_ir::topology::Shell::faces)
                .collect();
            let loops: BTreeSet<_> = result
                .model
                .loops
                .iter()
                .filter(|boundary| body_faces.contains(&boundary.face))
                .map(|boundary| boundary.id.clone())
                .collect();
            let has_wires = result
                .model
                .shells
                .iter()
                .filter(|shell| body_shells.contains(&shell.id))
                .any(|shell| !shell.wire_edges().is_empty() || !shell.free_vertices().is_empty());
            let kind = if body_faces.is_empty() {
                cadmpeg_ir::topology::BodyKind::Wire
            } else if has_wires {
                cadmpeg_ir::topology::BodyKind::General
            } else {
                cadmpeg_asm::brep::geometry::faced_body_kind(
                    result
                        .model
                        .coedges
                        .iter()
                        .filter(|coedge| loops.contains(&coedge.owner_loop))
                        .map(|coedge| (&coedge.edge, coedge.sense)),
                    |id| {
                        result
                            .model
                            .edges
                            .iter()
                            .find(|edge| &edge.id == id)
                            .is_some_and(|edge| edge.start == edge.end && edge.curve().is_none())
                    },
                )
            };
            (body.id.clone(), kind)
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    for body in &mut result.model.bodies {
        if let Some(kind) = kinds.get(&body.id) {
            body.kind = *kind;
        }
    }
    Ok(result)
}

fn body_graph(
    source: &CadIr,
    bodies: &BTreeSet<BodyId>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<CadIr, CodecError> {
    let mut result = source.clone();
    let model = &mut result.model;
    model.bodies.retain(|body| bodies.contains(&body.id));
    model.regions.retain(|region| bodies.contains(&region.body));
    let regions: BTreeSet<_> = model
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect();
    model.shells.retain(|shell| regions.contains(&shell.region));
    let shells: BTreeSet<_> = model.shells.iter().map(|shell| shell.id.clone()).collect();
    model.faces.retain(|face| shells.contains(&face.shell));
    let faces: BTreeSet<_> = model.faces.iter().map(|face| face.id.clone()).collect();
    model.shells = model
        .shells
        .iter()
        .filter_map(|shell| {
            let members = shell
                .faces()
                .iter()
                .filter(|id| faces.contains(*id))
                .cloned()
                .collect::<Vec<_>>();
            if members.is_empty()
                && shell.wire_edges().is_empty()
                && shell.free_vertices().is_empty()
            {
                None
            } else {
                Some(
                    cadmpeg_ir::topology::Shell::new(
                        shell.id.clone(),
                        shell.region.clone(),
                        members,
                        shell.wire_edges().to_vec(),
                        shell.free_vertices().to_vec(),
                    )
                    .map_err(|error| CodecError::Malformed(error.to_string())),
                )
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let shells = model
        .shells
        .iter()
        .map(|shell| shell.id.clone())
        .collect::<BTreeSet<_>>();
    for region in &mut model.regions {
        region.shells.retain(|id| shells.contains(id));
    }
    model.regions.retain(|region| !region.shells.is_empty());
    let regions = model
        .regions
        .iter()
        .map(|region| region.id.clone())
        .collect::<BTreeSet<_>>();
    for body in &mut model.bodies {
        body.regions.retain(|id| regions.contains(id));
    }
    model.bodies.retain(|body| !body.regions.is_empty());
    model
        .loops
        .retain(|boundary| faces.contains(&boundary.face));
    let loops: BTreeSet<_> = model
        .loops
        .iter()
        .map(|boundary| boundary.id.clone())
        .collect();
    model
        .coedges
        .retain(|coedge| loops.contains(&coedge.owner_loop));
    let surviving = model
        .coedges
        .iter()
        .map(|coedge| coedge.id.clone())
        .collect::<BTreeSet<_>>();
    for coedge in &mut model.coedges {
        let mut next = coedge.radial_next.clone();
        let mut visited = BTreeSet::new();
        while !surviving.contains(&next) {
            if !visited.insert(next.clone()) {
                return Err(CodecError::malformed(format_args!(
                    "coedge {} has an unclosed radial ring",
                    coedge.id
                )));
            }
            next = source
                .model
                .coedges
                .iter()
                .find(|candidate| candidate.id == next)
                .ok_or_else(|| CodecError::malformed(format_args!("missing radial coedge {next}")))?
                .radial_next
                .clone();
        }
        coedge.radial_next = next;
    }
    let edges: BTreeSet<_> = model
        .coedges
        .iter()
        .map(|coedge| coedge.edge.clone())
        .chain(
            model
                .shells
                .iter()
                .flat_map(|shell| shell.wire_edges().iter().cloned()),
        )
        .collect();
    model.edges.retain(|edge| edges.contains(&edge.id));
    let vertices: BTreeSet<_> = model
        .edges
        .iter()
        .flat_map(|edge| [edge.start.clone(), edge.end.clone()])
        .chain(
            model
                .shells
                .iter()
                .flat_map(|shell| shell.free_vertices().iter().cloned()),
        )
        .chain(
            model
                .loops
                .iter()
                .flat_map(|boundary| boundary.vertices().cloned()),
        )
        .collect();
    model
        .vertices
        .retain(|vertex| vertices.contains(&vertex.id));
    let points: BTreeSet<_> = model
        .vertices
        .iter()
        .map(|vertex| vertex.point.clone())
        .collect();
    let mut seeds: BTreeSet<String> = points
        .iter()
        .map(|id| id.as_str().to_owned())
        .chain(
            model
                .faces
                .iter()
                .map(|face| face.surface.as_str().to_owned()),
        )
        .chain(
            model
                .edges
                .iter()
                .filter_map(|edge| edge.curve().map(|id| id.as_str().to_owned())),
        )
        .chain(model.coedges.iter().filter_map(|coedge| {
            coedge
                .use_curve
                .as_ref()
                .map(|use_| use_.curve.as_str().to_owned())
        }))
        .chain(
            model
                .coedges
                .iter()
                .flat_map(|coedge| &coedge.pcurves)
                .map(|use_| use_.pcurve.as_str().to_owned()),
        )
        .chain(
            model
                .loops
                .iter()
                .flat_map(cadmpeg_ir::topology::Loop::vertex_pcurves)
                .map(|use_| use_.pcurve.as_str().to_owned()),
        )
        .collect();
    geometry_closure(source, &mut seeds, limits)?;
    model
        .points
        .retain(|record| seeds.contains(record.id.as_str()));
    model
        .surfaces
        .retain(|record| seeds.contains(record.id.as_str()));
    model
        .curves
        .retain(|record| seeds.contains(record.id.as_str()));
    model
        .pcurves
        .retain(|record| seeds.contains(record.id.as_str()));
    model
        .procedural_surfaces
        .retain(|record| seeds.contains(record.id.as_str()));
    model
        .procedural_curves
        .retain(|record| seeds.contains(record.id.as_str()));
    model.appearance_bindings.retain(|binding| {
        use cadmpeg_ir::appearance::AppearanceTarget;
        match &binding.target {
            AppearanceTarget::Body(id) => bodies.contains(id),
            AppearanceTarget::Face(id) => faces.contains(id),
            AppearanceTarget::Edge(id) => edges.contains(id),
            AppearanceTarget::Vertex(id) => vertices.contains(id),
            AppearanceTarget::Surface(id) => seeds.contains(id.as_str()),
            AppearanceTarget::Curve(id) => seeds.contains(id.as_str()),
            AppearanceTarget::Point(id) => seeds.contains(id.as_str()),
            _ => false,
        }
    });
    super::native::prune(&mut result, limits)?;
    Ok(result)
}

/// Include support geometry and child constructions that are not direct face
/// or edge carriers. Filtering topology must not create a dangling reference.
fn geometry_closure(
    source: &CadIr,
    ids: &mut BTreeSet<String>,
    limits: cadmpeg_core::decode::ResourceLimits,
) -> Result<(), CodecError> {
    use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
    super::design::with_context(limits, |ctx| {
        let mut visited = BTreeSet::new();
        loop {
            let previous = visited.len();
            macro_rules! visit { ($($field:ident),*) => {$({
                for record in &source.model.$field {
                    if ids.contains(record.id.as_str()) && visited.insert(record.id.as_str().to_owned()) {
                        record.visit_identity_references::<cadmpeg_core::CodecError>(ctx, &mut |id| { ids.insert(id.to_owned()); Ok(()) })?;
                    }
                }
            })*}; }
            visit!(
                points,
                surfaces,
                curves,
                pcurves,
                procedural_surfaces,
                procedural_curves
            );
            if previous == visited.len() {
                break;
            }
        }
        Ok(())
    })
}
