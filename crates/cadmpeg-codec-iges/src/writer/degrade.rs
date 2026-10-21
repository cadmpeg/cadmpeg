// SPDX-License-Identifier: Apache-2.0
//! Independent entity admission for lossy IGES synthesis.

use super::{
    native_omission_losses, reference_marker, synthesis_graph, unsupported_model_losses, BTreeMap,
    BTreeSet, BodyKind, CadIr, CodecError, DecodeArena, DecodeContext, DecodePolicy, IgesLossCode,
    SynthesisGraph,
};
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;

/// Preserve independent bodies and free carriers when one cannot be encoded.
pub(super) fn graph(
    ir: &CadIr,
    version: crate::IgesVersion,
    reason: &str,
) -> Result<SynthesisGraph, CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
    let mut result = SynthesisGraph {
        entities: Vec::new(),
        presentations: BTreeMap::new(),
        losses: unsupported_model_losses(ir)?,
    };
    result.losses.extend(native_omission_losses(ir));
    let mut owned = BTreeSet::new();
    for body in &ir.model.bodies {
        let (part, identities) = scoped(ir, &ctx, body.id.as_str())?;
        owned.extend(identities);
        admit(&mut result, &part, version, body.id.as_str())?;
    }
    // Body-owned carriers must not reappear as free geometry after a body was
    // omitted. Each remaining independent carrier is admitted on its own.
    for id in ir
        .model
        .edges
        .iter()
        .map(|record| record.id.as_str())
        .chain(ir.model.surfaces.iter().map(|record| record.id.as_str()))
        .chain(ir.model.curves.iter().map(|record| record.id.as_str()))
        .chain(ir.model.points.iter().map(|record| record.id.as_str()))
    {
        if owned.contains(id) {
            continue;
        }
        let (part, identities) = scoped(ir, &ctx, id)?;
        owned.extend(identities);
        admit(&mut result, &part, version, id)?;
    }
    if result.entities.is_empty() && result.losses.is_empty() {
        result
            .losses
            .push(IgesLossCode::WriterEntityOmitted.note(reason));
    }
    Ok(result)
}

