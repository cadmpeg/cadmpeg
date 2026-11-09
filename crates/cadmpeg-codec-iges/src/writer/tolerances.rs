// SPDX-License-Identifier: Apache-2.0
//! Per-entity tolerances that IGES cannot state.
//!
//! IGES states one tolerance, the Global minimum resolution (IGES 5.3
//! §2.2.4.3.19), and no tolerance per vertex, edge or face. A sheet or
//! free-geometry vertex or edge reads that resolution back; a manifold solid
//! B-rep vertex or edge reads none.

use std::collections::BTreeSet;

use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::CadIr;

use super::{IgesLossCode, LossNote};

/// One loss per topology arena that names each record whose tolerance the
/// output does not hold. `resolution` is the written Global resolution.
pub(super) fn losses(ir: &CadIr, resolution: Option<f64>) -> Vec<LossNote> {
    let solid = solid_topology(ir);
    let lost = |id: &str, tolerance: Option<cadmpeg_ir::scalar::PositiveReal>| {
        tolerance.is_some_and(|tolerance| solid.contains(id) || resolution != Some(tolerance.get()))
    };
    let vertices = ir
        .model
        .vertices
        .iter()
        .filter(|vertex| lost(vertex.id.as_str(), vertex.tolerance))
        .map(|vertex| vertex.id.as_str());
    let edges = ir
        .model
        .edges
        .iter()
        .filter(|edge| lost(edge.id.as_str(), edge.tolerance))
        .map(|edge| edge.id.as_str());
    let faces = ir
        .model
        .faces
        .iter()
        .filter(|face| face.tolerance.is_some())
        .map(|face| face.id.as_str());
    [("vertex", vertices.collect::<Vec<_>>()), ("edge", edges.collect()), ("face", faces.collect())]
        .into_iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(kind, ids)| {
            IgesLossCode::WriterToleranceNotRepresented.note(format!(
                "IGES states no {kind} tolerance; the tolerance of {} {kind} record(s) is not transferred: {}",
                ids.len(),
                ids.join(", ")
            ))
        })
        .collect()
}

/// The edges and vertices of solid bodies, which the manifold solid B-rep
/// path writes without a tolerance.
fn solid_topology(ir: &CadIr) -> BTreeSet<&str> {
    let regions = ir
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == BodyKind::Solid)
        .flat_map(|body| body.regions.iter())
        .collect::<BTreeSet<_>>();
    let shells = ir
        .model
        .shells
        .iter()
        .filter(|shell| regions.contains(&shell.region))
        .map(|shell| &shell.id)
        .collect::<BTreeSet<_>>();
    let faces = ir
        .model
        .faces
        .iter()
        .filter(|face| shells.contains(&face.shell))
        .map(|face| &face.id)
        .collect::<BTreeSet<_>>();
    let loops = ir
        .model
        .loops
        .iter()
        .filter(|boundary| faces.contains(&boundary.face))
        .map(|boundary| &boundary.id)
        .collect::<BTreeSet<_>>();
    let edges = ir
        .model
        .coedges
        .iter()
        .filter(|coedge| loops.contains(&coedge.owner_loop))
        .map(|coedge| &coedge.edge)
        .collect::<BTreeSet<_>>();
    let mut topology = BTreeSet::new();
    for edge in ir
        .model
        .edges
        .iter()
        .filter(|edge| edges.contains(&edge.id))
    {
        topology.insert(edge.id.as_str());
        topology.insert(edge.start.as_str());
        topology.insert(edge.end.as_str());
    }
    topology
}
