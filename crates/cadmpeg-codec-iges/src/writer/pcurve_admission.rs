// SPDX-License-Identifier: Apache-2.0
//! Adapt finite straight parameter curves to the IGES B-rep representation.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::codec::write::ExportLossCode;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveNurbs, PcurveNurbsPoles};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::report::loss::{LossNote, LossTaxonomy};
use cadmpeg_ir::topology::{LoopBoundary, LoopRing, PcurveUse};
use cadmpeg_ir::CadIr;

pub(super) fn admit(ir: &CadIr) -> (Cow<'_, CadIr>, Vec<LossNote>) {
    let mut used = BTreeSet::new();
    for coedge in &ir.model.coedges {
        used.extend(coedge.pcurves.iter().map(|item| item.pcurve.clone()));
    }
    for boundary in ir.model.loops.iter().map(|item| &item.boundary) {
        match boundary {
            LoopBoundary::Vertex { pcurves, .. } => {
                used.extend(pcurves.iter().map(|item| item.pcurve.clone()));
            }
            LoopBoundary::Ring(ring) => {
                for vertex in ring.vertex_uses() {
                    used.extend(vertex.pcurves.iter().map(|item| item.pcurve.clone()));
                }
            }
        }
    }
    let mut replacements = BTreeMap::new();
    for curve in &ir.model.pcurves {
        let PcurveGeometry::Line(line) = &curve.geometry else {
            continue;
        };
        if !used.contains(&curve.id) {
            continue;
        }
        let Some(range) = curve.parameter_range() else {
            continue;
        };
        let [start, end] = range.get();
        if start >= end {
            continue;
        }
        let origin = line.origin().get();
        let direction = line.direction().as_raw();
        let point = |t: f64| Point2::new(origin.u + direction.u * t, origin.v + direction.v * t);
        let Ok(nurbs) = PcurveNurbs::new(
            1,
            vec![start, start, end, end],
            PcurveNurbsPoles::Polynomial {
                points: vec![point(start), point(end)],
            },
            false,
        ) else {
            continue;
        };
        replacements.insert(
            curve.id.clone(),
            (nurbs, direction.u == 0.0 || direction.v == 0.0),
        );
    }
    if replacements.is_empty() {
        return (Cow::Borrowed(ir), Vec::new());
    }
    let mut document = ir.clone();
    let mut losses = Vec::new();
    for curve in &mut document.model.pcurves {
        if let Some((nurbs, _)) = replacements.get(&curve.id) {
            curve.geometry = PcurveGeometry::Nurbs {
                nurbs: nurbs.clone(),
            };
            losses.push(LossNote::new(
                LossTaxonomy::GeometryNotTransferred,
                format!(
                    "IGES pcurve {} converts its line representation to a degree-1 NURBS",
                    curve.id
                ),
            ));
        }
    }
    let mut adapt = |owner: &str, uses: &mut [PcurveUse]| {
        for item in uses {
            if item.isoparametric.is_none() {
                if let Some((_, isoparametric)) = replacements.get(&item.pcurve) {
                    item.isoparametric = Some(*isoparametric);
                    losses.push(ExportLossCode::RecordResidualUnavailable.note(format!("IGES {owner} pcurve {} requires an isoparametric flag; its line determines {isoparametric}", item.pcurve)));
                }
            }
        }
    };
    for coedge in &mut document.model.coedges {
        adapt(coedge.id.as_str(), &mut coedge.pcurves);
    }
    for loop_ in &mut document.model.loops {
        match &mut loop_.boundary {
            LoopBoundary::Vertex { pcurves, .. } => adapt(loop_.id.as_str(), pcurves),
            LoopBoundary::Ring(ring) => {
                let mut vertices = ring.vertex_uses().to_vec();
                for vertex in &mut vertices {
                    adapt(loop_.id.as_str(), &mut vertex.pcurves);
                }
                *ring = LoopRing::new(ring.coedges().to_vec(), vertices)
                    .expect("existing ring anchors remain valid");
            }
        }
    }
    (Cow::Owned(document), losses)
}
