// SPDX-License-Identifier: Apache-2.0
//! The body hierarchy of a decoded graph: the bodies the decode derives where
//! no body record links up with the faces, the body, region, and shell
//! records it states in their partial form where their faces are lost, and
//! the count of the typed ownership nodes it withholds.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::annotations::{AnnotationBuilder, AnnotationKey, StreamHandle};
use cadmpeg_ir::ids::{BodyId, EdgeId, FaceId, RegionId, ShellId};
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::topology::{Body, BodyKind, Region, RegionUnstated, Sense, Shell, Unstated};
use cadmpeg_ir::{Exactness, StreamName};

use super::{topology, BodyRecord, Brep, DroppedRecord, RegionRecord, ShellRecord};

/// The ids of the faces the decode states.
pub(super) fn stated_face_ids(out: &Brep) -> HashSet<String> {
    out.faces
        .iter()
        .map(|face| face.id.as_str().to_owned())
        .collect()
}

/// The face of `refs`, the faces a native shell lists, that comes first
/// among those the decode does not state, or none when it states them all.
pub(super) fn first_lost_face(refs: &[u32], stated: &HashSet<String>) -> Option<FaceId> {
    refs.iter()
        .map(|attr| super::id_face(*attr))
        .find(|face| !stated.contains(face.as_str()))
}

/// States, in their partial form, the shells, regions, and bodies of
/// `records` that the decode does not state, and marks each stated shell that
/// lists a face the decode does not state (§3.3, §5.6). A shell none of whose
/// faces is stated holds no face and says so; a stated region that gains such
/// a shell leaves its shells unclassified, since which is the exterior is no
/// longer stated; a body keeps its stored kind. A shell that lists a stated
/// face is the shell that holds that face, so nothing of its body is stated
/// beside it.
pub(super) fn state_unbuilt_records(
    ctx: &DecodeContext<'_>,
    out: &mut Brep,
    annotations: &mut AnnotationBuilder,
    records: &[BodyRecord],
    stream: &StreamName,
) -> Result<(), CodecError> {
    let face_shells: HashMap<FaceId, ShellId> = out
        .faces
        .iter()
        .map(|face| (face.id.clone(), face.shell.clone()))
        .collect();
    let built_shells: HashSet<ShellId> = out.shells.iter().map(|shell| shell.id.clone()).collect();
    let built_regions: HashSet<RegionId> =
        out.regions.iter().map(|region| region.id.clone()).collect();
    let built_bodies: HashSet<BodyId> = out.bodies.iter().map(|body| body.id.clone()).collect();
    let source = StreamHandle::new(stream.clone());
    let mut annotate = |key: AnnotationKey, offset: usize, tag: &str| {
        annotations.note(key.clone(), &source, u64_from_index(offset), Some(tag));
        annotations.exactness(key, Exactness::ByteExact);
    };
    let mut marked = HashSet::new();
    for record in records {
        let body_id = BodyId::compose(&super::body_namespace(), record.attr);
        let mut new_shells = HashMap::<u32, Vec<ShellId>>::new();
        for region in &record.regions {
            let region_id = RegionId::compose(&super::region_namespace(), region.attr);
            for shell in &region.shells {
                let id = ShellId::compose(&super::shell_namespace(), shell.attr);
                let faces: Vec<FaceId> = shell
                    .refs
                    .iter()
                    .map(|attr| super::id_face(*attr))
                    .collect();
                let mut holders: Vec<ShellId> = faces
                    .iter()
                    .filter_map(|face| face_shells.get(face).cloned())
                    .collect();
                if built_shells.contains(&id) {
                    holders.push(id.clone());
                }
                if !holders.is_empty() {
                    if faces.iter().any(|face| !face_shells.contains_key(face)) {
                        marked.extend(holders);
                    }
                    continue;
                }
                if faces.is_empty() {
                    continue;
                }
                super::admit_brep_entity(ctx)?;
                annotate(AnnotationKey::from(&id), shell.offset, "00_51_shell");
                let mut stated = Shell::new(id.clone(), region_id.clone());
                stated.unstated = Some(Unstated::Members);
                out.shells.push(stated);
                new_shells.entry(region.attr).or_default().push(id);
            }
        }
        let body_new = !built_bodies.contains(&body_id) && !new_shells.is_empty();
        for region in &record.regions {
            let id = RegionId::compose(&super::region_namespace(), region.attr);
            let added = new_shells.remove(&region.attr).unwrap_or_default();
            if built_regions.contains(&id) {
                if let (false, Some(stated)) = (
                    added.is_empty(),
                    out.regions.iter_mut().find(|stated| stated.id == id),
                ) {
                    let mut shells: Vec<ShellId> = stated.stated_shells().cloned().collect();
                    shells.extend(added);
                    stated.set_shells(Vec::new());
                    stated.unstated = Some(RegionUnstated::Unclassified { shells });
                }
                continue;
            }
            if added.is_empty() && !body_new {
                continue;
            }
            super::admit_brep_entity(ctx)?;
            annotate(AnnotationKey::from(&id), region.offset, "00_51_region");
            out.regions
                .push(region_of(id, body_id.clone(), added, true));
        }
        if body_new {
            super::admit_brep_entity(ctx)?;
            annotate(AnnotationKey::from(&body_id), record.offset, "00_51_body");
            note_body_precision(out, &body_id, Some(record.linear_resolution));
            out.bodies.push(Body {
                id: body_id,
                kind: record.kind,
                transform: None,
                name: None,
                color: None,
                visible: None,
                native_ref: None,
                unstated: None,
            });
        }
    }
    for shell in &mut out.shells {
        if marked.contains(&shell.id) {
            shell.unstated = Some(Unstated::Members);
        }
    }
    out.shells.sort_by(|a, b| a.id.cmp(&b.id));
    out.regions.sort_by(|a, b| a.id.cmp(&b.id));
    out.bodies.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(())
}

