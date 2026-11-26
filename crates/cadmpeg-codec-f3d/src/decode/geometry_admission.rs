// SPDX-License-Identifier: Apache-2.0
//! Admit vertex tolerances and cache-first spring support relations.

mod vertex_tolerance;

use crate::loss::F3dLossCode;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::curve_payloads::SpringCurvePayload;
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::surface_curves::SpringLayout;
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::CadIr;
use std::collections::HashMap;

/// What the admission of one record leaves unstated.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Note {
    /// A support side of the procedural curve `curve` keeps its native
    /// support fields without a neutral surface chart.
    SupportChart { curve: String, side: usize },
    /// The spring curve `curve` keeps native support pcurves whose endpoint
    /// relation is unresolved.
    SpringPcurve { curve: String },
    /// The tolerance of the vertex `vertex` is widened by `widening`
    /// millimetres.
    VertexTolerance { vertex: String, widening: f64 },
}

impl Note {
    /// The record the note names.
    pub(crate) fn record(&self) -> &str {
        match self {
            Self::SupportChart { curve, .. } | Self::SpringPcurve { curve } => curve,
            Self::VertexTolerance { vertex, .. } => vertex,
        }
    }

    /// The loss of the note for the record whose key is `key`.
    pub(crate) fn loss(&self, key: &str) -> LossNote {
        match self {
            Self::SupportChart { side, .. } => F3dLossCode::ProceduralSupportChartUnresolved.note(
                format!("Curve {key} support side {side} retains its native support fields without a neutral surface chart"),
            ),
            Self::SpringPcurve { .. } => F3dLossCode::SpringSupportPcurveUnresolved.note(format!(
                "Curve {key} retains native support pcurves whose endpoint relation is unresolved"
            )),
            Self::VertexTolerance { widening, .. } => F3dLossCode::VertexToleranceWidened.note(
                format!("Widened the tolerance of vertex {key} by {widening:.17} mm"),
            ),
        }
    }
}

/// Admits the geometry of `ir`, and pushes to `losses` what it leaves
/// unstated: one loss per curve note, and one for every widened vertex
/// tolerance.
pub(super) fn admit(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    losses: &mut Vec<LossNote>,
    annotations: &mut [cadmpeg_asm::brep::annotations::AnnotationRecord],
) -> Result<(), CodecError> {
    let mut notes = Vec::new();
    admit_noted(ctx, ir, &mut notes, annotations)?;
    let widened = notes
        .iter()
        .filter_map(|note| match note {
            Note::VertexTolerance { widening, .. } => Some(*widening),
            _ => None,
        })
        .collect::<Vec<_>>();
    if !widened.is_empty() {
        let maximum_widening = widened.iter().copied().fold(0.0, f64::max);
        losses.push(F3dLossCode::VertexToleranceWidened.note(format!(
            "Widened {} vertex tolerance(s); maximum widening {maximum_widening:.17} mm",
            widened.len()
        )));
    }
    losses.extend(
        notes
            .iter()
            .filter(|note| !matches!(note, Note::VertexTolerance { .. }))
            .map(|note| note.loss(note.record())),
    );
    Ok(())
}

/// Admits the geometry of `ir`, and pushes to `notes` what the admission of
/// each record leaves unstated.
pub(crate) fn admit_noted(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    notes: &mut Vec<Note>,
    annotations: &mut [cadmpeg_asm::brep::annotations::AnnotationRecord],
) -> Result<(), CodecError> {
    vertex_tolerance::admit(ctx, ir, notes, annotations)?;
    for curve in &ir.geometry.procedural_curves {
        let context = match curve.definition() {
            ProceduralCurveDefinition::Intersection { context, .. } => context,
            ProceduralCurveDefinition::SurfaceCurve { family } => family.context(),
            ProceduralCurveDefinition::SurfaceOffset(offset) => offset.context(),
            ProceduralCurveDefinition::Deformable(deformable) => deformable.context(),
            ProceduralCurveDefinition::Spring(spring) => spring.support_context(),
            _ => continue,
        };
        for (side, support) in context.sides().iter().enumerate() {
            if support.surface.is_none() && support.pcurve.is_some() {
                notes.push(Note::SupportChart {
                    curve: curve.id.as_str().to_owned(),
                    side,
                });
            }
        }
    }

    if !ir.geometry.procedural_curves.iter().any(|curve| {
        matches!(curve.definition(), ProceduralCurveDefinition::Spring(spring)
            if matches!(spring.layout(), SpringLayout::CacheFirst { .. }))
    }) {
        return Ok(());
    }
    let index = ModelIndex::new_model_only(ir);
    let mut owners = HashMap::new();
    for curve in &ir.geometry.curves {
        if let Some(construction) = curve.geometry.procedural_construction() {
            owners
                .entry(construction)
                .and_modify(|owner| *owner = None)
                .or_insert(Some(curve));
        }
    }
    let mut replacements = Vec::new();
    for (slot, curve) in ir.geometry.procedural_curves.iter().enumerate() {
        let ProceduralCurveDefinition::Spring(spring) = curve.definition() else {
            continue;
        };
        if !matches!(spring.layout(), SpringLayout::CacheFirst { .. }) {
            continue;
        }
        let Some(owner) = owners.get(&curve.id).copied().flatten() else {
            continue;
        };
        let sides = cadmpeg_ir::validate::geometry_consistency::cache_support_endpoint_failures(
            ctx,
            &index,
            owner,
            spring.support_context(),
            curve
                .cache_fit_tolerance()
                .map(cadmpeg_ir::geometry::legacy_cache::FitTolerance::get),
        )?;
        if sides.is_empty() {
            continue;
        }
        let mut layout = spring.layout().to_raw();
        let SpringLayout::CacheFirst { context, .. } = &mut layout else {
            continue;
        };
        for side in sides {
            context.set_unmapped_pcurve(side, None);
        }
        let replacement = SpringCurvePayload::try_new(layout, *spring.direction())
            .map_err(CodecError::malformed)?;
        replacements.push((slot, replacement));
    }
    for (slot, replacement) in replacements {
        let curve = &mut ir.geometry.procedural_curves[slot];
        curve.replace_definition(ProceduralCurveDefinition::Spring(replacement));
        notes.push(Note::SpringPcurve {
            curve: curve.id.as_str().to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests;
