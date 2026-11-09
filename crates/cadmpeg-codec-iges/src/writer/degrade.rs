// SPDX-License-Identifier: Apache-2.0
//! Independent entity admission for lossy IGES synthesis.

use super::{
    reference_marker, synthesis_graph, unsupported_model_losses, BTreeMap, BTreeSet, BodyKind,
    CadIr, CodecError, DecodeContext, IgesLossCode, SynthesisGraph,
};
use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;

pub(super) fn has_dangling_body_region(ir: &CadIr) -> bool {
    ir.model.bodies.iter().any(|body| {
        body.regions
            .iter()
            .any(|id| !ir.model.regions.iter().any(|region| region.id == *id))
    })
}

/// Preserve independent bodies and free carriers when one cannot be encoded.
pub(super) fn graph(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    version: crate::IgesVersion,
    reason: &str,
) -> Result<SynthesisGraph, crate::writer::WriteError> {
    let mut result = SynthesisGraph {
        entities: Vec::new(),
        presentations: BTreeMap::new(),
        losses: unsupported_model_losses(ir)?,
    };
    report_detached_topology(ir, &mut result.losses);
    let mut owned = BTreeSet::new();
    for body in &ir.model.bodies {
        let (part, identities) = scoped(ir, ctx, body.id.as_str())?;
        owned.extend(identities);
        admit(ctx, &mut result, &part, version, body.id.as_str())?;
    }
    // Body-owned carriers must not reappear as free geometry after a body was
    // omitted. Each remaining independent carrier is admitted on its own; a
    // dependent carrier that the whole document withholds stays withheld.
    let ownership = super::ownership::SourceOwnership::build(ir);
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
        if ownership.withholds(ir, id) {
            super::ownership::report_unowned_support(&mut result.losses, id);
            continue;
        }
        let (part, identities) = scoped(ir, ctx, id)?;
        owned.extend(identities);
        admit(ctx, &mut result, &part, version, id)?;
    }
    result
        .losses
        .push(IgesLossCode::WriterEntityOmitted.note(reason));
    Ok(result)
}

fn report_detached_topology(ir: &CadIr, losses: &mut Vec<super::LossNote>) {
    let invalid_bodies = ir
        .model
        .bodies
        .iter()
        .filter(|body| {
            body.regions
                .iter()
                .any(|id| !ir.model.regions.iter().any(|region| region.id == *id))
        })
        .map(|body| &body.id)
        .collect::<BTreeSet<_>>();
    let regions = ir
        .model
        .regions
        .iter()
        .filter(|region| invalid_bodies.contains(&region.body))
        .collect::<Vec<_>>();
    let shells = ir
        .model
        .shells
        .iter()
        .filter(|shell| regions.iter().any(|region| region.id == shell.region))
        .collect::<Vec<_>>();
    let faces = ir
        .model
        .faces
        .iter()
        .filter(|face| shells.iter().any(|shell| shell.id == face.shell))
        .collect::<Vec<_>>();
    let loops = ir
        .model
        .loops
        .iter()
        .filter(|boundary| faces.iter().any(|face| face.id == boundary.face))
        .collect::<Vec<_>>();
    let coedges = ir.model.coedges.iter().filter(|coedge| {
        loops
            .iter()
            .any(|boundary| boundary.id == coedge.owner_loop)
    });
    for id in regions
        .iter()
        .map(|row| row.id.as_str())
        .chain(shells.iter().map(|row| row.id.as_str()))
        .chain(faces.iter().map(|row| row.id.as_str()))
        .chain(loops.iter().map(|row| row.id.as_str()))
        .chain(coedges.map(|row| row.id.as_str()))
    {
        losses.push(IgesLossCode::WriterEntityOmitted.note(format!(
            "IGES topology record {id} is omitted because its body references a missing region"
        )));
    }
}

