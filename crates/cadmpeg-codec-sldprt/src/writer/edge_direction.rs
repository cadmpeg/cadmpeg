// SPDX-License-Identifier: Apache-2.0
//! Choose the stored direction of edges on written compact analytic curves.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::CodecError;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::ids::{CurveId, EdgeId};
use cadmpeg_ir::topology::Sense;

/// Edges whose stored direction runs against their compact curve, and the
/// curves that carry them.
pub(super) struct ReversedCompactEdges {
    pub(super) curves: HashSet<CurveId>,
    pub(super) edges: HashSet<EdgeId>,
}

impl ReversedCompactEdges {
    pub(super) fn stored_sense(&self, edge: &EdgeId, sense: Sense) -> Sense {
        match (self.edges.contains(edge), sense) {
            (true, Sense::Forward) => Sense::Reversed,
            (true, Sense::Reversed) => Sense::Forward,
            (false, sense) => sense,
        }
    }
}

/// A stored edge runs along its forward coedge. An edge on a compact analytic
/// curve whose coedges all run against it is stored reversed, and its curve
/// carries the marker that runs against edges.
pub(super) fn reversed_compact_edges(ir: &CadIr) -> Result<ReversedCompactEdges, CodecError> {
    let compact_curves = ir
        .model
        .curves
        .iter()
        .filter(|curve| {
            !matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))
            )
        })
        .map(|curve| &curve.id)
        .collect::<HashSet<_>>();
    let mut forward_coedges = HashMap::<&EdgeId, bool>::new();
    for coedge in &ir.model.coedges {
        *forward_coedges.entry(&coedge.edge).or_default() |= coedge.sense == Sense::Forward;
    }
    let mut curve_directions = HashMap::<&CurveId, bool>::new();
    let mut edges = HashSet::new();
    for edge in &ir.model.edges {
        let Some(curve) = edge.curve().filter(|curve| compact_curves.contains(curve)) else {
            continue;
        };
        let Some(&has_forward) = forward_coedges.get(&edge.id) else {
            continue;
        };
        let reversed = !has_forward;
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
    Ok(ReversedCompactEdges { curves, edges })
}
