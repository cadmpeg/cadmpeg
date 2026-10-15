// SPDX-License-Identifier: Apache-2.0
//! Remove standalone topology wrappers from physically dependent construction carriers.

use super::SourceSequences;
use crate::directory::{DirectoryEntry, Subordinate};
use cadmpeg_core::CodecError;
use cadmpeg_ir::{ids::ShellId, CadIr};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn retain_independent(
    ir: &mut CadIr,
    shell_id: &ShellId,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    sequences: &mut SourceSequences,
) -> Result<(), CodecError> {
    let Some(shell_index) = ir
        .model
        .shells
        .iter()
        .position(|shell| shell.id == *shell_id)
    else {
        return Ok(());
    };
    let mut removed_edges = BTreeSet::new();

    let mut edges = BTreeMap::new();
    for edge in &ir.model.edges {
        edges.insert(&edge.id, edge);
    }
    let mut source_ranges = BTreeMap::new();
    for id in ir.model.shells[shell_index].wire_edges() {
        if let Some(edge) = edges.get(id) {
            if let (Some(curve), Some(range)) = (edge.curve(), edge.param_range()) {
                source_ranges.insert(curve, range);
            }
        }
    }
    for curve in &mut ir.model.curves {
        if let Some(range) = source_ranges.get(&curve.id) {
            curve.parameter_range =
                cadmpeg_ir::topology::IncreasingParameterInterval::from_finite_endpoints(*range);
        }
    }
    drop(source_ranges);
    for id in ir.model.shells[shell_index].wire_edges() {
        let independent = edges
            .get(id)
            .and_then(|edge| edge.curve())
            .and_then(|curve| sequences.curve(curve))
            .and_then(|sequence| entries.get(&sequence))
            .is_some_and(|entry| {
                !matches!(
                    entry.status.subordinate(),
                    Some(Subordinate::Physically | Subordinate::Both)
                )
            });
        if !independent {
            removed_edges.insert(id.clone());
        }
    }
    drop(edges);
    if removed_edges.is_empty() {
        return Ok(());
    }
    if ir.model.shells[shell_index]
        .wire_edges()
        .iter()
        .all(|id| removed_edges.contains(id))
        && ir.model.shells[shell_index].free_vertices().is_empty()
    {
        let region = ir.model.shells[shell_index].region.clone();
        let body = ir
            .model
            .regions
            .iter()
            .find(|candidate| candidate.id == region)
            .map(|region| region.body.clone());
        ir.model.shells.remove(shell_index);
        ir.model.regions.retain(|candidate| candidate.id != region);
        if let Some(body) = body {
            ir.model.bodies.retain(|candidate| candidate.id != body);
        }
    } else {
        ir.model.shells[shell_index]
            .edit_topology(|_, wire_edges, _| {
                wire_edges.retain(|id| !removed_edges.contains(id));
            })
            .map_err(CodecError::malformed)?;
    }
    let mut used_edges = BTreeSet::new();
    for coedge in &ir.model.coedges {
        used_edges.insert(&coedge.edge);
    }
    for shell in &ir.model.shells {
        for edge in shell.wire_edges() {
            used_edges.insert(edge);
        }
    }
    let mut removed_vertices = BTreeSet::new();
    for edge in &ir.model.edges {
        if removed_edges.contains(&edge.id) && !used_edges.contains(&edge.id) {
            for vertex in [&edge.start, &edge.end] {
                removed_vertices.insert(vertex.clone());
            }
        }
    }
    ir.model
        .edges
        .retain(|edge| !removed_edges.contains(&edge.id) || used_edges.contains(&edge.id));
    let mut used_vertices = BTreeSet::new();
    for edge in &ir.model.edges {
        for vertex in [&edge.start, &edge.end] {
            used_vertices.insert(vertex);
        }
    }
    for shell in &ir.model.shells {
        for vertex in shell.free_vertices() {
            used_vertices.insert(vertex);
        }
    }
    let mut removed_points = BTreeSet::new();
    for vertex in &ir.model.vertices {
        if removed_vertices.contains(&vertex.id) && !used_vertices.contains(&vertex.id) {
            removed_points.insert(vertex.point.clone());
        }
    }
    ir.model.vertices.retain(|vertex| {
        !removed_vertices.contains(&vertex.id) || used_vertices.contains(&vertex.id)
    });
    let mut used_points = BTreeSet::new();
    for vertex in &ir.model.vertices {
        used_points.insert(&vertex.point);
    }
    ir.model.points.retain(|point| {
        !removed_points.contains(&point.id)
            || used_points.contains(&point.id)
            || sequences
                .point(&point.id)
                .and_then(|sequence| entries.get(&sequence))
                .is_some_and(|entry| entry.entity_type == 116)
    });
    let mut live_points = BTreeSet::new();
    for point in &ir.model.points {
        live_points.insert(&point.id);
    }
    sequences.points.retain(|id, _| live_points.contains(id));
    Ok(())
}
