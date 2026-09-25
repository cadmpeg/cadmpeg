// SPDX-License-Identifier: Apache-2.0
//! Cross-record curve and surface dependencies used by model evaluation.

use std::collections::{BTreeMap, HashMap, HashSet};

use crate::document::CadIr;
use crate::geometry::{ProceduralCurveDefinition, ProceduralSurfaceDefinition};
use crate::index::ModelIndex;
use crate::report::{
    check::{Check, Finding},
    Severity,
};

fn curve_dependencies(definition: &ProceduralCurveDefinition) -> Vec<&str> {
    match definition {
        ProceduralCurveDefinition::Replica { source, .. } => vec![source.as_str()],
        ProceduralCurveDefinition::Subset(payload) => vec![payload.source().as_str()],
        ProceduralCurveDefinition::TolerantIntersection {
            construction,
            parameterization: Some(_),
            ..
        } => construction
            .supports()
            .iter()
            .map(crate::ids::SurfaceId::as_str)
            .collect(),
        _ => Vec::new(),
    }
}

fn surface_dependencies(definition: &ProceduralSurfaceDefinition) -> Vec<&str> {
    match definition {
        ProceduralSurfaceDefinition::AxisRevolution(payload) => {
            vec![payload.directrix().as_str()]
        }
        ProceduralSurfaceDefinition::Extrusion(payload) => vec![payload.directrix().as_str()],
        ProceduralSurfaceDefinition::LinearSweep(payload) => vec![payload.directrix().as_str()],
        ProceduralSurfaceDefinition::Revolution(payload) => vec![payload.directrix().as_str()],
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
            payload.native().map_or_else(Vec::new, |native| {
                let mut dependencies = native
                    .sides
                    .iter()
                    .filter_map(|side| {
                        side.surface
                            .as_ref()
                            .map(|support| support.surface.as_str())
                    })
                    .collect::<Vec<_>>();
                dependencies.push(native.slice.as_str());
                dependencies
            })
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
            .collect(),
        ProceduralSurfaceDefinition::CurveBounded { support, .. } => vec![support.as_str()],
        ProceduralSurfaceDefinition::Replica { source, .. } => vec![source.as_str()],
        ProceduralSurfaceDefinition::Subset(payload) => vec![payload.support().as_str()],
        ProceduralSurfaceDefinition::ParallelOffset(payload) => vec![payload.support().as_str()],
        ProceduralSurfaceDefinition::Offset(payload) => vec![payload.support().as_str()],
        _ => Vec::new(),
    }
}

/// Report cycles in the dependencies an evaluator can follow from a carrier.
pub(super) fn check_evaluation_cycles(
    ir: &CadIr,
    index: &ModelIndex<'_>,
    findings: &mut Vec<Finding>,
) {
    let mut graph = BTreeMap::<&str, Vec<&str>>::new();
    for curve in &ir.model.curves {
        if let Some(procedural) = index
            .procedural_curves_for_curve(curve.id.as_str())
            .and_then(|procedurals| procedurals.first().copied())
        {
            let dependencies = curve_dependencies(procedural.definition());
            if !dependencies.is_empty() {
                graph.insert(curve.id.as_str(), dependencies);
            }
        }
    }
    for surface in &ir.model.surfaces {
        if let Some(procedural) = index.procedural_surface_for_surface(surface.id.as_str()) {
            let dependencies = surface_dependencies(procedural.definition());
            if !dependencies.is_empty() {
                graph.insert(surface.id.as_str(), dependencies);
            }
        }
    }

    let mut complete = HashSet::new();
    let mut active = HashMap::new();
    for &start in graph.keys() {
        if complete.contains(start) {
            continue;
        }
        active.insert(start, 0usize);
        let mut stack = vec![(start, 0usize)];
        while let Some((node, next_child)) = stack.last_mut() {
            let children = &graph[*node];
            if *next_child == children.len() {
                if let Some((finished, _)) = stack.pop() {
                    active.remove(finished);
                    complete.insert(finished);
                }
                continue;
            }
            let child = children[*next_child];
            *next_child += 1;
            if !graph.contains_key(child) || complete.contains(child) {
                continue;
            }
            if let Some(&start_index) = active.get(child) {
                let cycle = stack[start_index..]
                    .iter()
                    .map(|(member, _)| *member)
                    .chain(std::iter::once(child))
                    .collect::<Vec<_>>();
                findings.push(Finding {
                    check: Check::ReferentialIntegrity,
                    severity: Severity::Error,
                    message: format!(
                        "malformed curve/surface reference cycle: {}",
                        cycle.join(" -> ")
                    ),
                    entity: Some(child.to_owned()),
                });
                continue;
            }
            active.insert(child, stack.len());
            stack.push((child, 0));
        }
    }
}

/// Refuse a decoded model with a recursive curve or surface dependency.
pub(crate) fn admit_evaluation_cycles(ir: &CadIr) -> Result<(), cadmpeg_core::CodecError> {
    let index = ModelIndex::new_model_only(ir);
    let mut findings = Vec::new();
    check_evaluation_cycles(ir, &index, &mut findings);
    match findings.into_iter().next() {
        Some(finding) => Err(cadmpeg_core::CodecError::Malformed(finding.message)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests;
