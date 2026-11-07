// SPDX-License-Identifier: Apache-2.0
//! Exact Type 102 construction from a neutral compound curve.

use super::{CadIr, CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::geometry::{
    CompositeCurveSegment, CompositeCurveSegments, CompositeCurveTransition,
    ProceduralCurveDefinition,
};
use cadmpeg_ir::ids::CurveId;

pub(super) fn geometry(ir: &CadIr, id: &CurveId) -> Option<CurveGeometry> {
    let curve = ir.model.curves.iter().find(|curve| &curve.id == id)?;
    let construction = curve.geometry.procedural_construction()?;
    let procedural = ir
        .model
        .procedural_curves
        .iter()
        .find(|record| &record.id == construction)?;
    let ProceduralCurveDefinition::Compound(compound) = procedural.definition() else {
        return None;
    };
    let components = compound.components();
    let parameters = compound.parameters();
    if compound.legacy_cache().is_some()
        || parameters.len() != components.len() + 1
        || parameters.first()?.get() != 0.0
    {
        return None;
    }
    let mut segments = Vec::with_capacity(components.len());
    let mut previous_end = None;
    for (index, component) in components.iter().enumerate() {
        let child = ir
            .model
            .curves
            .iter()
            .find(|curve| curve.id == component.component)?;
        let range = child.parameter_range?.endpoints();
        let width = range[1] - range[0];
        if !width.is_finite()
            || width <= 0.0
            || parameters[index + 1].get() != parameters[index].get() + width
        {
            return None;
        }
        if range[0] != 0.0 || component.parameter.get() != 0.0 {
            return None;
        }
        let span = super::curve_reference_span(ir, &child.id, &child.geometry).ok()?;
        if previous_end.is_some_and(|end| {
            !super::close_point_with_tolerance(end, span.start, super::EPS_WRITE_POSITION)
        }) {
            return None;
        }
        previous_end = Some(span.end);
        segments.push(CompositeCurveSegment {
            curve: child.id.clone(),
            same_sense: true,
            transition: CompositeCurveTransition::Discontinuous,
        });
    }
    Some(CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: CompositeCurveSegments::try_from(segments).ok()?,
        self_intersect: None,
    }))
}

/// Name the owners whose composite definition loses an intermediate curve.
pub(super) fn flattened_owner_losses(
    ir: &CadIr,
    emitted: &std::collections::BTreeSet<&String>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    let mut losses = Vec::new();
    for curve in &ir.model.curves {
        if !emitted.contains(&curve.id.to_string()) {
            continue;
        }
        let Some(SolvedCurveGeometry::Composite { segments, .. }) = curve.geometry.solved() else {
            continue;
        };
        let flattened = segments.iter().any(|segment| {
            ir.model.curves.iter().any(|child| {
                child.id == segment.curve
                    && matches!(
                        child.geometry.solved(),
                        Some(SolvedCurveGeometry::Composite { .. })
                    )
            })
        });
        if !flattened {
            continue;
        }
        losses.push(super::IgesLossCode::WriterEntityOmitted.note(format!(
            "IGES curve {} loses a nested composite constituent in its representation",
            curve.id
        )));
        for edge in ir
            .model
            .edges
            .iter()
            .filter(|edge| edge.curve() == Some(&curve.id))
        {
            losses.push(super::IgesLossCode::WriterEntityOmitted.note(format!(
                "IGES edge {} uses curve {} with a flattened composite definition",
                edge.id, curve.id
            )));
            for coedge in ir
                .model
                .coedges
                .iter()
                .filter(|coedge| coedge.edge == edge.id)
            {
                losses.push(super::IgesLossCode::WriterEntityOmitted.note(format!(
                    "IGES coedge {} uses edge {} with a flattened composite definition",
                    coedge.id, edge.id
                )));
            }
        }
    }
    losses
}
