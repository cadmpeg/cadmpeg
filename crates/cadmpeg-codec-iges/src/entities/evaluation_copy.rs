// SPDX-License-Identifier: Apache-2.0
//! Scoped geometry copies used by model evaluation during append-only projection.

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::{
    Curve, CurveOffsetDistanceLaw, CurveOffsetRange, ExactSpline, OffsetExtension, OffsetSide,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
    Surface,
};
use cadmpeg_ir::CadIr;

const COPY: &str = "iges evaluation geometry copy";

fn text_allocation(text: &str, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    let bytes = u64_from_index(text.len());
    ctx.charge_work(bytes, COPY)?;
    ctx.charge_retained(bytes, COPY)
}

fn array_allocation<T>(count: usize, ctx: &DecodeContext<'_>) -> Result<(), CodecError> {
    if count == 0 {
        return Ok(());
    }
    let count = u64_from_index(count);
    let bytes = count
        .checked_mul(u64_from_index(std::mem::size_of::<T>()))
        .ok_or_else(|| ctx.refuse_codec_limit(COPY, u64::MAX, u64::MAX))?;
    ctx.charge_collection_items(count, COPY)?;
    ctx.charge_work(count, COPY)?;
    ctx.charge_retained(bytes, COPY)
}

pub(super) fn surface(surface: &Surface, ctx: &DecodeContext<'_>) -> Result<Surface, CodecError> {
    Ok(Surface {
        id: surface.id.try_clone_for_decode(ctx, COPY)?,
        geometry: surface.geometry.try_clone_for_decode(ctx, COPY)?,
        source_object: None,
    })
}

pub(super) fn curve(curve: &Curve, ctx: &DecodeContext<'_>) -> Result<Curve, CodecError> {
    Ok(Curve {
        id: curve.id.try_clone_for_decode(ctx, COPY)?,
        geometry: curve.geometry.try_clone_for_decode(ctx, COPY)?,
        source_object: None,
    })
}

pub(super) fn procedural_surface(
    surface: &ProceduralSurface,
    ctx: &DecodeContext<'_>,
) -> Result<ProceduralSurface, CodecError> {
    // IGES emits legacy constructions. Their variable storage consists of
    // the referenced identities; the exact legacy payload has fixed fields.
    match surface.definition() {
        ProceduralSurfaceDefinition::Exact(payload)
            if matches!(payload.spline(), ExactSpline::Legacy { .. }) => {}
        ProceduralSurfaceDefinition::Extrusion(payload) if payload.revision_form().is_none() => {
            text_allocation(payload.directrix().as_str(), ctx)?;
        }
        ProceduralSurfaceDefinition::Revolution(payload) if payload.revision_form().is_none() => {
            text_allocation(payload.directrix().as_str(), ctx)?;
        }
        ProceduralSurfaceDefinition::Offset(payload)
            if matches!(payload.extension(), OffsetExtension::Legacy { .. }) =>
        {
            text_allocation(payload.support().as_str(), ctx)?;
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            text_allocation(first.as_str(), ctx)?;
            text_allocation(second.as_str(), ctx)?;
        }
        ProceduralSurfaceDefinition::CurveBounded {
            support,
            boundaries,
            boundary_pcurves,
            ..
        } => {
            text_allocation(support.as_str(), ctx)?;
            array_allocation::<cadmpeg_ir::ids::CurveId>(boundaries.len(), ctx)?;
            array_allocation::<cadmpeg_ir::ids::PcurveId>(boundary_pcurves.len(), ctx)?;
            let mut input = boundaries.iter();
            while input.len() != 0 {
                let Some(id) = ctx.next_charged(&mut input, COPY)? else { break; };
                text_allocation(id.as_str(), ctx)?;
            }
            let mut input = boundary_pcurves.iter();
            while input.len() != 0 {
                let Some(id) = ctx.next_charged(&mut input, COPY)? else { break; };
                text_allocation(id.as_str(), ctx)?;
            }
        }
        ProceduralSurfaceDefinition::Unknown { record, .. } => {
            if let Some(record) = record {
                text_allocation(record.as_str(), ctx)?;
            }
        }
        _ => {
            return Err(CodecError::malformed(
                "unexpected IGES surface construction",
            ))
        }
    }
    Ok(ProceduralSurface::new(
        surface.id.try_clone_for_decode(ctx, COPY)?,
        surface.definition().clone(),
        surface.record_bounds(),
    ))
}

