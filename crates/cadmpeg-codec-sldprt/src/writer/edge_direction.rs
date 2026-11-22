// SPDX-License-Identifier: Apache-2.0
//! Choose the stored direction of written edges.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::{CurveId, EdgeId};
use cadmpeg_ir::topology::Sense;

/// Edges stored against their direction, and the compact curves that carry
/// them.
pub(super) struct ReversedEdges {
    pub(super) curves: HashSet<CurveId>,
    pub(super) edges: HashSet<EdgeId>,
}

impl ReversedEdges {
    pub(super) fn stored_sense(&self, edge: &EdgeId, sense: Sense) -> Sense {
        match (self.edges.contains(edge), sense) {
            (true, Sense::Forward) => Sense::Reversed,
            (true, Sense::Reversed) => Sense::Forward,
            (false, sense) => sense,
        }
    }
}

/// A stored edge runs along its canonical forward coedge (`sldprt.md` §4), so
/// an edge whose coedges all run against it is stored reversed. On a compact
/// analytic curve the curve carries the marker that runs against edges. On a
/// NURBS curve the reader orders the edge by its vertex parameters (§7.2),
/// which a closed edge does not order: that edge cannot be written.
pub(super) fn reversed_edges(ir: &CadIr) -> Result<ReversedEdges, CodecError> {
    let nurbs_curves = ir
        .geometry
        .curves
        .iter()
        .filter(|curve| matches!(curve.geometry.solved(), Some(SolvedCurveGeometry::Nurbs(_))))
        .map(|curve| &curve.id)
        .collect::<HashSet<_>>();
    let mut forward_coedges = HashMap::<&EdgeId, bool>::new();
    for coedge in &ir.geometry.coedges {
        *forward_coedges.entry(&coedge.edge).or_default() |= coedge.sense == Sense::Forward;
    }
    let mut curve_directions = HashMap::<&CurveId, bool>::new();
    let mut edges = HashSet::new();
    for edge in &ir.geometry.edges {
        let Some(curve) = edge.curve() else {
            continue;
        };
        let Some(&has_forward) = forward_coedges.get(&edge.id) else {
            continue;
        };
        let reversed = !has_forward;
        if nurbs_curves.contains(curve) {
            if reversed {
                if edge.ends.as_ref().is_none_or(|ends| ends.start == ends.end) {
                    return Err(CodecError::NotImplemented(format!(
                        "SLDPRT closed NURBS edge {} has only coedges that run against it, \
                         and its stored direction cannot state that",
                        edge.id
                    )));
                }
                edges.insert(edge.id.clone());
            }
            continue;
        }
        if *curve_directions.entry(curve).or_insert(reversed) != reversed {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT curve {curve} carries edges stored in both directions"
            )));
        }
        if reversed {
            edges.insert(edge.id.clone());
        }
    }
    let curves = curve_directions
        .into_iter()
        .filter(|(_, reversed)| *reversed)
        .map(|(curve, _)| curve.clone())
        .collect();
    Ok(ReversedEdges { curves, edges })
}

#[cfg(test)]
mod tests;
