// SPDX-License-Identifier: Apache-2.0
//! Face-granular sheet degradation after native Brep admission declines.

use crate::loss::RhinoLossCode;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{BodyKind, Shell};
use std::collections::{BTreeMap, BTreeSet};

fn unique<'a>(
    ids: impl Iterator<Item = &'a str>,
    arena: &str,
) -> Result<BTreeSet<&'a str>, CodecError> {
    let mut result = BTreeSet::new();
    for id in ids {
        if !result.insert(id) {
            return Err(CodecError::malformed(format_args!(
                "duplicate {arena} identifier {id}"
            )));
        }
    }
    Ok(result)
}

/// Validate the original graph before any omission can hide a malformed edge
/// use, dangling reference, or disconnected radial cycle.
pub(super) fn validate(ir: &CadIr) -> Result<(), CodecError> {
    let model = &ir.model;
    if model.bodies.len() != 1 {
        return Err(CodecError::malformed(
            "body projection requires exactly one body",
        ));
    }
    let regions = unique(model.regions.iter().map(|row| row.id.as_str()), "region")?;
    let shells = unique(model.shells.iter().map(|row| row.id.as_str()), "shell")?;
    let faces = unique(model.faces.iter().map(|row| row.id.as_str()), "face")?;
    let loops = unique(model.loops.iter().map(|row| row.id.as_str()), "loop")?;
    let coedges = unique(model.coedges.iter().map(|row| row.id.as_str()), "coedge")?;
    let edges = unique(model.edges.iter().map(|row| row.id.as_str()), "edge")?;
    let vertices = unique(model.vertices.iter().map(|row| row.id.as_str()), "vertex")?;
    let points = unique(model.points.iter().map(|row| row.id.as_str()), "point")?;
    let surfaces = unique(model.surfaces.iter().map(|row| row.id.as_str()), "surface")?;
    let curves = unique(model.curves.iter().map(|row| row.id.as_str()), "curve")?;
    let pcurves = unique(model.pcurves.iter().map(|row| row.id.as_str()), "pcurve")?;
    let missing = model.bodies[0]
        .regions
        .iter()
        .any(|id| !regions.contains(id.as_str()))
        || model.regions.iter().any(|row| {
            row.body != model.bodies[0].id
                || !model.bodies[0].regions.contains(&row.id)
                || row.shells.iter().any(|id| !shells.contains(id.as_str()))
        })
        || model.shells.iter().any(|row| {
            !model
                .regions
                .iter()
                .any(|region| region.id == row.region && region.shells.contains(&row.id))
                || row.faces().iter().any(|id| !faces.contains(id.as_str()))
                || row
                    .wire_edges()
                    .iter()
                    .any(|id| !edges.contains(id.as_str()))
                || row
                    .free_vertices()
                    .iter()
                    .any(|id| !vertices.contains(id.as_str()))
        })
        || model.faces.iter().any(|row| {
            !surfaces.contains(row.surface.as_str())
                || !model
                    .shells
                    .iter()
                    .any(|shell| shell.id == row.shell && shell.faces().contains(&row.id))
                || row.loops.iter().any(|id| !loops.contains(id.as_str()))
        })
        || model.loops.iter().any(|row| {
            !model
                .faces
                .iter()
                .any(|face| face.id == row.face && face.loops.iter().any(|id| id == &row.id))
                || row
                    .coedges()
                    .iter()
                    .any(|id| !coedges.contains(id.as_str()))
                || row.vertices().any(|id| !vertices.contains(id.as_str()))
                || row
                    .vertex_pcurves()
                    .any(|use_| !pcurves.contains(use_.pcurve.as_str()))
        })
        || model.coedges.iter().any(|row| {
            !edges.contains(row.edge.as_str())
                || !model
                    .loops
                    .iter()
                    .any(|loop_| loop_.id == row.owner_loop && loop_.coedges().contains(&row.id))
                || row
                    .pcurves
                    .iter()
                    .any(|use_| !pcurves.contains(use_.pcurve.as_str()))
                || row
                    .use_curve
                    .as_ref()
                    .is_some_and(|use_| !curves.contains(use_.curve.as_str()))
        })
        || model.edges.iter().any(|row| {
            !vertices.contains(row.start.as_str())
                || !vertices.contains(row.end.as_str())
                || row.curve().is_some_and(|id| !curves.contains(id.as_str()))
        })
        || model
            .vertices
            .iter()
            .any(|row| !points.contains(row.point.as_str()));
    if missing {
        return Err(CodecError::malformed(
            "sheet topology has missing or inconsistent ownership references",
        ));
    }
    validate_loops(ir)?;
    validate_radials(ir)
}

