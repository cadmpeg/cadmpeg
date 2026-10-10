// SPDX-License-Identifier: Apache-2.0
//! Remove standalone topology wrappers from physically dependent construction carriers.

use super::SourceSequences;
use crate::directory::{DirectoryEntry, Subordinate};
use cadmpeg_core::{decode::DecodeContext, CodecError};
use cadmpeg_ir::{ids::ShellId, CadIr};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn retain_independent(
    ir: &mut CadIr,
    shell_id: &ShellId,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    sequences: &mut SourceSequences,
    ctx: &DecodeContext<'_>,
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
    let mut scratch = ctx.reserve_scoped(0, "iges construction topology index")?;
    let mut edges = BTreeMap::new();
    for edge in &ir.model.edges {
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut edges,
                &edge.id,
                edge,
                "iges construction topology index",
            )
        })?;
    }
    let mut source_ranges = BTreeMap::new();
    for id in ir.model.shells[shell_index].wire_edges() {
        if let Some(edge) = edges.get(id) {
            if let (Some(curve), Some(range)) = (edge.curve(), edge.param_range()) {
                scratch.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut source_ranges,
                        curve,
                        range,
                        "iges source curve intervals",
                    )
                })?;
            }
        }
    }
    for curve in &mut ir.model.curves {
        ctx.charge_work(1, "iges source curve interval transfer")?;
        if let Some(range) = source_ranges.get(&curve.id) {
            curve.parameter_range =
                cadmpeg_ir::topology::IncreasingParameterInterval::from_finite_endpoints(*range);
        }
    }
    drop(source_ranges);
    for id in ir.model.shells[shell_index].wire_edges() {
        ctx.charge_work(1, "iges independent wire membership")?;
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
            ctx.insert_btree_set(
                &mut removed_edges,
                id.try_clone_for_decode(ctx, "iges construction edge identity")?,
                "iges construction edges",
            )?;
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
        let region = ir.model.shells[shell_index]
            .region
            .try_clone_for_decode(ctx, "iges empty wire region")?;
        let body = ir
            .model
            .regions
            .iter()
            .find(|candidate| candidate.id == region)
            .map(|region| {
                region
                    .body
                    .try_clone_for_decode(ctx, "iges empty wire body")
            })
            .transpose()?;
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
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut used_edges,
                &coedge.edge,
                "iges used construction edges",
            )
        })?;
    }
    for shell in &ir.model.shells {
        for edge in shell.wire_edges() {
            scratch.with_storage(|| {
                ctx.insert_btree_set(&mut used_edges, edge, "iges used construction edges")
            })?;
        }
    }
    let mut removed_vertices = BTreeSet::new();
    for edge in &ir.model.edges {
        if removed_edges.contains(&edge.id) && !used_edges.contains(&edge.id) {
            for vertex in [&edge.start, &edge.end] {
                ctx.insert_btree_set(
                    &mut removed_vertices,
                    vertex.try_clone_for_decode(ctx, "iges construction vertex identity")?,
                    "iges construction vertices",
                )?;
            }
        }
    }
    ir.model
        .edges
        .retain(|edge| !removed_edges.contains(&edge.id) || used_edges.contains(&edge.id));
    let mut used_vertices = BTreeSet::new();
    for edge in &ir.model.edges {
        for vertex in [&edge.start, &edge.end] {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut used_vertices,
                    vertex,
                    "iges used construction vertices",
                )
            })?;
        }
    }
    for shell in &ir.model.shells {
        for vertex in shell.free_vertices() {
            scratch.with_storage(|| {
                ctx.insert_btree_set(
                    &mut used_vertices,
                    vertex,
                    "iges used construction vertices",
                )
            })?;
        }
    }
    let mut removed_points = BTreeSet::new();
    for vertex in &ir.model.vertices {
        if removed_vertices.contains(&vertex.id) && !used_vertices.contains(&vertex.id) {
            ctx.insert_btree_set(
                &mut removed_points,
                vertex
                    .point
                    .try_clone_for_decode(ctx, "iges construction point identity")?,
                "iges construction points",
            )?;
        }
    }
    ir.model.vertices.retain(|vertex| {
        !removed_vertices.contains(&vertex.id) || used_vertices.contains(&vertex.id)
    });
    let mut used_points = BTreeSet::new();
    for vertex in &ir.model.vertices {
        scratch.with_storage(|| {
            ctx.insert_btree_set(
                &mut used_points,
                &vertex.point,
                "iges used construction points",
            )
        })?;
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
        scratch.with_storage(|| {
            ctx.insert_btree_set(&mut live_points, &point.id, "iges live construction points")
        })?;
    }
    sequences.points.retain(|id, _| live_points.contains(id));
    Ok(())
}