fn admit(
    ctx: &DecodeContext<'_>,
    result: &mut SynthesisGraph,
    ir: &CadIr,
    version: crate::IgesVersion,
    identity: &str,
) -> Result<(), crate::writer::WriteError> {
    if has_dangling_body_region(ir) {
        for body in &ir.model.bodies {
            if body
                .regions
                .iter()
                .any(|id| !ir.model.regions.iter().any(|region| region.id == *id))
            {
                result
                    .losses
                    .push(IgesLossCode::WriterEntityOmitted.note(format!(
                        "IGES body {} is omitted because it references a missing region",
                        body.id
                    )));
            }
        }
        return Ok(());
    }
    match synthesis_graph(ctx, ir, version) {
        Ok(graph) => append(result, graph),
        Err(
            crate::writer::WriteError::Unwritable(reason)
            | crate::writer::WriteError::Codec(
                CodecError::InvalidInput(reason) | CodecError::Malformed(reason),
            ),
        ) => {
            // Type 508 can carry the model-space edge boundary without UV
            // images. Prefer that degradation to discarding an entire body.
            if ir.model.bodies.len() == 1 && !ir.model.pcurves.is_empty() {
                let without_uv = without_pcurves(ir)?;
                match synthesis_graph(ctx, &without_uv, version) {
                    Ok(mut graph) => {
                        for pcurve in &ir.model.pcurves {
                            graph.losses.push(IgesLossCode::WriterPcurvesOmitted.note(format!(
                                "IGES entity {identity} retains its model-space boundary without parameter-space curve {}: {reason}",
                                pcurve.id
                            )));
                        }
                        return append(result, graph);
                    }
                    Err(
                        crate::writer::WriteError::Unwritable(_)
                        | crate::writer::WriteError::Codec(
                            CodecError::InvalidInput(_) | CodecError::Malformed(_),
                        ),
                    ) => {}
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
                return admit(ctx, result, &free, version, identity);
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
                for face in &ir.model.faces {
                    let part = face_document(ir, ctx, face)?;
                    admit(ctx, result, &part, version, face.id.as_str())?;
                }
                return Ok(());
            }
            result.losses.push(
                IgesLossCode::WriterEntityOmitted
                    .note(format!("IGES entity {identity} is omitted: {reason}")),
            );
            if ir
                .model
                .bodies
                .iter()
                .any(|body| body.id.as_str() == identity)
            {
                let owned = ir
                    .model
                    .regions
                    .iter()
                    .map(|record| record.id.as_str())
                    .chain(ir.model.shells.iter().map(|record| record.id.as_str()))
                    .chain(ir.model.faces.iter().map(|record| record.id.as_str()))
                    .chain(ir.model.loops.iter().map(|record| record.id.as_str()))
                    .chain(ir.model.coedges.iter().map(|record| record.id.as_str()));
                for id in owned {
                    result
                        .losses
                        .push(IgesLossCode::WriterEntityOmitted.note(format!(
                            "IGES topology record {id} is omitted with body {identity}: {reason}"
                        )));
                }
            }
            for procedural in &ir.model.procedural_surfaces {
                if ir
                    .model
                    .procedural_surface_owner(&procedural.id)
                    .is_some_and(|owner| owner.as_str() == identity)
                {
                    result
                        .losses
                        .push(IgesLossCode::WriterEntityOmitted.note(format!(
                        "IGES procedural surface {} is omitted with carrier {identity}: {reason}",
                        procedural.id
                    )));
                }
            }
            for procedural in &ir.model.procedural_curves {
                if ir
                    .model
                    .procedural_curve_owner(&procedural.id)
                    .is_some_and(|owner| owner.as_str() == identity)
                {
                    result
                        .losses
                        .push(IgesLossCode::WriterEntityOmitted.note(format!(
                            "IGES procedural curve {} is omitted with carrier {identity}: {reason}",
                            procedural.id
                        )));
                }
            }
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Remove the redundant UV representation without changing the 3D boundary.
fn without_pcurves(ir: &CadIr) -> Result<CadIr, crate::writer::WriteError> {
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
                    .map_err(|error| crate::writer::WriteError::Unwritable(error.to_string()))?;
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
) -> Result<CadIr, crate::writer::WriteError> {
    let mut part = ir.clone();
    let shell = ir
        .model
        .shells
        .iter()
        .find(|shell| shell.id == face.shell)
        .ok_or_else(|| {
            crate::writer::WriteError::Unwritable("IGES sheet face has no shell".into())
        })?;
    part.model.bodies[0].regions = vec![shell.region.clone()];
    part.model
        .regions
        .retain(|region| region.id == shell.region);
    let region = part.model.regions.first_mut().ok_or_else(|| {
        crate::writer::WriteError::Unwritable("IGES sheet shell has no region".into())
    })?;
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
                return Err(crate::writer::WriteError::Unwritable(
                    "IGES sheet has an unclosed radial ring".into(),
                ));
            }
            coedge.radial_next = ir
                .model
                .coedges
                .iter()
                .find(|candidate| candidate.id == coedge.radial_next)
                .ok_or_else(|| {
                    crate::writer::WriteError::Unwritable(
                        "IGES sheet radial ring names no coedge".into(),
                    )
                })?
                .radial_next
                .clone();
        }
    }
    scoped(&part, ctx, ir.model.bodies[0].id.as_str()).map(|(document, _)| document)
}

fn append(
    target: &mut SynthesisGraph,
    mut source: SynthesisGraph,
) -> Result<(), crate::writer::WriteError> {
    let offset = target.entities.len();
    for entity in &mut source.entities {
        if let Some(index) = &mut entity.structure {
            *index = index.checked_add(offset).ok_or_else(|| {
                crate::writer::WriteError::Unwritable("IGES Structure index overflows".into())
            })?;
        }
        for index in entity
            .property_links
            .iter_mut()
            .chain(entity.association_links.iter_mut())
        {
            *index = index.checked_add(offset).ok_or_else(|| {
                crate::writer::WriteError::Unwritable("IGES property index overflows".into())
            })?;
        }
        if let Some(super::DirectoryLineFont::Definition(index)) = &mut entity.line_font {
            *index = index.checked_add(offset).ok_or_else(|| {
                crate::writer::WriteError::Unwritable("IGES line font index overflows".into())
            })?;
        }
        entity.parameter_body = super::map_parameter_references(&entity.parameter_body, |index| {
            index
                .checked_add(offset)
                .map(reference_marker)
                .ok_or_else(|| {
                    crate::writer::WriteError::Unwritable(
                        "IGES graph entity index overflows".into(),
                    )
                })
        })?;
    }
    for (index, presentation) in source.presentations {
        let index = index.checked_add(offset).ok_or_else(|| {
            crate::writer::WriteError::Unwritable("IGES graph presentation index overflows".into())
        })?;
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
) -> Result<(CadIr, BTreeSet<String>), crate::writer::WriteError> {
    let mut identities = BTreeSet::from([seed.to_owned()]);
    loop {
        let mut references = BTreeSet::new();
        macro_rules! visit {
            ($($arena:ident),* $(,)?) => {$(
                for record in &ir.model.$arena {
                    if identities.contains(record.id.as_str()) {
                        record.visit_identity_references::<cadmpeg_core::CodecError>(ctx, &mut |identity| {
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