fn validate_loops(ir: &CadIr) -> Result<(), CodecError> {
    use cadmpeg_ir::topology::Sense;
    let coedges: BTreeMap<_, _> = ir
        .model
        .coedges
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    let edges: BTreeMap<_, _> = ir
        .model
        .edges
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    for loop_ in &ir.model.loops {
        let ring = loop_.coedges();
        for (index, id) in ring.iter().enumerate() {
            let current = coedges
                .get(id.as_str())
                .ok_or_else(|| CodecError::malformed("loop coedge is missing"))?;
            let next = coedges
                .get(ring[(index + 1) % ring.len()].as_str())
                .ok_or_else(|| CodecError::malformed("next loop coedge is missing"))?;
            let edge = edges
                .get(current.edge.as_str())
                .ok_or_else(|| CodecError::malformed("loop edge is missing"))?;
            let next_edge = edges
                .get(next.edge.as_str())
                .ok_or_else(|| CodecError::malformed("next loop edge is missing"))?;
            let end = if current.sense == Sense::Forward {
                &edge.end
            } else {
                &edge.start
            };
            let start = if next.sense == Sense::Forward {
                &next_edge.start
            } else {
                &next_edge.end
            };
            if end != start {
                return Err(CodecError::malformed(format_args!(
                    "loop {} coedge traversal does not close",
                    loop_.id
                )));
            }
        }
    }
    Ok(())
}

fn validate_radials(ir: &CadIr) -> Result<(), CodecError> {
    let by_id: BTreeMap<_, _> = ir
        .model
        .coedges
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    for edge in &ir.model.edges {
        let uses: BTreeSet<_> = ir
            .model
            .coedges
            .iter()
            .filter(|row| row.edge == edge.id)
            .map(|row| row.id.as_str())
            .collect();
        let Some(&start) = uses.first() else {
            // Wire edges have no radial uses and are unrelated to face pruning.
            if ir
                .model
                .shells
                .iter()
                .any(|shell| shell.wire_edges().contains(&edge.id))
            {
                continue;
            }
            return Err(CodecError::malformed(format_args!(
                "sheet edge {} has no owning use",
                edge.id
            )));
        };
        let mut visited = BTreeSet::new();
        let mut current = start;
        for _ in 0..uses.len() {
            if !uses.contains(current) || !visited.insert(current) {
                return Err(CodecError::malformed(format_args!(
                    "edge {} radial ring is inconsistent",
                    edge.id
                )));
            }
            let row = by_id
                .get(current)
                .ok_or_else(|| CodecError::malformed("radial reference is missing"))?;
            current = row.radial_next.as_str();
        }
        if current != start || visited != uses {
            return Err(CodecError::malformed(format_args!(
                "edge {} radial ring does not close over all uses",
                edge.id
            )));
        }
    }
    Ok(())
}

