// SPDX-License-Identifier: Apache-2.0
//! Remove standalone topology wrappers from physically dependent construction carriers.

use super::SourceSequences;
use crate::directory::{DirectoryEntry, Subordinate};
use cadmpeg_ir::{ids::ShellId, topology::ShellMember, CadIr};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn retain_independent(
    ir: &mut CadIr,
    shell_id: &ShellId,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    sequences: &mut SourceSequences,
) {
    let Some(shell_index) = ir
        .geometry
        .shells
        .iter()
        .position(|shell| shell.id == *shell_id)
    else {
        return;
    };
    let mut removed_edges = BTreeSet::new();

    let mut edges = BTreeMap::new();
    for edge in &ir.geometry.edges {
        edges.insert(&edge.id, edge);
    }
    let mut source_ranges = BTreeMap::new();
    for id in ir.geometry.shells[shell_index].wire_edges() {
        if let Some(edge) = edges.get(id) {
            if let (Some(curve), Some(range)) = (edge.curve(), edge.param_range()) {
                source_ranges.insert(curve, range);
            }
        }
    }
    for curve in &mut ir.geometry.curves {
        if let Some(range) = source_ranges.get(&curve.id) {
            curve.parameter_range =
                cadmpeg_ir::topology::IncreasingParameterInterval::from_finite_endpoints(*range);
        }
    }
    drop(source_ranges);
    for id in ir.geometry.shells[shell_index].wire_edges() {
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
        return;
    }
    if ir.geometry.shells[shell_index]
        .wire_edges()
        .all(|id| removed_edges.contains(id))
        && ir.geometry.shells[shell_index]
            .free_vertices()
            .next()
            .is_none()
    {
        let region = ir.geometry.shells[shell_index].region.clone();
        let body = ir
            .geometry
            .regions
            .iter()
            .find(|candidate| candidate.id == region)
            .map(|region| region.body.clone());
        ir.geometry.shells.remove(shell_index);
        ir.geometry
            .regions
            .retain(|candidate| candidate.id != region);
        if let Some(body) = body {
            ir.geometry.bodies.retain(|candidate| candidate.id != body);
        }
    } else {
        ir.geometry.shells[shell_index].members.retain(
            |member| !matches!(member, ShellMember::WireEdge { id } if removed_edges.contains(id)),
        );
    }
    let mut used_edges = BTreeSet::new();
    for coedge in &ir.geometry.coedges {
        used_edges.insert(&coedge.edge);
    }
    for shell in &ir.geometry.shells {
        for edge in shell.wire_edges() {
            used_edges.insert(edge);
        }
    }
    let mut removed_vertices = BTreeSet::new();
    for edge in &ir.geometry.edges {
        if removed_edges.contains(&edge.id) && !used_edges.contains(&edge.id) {
            for vertex in edge.vertices() {
                removed_vertices.insert(vertex.clone());
            }
        }
    }
    ir.geometry
        .edges
        .retain(|edge| !removed_edges.contains(&edge.id) || used_edges.contains(&edge.id));
    let mut used_vertices = BTreeSet::new();
    for edge in &ir.geometry.edges {
        for vertex in edge.vertices() {
            used_vertices.insert(vertex);
        }
    }
    for shell in &ir.geometry.shells {
        for vertex in shell.free_vertices() {
            used_vertices.insert(vertex);
        }
    }
    let mut removed_points = BTreeSet::new();
    for vertex in &ir.geometry.vertices {
        if removed_vertices.contains(&vertex.id) && !used_vertices.contains(&vertex.id) {
            removed_points.insert(vertex.point.clone());
        }
    }
    ir.geometry.vertices.retain(|vertex| {
        !removed_vertices.contains(&vertex.id) || used_vertices.contains(&vertex.id)
    });
    let mut used_points = BTreeSet::new();
    for vertex in &ir.geometry.vertices {
        used_points.insert(&vertex.point);
    }
    ir.geometry.points.retain(|point| {
        !removed_points.contains(&point.id)
            || used_points.contains(&point.id)
            || sequences
                .point(&point.id)
                .and_then(|sequence| entries.get(&sequence))
                .is_some_and(|entry| entry.entity_type == 116)
    });
    let mut live_points = BTreeSet::new();
    for point in &ir.geometry.points {
        live_points.insert(&point.id);
    }
    sequences.points.retain(|id, _| live_points.contains(id));
}
