// SPDX-License-Identifier: Apache-2.0
//! Adapt parameter curves and provable isoparametric uses to IGES B-rep.

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
    let mut unknown_flags = BTreeSet::new();
    let mut record_uses = |uses: &[PcurveUse]| {
        for item in uses {
            used.insert(item.pcurve.clone());
            if item.isoparametric.is_none() {
                unknown_flags.insert(item.pcurve.clone());
            }
        }
    };
    for coedge in &ir.model.coedges {
        record_uses(&coedge.pcurves);
    }
    for boundary in ir.model.loops.iter().map(|item| &item.boundary) {
        match boundary {
            LoopBoundary::Vertex { pcurves, .. } => {
                record_uses(pcurves);
            }
            LoopBoundary::Ring(ring) => {
                for vertex in ring.vertex_uses() {
                    record_uses(&vertex.pcurves);
                }
            }
        }
    }
    let mut replacements = BTreeMap::new();
    let mut flags = BTreeMap::new();
    for curve in &ir.model.pcurves {
        if unknown_flags.contains(&curve.id) {
            if let PcurveGeometry::Nurbs { nurbs } = &curve.geometry {
                let points = nurbs.control_points();
                if let Some(first) = points.first() {
                    let first = first.get();
                    if points.iter().all(|point| point.get().u == first.u)
                        || points.iter().all(|point| point.get().v == first.v)
                    {
                        flags.insert(curve.id.clone(), true);
                    }
                }
            }
        }
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
        flags.insert(curve.id.clone(), direction.u == 0.0 || direction.v == 0.0);
        replacements.insert(curve.id.clone(), nurbs);
    }
    if replacements.is_empty() && flags.is_empty() {
        return (Cow::Borrowed(ir), Vec::new());
    }
    let mut document = ir.clone();
    let mut losses = Vec::new();
    for curve in &mut document.model.pcurves {
        if let Some(nurbs) = replacements.get(&curve.id) {
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
                if let Some(isoparametric) = flags.get(&item.pcurve) {
                    item.isoparametric = Some(*isoparametric);
                    losses.push(ExportLossCode::RecordResidualUnavailable.note(format!("IGES {owner} pcurve {} requires an isoparametric flag; its geometry determines {isoparametric}", item.pcurve)));
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