/// The region `id` of `body` bounded by `shells`, the exterior first. Where
/// `faceless`, a shell holds no stated face, so which shell is the exterior
/// is not stated, and a region of more than one shell leaves them
/// unclassified (§5.6, Unstated children).
pub(super) fn region_of(
    id: RegionId,
    body: BodyId,
    shells: Vec<ShellId>,
    faceless: bool,
) -> Region {
    if faceless && shells.len() > 1 {
        let mut region = Region::new(id, body, Vec::new());
        region.unstated = Some(RegionUnstated::Unclassified { shells });
        region
    } else {
        Region::new(id, body, shells)
    }
}

/// The body records of the typed `hierarchies`, sorted by attribute.
pub(super) fn body_records_of(hierarchies: &[crate::brep::typed::Hierarchy]) -> Vec<BodyRecord> {
    let mut records = Vec::new();
    for hierarchy in hierarchies {
        let mut body_refs = Vec::new();
        for attr in hierarchy
            .regions
            .iter()
            .map(|region| region.attr)
            .chain(hierarchy.shells.iter().map(|shell| shell.attr))
            .chain(hierarchy.faces.iter().map(|(face, _)| *face))
        {
            body_refs.push(attr);
        }
        body_refs.sort_unstable();
        body_refs.dedup();
        let mut regions = Vec::new();
        for region in &hierarchy.regions {
            let mut shells = Vec::new();
            for shell in hierarchy
                .shells
                .iter()
                .filter(|shell| shell.refs[6] == region.attr)
            {
                let mut refs = Vec::new();
                for face_attr in hierarchy
                    .faces
                    .iter()
                    .filter(|(_, shell_attr)| *shell_attr == shell.attr)
                    .map(|(face_attr, _)| *face_attr)
                {
                    refs.push(face_attr);
                }
                refs.sort_unstable();
                refs.dedup();
                shells.push(ShellRecord {
                    attr: shell.attr,
                    offset: shell.offset,
                    refs,
                });
            }
            shells.sort_by_key(|record| record.attr);
            regions.push((
                region.kind,
                RegionRecord {
                    attr: region.attr,
                    offset: region.offset,
                    shells,
                },
            ));
        }
        let owns_faces =
            |record: &RegionRecord| record.shells.iter().any(|shell| !shell.refs.is_empty());
        let body_has_material = regions
            .iter()
            .any(|(kind, record)| *kind == super::RegionKind::Solid || owns_faces(record));
        // The faces of the neighbouring region's shell bound a void region that
        // owns no faces, so that region has no neutral boundary of its own.
        regions.retain(|(kind, record)| {
            !body_has_material || *kind == super::RegionKind::Solid || owns_faces(record)
        });
        let mut regions: Vec<_> = regions.into_iter().map(|(_, record)| record).collect();
        regions.sort_by_key(|record| record.attr);
        records.push(BodyRecord {
            attr: hierarchy.body.attr,
            kind: hierarchy.body.kind,
            linear_resolution: hierarchy.body.linear_resolution,
            refs: body_refs,
            offset: hierarchy.body.offset,
            regions,
        });
    }
    records.sort_by_key(|record| record.attr);
    records
}

