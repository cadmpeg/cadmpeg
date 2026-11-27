// SPDX-License-Identifier: Apache-2.0
//! The faces and kind of each body that the decode derives where the source
//! states no body hierarchy.

use std::collections::{HashMap, HashSet};

use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, RegionId, ShellId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{BodyKind, Region, RegionUnstated, Sense, Unstated};

use super::{topology, BodyRecord, Brep, DroppedRecord};

/// The ids of the faces the decode states.
pub(super) fn stated_face_ids(out: &Brep) -> HashSet<String> {
    out.faces
        .iter()
        .map(|face| face.id.as_str().to_owned())
        .collect()
}

/// The region of `shells`; once a shell of the region is lost, which of the
/// rest is the exterior is not stated (§5.6, Unstated children).
pub(super) fn native_region(
    id: RegionId,
    body: BodyId,
    shells: Vec<ShellId>,
    lost_shell: bool,
) -> Region {
    let mut region = Region::new(id, body, Vec::new());
    if lost_shell {
        region.unstated = Some(RegionUnstated::Unclassified { shells });
    } else {
        region.set_shells(shells);
    }
    region
}

/// The face of `refs`, the faces a native shell lists, that comes first
/// among those the decode does not state, or none when it states them all.
pub(super) fn first_lost_face(refs: &[u32], stated: &HashSet<String>) -> Option<FaceId> {
    refs.iter()
        .map(|attr| super::id_face(*attr))
        .find(|face| !stated.contains(face.as_str()))
}

/// The records of `records` that the decode drops because it states none of
/// their faces: each shell that lists a face, with the first of its faces;
/// each region of such shells, with the first of them; and each body of such
/// regions, with the first of them.
pub(super) fn dropped_without_faces(records: &[BodyRecord]) -> Vec<DroppedRecord> {
    let mut dropped = Vec::new();
    for body in records {
        let body_id = BodyId::compose(&super::body_namespace(), body.attr);
        let mut first_region = None;
        for region in &body.regions {
            let region_id = RegionId::compose(&super::region_namespace(), region.attr);
            let mut first_shell = None;
            for shell in &region.shells {
                let Some(face) = shell.refs.first() else {
                    continue;
                };
                let shell_id = ShellId::compose(&super::shell_namespace(), shell.attr);
                dropped.push(DroppedRecord::with(
                    shell_id.as_str(),
                    super::id_face(*face).as_str(),
                ));
                first_shell.get_or_insert(shell_id);
            }
            if let Some(shell) = first_shell {
                dropped.push(DroppedRecord::with(region_id.as_str(), shell.as_str()));
                first_region.get_or_insert(region_id);
            }
        }
        if let Some(region) = first_region {
            dropped.push(DroppedRecord::with(body_id.as_str(), region.as_str()));
        }
    }
    dropped
}

