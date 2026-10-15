// SPDX-License-Identifier: Apache-2.0
//! Radial-ring, wire ownership and shell connectivity checks.

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
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let by_id = BorrowedIdentities::build(|add| {
        for coedge in &ir.model.coedges {
            add(coedge.id.as_str(), coedge)?;
        }
        Ok(())
    })?;
    let mut statuses = BorrowedIdentities::build(|_| Ok(()))?;
    for coedge in &ir.model.coedges {
        let start = coedge.id.as_str();
        if statuses.contains(start) {
            continue;
        }
        let expected_edge = &coedge.edge;
        let mut path = Vec::<&str>::new();
        let mut positions = BorrowedIdentities::build(|_| Ok(()))?;
        let mut current = start;
        loop {
            if let Some(status) = statuses.get(current).copied() {
                let status = match status {
                    RadialStatus::Closed(_) => RadialStatus::DoesNotClose,
                    status => status,
                };
                for member in path {
                    statuses.insert(member, status);
                }
                break;
            }
            if let Some(&cycle_start) = positions.get(current) {
                let cycle_len = path.len() - cycle_start;
                for &member in &path[cycle_start..] {
                    statuses.insert(member, RadialStatus::Closed(cycle_len));
                }
                for &member in &path[..cycle_start] {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            }
            positions.insert(current, path.len());
            path.push(current);
            let Some(current_coedge) = by_id.get(current) else {
                for member in path {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            };
            let Some(next) = by_id.get(current_coedge.radial_next.as_str()) else {
                for member in path {
                    statuses.insert(member, RadialStatus::DoesNotClose);
                }
                break;
            };

            if next.edge != *expected_edge {
                for member in path {
                    statuses.insert(member, RadialStatus::CrossesEdge);
                }
                break;
            }
            current = next.id.as_str();
        }
    }
    for coedge in &ir.model.coedges {
        let owner = Some(coedge.id.as_str());
        let status = statuses
            .get(coedge.id.as_str())
            .copied()
            .ok_or_else(|| CodecError::malformed("radial ring has no classified status"))?;
        match status {
            RadialStatus::CrossesEdge => {
                record_finding(
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    format_args!("radial ring crosses edges"),
                );
                record_finding(
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    format_args!("radial ring does not close"),
                );
            }
            RadialStatus::DoesNotClose => {
                record_finding(
                    findings,
                    Check::CoedgePairing,
                    Severity::Error,
                    owner,
                    format_args!("radial ring does not close"),
                );
            }
            RadialStatus::Closed(2) => {
                if let Some(other) = by_id.get(coedge.radial_next.as_str()) {
                    if other.sense == coedge.sense {
                        record_finding(
                            findings,
                            Check::CoedgePairing,
                            Severity::Warning,
                            owner,
                            format_args!("two-member radial ring has equal coedge senses"),
                        );
                    }
                }
            }
            RadialStatus::Closed(_) => {}
        }
    }
    Ok(())
}

pub(in crate::validate) fn check_wire_topology(
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let coedge_edges = BorrowedIdentities::build(|add| {
        for coedge in &ir.model.coedges {
            add(coedge.edge.as_str(), ())?;
        }
        Ok(())
    })?;
    let edge_vertices = BorrowedIdentities::build(|add| {
        for edge in &ir.model.edges {
            add(edge.start.as_str(), ())?;
            add(edge.end.as_str(), ())?;
        }
        Ok(())
    })?;
    let loop_vertices = BorrowedIdentities::build(|add| {
        for loop_ in &ir.model.loops {
            for vertex in loop_.vertices() {
                add(vertex.as_str(), ())?;
            }
        }
        Ok(())
    })?;
    let wire_owners = BorrowedIdentities::build(|add| {
        for shell in &ir.model.shells {
            for edge in shell.wire_edges() {
                add(edge.as_str(), ())?;
            }
        }
        Ok(())
    })?;
    let free_owners = BorrowedIdentities::build(|add| {
        for shell in &ir.model.shells {
            for vertex in shell.free_vertices() {
                add(vertex.as_str(), ())?;
            }
        }
        Ok(())
    })?;
    for shell in &ir.model.shells {
        for edge in shell.wire_edges() {
            if coedge_edges.contains(edge.as_str()) {
                record_finding(
                    findings,
                    Check::WireTopology,
                    Severity::Error,
                    Some(shell.id.as_str()),
                    format_args!("wire edge is also referenced by a coedge"),
                );
            }
        }
        for vertex in shell.free_vertices() {
            if edge_vertices.contains(vertex.as_str()) {
                record_finding(
                    findings,
                    Check::WireTopology,
                    Severity::Error,
                    Some(shell.id.as_str()),
                    format_args!("free vertex is also referenced by an edge"),
                );
            }
        }
    }
    for edge in &ir.model.edges {
        if !coedge_edges.contains(edge.id.as_str())
            && wire_owners.match_count(edge.id.as_str()) != 1
        {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(edge.id.as_str()),
                format_args!("wire edge must belong to exactly one shell"),
            );
        }
    }
    for vertex in &ir.model.vertices {
        let owner_count = free_owners.match_count(vertex.id.as_str());
        if owner_count > 1
            || (!edge_vertices.contains(vertex.id.as_str())
                && !loop_vertices.contains(vertex.id.as_str())
                && owner_count != 1)
        {
            record_finding(
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(vertex.id.as_str()),
                format_args!("free vertex must belong to exactly one shell"),
            );
        }
    }
    let regions = BorrowedIdentities::build(|add| {
        for region in &ir.model.regions {
            add(region.id.as_str(), region)?;
        }
        Ok(())
    })?;
    let shells = BorrowedIdentities::build(|add| {
        for shell in &ir.model.shells {
            add(shell.id.as_str(), shell)?;
        }
        Ok(())
    })?;
    for body in &ir.model.bodies {
        if body.kind != crate::topology::BodyKind::Wire {
            continue;
        }
        let mut contains_faces = false;
        for region_id in &body.regions {
            if let Some(region) = regions.get(region_id.as_str()) {
                for shell_id in &region.shells {
                    if shells
                        .get(shell_id.as_str())
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
                findings,
                Check::WireTopology,
                Severity::Error,
                Some(body.id.as_str()),
                format_args!("wire body contains faces"),
            );
        }
    }
    Ok(())
}

fn insert_incident<'ir>(
    groups: &mut BorrowedIdentities<'ir, BorrowedIdentities<'ir>>,
    member: &'ir str,
    face: &'ir str,
) -> Result<(), CodecError> {
    if let Some(group) = groups.get_mut(member) {
        group.insert_unique(face, ());
    } else {
        let mut group = BorrowedIdentities::build(|_| Ok(()))?;
        group.insert_unique(face, ());
        groups.insert(member, group);
    }
    Ok(())
}