/// Records the linear precision of `body`, or charges its unusable resolution.
pub(super) fn note_body_precision(out: &mut Brep, body: &BodyId, res_linear: Option<f64>) {
    match res_linear.and_then(cadmpeg_parasolid::precision::linear_precision) {
        Some(linear) => out.body_linear_precisions.push(linear),
        None => {
            let code = if res_linear.is_some() {
                crate::loss::SldprtLossCode::BodyResolutionInvalid
            } else {
                crate::loss::SldprtLossCode::BodyResolutionUnread
            };
            out.losses.push(
                code.note(cadmpeg_parasolid::precision::unusable_resolution_message(
                    body.as_str(),
                    res_linear,
                )),
            );
        }
    }
}

/// The bodies, regions, and shells of the typed `hierarchies` that the decode
/// neither states nor names in a dropped record. The regions are those of
/// `records`, the body records of `hierarchies`.
pub(super) fn withheld_counts(
    out: &Brep,
    hierarchies: &[crate::brep::typed::Hierarchy],
    records: &[BodyRecord],
    accounted: &HashSet<String>,
) -> (usize, usize, usize) {
    let body_ids: HashSet<_> = out.bodies.iter().map(|body| body.id.as_str()).collect();
    let region_ids: HashSet<_> = out
        .regions
        .iter()
        .map(|region| region.id.as_str())
        .collect();
    let shell_ids: HashSet<_> = out.shells.iter().map(|shell| shell.id.as_str()).collect();
    let bodies = hierarchies
        .iter()
        .filter(|hierarchy| {
            let id = BodyId::compose(&super::body_namespace(), hierarchy.body.attr);
            !body_ids.contains(id.as_str()) && !accounted.contains(id.as_str())
        })
        .count();
    let regions = records
        .iter()
        .flat_map(|record| &record.regions)
        .filter(|region| {
            let id = RegionId::compose(&super::region_namespace(), region.attr);
            !region_ids.contains(id.as_str()) && !accounted.contains(id.as_str())
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
    (bodies, regions, shells)
}

/// Marks each shell of a derived body hierarchy as not stating all its
/// faces where the decode gives up a face, or where `typed_face_unstated`
/// says a typed FACE node is not stated: the source does not say which shell
/// held it (§5.6, Unstated children).
pub(super) fn mark_derived_shells(out: &mut Brep, typed_face_unstated: bool) {
    if !out.stats.synthetic_body_grouping {
        return;
    }
    let is_face = |id: &str| id.starts_with("sldprt:brep:face#");
    let lost = typed_face_unstated
        || out.dropped.iter().any(|record| match record {
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
