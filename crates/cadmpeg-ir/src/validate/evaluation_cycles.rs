// SPDX-License-Identifier: Apache-2.0
//! Cross-record curve and surface dependencies used by model evaluation.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use cadmpeg_core::decode::{u64_from_index, DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::geometry::{ProceduralCurveDefinition, ProceduralSurfaceDefinition};
use crate::index::ModelIndex;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

fn curve_dependencies<'a>(ctx: &DecodeContext<'_>, definition: &'a ProceduralCurveDefinition) -> Result<Vec<&'a str>, CodecError> {
    Ok(match definition {
        ProceduralCurveDefinition::Replica { source, .. } => ctx.collect_vec([source.as_str()], "cycle dependencies")?,
        ProceduralCurveDefinition::Subset(payload) => ctx.collect_vec([payload.source().as_str()], "cycle dependencies")?,
        ProceduralCurveDefinition::TolerantIntersection {
            construction,
            parameterization: Some(_),
            ..
        } => ctx.collect_vec(construction
            .supports()
            .iter()
            .map(crate::ids::SurfaceId::as_str), "cycle dependencies")?,
        _ => Vec::new(),
    })
}

fn surface_dependencies<'a>(ctx: &DecodeContext<'_>, definition: &'a ProceduralSurfaceDefinition) -> Result<Vec<&'a str>, CodecError> {
    Ok(match definition {
        ProceduralSurfaceDefinition::AxisRevolution(payload) => {
            ctx.collect_vec([payload.directrix().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Extrusion(payload) => ctx.collect_vec([payload.directrix().as_str()], "cycle dependencies")?,
        ProceduralSurfaceDefinition::LinearSweep(payload) => ctx.collect_vec([payload.directrix().as_str()], "cycle dependencies")?,
        ProceduralSurfaceDefinition::Revolution(payload) => ctx.collect_vec([payload.directrix().as_str()], "cycle dependencies")?,
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            ctx.collect_vec([first.as_str(), second.as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Sum(payload) => {
            ctx.collect_vec([payload.first().as_str(), payload.second().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Sweep(payload) if payload.native().is_some() => {
            ctx.collect_vec([payload.profile().as_str(), payload.spine().as_str()], "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::Blend(payload) => {
            if let Some(native) = payload.native() {
                ctx.charge_work(u64_from_index(native.sides.len()), "cycle dependency scan")?;
                ctx.collect_vec(native.sides.iter().filter_map(|side| side.surface.as_ref().map(|support| support.surface.as_str())).chain(std::iter::once(native.slice.as_str())), "cycle dependencies")?
            } else {
                Vec::new()
            }
        }
        ProceduralSurfaceDefinition::VariableBlend(payload) => {
            ctx.charge_work(u64_from_index(payload.construction().sides.len()), "cycle dependency scan")?;
            ctx.collect_vec(payload.construction().sides.iter().filter_map(|side| side.surface.as_ref().map(|support| support.surface.as_str())), "cycle dependencies")?
        }
        ProceduralSurfaceDefinition::CurveBounded { support, .. } => ctx.collect_vec([support.as_str()], "cycle dependencies")?,
        ProceduralSurfaceDefinition::Replica { source, .. } => ctx.collect_vec([source.as_str()], "cycle dependencies")?,
        ProceduralSurfaceDefinition::Subset(payload) => ctx.collect_vec([payload.support().as_str()], "cycle dependencies")?,
        ProceduralSurfaceDefinition::ParallelOffset(payload) => ctx.collect_vec([payload.support().as_str()], "cycle dependencies")?,
        ProceduralSurfaceDefinition::Offset(payload) => ctx.collect_vec([payload.support().as_str()], "cycle dependencies")?,
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

fn lookup_work(ctx: &DecodeContext<'_>, count: usize, key_bytes: usize, max_key: usize) -> Result<(), CodecError> {
    let levels = u64::from(usize::BITS - count.leading_zeros()) + 1;
    let work = u64_from_index(key_bytes).checked_add(u64_from_index(max_key))
        .and_then(|bytes| bytes.checked_add(1))
        .and_then(|bytes| bytes.checked_mul(levels))
        .and_then(|bytes| bytes.checked_mul(12))
        .ok_or_else(|| ctx.refuse_codec_limit("cycle identity comparisons", u64::MAX, u64::MAX))?;
    ctx.charge_work(work, "cycle identity comparisons")
}

/// Walk the evaluator graph, stopping when the finding consumer asks to stop.
fn walk_cycles(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    index: &ModelIndex<'_>,
    mut emit: impl FnMut(Finding) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let ((graph, max_key), mut graph_storage) = ctx.with_scoped_storage("cycle dependency graph", || {
        let mut graph = BTreeMap::<&str, Vec<&str>>::new();
        let mut max_key = 0;
        ctx.charge_work(u64_from_index(ir.model.curves.len()), "cycle carrier scan")?;
        for curve in &ir.model.curves {
            if let Some(procedural) = index.procedural_curves_for_curve(curve.id.as_str(), ctx)?.and_then(|rows| rows.first().copied()) {
                let dependencies = curve_dependencies(ctx, procedural.definition())?;
                if !dependencies.is_empty() {
                    max_key = max_key.max(curve.id.as_str().len());
                    lookup_work(ctx, graph.len(), curve.id.as_str().len(), max_key)?;
                    ctx.insert_btree_map(&mut graph, curve.id.as_str(), dependencies, "cycle graph nodes")?;
                }
            }
        }
        ctx.charge_work(u64_from_index(ir.model.surfaces.len()), "cycle carrier scan")?;
        for surface in &ir.model.surfaces {
            if let Some(procedural) = index.procedural_surface_for_surface(surface.id.as_str(), ctx)? {
                let dependencies = surface_dependencies(ctx, procedural.definition())?;
                if !dependencies.is_empty() {
                    max_key = max_key.max(surface.id.as_str().len());
                    lookup_work(ctx, graph.len(), surface.id.as_str().len(), max_key)?;
                    ctx.insert_btree_map(&mut graph, surface.id.as_str(), dependencies, "cycle graph nodes")?;
                }
            }
        }
        Ok::<_, CodecError>((graph, max_key))
    })?;
    let mut complete = BTreeSet::new();
    let mut active = BTreeMap::new();
    let mut stack = Vec::new();
    for &start in graph.keys() {
        lookup_work(ctx, graph.len(), start.len(), max_key)?;
        if complete.contains(start) {
            continue;
        }
        graph_storage.with_storage(|| ctx.insert_btree_map(&mut active, start, 0usize, "cycle active nodes"))?;
        let depth = ctx.enter_nested("cycle traversal depth")?;
        graph_storage.with_storage(|| ctx.push_vec(&mut stack, Frame { node: start, next_child: 0, _depth: depth }, "cycle traversal stack"))?;
        while let Some(frame) = stack.last_mut() {
            ctx.charge_work(1, "cycle traversal")?;
            lookup_work(ctx, graph.len(), frame.node.len(), max_key)?;
            let children = &graph[frame.node];
            if frame.next_child == children.len() {
                if let Some(finished) = stack.pop() {
                    lookup_work(ctx, graph.len(), finished.node.len(), max_key)?;
                    active.remove(finished.node);
                    graph_storage.with_storage(|| ctx.insert_btree_set(&mut complete, finished.node, "cycle completed nodes"))?;
                }
                continue;
            }
            let child = children[frame.next_child];
            frame.next_child += 1;
            lookup_work(ctx, graph.len(), child.len(), max_key)?;
            if !graph.contains_key(child) || complete.contains(child) {
                continue;
            }
            lookup_work(ctx, graph.len(), child.len(), max_key)?;
            if let Some(&start_index) = active.get(child) {
                ctx.charge_work(u64_from_index(stack.len() - start_index), "cycle path walk")?;
                let finding = Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: ctx.format_retained(format_args!("malformed curve/surface reference cycle: {}", CyclePath { stack: &stack[start_index..], child }), "cycle finding message")?,
                    entity: Some(ctx.copy_retained_text(child, "cycle finding identity")?),
                };
                if !emit(finding)? { return Ok(()); }
                continue;
            }
            graph_storage.with_storage(|| ctx.insert_btree_map(&mut active, child, stack.len(), "cycle active nodes"))?;
            let depth = ctx.enter_nested("cycle traversal depth")?;
            graph_storage.with_storage(|| ctx.push_vec(&mut stack, Frame { node: child, next_child: 0, _depth: depth }, "cycle traversal stack"))?;
        }
    }
    Ok(())
}

/// Report cycles in the dependencies an evaluator can follow from a carrier.
pub(super) fn check_evaluation_cycles(ctx: &DecodeContext<'_>, ir: &CadIr, index: &ModelIndex<'_>, findings: &mut Vec<Finding>) -> Result<(), CodecError> {
    walk_cycles(ctx, ir, index, |finding| {
        ctx.push_vec(findings, finding, "cycle findings")?;
        Ok(true)
    })
}

/// Refuse a decoded model with the first recursive curve or surface dependency.
pub(crate) fn admit_evaluation_cycles(ctx: &DecodeContext<'_>, ir: &CadIr) -> Result<(), CodecError> {
    let index = ModelIndex::new_model_only(ir, ctx)?;
    walk_cycles(ctx, ir, &index, |finding| Err(CodecError::Malformed(finding.message)))
}

#[cfg(test)]
mod tests;
