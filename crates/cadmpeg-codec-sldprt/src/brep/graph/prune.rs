// SPDX-License-Identifier: Apache-2.0
//! The topology a decode drops with each record it gives up.

use std::collections::HashSet;

use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CoedgeId, EdgeId, ProceduralCurveId};

use super::{Brep, DroppedRecord};

/// The records the decode has dropped, and the notes of the records
/// [`prune_rejected_topology`] drops only because they need one of them.
struct Lost {
    ids: HashSet<String>,
    notes: Vec<DroppedRecord>,
}

impl Lost {
    fn of(dropped: &[DroppedRecord]) -> Self {
        let mut ids = HashSet::new();
        for record in dropped {
            match record {
                DroppedRecord::GivenUp {
                    record, dropping, ..
                } => {
                    ids.insert(record.clone());
                    ids.extend(dropping.iter().cloned());
                }
                DroppedRecord::With { record, .. } => {
                    ids.insert(record.clone());
                }
            }
        }
        Self {
            ids,
            notes: Vec::new(),
        }
    }

    /// Notes `record` as dropped with `needs` when `needs` is lost. A record
    /// pruned with no lost record behind it gets no note.
    fn drop_with(&mut self, record: &str, needs: &str) {
        if self.ids.contains(needs) && !self.ids.contains(record) {
            self.ids.insert(record.to_owned());
            self.notes.push(DroppedRecord::with(record, needs));
        }
    }
}