/// The records of the typed `hierarchies` that the decode drops, with what
/// each needs, where no dropped record names them yet: each shell the decode
/// does not state and of which it states no face, with the first of its
/// faces; each region it does not state, with the first such shell; and each
/// body it does not state, with the first such region. A shell that lists a
/// stated face belongs to a derived hierarchy and is not dropped with it.
pub(super) fn dropped_with_lost_faces(
    out: &Brep,
    hierarchies: &[crate::brep::typed::Hierarchy],
    accounted: &HashSet<&str>,
) -> Vec<DroppedRecord> {
    let stated_faces = stated_face_ids(out);
    let shells: HashSet<&str> = out.shells.iter().map(|shell| shell.id.as_str()).collect();
    let regions: HashSet<&str> = out
        .regions
        .iter()
        .map(|region| region.id.as_str())
        .collect();
    let bodies: HashSet<&str> = out.bodies.iter().map(|body| body.id.as_str()).collect();
    let mut dropped = Vec::new();
    for hierarchy in hierarchies {
        let mut lost_shells = HashMap::new();
        for shell in &hierarchy.shells {
            let id = ShellId::compose(&super::shell_namespace(), shell.attr);
            let mut attrs: Vec<u32> = hierarchy
                .faces
                .iter()
                .filter(|(_, owner)| *owner == shell.attr)
                .map(|(face, _)| *face)
                .collect();
            attrs.sort_unstable();
            let faces: Vec<FaceId> = attrs.into_iter().map(super::id_face).collect();
            let Some(face) = faces.first() else {
                continue;
            };
            if shells.contains(id.as_str())
                || faces
                    .iter()
                    .any(|face| stated_faces.contains(face.as_str()))
            {
                continue;
            }
            if !accounted.contains(id.as_str()) {
                dropped.push(DroppedRecord::with(id.as_str(), face.as_str()));
            }
            lost_shells.entry(shell.refs[6]).or_insert(id);
        }
        let mut lost_region = None;
        for region in &hierarchy.regions {
            let id = RegionId::compose(&super::region_namespace(), region.attr);
            let Some(shell) = lost_shells.get(&region.attr) else {
                continue;
            };
            if regions.contains(id.as_str()) {
                continue;
            }
            if !accounted.contains(id.as_str()) {
                dropped.push(DroppedRecord::with(id.as_str(), shell.as_str()));
            }
            lost_region.get_or_insert(id);
        }
        let body = BodyId::compose(&super::body_namespace(), hierarchy.body.attr);
        if let Some(region) = lost_region {
            if !bodies.contains(body.as_str()) && !accounted.contains(body.as_str()) {
                dropped.push(DroppedRecord::with(body.as_str(), region.as_str()));
            }
        }
    }
    dropped
}

/// The bodies and shells of the typed `hierarchies` that the decode neither
/// states nor names in a dropped record.
pub(super) fn withheld_counts(
    out: &Brep,
    hierarchies: &[crate::brep::typed::Hierarchy],
    accounted: &HashSet<&str>,
) -> (usize, usize) {
    let body_ids: HashSet<_> = out.bodies.iter().map(|body| body.id.as_str()).collect();
    let shell_ids: HashSet<_> = out.shells.iter().map(|shell| shell.id.as_str()).collect();
    let bodies = hierarchies
        .iter()
        .filter(|hierarchy| {
            let id = BodyId::compose(&super::body_namespace(), hierarchy.body.attr);
            !body_ids.contains(id.as_str()) && !accounted.contains(id.as_str())
        })
        .count();
    let shells = hierarchies
        .iter()
        .flat_map(|hierarchy| &hierarchy.shells)
        .filter(|native| {
            let id = ShellId::compose(&super::shell_namespace(), native.attr);
            !shell_ids.contains(id.as_str()) && !accounted.contains(id.as_str())
        })
        .count();
    (bodies, shells)
}

/// Marks each shell of a derived body hierarchy as not stating all its
/// faces where the decode gives up a face: the source does not say which
/// shell held it (§5.6, Unstated children).
pub(super) fn mark_derived_shells(out: &mut Brep) {
    if !out.stats.synthetic_body_grouping {
        return;
    }
    let is_face = |id: &str| id.starts_with("sldprt:brep:face#");
    let lost = out.dropped.iter().any(|record| match record {
        DroppedRecord::GivenUp {
            record, dropping, ..
        } => is_face(record) || dropping.iter().any(|id| is_face(id)),
        DroppedRecord::With { record, .. } => is_face(record),
    });
    if lost {
        for shell in &mut out.shells {
            shell.unstated = Some(Unstated::Members);
        }
    }
}

/// The note that the loop chain of the face `face` names `loop_attr`, a
/// loop the walk does not reach: a missing record or another face's loop.
pub(super) fn chain_cut_note(face: u32, loop_attr: u32, t: &topology::Tables) -> LossNote {
    let whose = match t.loops().get(&loop_attr) {
        Some(record) => format!("which {} owns", super::id_face(record.refs[2])),
        None => "which no loop record states".to_owned(),
    };
    crate::loss::SldprtLossCode::TopologyLoopChainCut.note(format!(
        "{}: its loop chain names {}, {whose}; the face keeps the loops before it and is \
         stated partial",
        super::id_face(face),
        super::id_loop(loop_attr),
    ))
}

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
