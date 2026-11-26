// SPDX-License-Identifier: Apache-2.0
//! Parameter curve uses that cover part of their curve. An IGES edge use or
//! pole use states no interval: it uses the whole range of the curve it
//! references. The writer gives each such use a copy of its curve over the
//! used interval only, and removes a curve that no reference then names.

use super::same_range;
use cadmpeg_ir::document::Topology;
use cadmpeg_ir::geometry::pcurve::{Pcurve, PcurveGeneralForm, PcurveMetadata};
use cadmpeg_ir::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use cadmpeg_ir::ids::PcurveId;
use cadmpeg_ir::topology::{Loop, LoopBoundary, LoopRing, PcurveUse};
use cadmpeg_ir::units::FiniteVector;
use cadmpeg_ir::CadIr;
use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

/// Applies `visit` to each parameter curve use of the pole uses of `loop_`.
pub(super) fn visit_pole_uses(loop_: &mut Loop, mut visit: impl FnMut(&mut PcurveUse)) {
    visit_pole_use_lists(loop_, |uses| uses.iter_mut().for_each(&mut visit));
}

/// Applies `visit` to the parameter curve uses of each pole use of `loop_`.
pub(super) fn visit_pole_use_lists(loop_: &mut Loop, mut visit: impl FnMut(&mut Vec<PcurveUse>)) {
    match &mut loop_.boundary {
        LoopBoundary::Vertex { pcurves, .. } => visit(pcurves),
        LoopBoundary::Ring(ring) => {
            let mut vertex_uses = ring.vertex_uses().to_vec();
            for vertex_use in &mut vertex_uses {
                visit(&mut vertex_use.pcurves);
            }
            if let Ok(replaced) = LoopRing::new(ring.coedges().to_vec(), vertex_uses) {
                *ring = replaced;
            }
        }
    }
}

/// `pcurve` over `range`, or none where its metadata states no range.
pub(super) fn over(pcurve: &Pcurve, id: PcurveId, range: FiniteVector<2>) -> Option<Pcurve> {
    let PcurveMetadata::General { form } = &pcurve.metadata else {
        return None;
    };
    let mut covered =
        PcurveGeneralForm::new(form.wrapper_reversed, Some(range), form.fit_tolerance());
    covered.support_surface.clone_from(&form.support_surface);
    covered.uv_offset = form.uv_offset;
    Some(Pcurve {
        id,
        geometry: pcurve.geometry.clone(),
        metadata: PcurveMetadata::General { form: covered },
        native_ref: pcurve.native_ref.clone(),
    })
}

/// The document with each parameter curve use whose interval differs from
/// the range of its curve moved to a copy of the curve over that interval.
/// Uses of one curve over one interval share one copy.
pub(super) fn covered_pcurve_uses(ir: &CadIr) -> Cow<'_, CadIr> {
    let pcurves = ir
        .geometry
        .pcurves
        .iter()
        .map(|pcurve| (pcurve.id.as_str(), pcurve))
        .collect::<BTreeMap<_, _>>();
    let restricts = |pcurve_use: &PcurveUse| {
        let Some(used) = pcurve_use.parameter_range else {
            return false;
        };
        let [start, end] = used.endpoints();
        start != end
            && pcurves
                .get(pcurve_use.pcurve.as_str())
                .is_some_and(|pcurve| {
                    pcurve.parameter_range().is_none_or(|range| {
                        let range = range.get();
                        !same_range(range, [start, end]) && !same_range(range, [end, start])
                    })
                })
    };
    let geometry = &ir.geometry;
    let restricted = geometry
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter())
        .chain(geometry.loops.iter().flat_map(Loop::vertex_pcurves))
        .any(restricts);
    if !restricted {
        return Cow::Borrowed(ir);
    }
    let mut covered = ir.clone();
    let mut copies = BTreeMap::<(String, [u64; 2]), PcurveId>::new();
    let mut added = Vec::new();
    let mut cover = |pcurve_use: &mut PcurveUse| {
        if !restricts(pcurve_use) {
            return;
        }
        let Some(used) = pcurve_use.parameter_range else {
            return;
        };
        let [start, end] = used.endpoints();
        let (low, high) = (start.min(end), start.max(end));
        let source = pcurve_use.pcurve.as_str();
        let key = (source.to_owned(), [low.to_bits(), high.to_bits()]);
        if let Some(copy) = copies.get(&key) {
            pcurve_use.pcurve = copy.clone();
            return;
        }
        let ordinal = copies.keys().filter(|(other, _)| other == source).count();
        let Ok(id) = PcurveId::try_from(format!(
            "iges:writer:pcurve#{}@use{ordinal}",
            source.replace('#', "/")
        )) else {
            return;
        };
        let Some(range) = FiniteVector::new([low, high]) else {
            return;
        };
        if pcurves.contains_key(id.as_str()) {
            return;
        }
        let Some(copy) = pcurves
            .get(source)
            .and_then(|pcurve| over(pcurve, id.clone(), range))
        else {
            return;
        };
        added.push(copy);
        copies.insert(key, id.clone());
        pcurve_use.pcurve = id;
    };
    for coedge in &mut covered.geometry.coedges {
        coedge.pcurves.iter_mut().for_each(&mut cover);
    }
    for loop_ in &mut covered.geometry.loops {
        visit_pole_uses(loop_, &mut cover);
    }
    let sources = copies
        .into_keys()
        .map(|(source, _)| source)
        .collect::<BTreeSet<_>>();
    let geometry = &mut covered.geometry;
    geometry.pcurves.extend(added);
    let referenced = referenced_pcurves(geometry);
    geometry.pcurves.retain(|pcurve| {
        !sources.contains(pcurve.id.as_str()) || referenced.contains(pcurve.id.as_str())
    });
    Cow::Owned(covered)
}

/// The parameter curves that a use or a curve-bounded surface references.
pub(super) fn referenced_pcurves(geometry: &Topology) -> BTreeSet<String> {
    let uses = geometry
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter())
        .chain(geometry.loops.iter().flat_map(Loop::vertex_pcurves))
        .map(|pcurve_use| pcurve_use.pcurve.as_str().to_owned());
    let bounds = geometry
        .procedural_surfaces
        .iter()
        .flat_map(|surface| match surface.definition() {
            ProceduralSurfaceDefinition::CurveBounded {
                boundary_pcurves, ..
            } => boundary_pcurves.as_slice(),
            _ => &[],
        })
        .map(|pcurve| pcurve.as_str().to_owned());
    uses.chain(bounds).collect()
}