pub(super) fn procedural_curve(
    curve: &ProceduralCurve,
    ctx: &DecodeContext<'_>,
) -> Result<ProceduralCurve, CodecError> {
    match curve.definition() {
        ProceduralCurveDefinition::Compound(payload) => {
            array_allocation::<cadmpeg_ir::scalar::FiniteReal>(payload.parameters().len(), ctx)?;
            array_allocation::<
                cadmpeg_ir::geometry::CompoundComponent<
                    cadmpeg_ir::ids::CurveId,
                    cadmpeg_ir::scalar::FiniteReal,
                >,
            >(payload.components().len(), ctx)?;
            let mut input = payload.components().iter();
            while input.len() != 0 {
                let Some(component) = ctx.next_charged(&mut input, COPY)? else { break; };
                text_allocation(component.component.as_str(), ctx)?;
            }
        }
        ProceduralCurveDefinition::Offset(payload) => {
            text_allocation(payload.source().as_str(), ctx)?;
            if let OffsetSide::Direction {
                support: Some(support),
                ..
            } = payload.side()
            {
                text_allocation(support.as_str(), ctx)?;
            }
            if let Some(CurveOffsetRange::Variable {
                distance_law: CurveOffsetDistanceLaw::Coordinate { function, .. },
                ..
            }) = payload.range()
            {
                text_allocation(function.as_str(), ctx)?;
            }
        }
        _ => return Err(CodecError::malformed("unexpected IGES curve construction")),
    }
    Ok(ProceduralCurve::new(
        curve.id.try_clone_for_decode(ctx, COPY)?,
        curve.definition().clone(),
    ))
}

/// An immutable geometry population, created at the first procedural evaluation.
pub(super) struct SourceGeometrySnapshot<'ctx> {
    model: std::cell::OnceCell<CadIr>,
    storage: std::cell::RefCell<cadmpeg_core::decode::ScopedReservation<'ctx>>,
}

impl<'ctx> SourceGeometrySnapshot<'ctx> {
    pub(super) fn new(ctx: &'ctx DecodeContext<'_>) -> Result<Self, CodecError> {
        Ok(Self {
            model: std::cell::OnceCell::new(),
            storage: std::cell::RefCell::new(
                ctx.reserve_scoped(0, "iges source geometry snapshot")?,
            ),
        })
    }

    pub(super) fn model(
        &self,
        source: &CadIr,
        ctx: &DecodeContext<'_>,
    ) -> Result<&CadIr, CodecError> {
        if self.model.get().is_none() {
            let model =
                self.storage
                    .borrow_mut()
                    .with_storage(|| -> Result<CadIr, CodecError> {
                        let mut result = CadIr::empty();
                        result.tolerances = source.tolerances;
                        result.model.surfaces =
                            ctx.try_collect_retained_with(&source.model.surfaces, COPY, |value| {
                                surface(value, ctx)
                            })?;
                        result.model.curves =
                            ctx.try_collect_retained_with(&source.model.curves, COPY, |value| {
                                curve(value, ctx)
                            })?;
                        result.model.procedural_surfaces = ctx.try_collect_retained_with(
                            &source.model.procedural_surfaces,
                            COPY,
                            |value| procedural_surface(value, ctx),
                        )?;
                        result.model.procedural_curves = ctx.try_collect_retained_with(
                            &source.model.procedural_curves,
                            COPY,
                            |value| procedural_curve(value, ctx),
                        )?;
                        Ok(result)
                    })?;
            self.model.set(model).map_err(|_| {
                CodecError::malformed("source geometry snapshot is already populated")
            })?;
        }
        self.model
            .get()
            .ok_or_else(|| CodecError::malformed("source geometry snapshot is absent"))
    }
}

#[cfg(test)]
mod tests;