fn admit(
    result: &mut SynthesisGraph,
    ir: &CadIr,
    version: crate::IgesVersion,
    identity: &str,
) -> Result<(), CodecError> {
    match synthesis_graph(ir, version) {
        Ok(graph) => append(result, graph),
        Err(CodecError::UnsupportedContent(reason)) => {
            // Type 508 can carry the model-space edge boundary without UV
            // images. Prefer that degradation to discarding an entire body.
            if ir.model.bodies.len() == 1 && !ir.model.pcurves.is_empty() {
                let without_uv = without_pcurves(ir)?;
                match synthesis_graph(&without_uv, version) {
                    Ok(mut graph) => {
                        graph.losses.push(IgesLossCode::WriterPcurvesOmitted.note(format!(
                            "IGES entity {identity} retains its model-space boundary without {} parameter-space curves: {reason}",
                            ir.model.pcurves.len()
                        )));
                        return append(result, graph);
                    }
                    Err(CodecError::UnsupportedContent(_)) => {}
                    Err(error) => return Err(error),
                }
            }
            if ir.model.bodies.len() == 1 && ir.model.bodies[0].kind == BodyKind::Wire {
                let mut free = ir.clone();
                free.model.bodies.clear();
                free.model.regions.clear();
                free.model.shells.clear();
                result.losses.push(IgesLossCode::WriterEntityOmitted.note(format!(
                    "IGES wire body {identity} loses its ownership; its carriers are written as free geometry: {reason}"
                )));
                return admit(result, &free, version, identity);
            }
            if ir.model.bodies.len() == 1
                && ir.model.bodies[0].kind == BodyKind::Sheet
                && ir.model.faces.len() > 1
            {
                result
                    .losses
                    .push(IgesLossCode::WriterEntityOmitted.note(format!(
                        "IGES sheet {identity} is split into independently admitted faces: {reason}"
                    )));
                let arena = DecodeArena::new();
                let (ctx, _) =
                    DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default())?;
                for face in &ir.model.faces {
                    let part = face_document(ir, &ctx, face)?;
                    admit(result, &part, version, face.id.as_str())?;
                }
                return Ok(());
            }
            result.losses.push(
                IgesLossCode::WriterEntityOmitted
                    .note(format!("IGES entity {identity} is omitted: {reason}")),
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Remove the redundant UV representation without changing the 3D boundary.
fn without_pcurves(ir: &CadIr) -> Result<CadIr, CodecError> {
    let mut document = ir.clone();
    document.model.pcurves.clear();
    for coedge in &mut document.model.coedges {
        coedge.pcurves.clear();
    }
    for boundary in &mut document.model.loops {
        match &mut boundary.boundary {
            cadmpeg_ir::topology::LoopBoundary::Vertex { pcurves, .. } => pcurves.clear(),
            cadmpeg_ir::topology::LoopBoundary::Ring(ring) => {
                let mut vertices = ring.vertex_uses().to_vec();
                for vertex in &mut vertices {
                    vertex.pcurves.clear();
                }
                *ring = cadmpeg_ir::topology::LoopRing::new(ring.coedges().to_vec(), vertices)
                    .map_err(CodecError::malformed)?;
            }
        }
    }
    Ok(document)
}

/// A sheet can retain its other faces after one face has no target encoding.
fn face_document(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
    face: &cadmpeg_ir::topology::Face,
) -> Result<CadIr, CodecError> {
    let mut part = ir.clone();
    let shell = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == face.shell)
        .ok_or_else(|| CodecError::malformed("IGES sheet face has no shell"))?;
    part.model.bodies[0].regions = vec![shell.region.clone()];
    part.model
        .regions
        .retain(|region| region.id == shell.region);
    let region = part
        .model
        .regions
        .first_mut()
        .ok_or_else(|| CodecError::malformed("IGES sheet shell has no region"))?;
    region.shells = vec![shell.id.clone()];
    part.model.shells = vec![cadmpeg_ir::topology::Shell::with_face(
        shell.id.clone(),
        shell.region.clone(),
        face.id.clone(),
    )];
    part.model.faces = vec![face.clone()];
    part.model.loops.retain(|boundary| boundary.face == face.id);
    let loops = part
        .model
        .loops
        .iter()
        .map(|boundary| &boundary.id)
        .collect::<BTreeSet<_>>();
    part.model
        .coedges
        .retain(|coedge| loops.contains(&coedge.owner_loop));
    let retained = part
        .model
        .coedges
        .iter()
        .map(|coedge| coedge.id.clone())
        .collect::<BTreeSet<_>>();
    for coedge in &mut part.model.coedges {
        let mut visited = BTreeSet::new();
        while !retained.contains(&coedge.radial_next) {
            if !visited.insert(coedge.radial_next.clone()) {
                return Err(CodecError::malformed(
                    "IGES sheet has an unclosed radial ring",
                ));
            }
            coedge.radial_next = ir
                .model
                .coedges
                .iter()
                .find(|candidate| candidate.id == coedge.radial_next)
                .ok_or_else(|| CodecError::malformed("IGES sheet radial ring names no coedge"))?
                .radial_next
                .clone();
        }
    }
    scoped(&part, ctx, ir.model.bodies[0].id.as_str()).map(|(document, _)| document)
}

fn append(target: &mut SynthesisGraph, mut source: SynthesisGraph) -> Result<(), CodecError> {
    let offset = target.entities.len();
    for entity in &mut source.entities {
        if let Some(index) = &mut entity.structure {
            *index = index
                .checked_add(offset)
                .ok_or_else(|| CodecError::malformed("IGES Structure index overflows"))?;
        }
        for index in entity
            .property_links
            .iter_mut()
            .chain(entity.association_links.iter_mut())
        {
            *index = index
                .checked_add(offset)
                .ok_or_else(|| CodecError::malformed("IGES property index overflows"))?;
        }
        if let Some(super::DirectoryLineFont::Definition(index)) = &mut entity.line_font {
            *index = index
                .checked_add(offset)
                .ok_or_else(|| CodecError::malformed("IGES line font index overflows"))?;
        }
        entity.parameter_body = super::map_parameter_references(&entity.parameter_body, |index| {
            index
                .checked_add(offset)
                .map(reference_marker)
                .ok_or_else(|| CodecError::malformed("IGES graph entity index overflows"))
        })?;
    }
    for (index, presentation) in source.presentations {
        let index = index
            .checked_add(offset)
            .ok_or_else(|| CodecError::malformed("IGES graph presentation index overflows"))?;
        target.presentations.insert(index, presentation);
    }
    target.entities.extend(source.entities);
    target.losses.extend(source.losses);
    Ok(())
}

/// Follow typed references, including procedural support, before pruning.
/// A body and its topology remain one admission unit because a solid cannot
/// retain its meaning after a boundary face is removed.
fn scoped(
    ir: &CadIr,
    ctx: &DecodeContext<'_>,
    seed: &str,
) -> Result<(CadIr, BTreeSet<String>), CodecError> {
    let mut identities = BTreeSet::from([seed.to_owned()]);
    loop {
        let mut references = BTreeSet::new();
        macro_rules! visit {
            ($($arena:ident),* $(,)?) => {$(
                for record in &ir.model.$arena {
                    if identities.contains(record.id.as_str()) {
                        record.visit_identity_references(ctx, &mut |identity| {
                            references.insert(identity.to_owned());
                            Ok(())
                        })?;
                    }
                }
            )*};
        }
        visit!(
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
        let previous = identities.len();
        identities.extend(references);
        if identities.len() == previous {
            break;
        }
    }
    let mut document = CadIr::empty();
    document.scope = ir.scope;
    document.tolerances = ir.tolerances;
    macro_rules! retain {
        ($($arena:ident),* $(,)?) => {$(
            document.model.$arena = ir.model.$arena.iter()
                .filter(|record| identities.contains(record.id.as_str())).cloned().collect();
        )*};
    }
    retain!(
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
    Ok((document, identities))
}
