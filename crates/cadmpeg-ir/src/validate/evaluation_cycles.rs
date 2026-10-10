// SPDX-License-Identifier: Apache-2.0
//! Cross-record curve and surface dependencies used by model evaluation.

use std::fmt;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::geometry::{
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, ProceduralSurfaceDefinition,
};
use crate::index::{identities::BorrowedIdentities, ModelIndex};
use crate::report::{
    check::{Check, Finding},
    Severity,
};

fn curve_dependencies<'a>(
    ctx: &DecodeContext<'_>,
    definition: &'a ProceduralCurveDefinition,
) -> Result<Vec<&'a str>, CodecError> {
    Ok(match definition {
        ProceduralCurveDefinition::Replica { source, .. } => {
            ctx.collect_vec([source.as_str()], "cycle dependencies")?
        }
        ProceduralCurveDefinition::Subset(payload) => {
            ctx.collect_vec([payload.source().as_str()], "cycle dependencies")?
        }
        ProceduralCurveDefinition::TolerantIntersection {
            construction,
            parameterization: Some(_),
            ..
        } => ctx.collect_vec(
            construction
                .supports()
                .iter()
                .map(crate::ids::SurfaceId::as_str),
            "cycle dependencies",
        )?,
        _ => Vec::new(),
    })
}

