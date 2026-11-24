// SPDX-License-Identifier: Apache-2.0
//! The trim curves of a face written as a `NurbsPatch` prim (spec §14.1).

use cadmpeg_ir::index::StandardIndex;
use cadmpeg_ir::topology::{Face, FaceBoundary, LoopBoundary, PcurveUse, Sense};

use super::records::pcurve_nurbs;
use super::text::{real, tuple};
use super::Writer;
use crate::brep::nurbs::Nurbs;
use crate::brep::patch::{reversed_loop, winding};

impl Writer<'_> {
    /// The parameter-space curve of a pcurve use, in the direction of its
    /// coedge.
    fn use_curve(&self, use_: &PcurveUse, edge_range: Option<[f64; 2]>) -> Option<Nurbs<2>> {
        let pcurve = self.index.pcurves(use_.pcurve.as_str(), StandardIndex)?;
        let range = use_
            .parameter_range
            .map(cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange::endpoints)
            .or(edge_range);
        pcurve_nurbs(&pcurve.geometry, range)
    }

    /// The loops of `face` as closed chains of parameter-space curves, the
    /// outer loop first: counterclockwise when outer and clockwise when
    /// inner. `None` when the face does not classify its loops, or when a
    /// coedge or pole has no parameter-space curve.
    pub(super) fn trim_loops(&self, face: &Face) -> Option<Vec<Vec<Nurbs<2>>>> {
        let boundary = face.loops.boundary()?;
        if matches!(boundary, FaceBoundary::Unspecified(_)) {
            return None;
        }
        let mut loops = Vec::new();
        for (position, loop_id) in boundary.iter().enumerate() {
            let loop_ = self.index.loops(loop_id.as_str(), StandardIndex)?;
            let LoopBoundary::Ring(ring) = &loop_.boundary else {
                return None;
            };
            let mut curves = Vec::new();
            for coedge_id in ring.coedges() {
                let coedge = self.index.coedges(coedge_id.as_str(), StandardIndex)?;
                let edge = self.index.edges(coedge.edge.as_str(), StandardIndex)?;
                let edge_range = edge.carrier.param_range().map(|range| {
                    if coedge.sense == Sense::Forward {
                        [range[0], range[1]]
                    } else {
                        [range[1], range[0]]
                    }
                });
                curves.push(self.use_curve(coedge.pcurves.first()?, edge_range)?);
                for pole in ring
                    .vertex_uses()
                    .iter()
                    .filter(|pole| pole.after == *coedge_id)
                {
                    curves.push(self.use_curve(pole.pcurves.first()?, None)?);
                }
            }
            let counterclockwise = winding(&curves) > 0.0;
            if counterclockwise != (position == 0) {
                curves = reversed_loop(&curves)?;
            }
            loops.push(curves);
        }
        (!loops.is_empty()).then_some(loops)
    }

    /// Writes `loops` as the trim-curve attributes of a `NurbsPatch` prim.
    pub(super) fn trim_attributes(&mut self, loops: &[Vec<Nurbs<2>>]) {
        let mut counts = Vec::new();
        let mut orders = Vec::new();
        let mut vertex_counts = Vec::new();
        let mut knots = Vec::new();
        let mut ranges = Vec::new();
        let mut points = Vec::new();
        for curves in loops {
            counts.push(curves.len().to_string());
            for curve in curves {
                orders.push((curve.degree + 1).to_string());
                vertex_counts.push(curve.points.len().to_string());
                knots.extend(curve.knots.iter().map(|knot| real(*knot)));
                let [low, high] = curve.domain().unwrap_or_default();
                ranges.push(tuple(&[low, high]));
                points.extend(
                    curve
                        .points
                        .iter()
                        .zip(&curve.weights)
                        .map(|([u, v], weight)| tuple(&[u * weight, v * weight, *weight])),
                );
            }
        }
        self.out.array("int[] trimCurve:counts", &counts, "");
        self.out.array("int[] trimCurve:orders", &orders, "");
        self.out
            .array("int[] trimCurve:vertexCounts", &vertex_counts, "");
        self.out.array("double[] trimCurve:knots", &knots, "");
        self.out.array("double2[] trimCurve:ranges", &ranges, "");
        self.out.array("double3[] trimCurve:points", &points, "");
    }
}