pub(in crate::validate) fn check_shell_connectivity(
    ir: &CadIr,
    findings: &mut Vec<Finding>,
) -> Result<(), CodecError> {
    let faces = BorrowedIdentities::build(|add| {
        for face in &ir.model.faces {
            add(face.id.as_str(), face)?;
        }
        Ok(())
    })?;
    let loop_faces = BorrowedIdentities::build(|add| {
        for loop_ in &ir.model.loops {
            add(loop_.id.as_str(), loop_.face.as_str())?;
        }
        Ok(())
    })?;
    let edges = BorrowedIdentities::build(|add| {
        for edge in &ir.model.edges {
            add(edge.id.as_str(), edge)?;
        }
        Ok(())
    })?;
    let mut faces_by_edge = BorrowedIdentities::build(|_| Ok(()))?;
    let mut faces_by_vertex = BorrowedIdentities::build(|_| Ok(()))?;
    for coedge in &ir.model.coedges {
        let Some(face) = loop_faces.get(coedge.owner_loop.as_str()).copied() else {
            continue;
        };
        insert_incident(&mut faces_by_edge, coedge.edge.as_str(), face)?;
        if let Some(edge) = edges.get(coedge.edge.as_str()) {
            insert_incident(&mut faces_by_vertex, edge.start.as_str(), face)?;
            insert_incident(&mut faces_by_vertex, edge.end.as_str(), face)?;
        }
    }
    for loop_ in &ir.model.loops {
        let Some(face) = loop_faces.get(loop_.id.as_str()).copied() else {
            continue;
        };
        match &loop_.boundary {
            crate::topology::LoopBoundary::Vertex { vertex, .. } => {
                insert_incident(&mut faces_by_vertex, vertex.as_str(), face)?;
            }
            crate::topology::LoopBoundary::Ring(ring) => {
                for use_ in ring.vertex_uses() {
                    insert_incident(&mut faces_by_vertex, use_.vertex.as_str(), face)?;
                }
            }
        }
    }
    let mut neighbors = BorrowedIdentities::build(|_| Ok(()))?;
    for incident_faces in faces_by_edge.values().chain(faces_by_vertex.values()) {
        for face in incident_faces.identities() {
            for other in incident_faces.identities() {
                if other != face {
                    insert_incident(&mut neighbors, face, other)?;
                }
            }
        }
    }
    for shell in &ir.model.shells {
        if shell.faces().len() < 2 {
            continue;
        }
        let mut skips = false;
        for face in shell.faces() {
            if faces
                .get(face.as_str())
                .is_none_or(|face| face.loops.is_empty())
            {
                skips = true;
                break;
            }
        }
        if skips {
            continue;
        }
        let mut owned = BorrowedIdentities::build(|_| Ok(()))?;
        owned.extend_unique(shell.faces().iter().map(crate::ids::FaceId::as_str));
        let mut reached = BorrowedIdentities::build(|_| Ok(()))?;
        let first = shell.faces()[0].as_str();
        reached.insert_unique(first, ());
        let mut pending = Vec::new();
        pending.push(first);
        while !pending.is_empty() {
            let Some(face) = pending.pop() else {
                break;
            };
            if let Some(group) = neighbors.get(face) {
                for neighbor in group.identities() {
                    if owned.contains(neighbor) && reached.insert_unique(neighbor, ()) {
                        pending.push(neighbor);
                    }
                }
            }
        }
        if reached.len() != owned.len() {
            record_finding(
                findings,
                Check::ShellTopology,
                Severity::Error,
                Some(shell.id.as_str()),
                format_args!("shell faces are disconnected through shared edges or vertices"),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
