// SPDX-License-Identifier: Apache-2.0
//! Complete body closures before Rhino capability-specific projection.

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;

pub(super) struct BodyScope {
    pub(super) ir: CadIr,
    pub(super) points: std::collections::BTreeSet<String>,
    pub(super) surfaces: std::collections::BTreeSet<String>,
    pub(super) curves: std::collections::BTreeSet<String>,
    pub(super) pcurves: std::collections::BTreeSet<String>,
}

pub(super) fn collect(ir: &CadIr) -> Result<Vec<BodyScope>, CodecError> {
    use std::collections::BTreeSet;

    let model = &ir.model;
    let mut scopes = Vec::new();

    for body in &model.bodies {
        let regions = body
            .regions
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let shells = model
            .regions
            .iter()
            .filter(|region| regions.contains(region.id.as_str()))
            .flat_map(|region| region.shells.iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let faces = model
            .shells
            .iter()
            .filter(|shell| shells.contains(shell.id.as_str()))
            .flat_map(|shell| shell.faces().iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let surfaces = model
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .map(|face| face.surface.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let loops = model
            .faces
            .iter()
            .filter(|face| faces.contains(face.id.as_str()))
            .flat_map(|face| face.loops.iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let coedges = model
            .loops
            .iter()
            .filter(|loop_| loops.contains(loop_.id.as_str()))
            .flat_map(|loop_| loop_.coedges().iter().map(|id| id.as_str().to_owned()))
            .collect::<BTreeSet<_>>();
        let edges = model
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(coedge.id.as_str()))
            .map(|coedge| coedge.edge.as_str().to_owned())
            .chain(
                model
                    .shells
                    .iter()
                    .filter(|shell| shells.contains(shell.id.as_str()))
                    .flat_map(|shell| shell.wire_edges().iter().map(ToString::to_string)),
            )
            .collect::<BTreeSet<_>>();
        let pcurves = model
            .coedges
            .iter()
            .filter(|coedge| coedges.contains(coedge.id.as_str()))
            .flat_map(|coedge| coedge.pcurves.iter())
            .chain(
                model
                    .loops
                    .iter()
                    .filter(|row| loops.contains(row.id.as_str()))
                    .flat_map(cadmpeg_ir::topology::Loop::vertex_pcurves),
            )
            .map(|use_| use_.pcurve.as_str().to_owned())
            .collect::<BTreeSet<_>>();
        let vertices = model
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()))
            .flat_map(|edge| [edge.start.as_str().to_owned(), edge.end.as_str().to_owned()])
            .chain(
                model
                    .loops
                    .iter()
                    .filter(|row| loops.contains(row.id.as_str()))
                    .flat_map(cadmpeg_ir::topology::Loop::vertices)
                    .map(ToString::to_string),
            )
            .chain(
                model
                    .shells
                    .iter()
                    .filter(|shell| shells.contains(shell.id.as_str()))
                    .flat_map(|shell| shell.free_vertices().iter().map(ToString::to_string)),
            )
            .collect::<BTreeSet<_>>();
        let curves = model
            .edges
            .iter()
            .filter(|edge| edges.contains(edge.id.as_str()))
            .filter_map(|edge| edge.curve().map(|id| id.as_str().to_owned()))
            .chain(
                model
                    .coedges
                    .iter()
                    .filter(|row| coedges.contains(row.id.as_str()))
                    .filter_map(|row| row.use_curve.as_ref().map(|use_| use_.curve.to_string())),
            )
            .collect::<BTreeSet<_>>();
        let points = model
            .vertices
            .iter()
            .filter(|vertex| vertices.contains(vertex.id.as_str()))
            .map(|vertex| vertex.point.as_str().to_owned())
            .collect::<BTreeSet<_>>();

        let mut scoped = CadIr::empty();
        scoped.tolerances = ir.tolerances;
        scoped.model.bodies.push(body.clone());
        scoped.model.regions = model
            .regions
            .iter()
            .filter(|entity| regions.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.shells = model
            .shells
            .iter()
            .filter(|entity| shells.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.faces = model
            .faces
            .iter()
            .filter(|entity| faces.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.loops = model
            .loops
            .iter()
            .filter(|entity| loops.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.coedges = model
            .coedges
            .iter()
            .filter(|entity| coedges.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.edges = model
            .edges
            .iter()
            .filter(|entity| edges.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.vertices = model
            .vertices
            .iter()
            .filter(|entity| vertices.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.points = model
            .points
            .iter()
            .filter(|entity| points.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.surfaces = model
            .surfaces
            .iter()
            .filter(|entity| surfaces.contains(entity.id.as_str()))
            .cloned()
            .collect();
        scoped.model.curves = model
            .curves
            .iter()
            .filter(|entity| curves.contains(entity.id.as_str()))
            .cloned()
            .collect();
        for curve in &mut scoped.model.curves {
            if let Some(cache) = curve.geometry.solved().cloned() {
                curve.geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(cache);
            }
        }
        for surface in &mut scoped.model.surfaces {
            if let Some(cache) = surface.geometry.solved().cloned() {
                surface.geometry = cadmpeg_ir::geometry::SurfaceGeometry::Solved(cache);
            }
        }
        scoped.model.pcurves = model
            .pcurves
            .iter()
            .filter(|entity| pcurves.contains(entity.id.as_str()))
            .cloned()
            .collect();
        super::reduce::validate(&scoped)?;
        scopes.push(BodyScope {
            ir: scoped,
            points,
            surfaces,
            curves,
            pcurves,
        });
    }
    Ok(scopes)
}
