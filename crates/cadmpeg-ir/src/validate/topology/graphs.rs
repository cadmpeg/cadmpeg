// SPDX-License-Identifier: Apache-2.0
//! Radial-ring, wire ownership and shell connectivity checks.

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;

use crate::document::CadIr;
use crate::index::identities::BorrowedIdentities;
use crate::report::{
    check::{Check, Finding},
    Severity,
};
use crate::validate::record_finding;

#[derive(Clone, Copy)]
enum RadialStatus {
    Closed(usize),
    DoesNotClose,
    CrossesEdge,
}

pub(in crate::validate) fn check_coedge_pairing(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let by_id = BorrowedIdentities::build(ctx, |add| {
        for coedge in ctx.admit_iter(&ir.model.coedges, "coedge identity scan")? {
            add(coedge.id.as_str(), coedge)?;
        }
        Ok(())
    })?;
    let mut statuses = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for coedge in &ir.model.coedges {
        ctx.charge_work(1, "radial ring owner scan")?;
        let start = coedge.id.as_str();
        if statuses.contains(ctx, start)? {
            continue;
        }
        let expected_edge = &coedge.edge;
        let mut path_storage = ctx.reserve_scoped(0, "radial path storage")?;
        let mut path = Vec::<&str>::new();
        let mut positions = BorrowedIdentities::build(ctx, |_| Ok(()))?;
        let mut current = start;
        loop {
            ctx.charge_work(1, "radial ring visit")?;
            if let Some(status) = statuses.get(ctx, current)?.copied() {
                let status = match status {
                    RadialStatus::Closed(_) => RadialStatus::DoesNotClose,
                    status => status,
                };
                for member in path {
                    ctx.charge_work(1, "radial status scan")?;
                    statuses.insert(member, status)?;
                }
                break;
            }
            if let Some(&cycle_start) = positions.get(ctx, current)? {
                let cycle_len = path.len() - cycle_start;
                for &member in &path[cycle_start..] {
                    ctx.charge_work(1, "radial status scan")?;
                    statuses.insert(member, RadialStatus::Closed(cycle_len))?;
                }
                for &member in &path[..cycle_start] {
                    ctx.charge_work(1, "radial status scan")?;
                    statuses.insert(member, RadialStatus::DoesNotClose)?;
                }
                break;
            }
            positions.insert(current, path.len())?;
            path_storage.with_storage(|| ctx.push_vec(&mut path, current, "radial path slots"))?;
            let Some(current_coedge) = by_id.get(ctx, current)? else {
                for member in path {
                    ctx.charge_work(1, "radial status scan")?;
                    statuses.insert(member, RadialStatus::DoesNotClose)?;
                }
                break;
            };
            let Some(next) = by_id.get(ctx, current_coedge.radial_next.as_str())? else {
                for member in path {
                    ctx.charge_work(1, "radial status scan")?;
                    statuses.insert(member, RadialStatus::DoesNotClose)?;
                }
                break;
            };
            if !ctx.equal(
                next.edge.as_str(),
                expected_edge.as_str(),
                "radial edge comparison",
            )? {
                for member in path {
                    ctx.charge_work(1, "radial status scan")?;
                    statuses.insert(member, RadialStatus::CrossesEdge)?;
                }
                break;
            }
            current = next.id.as_str();
        }
    }
    for coedge in &ir.model.coedges {
        ctx.charge_work(1, "radial finding owner scan")?;
        let owner = Some(coedge.id.as_str());
        let status = statuses
            .get(ctx, coedge.id.as_str())?
            .copied()
            .ok_or_else(|| CodecError::malformed("radial ring has no classified status"))?;
        match status {
            RadialStatus::CrossesEdge => {
                record_finding(
                    ctx,
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    format_args!("radial ring crosses edges"),
                )?;
                record_finding(
                    ctx,
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    format_args!("radial ring does not close"),
                )?;
            }
            RadialStatus::DoesNotClose => {
                record_finding(
                    ctx,
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    format_args!("radial ring does not close"),
                )?;
            }
            RadialStatus::Closed(2) => {
                if let Some(other) = by_id.get(ctx, coedge.radial_next.as_str())? {
                    if other.sense == coedge.sense {
                        record_finding(
                            ctx,
                            findings,
                            Check::CoedgePairing,
                            Severity::Warning,
                            owner,
                            format_args!("two-member radial ring has equal coedge senses"),
                        )?;
                    }
                }
            }
            RadialStatus::Closed(_) => {}
        }
    }
    Ok(())
}