pub(super) fn retain_faces(ir: &CadIr, selected: &BTreeSet<&str>) -> Result<CadIr, CodecError> {
    let mut result = ir.clone();
    result
        .model
        .faces
        .retain(|face| selected.contains(face.id.as_str()));
    let loop_ids: BTreeSet<_> = result
        .model
        .faces
        .iter()
        .flat_map(|face| face.loops.iter().map(|id| id.as_str()))
        .collect();
    result
        .model
        .loops
        .retain(|row| loop_ids.contains(row.id.as_str()));
    let coedge_ids: BTreeSet<_> = result
        .model
        .loops
        .iter()
        .flat_map(|row| row.coedges().iter().map(|id| id.as_str()))
        .collect();
    result
        .model
        .coedges
        .retain(|row| coedge_ids.contains(row.id.as_str()));
    let original: BTreeMap<_, _> = ir
        .model
        .coedges
        .iter()
        .map(|row| (row.id.as_str(), row))
        .collect();
    let kept: BTreeSet<_> = result
        .model
        .coedges
        .iter()
        .map(|row| row.id.as_str().to_owned())
        .collect();
    for coedge in &mut result.model.coedges {
        let mut next = original[coedge.id.as_str()].radial_next.as_str();
        while !kept.contains(next) {
            next = original
                .get(next)
                .ok_or_else(|| {
                    CodecError::malformed("radial reference disappeared during sheet reduction")
                })?
                .radial_next
                .as_str();
        }
        coedge.radial_next = original[next].id.clone();
    }
    let edge_ids: BTreeSet<_> = result
        .model
        .coedges
        .iter()
        .map(|row| row.edge.as_str())
        .collect();
    result
        .model
        .edges
        .retain(|row| edge_ids.contains(row.id.as_str()));
    let vertex_ids: BTreeSet<_> = result
        .model
        .edges
        .iter()
        .flat_map(|row| [row.start.as_str(), row.end.as_str()])
        .chain(
            result
                .model
                .loops
                .iter()
                .flat_map(|row| row.vertices().map(|id| id.as_str())),
        )
        .collect();
    result
        .model
        .vertices
        .retain(|row| vertex_ids.contains(row.id.as_str()));
    let point_ids: BTreeSet<_> = result
        .model
        .vertices
        .iter()
        .map(|row| row.point.as_str())
        .collect();
    result
        .model
        .points
        .retain(|row| point_ids.contains(row.id.as_str()));
    let curve_ids: BTreeSet<_> = result
        .model
        .edges
        .iter()
        .filter_map(|row| row.curve().map(|id| id.as_str()))
        .chain(
            result
                .model
                .coedges
                .iter()
                .filter_map(|row| row.use_curve.as_ref().map(|use_| use_.curve.as_str())),
        )
        .collect();
    result
        .model
        .curves
        .retain(|row| curve_ids.contains(row.id.as_str()));
    let surface_ids: BTreeSet<_> = result
        .model
        .faces
        .iter()
        .map(|row| row.surface.as_str())
        .collect();
    result
        .model
        .surfaces
        .retain(|row| surface_ids.contains(row.id.as_str()));
    let pcurve_ids: BTreeSet<_> = result
        .model
        .coedges
        .iter()
        .flat_map(|row| &row.pcurves)
        .chain(
            result
                .model
                .loops
                .iter()
                .flat_map(|row| row.vertex_pcurves()),
        )
        .map(|use_| use_.pcurve.as_str())
        .collect();
    result
        .model
        .pcurves
        .retain(|row| pcurve_ids.contains(row.id.as_str()));
    let procedural_curves: BTreeSet<_> = result
        .model
        .curves
        .iter()
        .filter_map(|row| row.geometry.procedural_construction())
        .collect();
    result
        .model
        .procedural_curves
        .retain(|row| procedural_curves.contains(&row.id));
    let procedural_surfaces: BTreeSet<_> = result
        .model
        .surfaces
        .iter()
        .filter_map(|row| row.geometry.procedural_construction())
        .collect();
    result
        .model
        .procedural_surfaces
        .retain(|row| procedural_surfaces.contains(&row.id));
    let mut shells = Vec::new();
    for shell in &ir.model.shells {
        let faces = shell
            .faces()
            .iter()
            .filter(|id| selected.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        if !faces.is_empty() {
            shells.push(
                Shell::with_faces(shell.id.clone(), shell.region.clone(), faces)
                    .map_err(CodecError::malformed)?,
            );
        }
    }
    result.model.shells = shells;
    let shell_ids: BTreeSet<_> = result
        .model
        .shells
        .iter()
        .map(|row| row.id.as_str())
        .collect();
    for region in &mut result.model.regions {
        region.shells.retain(|id| shell_ids.contains(id.as_str()));
    }
    result.model.regions.retain(|row| !row.shells.is_empty());
    let region_ids: BTreeSet<_> = result
        .model
        .regions
        .iter()
        .map(|row| row.id.as_str())
        .collect();
    result.model.bodies[0]
        .regions
        .retain(|id| region_ids.contains(id.as_str()));
    result.model.tessellations.clear();
    Ok(result)
}

/// Admit sheet faces incrementally. Only an unsupported-content outcome drops
/// a face; malformed input, native capacity, and resource errors propagate.
pub(super) fn sheet(
    ir: &CadIr,
    admit: impl Fn(&CadIr) -> Result<(), CodecError>,
) -> Result<(Option<CadIr>, Vec<LossNote>), CodecError> {
    if ir
        .model
        .bodies
        .first()
        .is_none_or(|body| body.kind != BodyKind::Sheet)
    {
        return Err(CodecError::malformed(
            "sheet reduction requires one sheet body",
        ));
    }
    validate(ir)?;
    let mut selected = BTreeSet::new();
    let mut accepted = None;
    let mut losses = Vec::new();
    for face in &ir.model.faces {
        selected.insert(face.id.as_str());
        let candidate = retain_faces(ir, &selected)?;
        match admit(&candidate) {
            Ok(()) => accepted = Some(candidate),
            Err(CodecError::UnsupportedContent(reason)) => {
                selected.remove(face.id.as_str());
                losses.push(
                    RhinoLossCode::WriterEntityOmitted
                        .note(format!("sheet face {} omitted: {reason}", face.id)),
                );
            }
            Err(error) => return Err(error),
        }
    }
    Ok((accepted, losses))
}
