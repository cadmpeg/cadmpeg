// SPDX-License-Identifier: Apache-2.0
//! The faces and kind of each body that the decode derives where the source
//! states no body hierarchy.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::ids::{EdgeId, FaceId};
use cadmpeg_ir::topology::{BodyKind, Sense};

use super::Brep;

/// The kind of a body whose source does not classify it, from its `faces`:
/// a solid where they close, and a sheet otherwise (§5.6). The faces close
/// where none is partial and every edge on a curve is used as often in one
/// sense as in the other. The decoder states a face with no loops only on a
/// surface with no boundary.
pub(super) fn unclassified_body_kind(out: &Brep, faces: &HashSet<FaceId>) -> BodyKind {
    if faces.is_empty()
        || out
            .faces
            .iter()
            .any(|face| faces.contains(&face.id) && face.loops.is_partial())
    {
        return BodyKind::Sheet;
    }
    let loops = out
        .loops
        .iter()
        .filter(|r#loop| faces.contains(&r#loop.face))
        .map(|r#loop| &r#loop.id)
        .collect::<HashSet<_>>();
    let curved = out
        .edges
        .iter()
        .filter(|edge| edge.curve().is_some())
        .map(|edge| &edge.id)
        .collect::<HashSet<_>>();
    let mut balance = HashMap::<&EdgeId, i64>::new();
    for coedge in &out.coedges {
        if loops.contains(&coedge.owner_loop) && curved.contains(&coedge.edge) {
            *balance.entry(&coedge.edge).or_default() += match coedge.sense {
                Sense::Forward => 1,
                Sense::Reversed => -1,
            };
        }
    }
    if balance.values().all(|uses| *uses == 0) {
        BodyKind::Solid
    } else {
        BodyKind::Sheet
    }
}

pub(super) fn shell_face_components(out: &Brep, native_shell_id: &str) -> Vec<Vec<FaceId>> {
    let mut candidates = Vec::new();
    for face in &out.faces {
        if face.shell.as_str() == native_shell_id {
            candidates.push(&face.id);
        }
    }
    // A face that does not state all its loops does not state all its
    // neighbours, so the shell is not split on its edges.
    if out
        .faces
        .iter()
        .any(|face| face.shell.as_str() == native_shell_id && face.loops.is_partial())
    {
        let mut component: Vec<FaceId> = candidates.into_iter().cloned().collect();
        component.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        return vec![component];
    }
    let mut candidate_ids = HashSet::new();
    for face in &candidates {
        let key = face.as_str();
        candidate_ids.insert(key);
    }
    let mut loop_faces = HashMap::new();
    for loop_ in &out.loops {
        if candidate_ids.contains(loop_.face.as_str()) {
            let key = loop_.id.as_str();
            loop_faces.insert(key, loop_.face.as_str());
        }
    }
    let mut faces_by_edge = HashMap::<&str, HashSet<&str>>::new();
    for coedge in &out.coedges {
        if let Some(face) = loop_faces.get(coedge.owner_loop.as_str()) {
            let key = coedge.edge.as_str();
            let edge_faces = faces_by_edge.entry(key).or_default();
            edge_faces.insert(*face);
        }
    }
    let mut neighbors = HashMap::<&str, HashSet<&str>>::new();
    for edge_faces in faces_by_edge.values() {
        for &face in edge_faces {
            let adjacent = neighbors.entry(face).or_default();
            for &other in edge_faces {
                if other != face {
                    adjacent.insert(other);
                }
            }
        }
    }

    // The walk moves over borrowed keys, so the face each key names is looked
    // up rather than rebuilt from its text.
    let mut faces_by_key = HashMap::new();
    for face in &candidates {
        let key = face.as_str();
        faces_by_key.insert(key, *face);
    }
    let mut assigned = HashSet::new();
    let mut components = Vec::new();
    for face in &candidates {
        let key = face.as_str();
        if !assigned.insert(key) {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = Vec::new();
        pending.push(key);
        while let Some(current) = pending.pop() {
            let Some(face) = faces_by_key.get(current) else {
                continue;
            };
            component.push((*face).clone());
            for &neighbor in neighbors.get(current).into_iter().flatten() {
                if assigned.insert(neighbor) {
                    pending.push(neighbor);
                }
            }
        }
        component.sort_by(|left, right| left.as_str().cmp(right.as_str()));
        components.push(component);
    }
    components
}