pub(in crate::validate) fn check_wire_topology(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let coedge_edges = BorrowedIdentities::build(ctx, |add| {
        for coedge in ctx.admit_iter(&ir.model.coedges, "coedge edge index scan")? {
            add(coedge.edge.as_str(), ())?;
        }
        Ok(())
    })?;
    let edge_vertices = BorrowedIdentities::build(ctx, |add| {
        for edge in ctx.admit_iter(&ir.model.edges, "edge endpoint index scan")? {
            add(edge.start.as_str(), ())?;
            add(edge.end.as_str(), ())?;
        }
        Ok(())
    })?;
    let loop_vertices = BorrowedIdentities::build(ctx, |add| {
        for loop_ in &ir.model.loops {
            ctx.charge_work(1, "wire loop vertex scan")?;
            if let Some((vertex, _)) = loop_.singular_vertex() {
                add(vertex.as_str(), ())?;
            }
            for use_ in ctx.admit_iter(loop_.anchored_vertex_uses(), "wire loop vertex scan")? {
                add(use_.vertex.as_str(), ())?;
            }
        }
        Ok(())
    })?;
    let wire_owners = BorrowedIdentities::build(ctx, |add| {
        for shell in &ir.model.shells {
            ctx.charge_work(1, "wire shell owner scan")?;
            for edge in ctx.admit_iter(shell.wire_edges(), "wire shell owner scan")? {
                add(edge.as_str(), ())?;
            }
        }
        Ok(())
    })?;
    let free_owners = BorrowedIdentities::build(ctx, |add| {
        for shell in &ir.model.shells {
            ctx.charge_work(1, "free vertex shell owner scan")?;
            for vertex in ctx.admit_iter(shell.free_vertices(), "free vertex shell owner scan")? {
                add(vertex.as_str(), ())?;
            }
        }
        Ok(())
    })?;
    for shell in &ir.model.shells {
        ctx.charge_work(1, "wire shell reference scan")?;
        for edge in shell.wire_edges() {
            ctx.charge_work(1, "wire shell edge scan")?;
            if coedge_edges.contains(ctx, edge.as_str())? {
                record_finding(
                    ctx,
                    findings,
                    Check::WireTopology,
                    Severity::Error,
                    Some(shell.id.as_str()),
                    format_args!("wire edge is also referenced by a coedge"),
                )?;
            }
        }
        for vertex in shell.free_vertices() {
            ctx.charge_work(1, "free shell vertex scan")?;
            if edge_vertices.contains(ctx, vertex.as_str())? {
                record_finding(
                    ctx,
                    findings,
                    Check::WireTopology,
                    Severity::Error,
                    Some(shell.id.as_str()),
                    format_args!("free vertex is also referenced by an edge"),
                )?;
            }
        }
    }
    for edge in &ir.model.edges {
        ctx.charge_work(1, "wire edge owner scan")?;
        if !coedge_edges.contains(ctx, edge.id.as_str())?
            && wire_owners.match_count(ctx, edge.id.as_str())? != 1
        {
            record_finding(
                ctx,
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(edge.id.as_str()),
                format_args!("wire edge must belong to exactly one shell"),
            )?;
        }
    }
    for vertex in &ir.model.vertices {
        ctx.charge_work(1, "free vertex owner scan")?;
        let owner_count = free_owners.match_count(ctx, vertex.id.as_str())?;
        if owner_count > 1
            || (!edge_vertices.contains(ctx, vertex.id.as_str())?
                && !loop_vertices.contains(ctx, vertex.id.as_str())?
                && owner_count != 1)
        {
            record_finding(
                ctx,
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(vertex.id.as_str()),
                format_args!("free vertex must belong to exactly one shell"),
            )?;
        }
    }
    let regions = BorrowedIdentities::build(ctx, |add| {
        for region in ctx.admit_iter(&ir.model.regions, "region identity scan")? {
            add(region.id.as_str(), region)?;
        }
        Ok(())
    })?;
    let shells = BorrowedIdentities::build(ctx, |add| {
        for shell in ctx.admit_iter(&ir.model.shells, "shell identity scan")? {
            add(shell.id.as_str(), shell)?;
        }
        Ok(())
    })?;
    for body in &ir.model.bodies {
        ctx.charge_work(1, "wire body scan")?;
        if body.kind != crate::topology::BodyKind::Wire {
            continue;
        }
        let mut contains_faces = false;
        for region_id in &body.regions {
            ctx.charge_work(1, "wire body region scan")?;
            if let Some(region) = regions.get(ctx, region_id.as_str())? {
                for shell_id in &region.shells {
                    ctx.charge_work(1, "wire body shell scan")?;
                    if shells
                        .get(ctx, shell_id.as_str())?
                        .is_some_and(|shell| !shell.faces().is_empty())
                    {
                        contains_faces = true;
                        break;
                    }
                }
            }
            if contains_faces {
                break;
            }
        }
        if contains_faces {
            record_finding(
                ctx,
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(body.id.as_str()),
                format_args!("wire body contains faces"),
            )?;
        }
    }
    Ok(())
}

