// SPDX-License-Identifier: Apache-2.0
//! Mesh-body ownership through framed Parasolid nodes.

use std::collections::{HashMap, HashSet};

use cadmpeg_parasolid::records::node_type::{BODY, FACE, SHELL};
use cadmpeg_parasolid::records::PrimaryRecords;

const MESH: u16 = 201;

pub(super) fn bodies(records: &PrimaryRecords<'_>) -> HashSet<u32> {
    if !records.is_terminated() || !records.nodes().any(|node| node.kind() == MESH) {
        return HashSet::new();
    }
    let mut nodes = HashMap::new();
    let mut repeated = HashSet::new();
    for node in records.nodes() {
        if nodes.insert(node.index(), node).is_some() {
            repeated.insert(node.index());
        }
    }
    nodes.retain(|index, _| !repeated.contains(index));
    let mut bodies = HashSet::new();
    for mesh in nodes.values().filter(|node| node.kind() == MESH) {
        let Some(face) = mesh.pointer("owner").and_then(|index| nodes.get(&index)) else {
            continue;
        };
        if face.kind() != FACE || face.pointer("surface") != Some(mesh.index()) {
            continue;
        }
        let Some(shell) = face.pointer("shell").and_then(|index| nodes.get(&index)) else {
            continue;
        };
        if shell.kind() != SHELL {
            continue;
        }
        let Some(body) = shell.pointer("body").and_then(|index| nodes.get(&index)) else {
            continue;
        };
        if body.kind() == BODY && body.byte("state") == Some(1) {
            bodies.insert(body.index());
        }
    }
    bodies
}

#[cfg(test)]
mod tests;
