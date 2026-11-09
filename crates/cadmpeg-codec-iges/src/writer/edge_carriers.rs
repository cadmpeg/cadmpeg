// SPDX-License-Identifier: Apache-2.0
//! Edge and parameter-curve carriers stated so the re-decode reads them back.

use super::{
    ownership, same_range, CurveGeometry, CurveSpan, EntityStatus, FiniteVector, IgesLossCode,
    LossNote, SolvedCurveGeometry,
};
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::CadIr;

/// The span of a Type 110 segment for an edge on `geometry`. A line carrier
/// without its own range keeps its origin: the segment starts there when the
/// edge lies at nonnegative parameters, since a Type 110 reads back with its
/// first point as origin (IGES 5.3 §4.13).
pub(super) fn origin_anchored_span(
    curve: &cadmpeg_ir::geometry::Curve,
    geometry: &CurveGeometry,
    span: CurveSpan,
) -> CurveSpan {
    let CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) = geometry else {
        return span;
    };
    let [start, end] = span.range.get();
    if curve.parameter_range.is_some() || start < 0.0 || end <= 0.0 {
        return span;
    }
    let Some(range) = FiniteVector::new([0.0, end]) else {
        return span;
    };
    CurveSpan {
        range,
        start: line.origin().get(),
        end: span.end,
    }
}

/// The loss for a placed curve the writer states with its placement applied:
/// IGES Type 110, 100 and 126 entities read back in model space.
pub(super) fn placement_applied_loss(curve: &cadmpeg_ir::geometry::Curve) -> Option<LossNote> {
    matches!(
        curve.geometry.solved(),
        Some(SolvedCurveGeometry::Transformed(_))
    )
    .then(|| placement_loss("curve", curve.id.as_str()))
}

/// The loss for a placed surface: IGES surfaces read back in model space,
/// whether the writer applies the placement or states it as a Type 124.
pub(super) fn surface_placement_applied_loss(
    surface: &cadmpeg_ir::geometry::Surface,
) -> Option<LossNote> {
    matches!(
        surface.geometry.solved(),
        Some(cadmpeg_ir::geometry::SolvedSurfaceGeometry::Transformed(_))
    )
    .then(|| placement_loss("surface", surface.id.as_str()))
}

fn placement_loss(kind: &str, id: &str) -> LossNote {
    IgesLossCode::WriterPlacementApplied.note(format!(
        "IGES {kind} {id} is written with its placement applied; the placed carrier is not preserved"
    ))
}

/// The model curve record that holds `curve` over `range` as a parameter-space
/// carrier: the decoder reads each parameter-space entity also as a model
/// curve, so the entity written for a parameter curve is that record's entity.
/// Among equal carriers, the one the source states as a parameter curve is it.
pub(super) fn parameter_image_curve<'ir>(
    ir: &'ir CadIr,
    curve: &NurbsCurve,
    range: FiniteVector<2>,
) -> Option<&'ir cadmpeg_ir::geometry::Curve> {
    let images = ir
        .model
        .curves
        .iter()
        .filter(|candidate| {
            matches!(
                candidate.geometry.solved(),
                Some(SolvedCurveGeometry::Nurbs(nurbs)) if nurbs == curve
            ) && candidate
                .parameter_range
                .is_some_and(|candidate_range| same_range(candidate_range.endpoints(), range.get()))
        })
        .collect::<Vec<_>>();
    let ownership = ownership::SourceOwnership::build(ir);
    let parameter = images
        .iter()
        .copied()
        .filter(|image| {
            matches!(
                ownership.status(image.source_object.as_ref()),
                EntityStatus::ParameterCurve
            )
        })
        .collect::<Vec<_>>();
    match (images.as_slice(), parameter.as_slice()) {
        (_, [image]) | ([image], []) => Some(*image),
        _ => None,
    }
}
