// SPDX-License-Identifier: Apache-2.0
//! Choose the stored direction of written edges.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::ids::{CurveId, EdgeId};
use cadmpeg_ir::topology::Sense;

/// Edges stored against their direction, and the curves written to run along
/// those stored edges.
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
/// an edge whose coedges all run against it is stored reversed, and its curve
/// is written to run along the stored edge. A compact analytic curve carries
/// the marker that runs against edges; a NURBS curve whose edges are all stored
/// reversed is written with its parameterization reversed. A NURBS curve that
/// also carries an edge stored along it keeps its parameterization, and the
/// reader orders its reversed edges by their vertex parameters (§7.2), which a
/// closed edge does not order: that edge cannot be written.
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
    // Compact curves: the one direction of their edges. NURBS curves: whether
    // every edge is stored reversed.
    let mut curve_directions = HashMap::<&CurveId, bool>::new();
    let mut nurbs_reversed = HashMap::<&CurveId, bool>::new();
    let mut closed_nurbs_edges = Vec::new();
    let mut edges = HashSet::new();
    for edge in &ir.geometry.edges {
        let Some(curve) = edge.curve() else {
            continue;
        };
        let Some(&has_forward) = forward_coedges.get(&edge.id) else {
            continue;
        };
        let reversed = !has_forward;
        if reversed {
            edges.insert(edge.id.clone());
        }
        if nurbs_curves.contains(curve) {
            *nurbs_reversed.entry(curve).or_insert(true) &= reversed;
            if reversed && edge.ends.as_ref().is_none_or(|ends| ends.start == ends.end) {
                closed_nurbs_edges.push((&edge.id, curve));
            }
            continue;
        }
        if *curve_directions.entry(curve).or_insert(reversed) != reversed {
            return Err(CodecError::NotImplemented(format!(
                "SLDPRT curve {curve} carries edges stored in both directions"
            )));
        }
    }
    if let Some((edge, _)) = closed_nurbs_edges
        .into_iter()
        .find(|(_, curve)| !nurbs_reversed[curve])
    {
        return Err(CodecError::NotImplemented(format!(
            "SLDPRT closed NURBS edge {edge} has only coedges that run against it, \
             on a curve another edge runs along, and its stored direction cannot state that"
        )));
    }
    let curves = curve_directions
        .into_iter()
        .chain(nurbs_reversed)
        .filter(|(_, reversed)| *reversed)
        .map(|(curve, _)| curve.clone())
        .collect();
    Ok(ReversedEdges { curves, edges })
}

#[cfg(test)]
mod tests;
