// SPDX-License-Identifier: Apache-2.0
//! Cross-record curve and surface dependencies used by model evaluation.

use std::fmt;

use cadmpeg_core::decode::{DecodeContext, DepthGuard};
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::geometry::procedural_curves::ProceduralCurveDefinition;
use crate::geometry::procedural_surfaces::ProceduralSurfaceDefinition;
use crate::index::{identities::BorrowedIdentities, ModelIndex};
use crate::report::{
    check::{Check, Finding},
    Severity,
};

fn curve_dependencies(definition: &ProceduralCurveDefinition) -> Vec<&str> {
    match definition {
        ProceduralCurveDefinition::Replica { source, .. } => vec![source.as_str()],
        ProceduralCurveDefinition::Subset(payload) => {
            vec![payload.source().as_str()]
        }
        ProceduralCurveDefinition::TolerantIntersection {
            construction,
            parameterization: Some(_),
            ..
        } => construction
            .supports()
            .iter()
            .map(crate::ids::SurfaceId::as_str)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    }
}

fn surface_dependencies(definition: &ProceduralSurfaceDefinition) -> Vec<&str> {
    match definition {
        ProceduralSurfaceDefinition::AxisRevolution(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::Extrusion(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::LinearSweep(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::Revolution(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::Ruled { first, second, .. } => {
            vec![first.as_str(), second.as_str()]
        }
        ProceduralSurfaceDefinition::Sum(payload) => {
            vec![payload.first().as_str(), payload.second().as_str()]
        }
        ProceduralSurfaceDefinition::Sweep(payload) if payload.native().is_some() => {
            vec![payload.profile().as_str(), payload.spine().as_str()]
        }
        ProceduralSurfaceDefinition::Blend(payload) => {
            if let Some(native) = payload.native() {
                native
                    .sides
                    .iter()
                    .filter_map(|side| {
                        side.surface
                            .as_ref()
                            .map(|support| support.surface.as_str())
                    })
                    .chain(std::iter::once(native.slice.as_str()))
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            }
        }
        ProceduralSurfaceDefinition::VariableBlend(payload) => payload
            .construction()
            .sides
            .iter()
            .filter_map(|side| {
                side.surface
                    .as_ref()
                    .map(|support| support.surface.as_str())
            })
            .collect::<Vec<_>>(),
        ProceduralSurfaceDefinition::CurveBounded { support, .. } => {
            vec![support.as_str()]
        }
        ProceduralSurfaceDefinition::Replica { source, .. } => {
            vec![source.as_str()]
        }
        ProceduralSurfaceDefinition::Subset(payload) => {
            vec![payload.support().as_str()]
        }
        ProceduralSurfaceDefinition::ParallelOffset(payload) => {
            vec![payload.support().as_str()]
        }
        ProceduralSurfaceDefinition::Offset(payload) => {
            vec![payload.support().as_str()]
        }
        _ => Vec::new(),
    }
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

/// Report cycles in the dependencies an evaluator can follow from a carrier.
pub(super) fn check_evaluation_cycles<'ir>(
    ctx: &DecodeContext<'_>,
    ir: &'ir CadIr,
    index: &ModelIndex<'ir>,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let mut graph = BorrowedIdentities::build(|add| {
        for curve in &ir.geometry.curves {
            if let Some(procedural) = index
                .procedural_curves_for_curve(curve.id.as_str(), crate::index::StandardIndex)
                .and_then(|rows| rows.first().copied())
            {
                let dependencies = curve_dependencies(procedural.definition());
                if !dependencies.is_empty() {
                    add(
                        curve.id.as_str(),
                        Node {
                            dependencies,
                            visit: Visit::Unseen,
                        },
                    );
                }
            }
        }
        for surface in &ir.geometry.surfaces {
            if let Some(procedural) = index
                .procedural_surface_for_surface(surface.id.as_str(), crate::index::StandardIndex)
            {
                let dependencies = surface_dependencies(procedural.definition());
                if !dependencies.is_empty() {
                    add(
                        surface.id.as_str(),
                        Node {
                            dependencies,
                            visit: Visit::Unseen,
                        },
                    );
                }
            }
        }
    });
    let mut starts = graph.identities().collect::<Vec<_>>();
    starts.sort_by(Ord::cmp);
    let mut stack = Vec::new();
    for start in starts {
        let Some(node) = graph.get_mut(start) else {
            continue;
        };
        if matches!(node.visit, Visit::Complete) {
            continue;
        }
        node.visit = Visit::Active(0);
        let depth = ctx.enter_nested("cycle traversal depth")?;
        stack.push(Frame {
            node: start,
            next_child: 0,
            _depth: depth,
        });
        while let Some(frame) = stack.last_mut() {
            let child = graph
                .get(frame.node)
                .and_then(|node| node.dependencies.get(frame.next_child))
                .copied();
            let Some(child) = child else {
                if let Some(finished) = stack.pop() {
                    if let Some(node) = graph.get_mut(finished.node) {
                        node.visit = Visit::Complete;
                    }
                }
                continue;
            };
            frame.next_child += 1;
            let Some(node) = graph.get_mut(child) else {
                continue;
            };
            match node.visit {
                Visit::Complete => {}
                Visit::Active(start_index) => {
                    findings.push(Finding {
                        related: stack[start_index..]
                            .iter()
                            .map(|frame| frame.node)
                            .filter(|id| *id != child)
                            .map(str::to_owned)
                            .collect(),
                        check: Check::ReferentialIntegrity,
                        severity: Severity::Error,
                        message: format!(
                            "malformed curve/surface reference cycle: {}",
                            CyclePath {
                                stack: &stack[start_index..],
                                child
                            }
                        ),
                        entity: Some(child.to_owned()),
                    });
                }
                Visit::Unseen => {
                    node.visit = Visit::Active(stack.len());
                    let depth = ctx.enter_nested("cycle traversal depth")?;
                    stack.push(Frame {
                        node: child,
                        next_child: 0,
                        _depth: depth,
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
