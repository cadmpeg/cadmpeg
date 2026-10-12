// SPDX-License-Identifier: Apache-2.0
//! Cross-record curve and surface dependencies used by model evaluation.

use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::geometry::{ProceduralCurveDefinition, ProceduralSurfaceDefinition};
use crate::index::{identities::BorrowedIdentities, ModelIndex};
use crate::report::{
    check::{Check, Finding},
    Severity,
};

fn curve_dependencies(definition: &ProceduralCurveDefinition) -> [Option<&str>; 3] {
    match definition {
        ProceduralCurveDefinition::Replica { source, .. } => [Some(source.as_str()), None, None],
        ProceduralCurveDefinition::Subset(payload) => [Some(payload.source().as_str()), None, None],
        ProceduralCurveDefinition::TolerantIntersection {
            construction,
            parameterization: Some(_),
            ..
        } => [
            Some(construction.supports()[0].as_str()),
            Some(construction.supports()[1].as_str()),
            None,
        ],
        _ => [None; 3],
    }
}

fn surface_dependencies(definition: &ProceduralSurfaceDefinition) -> [Option<&str>; 3] {
    match definition {
        ProceduralSurfaceDefinition::AxisRevolution(payload) => {
            [Some(payload.directrix().as_str()), None, None]
        }
        ProceduralSurfaceDefinition::Extrusion(payload) => {
            [Some(payload.directrix().as_str()), None, None]
        }
        ProceduralSurfaceDefinition::LinearSweep(payload) => {
            [Some(payload.directrix().as_str()), None, None]
        }
        ProceduralSurfaceDefinition::Revolution(payload) => {
            [Some(payload.directrix().as_str()), None, None]
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            [Some(first.as_str()), Some(second.as_str()), None]
        }
        ProceduralSurfaceDefinition::Sum(payload) => [
            Some(payload.first().as_str()),
            Some(payload.second().as_str()),
            None,
        ],
        ProceduralSurfaceDefinition::Sweep(payload) if payload.native().is_some() => [
            Some(payload.profile().as_str()),
            Some(payload.spine().as_str()),
            None,
        ],
        ProceduralSurfaceDefinition::Blend(payload) => {
            if let Some(native) = payload.native() {
                let [first, second] = &native.sides;
                [
                    first
                        .surface
                        .as_ref()
                        .map(|support| support.surface.as_str()),
                    second
                        .surface
                        .as_ref()
                        .map(|support| support.surface.as_str()),
                    Some(native.slice.as_str()),
                ]
            } else {
                [None; 3]
            }
        }
        ProceduralSurfaceDefinition::VariableBlend(payload) => {
            let [first, second] = &payload.construction().sides;
            [
                first
                    .surface
                    .as_ref()
                    .map(|support| support.surface.as_str()),
                second
                    .surface
                    .as_ref()
                    .map(|support| support.surface.as_str()),
                None,
            ]
        }
        ProceduralSurfaceDefinition::CurveBounded { support, .. } => {
            [Some(support.as_str()), None, None]
        }
        ProceduralSurfaceDefinition::Replica { source, .. } => [Some(source.as_str()), None, None],
        ProceduralSurfaceDefinition::Subset(payload) => {
            [Some(payload.support().as_str()), None, None]
        }
        ProceduralSurfaceDefinition::ParallelOffset(payload) => {
            [Some(payload.support().as_str()), None, None]
        }
        ProceduralSurfaceDefinition::Offset(payload) => {
            [Some(payload.support().as_str()), None, None]
        }
        _ => [None; 3],
    }
}

struct Frame<'a, 'session> {
    node: &'a str,
    next_child: usize,
    _depth: DepthGuard<'session>,
}

#[derive(Clone, Copy)]
enum Visit {
    Unseen,
    Active(usize),
    Complete,
}

struct Node<'ir> {
    dependencies: [Option<&'ir str>; 3],
    visit: Visit,
}

/// Walk the evaluator graph, stopping when the finding consumer asks to stop.
fn walk_cycles(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    index: &ModelIndex<'_>,
    mut emit: impl FnMut(Finding) -> Result<bool, CodecError>,
) -> Result<(), CodecError> {
    let mut graph_storage = ctx.reserve_scoped(0, "cycle dependency graph")?;
    let mut graph = graph_storage.with_storage(|| {
        BorrowedIdentities::build(ctx, |add| {
            for curve in &ir.model.curves {
                ctx.charge_work(1, "cycle carrier scan")?;
                if let Some(procedural) = index
                    .procedural_curves_for_curve(curve.id.as_str(), ctx)?
                    .and_then(|rows| rows.first().copied())
                {
                    let dependencies = curve_dependencies(procedural.definition());
                    if dependencies.iter().any(Option::is_some) {
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
                if let Some(procedural) =
                    index.procedural_surface_for_surface(surface.id.as_str(), ctx)?
                {
                    let dependencies = surface_dependencies(procedural.definition());
                    if dependencies.iter().any(Option::is_some) {
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
    let mut starts = Vec::new();
    graph_storage.with_storage(|| {
        for identity in graph.identities("cycle start identities")? {
            ctx.push_vec(&mut starts, identity, "cycle start identities")?;
        }
        Ok::<_, CodecError>(())
    })?;
    ctx.stable_sort_by(&mut starts, |id| *id, Ord::cmp, "cycle start order")?;
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
                .and_then(|node| node.dependencies.iter().flatten().nth(frame.next_child))
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
                    let frames = ctx.admit_iter(&stack[start_index..], "cycle path walk")?;
                    let mut message = ctx.format_retained(
                        format_args!("malformed curve/surface reference cycle: "),
                        "cycle finding message",
                    )?;
                    for frame in frames {
                        ctx.append_formatted_retained(
                            &mut message,
                            format_args!("{} -> ", frame.node),
                            "cycle finding message",
                        )?;
                    }
                    ctx.append_formatted_retained(
                        &mut message,
                        format_args!("{child}"),
                        "cycle finding message",
                    )?;
                    let finding = Finding {
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message,
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
    let index = ModelIndex::new_model_only(ir, ctx)?;
    walk_cycles(ctx, ir, &index, |finding| {
        Err(CodecError::Malformed(finding.message))
    })
}

#[cfg(test)]
mod tests;