/// Removes the records that no kept face reaches. `unbuilt_uses` pairs each
/// edge with a coedge of a ring that built no loop.
pub(super) fn prune_rejected_topology(out: &mut Brep, unbuilt_uses: &[(EdgeId, CoedgeId)]) {
    let mut lost = Lost::of(&out.dropped);
    let kept_loops = out
        .faces
        .iter()
        .flat_map(|face| face.loops.stated())
        .map(cadmpeg_ir::ids::LoopId::as_str)
        .collect::<std::collections::HashSet<_>>();
    let (loops, removed_loops): (Vec<_>, Vec<_>) = std::mem::take(&mut out.loops)
        .into_iter()
        .partition(|loop_| kept_loops.contains(loop_.id.as_str()));
    out.loops = loops;
    for loop_ in &removed_loops {
        lost.drop_with(loop_.id.as_str(), loop_.face.as_str());
    }

    let kept_coedges = out
        .loops
        .iter()
        .flat_map(cadmpeg_ir::topology::Loop::coedges)
        .map(cadmpeg_ir::ids::CoedgeId::as_str)
        .collect::<std::collections::HashSet<_>>();
    let (coedges, removed_coedges): (Vec<_>, Vec<_>) = std::mem::take(&mut out.coedges)
        .into_iter()
        .partition(|coedge| kept_coedges.contains(coedge.id.as_str()));
    out.coedges = coedges;
    for coedge in &removed_coedges {
        lost.drop_with(coedge.id.as_str(), coedge.owner_loop.as_str());
    }
    for coedge in &mut out.coedges {
        if !kept_coedges.contains(coedge.radial_next.as_str()) {
            coedge.radial_next = coedge.id.clone();
        }
    }

    let kept_pcurves = out
        .coedges
        .iter()
        .flat_map(|coedge| &coedge.pcurves)
        .map(|use_| &use_.pcurve)
        .map(cadmpeg_ir::ids::PcurveId::as_str)
        .collect::<std::collections::HashSet<_>>();
    let (pcurves, removed_pcurves): (Vec<_>, Vec<_>) = std::mem::take(&mut out.pcurves)
        .into_iter()
        .partition(|pcurve| kept_pcurves.contains(pcurve.id.as_str()));
    out.pcurves = pcurves;
    let removed = removed_pcurves
        .iter()
        .map(|pcurve| pcurve.id.as_str())
        .collect::<HashSet<_>>();
    for coedge in &removed_coedges {
        for use_ in &coedge.pcurves {
            if removed.contains(use_.pcurve.as_str()) {
                lost.drop_with(use_.pcurve.as_str(), coedge.id.as_str());
            }
        }
    }

    let kept_edges = out
        .coedges
        .iter()
        .map(|coedge| coedge.edge.as_str())
        .collect::<std::collections::HashSet<_>>();
    let (edges, removed_edges): (Vec<_>, Vec<_>) = std::mem::take(&mut out.edges)
        .into_iter()
        .partition(|edge| kept_edges.contains(edge.id.as_str()));
    out.edges = edges;
    let removed = removed_edges
        .iter()
        .map(|edge| edge.id.as_str())
        .collect::<HashSet<_>>();
    let uses = removed_coedges
        .iter()
        .map(|coedge| (coedge.edge.as_str(), coedge.id.as_str()))
        .chain(
            unbuilt_uses
                .iter()
                .map(|(edge, coedge)| (edge.as_str(), coedge.as_str())),
        );
    for (edge, coedge) in uses {
        if removed.contains(edge) {
            lost.drop_with(edge, coedge);
        }
    }

    let kept_vertices = out
        .edges
        .iter()
        .flat_map(cadmpeg_ir::topology::Edge::vertices)
        .chain(
            out.loops
                .iter()
                .flat_map(cadmpeg_ir::topology::Loop::vertices),
        )
        .map(cadmpeg_ir::ids::VertexId::as_str)
        .collect::<std::collections::HashSet<_>>();
    let (vertices, removed_vertices): (Vec<_>, Vec<_>) = std::mem::take(&mut out.vertices)
        .into_iter()
        .partition(|vertex| kept_vertices.contains(vertex.id.as_str()));
    out.vertices = vertices;
    let removed = removed_vertices
        .iter()
        .map(|vertex| vertex.id.as_str())
        .collect::<HashSet<_>>();
    let uses = removed_edges
        .iter()
        .flat_map(|edge| edge.vertices().map(|vertex| (vertex, edge.id.as_str())))
        .chain(
            removed_loops
                .iter()
                .flat_map(|loop_| loop_.vertices().map(|vertex| (vertex, loop_.id.as_str()))),
        );
    for (vertex, user) in uses {
        if removed.contains(vertex.as_str()) {
            lost.drop_with(vertex.as_str(), user);
        }
    }

    let kept_points = out
        .vertices
        .iter()
        .map(|vertex| vertex.point.as_str())
        .collect::<std::collections::HashSet<_>>();
    let (points, removed_points): (Vec<_>, Vec<_>) = std::mem::take(&mut out.points)
        .into_iter()
        .partition(|point| kept_points.contains(point.id.as_str()));
    out.points = points;
    let removed = removed_points
        .iter()
        .map(|point| point.id.as_str())
        .collect::<HashSet<_>>();
    for vertex in &removed_vertices {
        if removed.contains(vertex.point.as_str()) {
            lost.drop_with(vertex.point.as_str(), vertex.id.as_str());
        }
    }

    let kept_curves = out
        .edges
        .iter()
        .filter_map(|edge| edge.curve().map(cadmpeg_ir::ids::CurveId::as_str))
        .chain(out.procedural_surfaces.iter().filter_map(|surface| {
            match surface.definition() {
                ProceduralSurfaceDefinition::Blend(definition) => definition.spine().as_ref(),
                ProceduralSurfaceDefinition::LinearSweep(definition) => {
                    Some(definition.directrix())
                }
                _ => None,
            }
            .map(cadmpeg_ir::ids::CurveId::as_str)
        }))
        .collect::<std::collections::HashSet<_>>();
    let (curves, removed_curves): (Vec<_>, Vec<_>) = std::mem::take(&mut out.curves)
        .into_iter()
        .partition(|curve| kept_curves.contains(curve.id.as_str()));
    out.curves = curves;
    let removed = removed_curves
        .iter()
        .map(|curve| curve.id.as_str())
        .collect::<HashSet<_>>();
    for edge in &removed_edges {
        if let Some(curve) = edge
            .curve()
            .filter(|curve| removed.contains(curve.as_str()))
        {
            lost.drop_with(curve.as_str(), edge.id.as_str());
        }
    }
    let kept_constructions = out
        .curves
        .iter()
        .filter_map(|curve| {
            curve
                .geometry
                .procedural_construction()
                .map(ProceduralCurveId::as_str)
        })
        .collect::<std::collections::HashSet<_>>();
    let (procedural_curves, removed_constructions): (Vec<_>, Vec<_>) =
        std::mem::take(&mut out.procedural_curves)
            .into_iter()
            .partition(|procedural| kept_constructions.contains(procedural.id.as_str()));
    out.procedural_curves = procedural_curves;
    let removed = removed_constructions
        .iter()
        .map(|procedural| procedural.id.as_str())
        .collect::<HashSet<_>>();
    for curve in &removed_curves {
        if let Some(construction) = curve
            .geometry
            .procedural_construction()
            .filter(|construction| removed.contains(construction.as_str()))
        {
            lost.drop_with(construction.as_str(), curve.id.as_str());
        }
    }
    out.dropped.append(&mut lost.notes);
    let unknown_curves = out
        .curves
        .iter()
        .filter(|curve| {
            matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
            )
        })
        .map(|curve| curve.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    out.stats.unknown_curve_edges = out
        .edges
        .iter()
        .filter(|edge| {
            edge.curve()
                .is_some_and(|curve_id| unknown_curves.contains(curve_id.as_str()))
        })
        .count();
}

/// Why the FIN ring of a loop record does not build a loop.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RingLoss {
    /// The walk reaches a FIN that complete records claim in conflict.
    AmbiguousFin,
    /// The ring closes, and a forward link has no reciprocal backward link.
    NonReciprocal,
    /// The ring ends before it returns to its first FIN.
    Open,
}

impl RingLoss {
    pub(super) fn code(self) -> crate::loss::SldprtLossCode {
        match self {
            Self::AmbiguousFin => crate::loss::SldprtLossCode::TopologyAmbiguousFinFaceWithheld,
            Self::NonReciprocal | Self::Open => {
                crate::loss::SldprtLossCode::TopologyFinLinkUnresolved
            }
        }
    }

    pub(super) const fn why(self) -> &'static str {
        match self {
            Self::AmbiguousFin => "the ring of the loop holds an ambiguous FIN",
            Self::NonReciprocal => {
                "a FIN forward link of the loop names no reciprocal backward link"
            }
            Self::Open => "the FIN ring of the loop does not close",
        }
    }
}