fn surface_dependencies<'a>(
    ctx: &DecodeContext<'_>,
    definition: &'a ProceduralSurfaceDefinition,
) -> Result<Vec<&'a str>, CodecError> {
    Ok(match definition {
        ProceduralSurfaceDefinition::AxisRevolution(payload) => {
            ctx.collect_vec([payload.directrix().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Extrusion(payload) => {
            ctx.collect_vec([payload.directrix().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::LinearSweep(payload) => {
            ctx.collect_vec([payload.directrix().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Revolution(payload) => {
            ctx.collect_vec([payload.directrix().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            ctx.collect_vec([first.as_str(), second.as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Sum(payload) => ctx.collect_vec(
            [payload.first().as_str(), payload.second().as_str()],
            "cycle dependencies",
        )?,
        ProceduralSurfaceDefinition::Sweep(payload) if payload.native().is_some() => ctx
            .collect_vec(
                [payload.profile().as_str(), payload.spine().as_str()],
                "cycle dependencies",
            )?,
        ProceduralSurfaceDefinition::Blend(payload) => {
            if let Some(native) = payload.native() {
                ctx.charge_work(u64_from_index(native.sides.len()), "cycle dependency scan")?;
                ctx.collect_vec(
                    native
                        .sides
                        .iter()
                        .filter_map(|side| {
                            side.surface
                                .as_ref()
                                .map(|support| support.surface.as_str())
                        })
                        .chain(std::iter::once(native.slice.as_str())),
                    "cycle dependencies",
                )?
            } else {
                Vec::new()
            }
        }
        ProceduralSurfaceDefinition::VariableBlend(payload) => {
            ctx.charge_work(
                u64_from_index(payload.construction().sides.len()),
                "cycle dependency scan",
            )?;
            ctx.collect_vec(
                payload.construction().sides.iter().filter_map(|side| {
                    side.surface
                        .as_ref()
                        .map(|support| support.surface.as_str())
                }),
                "cycle dependencies",
            )?
        }
        ProceduralSurfaceDefinition::CurveBounded { support, .. } => {
            ctx.collect_vec([support.as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Replica { source, .. } => {
            ctx.collect_vec([source.as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Subset(payload) => {
            ctx.collect_vec([payload.support().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::ParallelOffset(payload) => {
            ctx.collect_vec([payload.support().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Offset(payload) => {
            ctx.collect_vec([payload.support().as_str()], "cycle dependencies")?
        }
        _ => Vec::new(),
    })
}

struct Frame<'a, 'session> {
    node: &'a str,
    next_child: usize,
    _depth: DepthGuard<'session>,
}

struct CyclePath<'stack, 'a, 'session> {
    stack: &'stack [Frame<'a, 'session>],
    child: &'a str,
}

impl fmt::Display for CyclePath<'_, '_, '_> {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        for frame in self.stack {
            write!(out, "{} -> ", frame.node)?;
        }
        out.write_str(self.child)
    }
}

#[derive(Clone, Copy)]
enum Visit {
    Unseen,
    Active(usize),
    Complete,
}

struct Node<'ir> {
    dependencies: Vec<&'ir str>,
    visit: Visit,
}

/// Only carrier-to-construction associations are needed by the cycle graph.
trait CarrierConstructions<'ir> {
    fn curve(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&'ir ProceduralCurve>, CodecError>;
    fn surface(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&'ir ProceduralSurface>, CodecError>;
}

impl<'ir> CarrierConstructions<'ir> for ModelIndex<'ir> {
    fn curve(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&'ir ProceduralCurve>, CodecError> {
        Ok(self
            .procedural_curves_for_curve(id, ctx)?
            .and_then(|rows| rows.first().copied()))
    }
    fn surface(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&'ir ProceduralSurface>, CodecError> {
        Ok(self.procedural_surface_for_surface(id, ctx)?)
    }
}

struct CycleIndex<'ctx, 'ir> {
    curves: BorrowedIdentities<'ctx, 'ir, &'ir ProceduralCurve>,
    surfaces: BorrowedIdentities<'ctx, 'ir, &'ir ProceduralSurface>,
}

impl<'ctx, 'ir> CycleIndex<'ctx, 'ir> {
    fn build(ctx: &'ctx DecodeContext<'_>, ir: &'ir CadIr) -> Result<Self, CodecError> {
        // The general index selects the first construction with an exact ID.
        // BorrowedIdentities selects the last insertion, so reverse these rows.
        let curves_by_id = BorrowedIdentities::build(ctx, |add| {
            for row in ir.model.procedural_curves.iter().rev() {
                add(row.id.as_str(), row)?;
            }
            Ok(())
        })?;
        let curves = BorrowedIdentities::<&ProceduralCurve>::build(ctx, |add| {
            for carrier in ir.model.curves.iter().rev() {
                ctx.charge_work(1, "cycle curve association scan")?;
                if let Some(construction) = carrier.geometry.procedural_construction() {
                    if let Some(row) = curves_by_id.get(ctx, construction.as_str())? {
                        add(carrier.id.as_str(), row)?;
                    }
                }
            }
            Ok(())
        })?;
        drop(curves_by_id);
        let surfaces_by_id = BorrowedIdentities::build(ctx, |add| {
            for row in ir.model.procedural_surfaces.iter().rev() {
                add(row.id.as_str(), row)?;
            }
            Ok(())
        })?;
        let surfaces = BorrowedIdentities::<&ProceduralSurface>::build(ctx, |add| {
            for carrier in &ir.model.surfaces {
                ctx.charge_work(1, "cycle surface association scan")?;
                if let Some(construction) = carrier.geometry.procedural_construction() {
                    if let Some(row) = surfaces_by_id.get(ctx, construction.as_str())? {
                        add(carrier.id.as_str(), row)?;
                    }
                }
            }
            Ok(())
        })?;
        Ok(Self { curves, surfaces })
    }
}

impl<'ir> CarrierConstructions<'ir> for CycleIndex<'_, 'ir> {
    fn curve(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&'ir ProceduralCurve>, CodecError> {
        Ok(self.curves.get(ctx, id)?.copied())
    }
    fn surface(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<Option<&'ir ProceduralSurface>, CodecError> {
        Ok(self.surfaces.get(ctx, id)?.copied())
    }
}

/// Walk the evaluator graph, stopping when the finding consumer asks to stop.
fn walk_cycles<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
    index: &impl CarrierConstructions<'ir>,
    mut emit: impl FnMut(Finding) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let mut graph_storage = ctx.reserve_scoped(0, "cycle dependency graph")?;
    let mut graph = graph_storage.with_storage(|| {
        BorrowedIdentities::build(ctx, |add| {
            for curve in &ir.model.curves {
                ctx.charge_work(1, "cycle carrier scan")?;
                if let Some(procedural) = index.curve(ctx, curve.id.as_str())? {
                    let dependencies = curve_dependencies(ctx, procedural.definition())?;
                    if !dependencies.is_empty() {
                        add(
                            curve.id.as_str(),
                            Node {
                                dependencies,
                                visit: Visit::Unseen,
                            },
                        )?;
                    }
                }
            }
            for surface in &ir.model.surfaces {
                ctx.charge_work(1, "cycle carrier scan")?;
                if let Some(procedural) = index.surface(ctx, surface.id.as_str())? {
                    let dependencies = surface_dependencies(ctx, procedural.definition())?;
                    if !dependencies.is_empty() {
                        add(
                            surface.id.as_str(),
                            Node {
                                dependencies,
                                visit: Visit::Unseen,
                            },
                        )?;
                    }
                }
            }
            Ok(())
        })
    })?;
    let mut starts = graph_storage
        .with_storage(|| ctx.collect_vec(graph.identities(), "cycle start identities"))?;
    ctx.stable_sort_by(&mut starts, Ord::cmp, |id| id.len(), "cycle start order")?;
    let mut stack = Vec::new();
    for start in starts {
        ctx.charge_work(1, "cycle start scan")?;
        let Some(node) = graph.get_mut(ctx, start)? else {
            continue;
        };
        if matches!(node.visit, Visit::Complete) {
            continue;
        }
        node.visit = Visit::Active(0);
        let depth = ctx.enter_nested("cycle traversal depth")?;
        graph_storage.with_storage(|| {
            ctx.push_vec(
                &mut stack,
                Frame {
                    node: start,
                    next_child: 0,
                    _depth: depth,
                },
                "cycle traversal stack",
            )
        })?;
        while let Some(frame) = stack.last_mut() {
            ctx.charge_work(1, "cycle traversal")?;
            let child = graph
                .get(ctx, frame.node)?
                .and_then(|node| node.dependencies.get(frame.next_child))
                .copied();
            let Some(child) = child else {
                if let Some(finished) = stack.pop() {
                    if let Some(node) = graph.get_mut(ctx, finished.node)? {
                        node.visit = Visit::Complete;
                    }
                }
                continue;
            };
            frame.next_child += 1;
            let Some(node) = graph.get_mut(ctx, child)? else {
                continue;
            };
            match node.visit {
                Visit::Complete => {}
                Visit::Active(start_index) => {
                    ctx.charge_work(u64_from_index(stack.len() - start_index), "cycle path walk")?;
                    let finding = Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: ctx.format_retained(
                            format_args!(
                                "malformed curve/surface reference cycle: {}",
                                CyclePath {
                                    stack: &stack[start_index..],
                                    child
                                }
                            ),
                            "cycle finding message",
                        )?,
                        entity: Some(ctx.copy_retained_text(child, "cycle finding identity")?),
                    };
                    if !emit(finding)? {
                        return Ok(());
                    }
                }
                Visit::Unseen => {
                    node.visit = Visit::Active(stack.len());
                    let depth = ctx.enter_nested("cycle traversal depth")?;
                    graph_storage.with_storage(|| {
                        ctx.push_vec(
                            &mut stack,
                            Frame {
                                node: child,
                                next_child: 0,
                                _depth: depth,
                            },
                            "cycle traversal stack",
                        )
                    })?;
                }
            }
        }
    }
    Ok(())
}

/// Report cycles in the dependencies an evaluator can follow from a carrier.
pub(super) fn check_evaluation_cycles(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    index: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    walk_cycles(ctx, ir, index, |finding| {
        ctx.push_vec(findings, finding, "cycle findings")?;
        Ok(true)
    })
}

/// Refuse a decoded model with the first recursive curve or surface dependency.
pub(crate) fn admit_evaluation_cycles(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
) -> Result<(), CodecError> {
    if ir.model.procedural_curves.is_empty() && ir.model.procedural_surfaces.is_empty() {
        return ctx.charge_work(0, "cycle admission");
    }
    let index = CycleIndex::build(ctx, ir)?;
    walk_cycles(ctx, ir, &index, |finding| {
        Err(CodecError::Malformed(finding.message))
    })
}

#[cfg(test)]
mod tests;