fn insert_incident<'ctx, 'ir>(
    ctx: &'ctx DecodeContext<'_>,
    groups: &mut BorrowedIdentities<'ctx, 'ir, BorrowedIdentities<'ctx, 'ir>>,
    member: &'ir str,
    face: &'ir str,
) -> Result<(), CodecError> {
    if let Some(group) = groups.get_mut(ctx, member)? {
        group.insert_unique(face, ())?;
    } else {
        let mut group = BorrowedIdentities::build(ctx, |_| Ok(()))?;
        group.insert_unique(face, ())?;
        groups.insert(member, group)?;
    }
    Ok(())
}

pub(in crate::validate) fn check_shell_connectivity(
    ctx: &DecodeContext<'_>,
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let faces = BorrowedIdentities::build(ctx, |add| {
        for face in ctx.admit_iter(&ir.model.faces, "face identity scan")? {
            add(face.id.as_str(), face)?;
        }
        Ok(())
    })?;
    let loop_faces = BorrowedIdentities::build(ctx, |add| {
        for loop_ in ctx.admit_iter(&ir.model.loops, "loop face index scan")? {
            add(loop_.id.as_str(), loop_.face.as_str())?;
        }
        Ok(())
    })?;
    let edges = BorrowedIdentities::build(ctx, |add| {
        for edge in ctx.admit_iter(&ir.model.edges, "edge identity scan")? {
            add(edge.id.as_str(), edge)?;
        }
        Ok(())
    })?;
    let mut faces_by_edge = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    let mut faces_by_vertex = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for coedge in &ir.model.coedges {
        ctx.charge_work(1, "shell coedge incidence scan")?;
        let Some(face) = loop_faces.get(ctx, coedge.owner_loop.as_str())?.copied() else {
            continue;
        };
        insert_incident(ctx, &mut faces_by_edge, coedge.edge.as_str(), face)?;
        if let Some(edge) = edges.get(ctx, coedge.edge.as_str())? {
            insert_incident(ctx, &mut faces_by_vertex, edge.start.as_str(), face)?;
            insert_incident(ctx, &mut faces_by_vertex, edge.end.as_str(), face)?;
        }
    }
    for loop_ in &ir.model.loops {
        ctx.charge_work(1, "shell loop incidence scan")?;
        let Some(face) = loop_faces.get(ctx, loop_.id.as_str())?.copied() else {
            continue;
        };
        match &loop_.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, .. } => {
                insert_incident(ctx, &mut faces_by_vertex, vertex.as_str(), face)?;
            }
            crate::topology::LoopBoundary::Ring(ring) => {
                for use_ in ring.vertex_uses() {
                    ctx.charge_work(1, "shell vertex use incidence scan")?;
                    insert_incident(ctx, &mut faces_by_vertex, use_.vertex.as_str(), face)?;
                }
            }
        }
    }
    // Store each incidence once. A shell walks each group at most once,
    // regardless of how many of its faces share the edge or vertex.
    let mut incidence_storage = ctx.reserve_scoped(0, "shell incidence slots")?;
    let mut groups = Vec::new();
    let mut groups_by_face = BorrowedIdentities::build(ctx, |_| Ok(()))?;
    for incident_faces in faces_by_edge
        .values("shell incidence group scan")?
        .chain(faces_by_vertex.values("shell incidence group scan")?)
    {
        let group_index = groups.len();
        incidence_storage.with_storage(|| {
            ctx.push_vec(&mut groups, incident_faces, "shell incidence group slots")
        })?;
        for face in incident_faces.identities("shell incidence face scan")? {
            if let Some(face_groups) = groups_by_face.get_mut(ctx, face)? {
                incidence_storage.with_storage(|| {
                    ctx.push_vec(face_groups, group_index, "shell face incidence slots")
                })?;
            } else {
                let mut face_groups = Vec::new();
                incidence_storage.with_storage(|| {
                    ctx.push_vec(&mut face_groups, group_index, "shell face incidence slots")
                })?;
                groups_by_face.insert(face, face_groups)?;
            }
        }
    }
    let mut visited_groups = incidence_storage.with_storage(|| {
        ctx.alloc_filled(groups.len(), None, "shell incidence visited slots")
    })?;
    for (shell_index, shell) in ir.model.shells.iter().enumerate() {
        ctx.charge_work(1, "shell connectivity owner scan")?;
        if shell.faces().len() < 2 {
            continue;
        }
        let mut skips = false;
        for face in shell.faces() {
            ctx.charge_work(1, "shell connectivity face scan")?;
            if faces
                .get(ctx, face.as_str())?
                .is_none_or(|face| face.loops.is_empty())
            {
                skips = true;
                break;
            }
        }
        if skips {
            continue;
        }
        let mut owned = BorrowedIdentities::build(ctx, |_| Ok(()))?;
        owned.extend_unique(shell.faces().iter().map(crate::ids::FaceId::as_str))?;
        let mut reached = BorrowedIdentities::build(ctx, |_| Ok(()))?;
        let first = shell.faces()[0].as_str();
        reached.insert_unique(first, ())?;
        let mut pending_storage = ctx.reserve_scoped(0, "shell pending storage")?;
        let mut pending = Vec::new();
        pending_storage
            .with_storage(|| ctx.push_vec(&mut pending, first, "shell pending slots"))?;
        while !pending.is_empty() {
            ctx.charge_work(1, "shell connectivity pop")?;
            let Some(face) = pending.pop() else {
                break;
            };
            if let Some(face_groups) = groups_by_face.get(ctx, face)? {
                for &group_index in ctx.admit_iter(face_groups, "shell connectivity incidence scan")? {
                    if visited_groups[group_index] == Some(shell_index) {
                        continue;
                    }
                    visited_groups[group_index] = Some(shell_index);
                    for neighbor in groups[group_index].identities("shell connectivity neighbor scan")? {
                        if owned.contains(ctx, neighbor)? && reached.insert_unique(neighbor, ())? {
                            pending_storage.with_storage(|| {
                                ctx.push_vec(&mut pending, neighbor, "shell pending slots")
                            })?;
                        }
                    }
                }
            }
        }
        if reached.len() != owned.len() {
            record_finding(
                ctx,
                findings,
                Check::ShellTopology,
                Severity::Error,
                Some(shell.id.as_str()),
                format_args!("shell faces are disconnected through shared edges or vertices"),
            )?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
