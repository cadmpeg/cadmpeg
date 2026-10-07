// SPDX-License-Identifier: Apache-2.0
//! Historical topology projection, membership, boundary and carrier queries.

use super::selection;
use super::{same_axis_line, stable_ref, UniqueIndex, UniqueLookup};
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalEdge,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalTopology, AsmHistoricalTopologyDelta, AsmHistory,
};
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

#[derive(Debug)]
pub(super) struct HistoricalTopologyEntitySlots<'ctx> {
    pub(super) slots: HashSet<i64>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn topology_entity_slots<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<HistoricalTopologyEntitySlots<'ctx>, cadmpeg_core::CodecError> {
    let operation = "index F3D historical topology slots";
    let mut storage = ctx.reserve_scoped(0, operation)?;
    let mut slots = HashSet::new();
    for slot in ctx.admit_iter(&topology.bodies, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.regions, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.shells, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.faces, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.loops, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.coedges, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.edges, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.vertices, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.points, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.surfaces, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.curves, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    for slot in ctx.admit_iter(&topology.pcurves, operation)? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut slots, *slot, operation)
                .map(|_| ())
        })?;
    }
    Ok(HistoricalTopologyEntitySlots {
        slots,
        _storage: storage,
    })
}

/// One topology's faces, vertices, edge endpoints and face boundary edges,
/// indexed once for vertex recipe resolution.
pub(super) struct BoundaryVertexIndex<'ctx> {
    boundaries: FaceBoundaryEdgeIndex<'ctx>,
    pub(super) faces: HashSet<i64>,
    pub(super) vertices: HashSet<i64>,
    /// Endpoints of each edge; an edge listed twice has none.
    edge_vertices: HashMap<i64, Option<(i64, i64)>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn boundary_vertex_index<'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<BoundaryVertexIndex<'ctx>, cadmpeg_core::CodecError> {
    let boundaries = face_boundary_edge_index(decode, topology)?;
    let operation = "index F3D boundary vertex topology";
    let mut storage = decode.reserve_scoped(0, operation)?;
    let faces = storage
        .with_storage(|| decode.collect_hash_set(topology.faces.iter().copied(), operation))?;
    let vertices = storage
        .with_storage(|| decode.collect_hash_set(topology.vertices.iter().copied(), operation))?;
    let mut edge_vertices = HashMap::new();
    for edge in decode.admit_iter(&topology.edge_vertices, operation)? {
        if let Some(endpoints) = decode.get_mut_hash_map(
            &mut edge_vertices,
            &edge.edge,
            "find F3D history edge vertices",
        )? {
            *endpoints = None;
            continue;
        }
        storage.with_storage(|| {
            decode
                .insert_hash_map(
                    &mut edge_vertices,
                    edge.edge,
                    Some((edge.start_vertex, edge.end_vertex)),
                    operation,
                )
                .map(|_| ())
        })?;
    }
    Ok(BoundaryVertexIndex {
        boundaries,
        faces,
        vertices,
        edge_vertices,
        _storage: storage,
    })
}

pub(super) fn boundary_vertices_for_faces(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: impl IntoIterator<Item = Result<Option<i64>, cadmpeg_core::CodecError>>,
    index: &BoundaryVertexIndex<'_>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    let mut vertices = BTreeSet::new();
    let mut faces = faces.into_iter();
    while let Some(face) = decode.next_charged(&mut faces, "scan F3D boundary vertex faces")? {
        let Some(face) = face? else {
            continue;
        };
        let Some(edges) = decode.get_btree_map(
            &index.boundaries.boundaries,
            &face,
            "find F3D boundary vertex face",
        )?
        else {
            return Ok(None);
        };
        let mut history_source = IntoIterator::into_iter(&edges.edges);
        while let Some(edge_slot) =
            decode.next_charged(&mut history_source, "scan F3D boundary vertex edges")?
        {
            let Some(&Some((start, end))) = decode.get_hash_map(
                &index.edge_vertices,
                edge_slot,
                "find F3D history edge vertices",
            )?
            else {
                return Ok(None);
            };
            if !decode.contains_hash_set(&index.vertices, &start, "find F3D history vertices")?
                || !decode.contains_hash_set(&index.vertices, &end, "find F3D history vertices")?
            {
                return Ok(None);
            }
            decode.insert_btree_set(&mut vertices, start, "collect F3D boundary vertices")?;
            decode.insert_btree_set(&mut vertices, end, "collect F3D boundary vertices")?;
        }
    }
    Ok((!vertices.is_empty()).then_some(vertices))
}

/// One topology's entity occurrence counts and body, region and shell
/// relations, indexed for complete-body face queries. A relation owner or
/// member listed twice resolves to none.
pub(super) struct BodyFaceIndex<'a, 'ctx> {
    /// Occurrences of each body, region, shell and face slot, by family.
    counts: [HashMap<i64, usize>; 4],
    body_regions: RelationIndex<'a>,
    region_shells: RelationIndex<'a>,
    shell_faces: RelationIndex<'a>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

struct RelationIndex<'a> {
    members_by_owner: HashMap<i64, Option<&'a [i64]>>,
    owner_by_member: HashMap<i64, Option<i64>>,
}

pub(super) fn body_face_index<'a, 'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &'a AsmHistoricalTopology,
) -> Result<BodyFaceIndex<'a, 'ctx>, cadmpeg_core::CodecError> {
    let mut storage = decode.reserve_scoped(0, "index F3D complete body entity counts")?;
    let mut occurrence_counts = |slots: &[i64]| {
        let mut counts = HashMap::new();
        for &slot in decode.admit_iter(slots, "scan F3D topology member slots")? {
            if let Some(count) =
                decode.get_mut_hash_map(&mut counts, &slot, "find F3D history counts")?
            {
                *count += 1;
                continue;
            }
            storage.with_storage(|| {
                decode
                    .insert_hash_map(
                        &mut counts,
                        slot,
                        1_usize,
                        "index F3D complete body entity counts",
                    )
                    .map(|_| ())
            })?;
        }
        Ok::<_, cadmpeg_core::CodecError>(counts)
    };
    let counts = [
        occurrence_counts(&topology.bodies)?,
        occurrence_counts(&topology.regions)?,
        occurrence_counts(&topology.shells)?,
        occurrence_counts(&topology.faces)?,
    ];
    let mut relation_index = |relations: &'a [AsmHistoricalRelation]| {
        let mut members_by_owner = HashMap::new();
        let mut owner_by_member = HashMap::new();
        for relation in decode.admit_iter(relations, "scan F3D complete body relations")? {
            if let Some(members) = decode.get_mut_hash_map(
                &mut members_by_owner,
                &relation.owner_ref,
                "find F3D history members by owner",
            )? {
                *members = None;
            } else {
                storage.with_storage(|| {
                    decode
                        .insert_hash_map(
                            &mut members_by_owner,
                            relation.owner_ref,
                            Some(relation.member_refs.as_slice()),
                            "index F3D complete body relation owners",
                        )
                        .map(|_| ())
                })?;
            }
            for &member in
                decode.admit_iter(&relation.member_refs, "scan F3D relation member refs")?
            {
                if let Some(owner) = decode.get_mut_hash_map(
                    &mut owner_by_member,
                    &member,
                    "find F3D history owner by member",
                )? {
                    *owner = None;
                    continue;
                }
                storage.with_storage(|| {
                    decode
                        .insert_hash_map(
                            &mut owner_by_member,
                            member,
                            Some(relation.owner_ref),
                            "index F3D complete body relation members",
                        )
                        .map(|_| ())
                })?;
            }
        }
        Ok::<_, cadmpeg_core::CodecError>(RelationIndex {
            members_by_owner,
            owner_by_member,
        })
    };
    let body_regions = relation_index(&topology.body_regions)?;
    let region_shells = relation_index(&topology.region_shells)?;
    let shell_faces = relation_index(&topology.shell_faces)?;
    Ok(BodyFaceIndex {
        counts,
        body_regions,
        region_shells,
        shell_faces,
        _storage: storage,
    })
}

pub(super) fn complete_body_face_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    index: &BodyFaceIndex<'_, '_>,
    body: i64,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    macro_rules! complete_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let [body_counts, region_counts, shell_counts, face_counts] = &index.counts;
    let count_is_one = |counts: &HashMap<i64, usize>, slot: i64| {
        Ok::<_, cadmpeg_core::CodecError>(
            decode
                .get_hash_map(counts, &slot, "find F3D history counts")?
                .copied()
                == Some(1),
        )
    };
    if !count_is_one(body_counts, body)? {
        return Ok(None);
    }
    let regions = complete_some!(decode
        .get_hash_map(
            &index.body_regions.members_by_owner,
            &body,
            "find F3D history body regions members by owner"
        )?
        .copied()
        .flatten());
    if regions.is_empty() {
        return Ok(None);
    }
    let mut storage = decode.reserve_scoped(0, "collect F3D complete body faces")?;
    let mut seen_regions = HashSet::new();
    let mut seen_shells = HashSet::new();
    let mut seen_faces = BTreeSet::new();
    let mut history_source = IntoIterator::into_iter(regions);
    while let Some(&region) =
        decode.next_charged(&mut history_source, "scan F3D complete body regions")?
    {
        if !storage.with_storage(|| {
            decode.insert_hash_set(
                &mut seen_regions,
                region,
                "collect F3D complete body regions",
            )
        })? || !count_is_one(region_counts, region)?
            || decode
                .get_hash_map(
                    &index.body_regions.owner_by_member,
                    &region,
                    "find F3D history body regions owner by member",
                )?
                .copied()
                .flatten()
                != Some(body)
        {
            return Ok(None);
        }
        let shells = complete_some!(decode
            .get_hash_map(
                &index.region_shells.members_by_owner,
                &region,
                "find F3D history region shells members by owner"
            )?
            .copied()
            .flatten());
        if shells.is_empty() {
            return Ok(None);
        }
        let mut history_source = IntoIterator::into_iter(shells);
        while let Some(&shell) =
            decode.next_charged(&mut history_source, "scan F3D complete body shells")?
        {
            if !storage.with_storage(|| {
                decode.insert_hash_set(&mut seen_shells, shell, "collect F3D complete body shells")
            })? || !count_is_one(shell_counts, shell)?
                || decode
                    .get_hash_map(
                        &index.region_shells.owner_by_member,
                        &shell,
                        "find F3D history region shells owner by member",
                    )?
                    .copied()
                    .flatten()
                    != Some(region)
            {
                return Ok(None);
            }
            let faces = complete_some!(decode
                .get_hash_map(
                    &index.shell_faces.members_by_owner,
                    &shell,
                    "find F3D history shell faces members by owner"
                )?
                .copied()
                .flatten());
            if faces.is_empty() {
                return Ok(None);
            }
            let mut history_source = IntoIterator::into_iter(faces);
            while let Some(&face) =
                decode.next_charged(&mut history_source, "scan F3D complete body faces")?
            {
                if !decode.insert_scoped_btree_set(
                    &mut storage,
                    &mut seen_faces,
                    face,
                    "collect F3D complete body faces",
                    "collect F3D complete body faces",
                )? || !count_is_one(face_counts, face)?
                    || decode
                        .get_hash_map(
                            &index.shell_faces.owner_by_member,
                            &face,
                            "find F3D history shell faces owner by member",
                        )?
                        .copied()
                        .flatten()
                        != Some(shell)
                {
                    return Ok(None);
                }
            }
        }
    }
    // The ordered set yields the face slots already sorted.
    let mut faces =
        decode.collection_vec(seen_faces.len(), "collect F3D complete body face slots")?;
    for face in decode.admit_iter(seen_faces, "scan F3D complete body face slots")? {
        faces.push(face);
    }
    Ok((!faces.is_empty()).then_some(faces))
}

pub(super) fn historical_face_support_contexts(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    preceding_topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceSupportContext>,
    cadmpeg_core::CodecError,
> {
    if candidates.is_empty() {
        return Ok(Vec::new());
    }
    let mut preceding_storage = decode.reserve_scoped(0, "index F3D historical support faces")?;
    let mut preceding_faces = HashSet::new();
    for face in decode.admit_iter(
        &preceding_topology.faces,
        "scan F3D preceding topology faces",
    )? {
        preceding_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut preceding_faces,
                *face,
                "index F3D historical support faces",
            )
        })?;
    }
    let preceding_bindings = UniqueIndex::new(
        &preceding_topology.face_surfaces,
        |_, binding| Ok(Some(binding.entity)),
        "index F3D historical support bindings",
    );
    let history_bindings = std::cell::OnceCell::new();
    let mut history_binding_storage =
        decode.reserve_scoped(0, "index F3D history support states")?;
    let mut contexts = Vec::new();
    let mut history_source = IntoIterator::into_iter(candidates);
    'candidates: while let Some(candidate) =
        decode.next_charged(&mut history_source, "scan F3D face support candidates")?
    {
        let Some(active_face_slot) = stable_ref(decode, candidate.as_str())? else {
            continue;
        };
        let surface_slot = match preceding_bindings.get_entry(decode, &active_face_slot)? {
            UniqueLookup::Value(binding) => binding.carrier,
            UniqueLookup::Repeated => continue,
            UniqueLookup::Absent => {
                let mut carrier = None;
                let mut carrier_is_ambiguous = false;
                let bindings = match history_bindings.get() {
                    Some(bindings) => bindings,
                    None => {
                        let built = history_binding_storage.with_storage(|| {
                            decode.collect_indexed_vec(
                                history.states.len(),
                                "index F3D history support states",
                                |index| {
                                    Ok(history.states[index].topology().map(|topology| {
                                        UniqueIndex::new(
                                            &topology.face_surfaces,
                                            |_, binding| Ok(Some(binding.entity)),
                                            "index F3D history support bindings",
                                        )
                                    }))
                                },
                            )
                        })?;
                        history_bindings.get_or_init(|| built)
                    }
                };
                let mut history_source = IntoIterator::into_iter(bindings);
                while let Some(bindings) =
                    decode.next_charged(&mut history_source, "scan F3D history support states")?
                {
                    let Some(bindings) = bindings else {
                        continue;
                    };
                    // A repeated entity binding rejects the candidate. The lazy
                    // index keeps the repeated-key marker separate from absence.
                    match bindings.get_entry(decode, &active_face_slot)? {
                        UniqueLookup::Value(binding) => match carrier {
                            None => carrier = Some(binding.carrier),
                            Some(known) if known != binding.carrier => carrier_is_ambiguous = true,
                            Some(_) => {}
                        },
                        UniqueLookup::Repeated => continue 'candidates,
                        UniqueLookup::Absent => {}
                    }
                }
                let Some(carrier) = carrier.filter(|_| !carrier_is_ambiguous) else {
                    continue;
                };
                carrier
            }
        };
        let mut candidate_storage =
            decode.reserve_scoped(0, "collect F3D historical support contexts")?;
        let mut preceding_face_slots = Vec::new();
        for binding in decode.admit_iter(
            &preceding_topology.face_surfaces,
            "scan F3D historical support carriers",
        )? {
            if binding.carrier == surface_slot
                && decode.contains_hash_set(
                    &preceding_faces,
                    &binding.entity,
                    "find F3D historical support face",
                )?
            {
                candidate_storage.with_storage(|| {
                    decode.push_vec(
                        &mut preceding_face_slots,
                        binding.entity,
                        "collect F3D historical support face slots",
                    )
                })?;
            }
        }
        decode.sort_unstable_by(
            &mut preceding_face_slots,
            |value| value,
            Ord::cmp,
            "sort F3D historical support face slots",
        )?;
        decode.dedup_vec(
            &mut preceding_face_slots,
            "deduplicate F3D historical support face slots",
        )?;
        if preceding_face_slots.is_empty() {
            continue;
        }
        let mut changed_preceding_face_slots = Vec::new();
        for face in decode.admit_iter(
            &preceding_face_slots,
            "scan F3D historical support face slots",
        )? {
            if decode.contains_hash_set(changed_faces, face, "find F3D changed support face")? {
                candidate_storage.with_storage(|| {
                    decode.push_vec(
                        &mut changed_preceding_face_slots,
                        *face,
                        "collect F3D changed support face slots",
                    )
                })?;
            }
        }
        let preceding_face_boundaries = candidate_storage.with_storage(|| {
            face_boundary_contexts_for_slots(decode, &preceding_face_slots, preceding_topology)
        })?;
        candidate_storage.commit()?;
        decode.push_vec(
            &mut contexts,
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot,
                surface_slot,
                preceding_face_slots,
                preceding_face_boundaries,
                changed_preceding_face_slots,
            },
            "collect F3D historical support contexts",
        )?;
    }
    Ok(contexts)
}

pub(super) fn face_boundary_edges(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &AsmHistoricalTopology,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut storage = decode.reserve_scoped(0, "index F3D boundary topology")?;
    let mut face_slots = HashSet::new();
    for face in decode.admit_iter(faces, "scan F3D boundary faces")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            storage.with_storage(|| {
                decode.insert_hash_set(&mut face_slots, face_slot, "index F3D boundary faces")
            })?;
        }
    }
    let mut loops = HashSet::new();
    for relation in decode.admit_iter(&topology.face_loops, "scan F3D boundary face loops")? {
        if !decode.contains_hash_set(&face_slots, &relation.owner_ref, "find F3D boundary face")? {
            continue;
        }
        for loop_slot in
            decode.admit_iter(&relation.member_refs, "scan F3D relation member refs")?
        {
            storage.with_storage(|| {
                decode.insert_hash_set(&mut loops, *loop_slot, "index F3D boundary loops")
            })?;
        }
    }
    let mut coedges = HashSet::new();
    for relation in decode.admit_iter(&topology.loop_coedges, "scan F3D boundary loop coedges")? {
        if !decode.contains_hash_set(&loops, &relation.owner_ref, "find F3D boundary loop")? {
            continue;
        }
        for coedge in decode.admit_iter(&relation.member_refs, "scan F3D relation member refs")? {
            storage.with_storage(|| {
                decode.insert_hash_set(&mut coedges, *coedge, "index F3D boundary coedges")
            })?;
        }
    }
    let mut edges = Vec::new();
    for coedge in decode.admit_iter(
        &topology.coedge_topology,
        "scan F3D boundary coedge topology",
    )? {
        if decode.contains_hash_set(&coedges, &coedge.coedge, "find F3D boundary coedge")? {
            decode.push_vec(&mut edges, coedge.edge, "collect F3D boundary edges")?;
        }
    }
    decode.sort_unstable_by(
        &mut edges,
        |value| value,
        Ord::cmp,
        "sort F3D boundary edges",
    )?;
    decode.dedup_vec(&mut edges, "deduplicate F3D boundary edges")?;
    Ok(edges)
}

pub(super) fn collect_reference_edge_sets(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
    topology: &AsmHistoricalTopology,
) -> Result<Vec<Vec<i64>>, cadmpeg_core::CodecError> {
    let mut sets = Vec::new();
    for faces in decode.admit_iter(reference_faces, "scan F3D recipe reference faces")? {
        let (faces, _face_storage) = decode
            .with_scoped_storage("collect F3D recipe reference faces", || {
                selection::faces_in_topology(decode, faces, topology)
            })?;
        let edges = face_boundary_edges(decode, &faces, topology)?;

        decode.reserve_vec(&mut sets, 1, "collect F3D reference edge sets")?;
        sets.push(edges);
    }
    Ok(sets)
}

fn face_boundary_contexts(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &AsmHistoricalTopology,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>,
    cadmpeg_core::CodecError,
> {
    let mut face_slots_storage = decode.reserve_scoped(0, "collect F3D boundary face slots")?;
    let mut face_slots = Vec::new();
    for face in decode.admit_iter(faces, "scan F3D boundary face slots")? {
        if let Some(face_slot) = stable_ref(decode, face.as_str())? {
            face_slots_storage.with_storage(|| {
                decode.push_vec(
                    &mut face_slots,
                    face_slot,
                    "collect F3D boundary face slots",
                )
            })?;
        }
    }
    face_boundary_contexts_for_slots(decode, &face_slots, topology)
}

pub(super) fn face_boundary_contexts_for_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    face_slots: &[i64],
    topology: &AsmHistoricalTopology,
) -> Result<
    Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext>,
    cadmpeg_core::CodecError,
> {
    let face_relations = UniqueIndex::new(
        &topology.face_loops,
        |_, relation| Ok(Some(relation.owner_ref)),
        "index F3D boundary face loops",
    );
    let loop_relations = UniqueIndex::new(
        &topology.loop_coedges,
        |_, relation| Ok(Some(relation.owner_ref)),
        "index F3D boundary loop coedges",
    );
    let coedge_edges = UniqueIndex::new(
        &topology.coedge_topology,
        |_, coedge| Ok(Some(coedge.coedge)),
        "index F3D boundary coedge edges",
    );
    let mut contexts = Vec::new();
    let mut history_source = IntoIterator::into_iter(face_slots);
    'faces: while let Some(face_slot) =
        decode.next_charged(&mut history_source, "scan F3D boundary face slots")?
    {
        let mut face_storage = decode.reserve_scoped(0, "collect F3D face boundary loops")?;
        let Some(face_relation) = face_relations.get(decode, face_slot)? else {
            continue;
        };
        let mut loops = Vec::new();
        let mut history_source = IntoIterator::into_iter(&face_relation.member_refs);
        while let Some(loop_slot) =
            decode.next_charged(&mut history_source, "scan F3D face relation member refs")?
        {
            let Some(loop_relation) = loop_relations.get(decode, loop_slot)? else {
                continue 'faces;
            };
            let mut coedges_storage = decode.reserve_scoped(0, "collect F3D face loop coedges")?;
            let mut coedges = Vec::new();
            let mut history_source = IntoIterator::into_iter(&loop_relation.member_refs);
            while let Some(coedge_slot) =
                decode.next_charged(&mut history_source, "scan F3D loop relation member refs")?
            {
                let Some(coedge) = coedge_edges.get(decode, coedge_slot)? else {
                    continue 'faces;
                };
                let edge_slot = coedge.edge;
                coedges_storage.with_storage(|| {
                    decode.reserve_vec(&mut coedges, 1, "collect F3D face loop coedges")
                })?;
                coedges.push(
                    crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                        coedge_slot: *coedge_slot,
                        edge_slot,
                    },
                );
            }
            let boundary = face_storage
                .with_storage(|| historical_loop_boundary(decode, coedges, topology))?;
            if matches!(
                boundary,
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(
                    _
                )
            ) {
                face_storage.with_storage(|| coedges_storage.commit())?;
            }

            face_storage.with_storage(|| {
                decode.reserve_vec(&mut loops, 1, "collect F3D face boundary loops")
            })?;
            loops.push(
                crate::records::topology::historical_context::DesignHistoricalFaceLoopContext {
                    loop_slot: *loop_slot,
                    boundary,
                },
            );
        }

        face_storage.commit()?;
        decode.reserve_vec(&mut contexts, 1, "collect F3D face boundary contexts")?;
        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext {
                face_slot: *face_slot,
                loops,
            },
        );
    }
    Ok(contexts)
}

fn historical_loop_boundary(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    coedges: Vec<crate::records::topology::historical_context::DesignHistoricalLoopCoedge>,
    topology: &AsmHistoricalTopology,
) -> Result<
    crate::records::topology::historical_context::DesignHistoricalLoopBoundary,
    cadmpeg_core::CodecError,
> {
    use crate::records::topology::historical_context::{
        DesignHistoricalLoopBoundary, DesignHistoricalLoopPoint, DesignHistoricalLoopPosition,
        DesignHistoricalLoopVertex,
    };
    let edges = UniqueIndex::new(
        &topology.edge_vertices,
        |_, edge| Ok(Some(edge.edge)),
        "index F3D loop edge vertices",
    );
    let bindings = UniqueIndex::new(
        &topology.vertex_points,
        |_, binding| Ok(Some(binding.entity)),
        "index F3D loop vertex points",
    );
    let values = UniqueIndex::new(
        &topology.point_positions,
        |_, value| Ok(Some(value.point)),
        "index F3D loop point positions",
    );
    let mut vertices_storage = decode.reserve_scoped(0, "collect F3D loop vertices")?;
    let mut vertices = Vec::new();
    let mut history_source = IntoIterator::into_iter(&coedges).enumerate();
    while let Some((ordinal, coedge)) =
        decode.next_charged(&mut history_source, "scan F3D loop coedges")?
    {
        let previous = coedges[(ordinal + coedges.len() - 1) % coedges.len()].edge_slot;
        let (Some(previous), Some(current)) = (
            edges.get(decode, &previous)?,
            edges.get(decode, &coedge.edge_slot)?,
        ) else {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        };
        let previous = [previous.start_vertex, previous.end_vertex];
        let current = [current.start_vertex, current.end_vertex];
        let mut shared = previous
            .into_iter()
            .filter(|vertex| current.contains(vertex));
        let Some(vertex_slot) = shared.next() else {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        };
        if shared.any(|vertex| vertex != vertex_slot) {
            return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
        }
        vertices_storage.with_storage(|| {
            decode.push_vec(
                &mut vertices,
                DesignHistoricalLoopVertex {
                    coedge: coedge.clone(),
                    vertex_slot,
                },
                "collect F3D loop vertices",
            )
        })?;
    }
    if vertices.is_empty() {
        return Ok(DesignHistoricalLoopBoundary::Coedges(coedges));
    }
    let mut points_storage = decode.reserve_scoped(0, "collect F3D loop points")?;
    let mut points = Vec::new();
    let mut history_source = IntoIterator::into_iter(&vertices);
    while let Some(vertex) = decode.next_charged(&mut history_source, "scan F3D vertices")? {
        let Some(binding) = bindings.get(decode, &vertex.vertex_slot)? else {
            vertices_storage.commit()?;
            return Ok(DesignHistoricalLoopBoundary::Vertices(vertices));
        };
        points_storage.with_storage(|| {
            decode.push_vec(
                &mut points,
                DesignHistoricalLoopPoint {
                    vertex: vertex.clone(),
                    point_slot: binding.carrier,
                },
                "collect F3D loop points",
            )
        })?;
    }
    let mut positions_storage = decode.reserve_scoped(0, "collect F3D loop positions")?;
    let mut positions = Vec::new();
    let mut history_source = IntoIterator::into_iter(&points);
    while let Some(point) = decode.next_charged(&mut history_source, "scan F3D points")? {
        let Some(value) = values.get(decode, &point.point_slot)? else {
            points_storage.commit()?;
            return Ok(DesignHistoricalLoopBoundary::Points(points));
        };
        positions_storage.with_storage(|| {
            decode.push_vec(
                &mut positions,
                DesignHistoricalLoopPosition {
                    point: point.clone(),
                    position: value.position,
                },
                "collect F3D loop positions",
            )
        })?;
    }
    positions_storage.commit()?;
    Ok(DesignHistoricalLoopBoundary::Positions(positions))
}

pub(super) fn preceding_support_face_slots(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_faces: &[cadmpeg_ir::ids::FaceId],
    result_topology: &AsmHistoricalTopology,
    preceding_topology: &AsmHistoricalTopology,
) -> Result<Vec<i64>, cadmpeg_core::CodecError> {
    let mut preceding_storage = decode.reserve_scoped(0, "index F3D preceding support faces")?;
    let mut preceding_faces = HashSet::new();
    for face in decode.admit_iter(
        &preceding_topology.faces,
        "scan F3D preceding topology faces",
    )? {
        preceding_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut preceding_faces,
                *face,
                "index F3D preceding support faces",
            )
        })?;
    }
    let result_bindings = UniqueIndex::new(
        &result_topology.face_surfaces,
        |_, binding| Ok(Some(binding.entity)),
        "index F3D result support bindings",
    );
    let mut reverse_storage = decode.reserve_scoped(0, "index F3D preceding support carriers")?;
    let mut preceding_carriers = HashMap::new();
    for binding in decode.admit_iter(
        &preceding_topology.face_surfaces,
        "scan F3D preceding support carriers",
    )? {
        if !decode.contains_hash_set(
            &preceding_faces,
            &binding.entity,
            "find F3D preceding support face",
        )? {
            continue;
        }
        reverse_storage.with_storage(|| {
            decode
                .entry_hash_map(
                    &mut preceding_carriers,
                    binding.carrier,
                    "index F3D preceding support carriers",
                )?
                .and_modify(|face| *face = None)
                .or_insert(Some(binding.entity));
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
    }
    let mut support_faces = Vec::new();
    for result_face in decode.admit_iter(result_faces, "scan F3D result faces")? {
        let Some(result_face) = stable_ref(decode, result_face.as_str())? else {
            continue;
        };
        let Some(binding) = result_bindings.get(decode, &result_face)? else {
            continue;
        };
        let Some(preceding_face) = decode
            .get_hash_map(
                &preceding_carriers,
                &binding.carrier,
                "find F3D preceding support carrier",
            )?
            .copied()
            .flatten()
        else {
            continue;
        };
        if !decode.contains(
            &support_faces,
            &preceding_face,
            "find F3D history support faces",
        )? {
            decode.push_vec(
                &mut support_faces,
                preceding_face,
                "collect F3D preceding support faces",
            )?;
        }
    }
    Ok(support_faces)
}

#[derive(Clone, Copy)]
pub(super) struct EdgeBoundaryContext<'a> {
    pub(super) topology: &'a AsmHistoricalTopology,
    pub(super) boundary_edges: &'a [i64],
}

pub(super) fn edge_recipe_reference_context(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    reference_ordinal: u32,
    reference: &crate::records::dimensions::DesignRecipeReference,
    result: EdgeBoundaryContext<'_>,
    preceding: EdgeBoundaryContext<'_>,
    changed_edges: &HashSet<i64>,
) -> Result<
    crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext,
    cadmpeg_core::CodecError,
> {
    let candidate_faces = if reference.candidate_faces.is_empty() {
        reference.alternate_selector_faces.as_slice()
    } else {
        reference.candidate_faces.as_slice()
    };
    let result_faces = selection::faces_in_topology(decode, candidate_faces, result.topology)?;
    let result_face_boundaries = face_boundary_contexts(decode, &result_faces, result.topology)?;
    let (result_edges, _result_edges_storage) = decode
        .with_scoped_storage("collect F3D result reference boundary edges", || {
            face_boundary_edges(decode, &result_faces, result.topology)
        })?;
    let result_shared_edge_slots = decode.try_collect_vec(
        decode
            .admit_iter(result.boundary_edges, "scan F3D result boundary edges")?
            .filter_map(|edge| {
                match decode.contains(
                    result_edges.as_slice(),
                    edge,
                    "find F3D result boundary edge",
                ) {
                    Ok(true) => Some(Ok(*edge)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            }),
        "collect F3D result shared edges",
    )?;
    let preceding_faces =
        selection::faces_in_topology(decode, candidate_faces, preceding.topology)?;
    let preceding_face_boundaries =
        face_boundary_contexts(decode, &preceding_faces, preceding.topology)?;
    let preceding_support_face_slots =
        preceding_support_face_slots(decode, &result_faces, result.topology, preceding.topology)?;
    let preceding_support_face_boundaries = face_boundary_contexts_for_slots(
        decode,
        &preceding_support_face_slots,
        preceding.topology,
    )?;
    let (preceding_edges, _preceding_edges_storage) = decode
        .with_scoped_storage("collect F3D preceding reference boundary edges", || {
            face_boundary_edges(decode, &preceding_faces, preceding.topology)
        })?;
    let shared_edge_slots = decode.try_collect_vec(
        decode
            .admit_iter(
                preceding.boundary_edges,
                "scan F3D preceding boundary edges",
            )?
            .filter_map(|edge| {
                match decode.contains(
                    preceding_edges.as_slice(),
                    edge,
                    "find F3D preceding boundary edge",
                ) {
                    Ok(true) => Some(Ok(*edge)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            }),
        "collect F3D shared edge slots",
    )?;
    let changed_shared_edge_slots = decode.try_collect_vec(
        decode
            .admit_iter(&shared_edge_slots, "scan F3D shared edge slots")?
            .filter_map(|edge| {
                match decode.contains_hash_set(changed_edges, edge, "find F3D changed shared edge")
                {
                    Ok(true) => Some(Ok(*edge)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            }),
        "collect F3D changed shared edges",
    )?;
    let mut support_edges_storage = decode.reserve_scoped(0, "index F3D support edges")?;
    let mut support_edges = BTreeSet::new();
    for face in decode.admit_iter(
        &preceding_support_face_boundaries,
        "scan F3D support face boundaries",
    )? {
        for face_loop in decode.admit_iter(&face.loops, "scan F3D support face loops")? {
            match &face_loop.boundary {
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D support loop members")? {
                        decode.insert_scoped_btree_set(&mut support_edges_storage, &mut support_edges, row.edge_slot, "index F3D support edges", "index F3D support edges")?;
                    }
                }
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Vertices(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D support loop members")? {
                        decode.insert_scoped_btree_set(&mut support_edges_storage, &mut support_edges, row.coedge.edge_slot, "index F3D support edges", "index F3D support edges")?;
                    }
                }
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Points(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D support loop members")? {
                        decode.insert_scoped_btree_set(&mut support_edges_storage, &mut support_edges, row.vertex.coedge.edge_slot, "index F3D support edges", "index F3D support edges")?;
                    }
                }
                crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Positions(rows) => {
                    for row in decode.admit_iter(rows, "scan F3D support loop members")? {
                        decode.insert_scoped_btree_set(&mut support_edges_storage, &mut support_edges, row.point.vertex.coedge.edge_slot, "index F3D support edges", "index F3D support edges")?;
                    }
                }
            }
        }
    }
    let mut changed_reference_edge_slots = decode.try_collect_vec(
        decode
            .admit_iter(&preceding_edges, "scan F3D preceding reference edges")?
            .chain(decode.admit_iter(&support_edges, "scan F3D support reference edges")?)
            .filter_map(|edge| {
                match decode.contains_hash_set(
                    changed_edges,
                    edge,
                    "find F3D changed reference edge",
                ) {
                    Ok(true) => Some(Ok(*edge)),
                    Ok(false) => None,
                    Err(error) => Some(Err(error)),
                }
            }),
        "collect F3D changed reference edges",
    )?;
    decode.sort_unstable_by(
        &mut changed_reference_edge_slots,
        |value| value,
        Ord::cmp,
        "sort F3D changed reference edges",
    )?;
    decode.dedup_vec(
        &mut changed_reference_edge_slots,
        "deduplicate F3D changed reference edges",
    )?;
    Ok(
        crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
            reference_ordinal,
            result_faces,
            result_face_boundaries,
            result_shared_edge_slots,
            preceding_faces,
            preceding_face_boundaries,
            preceding_support_face_slots,
            preceding_support_face_boundaries,
            shared_edge_slots,
            changed_shared_edge_slots,
            changed_reference_edge_slots,
        },
    )
}

pub(super) fn historical_edge_axis(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Result<Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>, cadmpeg_core::CodecError>
{
    if let Some(binding) = decode.find_by(
        &topology.edge_curves,
        |binding| Ok(binding.entity == edge),
        "find F3D edge axis curve",
    )? {
        if let Some(curve) = binding.carrier {
            if let Some(axis) = decode.find_by(
                &topology.curve_axes,
                |axis| Ok(axis.curve == curve),
                "find F3D edge curve axis",
            )? {
                return Ok(Some((axis.origin, axis.direction)));
            }
        }
    }
    let (context, _context_storage) = decode
        .with_scoped_storage("collect F3D edge axis support context", || {
            selection::historical_edge_context(decode, edge, topology)
        })?;
    let bindings = UniqueIndex::new(
        &topology.face_surfaces,
        |_, binding| Ok(Some(binding.entity)),
        "index F3D edge axis support bindings",
    );
    let mut support_storage = decode.reserve_scoped(0, "index F3D edge axis support surfaces")?;
    let mut support_surfaces = HashSet::new();
    for context in decode.admit_iter(
        &context.incident_loops,
        "scan F3D edge axis support contexts",
    )? {
        if let Some(binding) = bindings.get(decode, &context.face_slot)? {
            support_storage.with_storage(|| {
                decode.insert_hash_set(
                    &mut support_surfaces,
                    binding.carrier,
                    "index F3D edge axis support surfaces",
                )
            })?;
        }
    }
    let mut axes = topology.surface_axes.iter();
    let Some(first) = decode.find_by(
        &mut axes,
        |axis| {
            decode.contains_hash_set(
                &support_surfaces,
                &axis.surface,
                "find F3D edge axis support surface",
            )
        },
        "find F3D edge support axis",
    )?
    else {
        return Ok(None);
    };
    let first = (first.origin, first.direction);
    let coincident = decode.all_by(
        axes,
        |axis| {
            Ok(!decode.contains_hash_set(
                &support_surfaces,
                &axis.surface,
                "find F3D edge axis support surface",
            )? || same_axis_line(first, (axis.origin, axis.direction)))
        },
        "compare F3D edge support axes",
    )?;
    Ok(coincident.then_some(first))
}

pub(super) fn treatment_radius_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    deleted_edges: &[i64],
) -> Result<
    Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
    cadmpeg_core::CodecError,
> {
    Ok(treatment_edge_candidates::<false>(
        decode,
        result_candidate_faces,
        inserted_faces,
        result,
        preceding,
        deleted_edges,
    )?
    .0)
}

pub(super) fn treatment_edge_candidates<const TRANSITIONS: bool>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    deleted_edges: &[i64],
) -> Result<
    (
        Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
        Vec<i64>,
    ),
    cadmpeg_core::CodecError,
> {
    let result_boundaries = face_boundary_edge_index(decode, result)?;
    let preceding_boundaries = face_boundary_edge_index(decode, preceding)?;
    let (supports, _supports_storage) =
        decode.with_scoped_storage("collect F3D treatment face supports", || {
            treatment_face_supports(
                decode,
                inserted_faces,
                result,
                preceding,
                &result_boundaries,
            )
        })?;
    let mut deleted_edges_storage =
        decode.reserve_scoped(0, "index F3D treatment deleted edges")?;
    let mut deleted_edge_set = HashSet::new();
    for edge in decode.admit_iter(deleted_edges, "scan F3D deleted edges")? {
        deleted_edges_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut deleted_edge_set,
                *edge,
                "index F3D treatment deleted edges",
            )
        })?;
    }
    let mut candidate_edges_storage =
        decode.reserve_scoped(0, "index F3D treatment candidate edges")?;
    let mut candidate_edges = BTreeSet::new();
    if let Some(result_candidate_faces) = result_candidate_faces {
        for face in
            decode.admit_iter(result_candidate_faces, "scan F3D treatment candidate faces")?
        {
            let Some(face_slot) = stable_ref(decode, face.as_str())? else {
                continue;
            };
            let Some(edges) = decode.get_btree_map(
                &result_boundaries.boundaries,
                &face_slot,
                "find F3D treatment candidate face boundary",
            )?
            else {
                continue;
            };
            for edge in decode.admit_iter(&edges.edges, "scan F3D treatment candidate edges")? {
                decode.insert_scoped_btree_set(
                    &mut candidate_edges_storage,
                    &mut candidate_edges,
                    *edge,
                    "index F3D treatment candidate edges",
                    "index F3D treatment candidate edges",
                )?;
            }
        }
    }
    let surface_radii = UniqueIndex::new(
        &result.surface_radii,
        |_, candidate| Ok(Some(candidate.surface)),
        "index F3D treatment surface radii",
    );
    let mut radii_out = Vec::new();
    let mut transitions_out = Vec::new();
    for (inserted, carrier, supports) in
        decode.admit_iter(supports, "scan F3D treatment face supports")?
    {
        let Some(inserted_boundary) = decode.get_btree_map(
            &result_boundaries.boundaries,
            &inserted,
            "find F3D inserted treatment face boundary",
        )?
        else {
            continue;
        };
        let radius = match surface_radii
            .get(decode, &carrier)?
            .and_then(|candidate| cadmpeg_ir::scalar::PositiveReal::new(candidate.radius))
        {
            Some(radius)
                if candidate_edges.is_empty()
                    || !decode.is_disjoint_btree_set(
                        &inserted_boundary.edges,
                        &candidate_edges,
                        "compare F3D treatment candidate edges",
                    )? =>
            {
                Some(radius)
            }
            _ => None,
        };
        if !TRANSITIONS && radius.is_none() {
            continue;
        }
        for (ordinal, left) in decode
            .admit_iter(&supports, "scan F3D treatment support pairs")?
            .enumerate()
        {
            let Some(left_edges) = decode.get_btree_map(
                &preceding_boundaries.boundaries,
                left,
                "find F3D treatment support boundary",
            )?
            else {
                continue;
            };
            for right in
                decode.admit_iter(&supports[ordinal + 1..], "scan F3D treatment support pairs")?
            {
                let Some(right_edges) = decode.get_btree_map(
                    &preceding_boundaries.boundaries,
                    right,
                    "find F3D treatment support boundary",
                )?
                else {
                    continue;
                };
                for edge in decode.admit_iter(
                    &left_edges.edges,
                    "scan F3D treatment support boundary edges",
                )? {
                    if !decode.contains_btree_set(
                        &right_edges.edges,
                        edge,
                        "find F3D shared treatment support edge",
                    )? || !decode.contains_hash_set(
                        &deleted_edge_set,
                        edge,
                        "find F3D deleted treatment edge",
                    )? {
                        continue;
                    }
                    if TRANSITIONS {
                        decode.push_vec(
                            &mut transitions_out,
                            *edge,
                            "collect F3D treatment transition edges",
                        )?;
                    }
                    if let Some(radius) = radius {
                        decode.reserve_vec(&mut radii_out, 1, "collect F3D treatment radii")?;
                        radii_out.push(
                            crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
                                edge_slot: *edge,
                                radius,
                            },
                        );
                    }
                }
            }
        }
    }
    decode.stable_sort_by_key(
        &mut radii_out,
        |value| (value.radius.get(), value.edge_slot),
        |left, right| left.0.total_cmp(&right.0).then(left.1.cmp(&right.1)),
        "sort F3D treatment edge radii",
    )?;
    decode.dedup_by(
        &mut radii_out,
        |left, right| Ok(left.radius == right.radius && left.edge_slot == right.edge_slot),
        "deduplicate F3D treatment edge radii",
    )?;
    if TRANSITIONS {
        decode.sort_unstable_by(
            &mut transitions_out,
            |value| value,
            Ord::cmp,
            "sort F3D treatment edge transitions",
        )?;
        decode.dedup_vec(
            &mut transitions_out,
            "deduplicate F3D treatment edge transitions",
        )?;
    }
    Ok((radii_out, transitions_out))
}

fn treatment_face_supports(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    result_boundaries: &FaceBoundaryEdgeIndex<'_>,
) -> Result<Vec<(i64, i64, Vec<i64>)>, cadmpeg_core::CodecError> {
    let mut treatment_storage = decode.reserve_scoped(0, "index F3D treatment support topology")?;
    let mut preceding_faces = HashSet::new();
    for face in decode.admit_iter(&preceding.faces, "scan F3D preceding faces")? {
        treatment_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut preceding_faces,
                *face,
                "index F3D treatment preceding faces",
            )
        })?;
    }
    let mut preceding_surfaces = HashSet::new();
    for surface in decode.admit_iter(&preceding.surfaces, "scan F3D preceding surfaces")? {
        treatment_storage.with_storage(|| {
            decode.insert_hash_set(
                &mut preceding_surfaces,
                *surface,
                "index F3D treatment preceding surfaces",
            )
        })?;
    }
    let (result_carriers, _result_carriers_storage) = decode.unique_index(
        result
            .face_surfaces
            .iter()
            .map(|binding| (binding.entity, binding.carrier)),
        "index F3D face carriers",
    )?;
    let mut preceding_carrier_faces = HashMap::new();
    for binding in
        decode.admit_iter(&preceding.face_surfaces, "scan F3D carrier face candidates")?
    {
        if !decode.contains_hash_set(
            &preceding_faces,
            &binding.entity,
            "find F3D included support face",
        )? {
            continue;
        }
        treatment_storage.with_storage(|| {
            decode
                .entry_hash_map(
                    &mut preceding_carrier_faces,
                    binding.carrier,
                    "index F3D carrier faces",
                )?
                .and_modify(|face| *face = None)
                .or_insert(Some(binding.entity));
            Ok::<_, cadmpeg_core::CodecError>(())
        })?;
    }
    let mut adjacent_faces = HashMap::<i64, Vec<i64>>::new();
    for (face, edges) in decode.admit_iter(
        &result_boundaries.boundaries,
        "scan F3D result face boundaries",
    )? {
        for edge in decode.admit_iter(&edges.edges, "scan F3D result face boundary edges")? {
            treatment_storage.with_storage(|| {
                decode.push_hash_group(
                    &mut adjacent_faces,
                    *edge,
                    *face,
                    "index F3D adjacent treatment edges",
                    "collect F3D adjacent treatment faces",
                )
            })?;
        }
    }
    let mut selected = Vec::new();
    for inserted in decode
        .admit_iter(inserted_faces, "scan F3D inserted faces")?
        .copied()
    {
        let Some(carrier) = decode
            .get_hash_map(
                &result_carriers,
                &inserted,
                "find F3D history result carriers",
            )?
            .copied()
            .flatten()
        else {
            continue;
        };
        if decode.contains_hash_set(
            &preceding_surfaces,
            &carrier,
            "find F3D history preceding surfaces",
        )? {
            continue;
        }
        let Some(inserted_boundary) = decode.get_btree_map(
            &result_boundaries.boundaries,
            &inserted,
            "find F3D inserted support face boundary",
        )?
        else {
            continue;
        };
        let inserted_boundary_edges =
            decode.admit_iter(&inserted_boundary.edges, "scan F3D inserted boundary edges")?;
        let mut supports = Vec::new();
        for edge in inserted_boundary_edges {
            let Some(adjacent) =
                decode.get_hash_map(&adjacent_faces, edge, "find F3D history adjacent faces")?
            else {
                continue;
            };
            for face in decode.admit_iter(adjacent, "scan F3D adjacent treatment faces")? {
                if *face == inserted {
                    continue;
                }
                let Some(carrier) = decode
                    .get_hash_map(&result_carriers, face, "find F3D history result carriers")?
                    .copied()
                    .flatten()
                else {
                    continue;
                };
                let Some(support) = decode
                    .get_hash_map(
                        &preceding_carrier_faces,
                        &carrier,
                        "find F3D history preceding carrier faces",
                    )?
                    .copied()
                    .flatten()
                else {
                    continue;
                };
                decode.push_vec(
                    &mut supports,
                    support,
                    "collect F3D treatment support faces",
                )?;
            }
        }
        decode.sort_unstable_by(
            &mut supports,
            |value| value,
            Ord::cmp,
            "sort F3D treatment support faces",
        )?;
        decode.dedup_vec(&mut supports, "deduplicate F3D treatment support faces")?;

        decode.reserve_vec(&mut selected, 1, "collect F3D treatment face supports")?;
        selected.push((inserted, carrier, supports));
    }
    Ok(selected)
}

#[derive(Debug)]
struct FaceBoundaryEdges<'ctx> {
    edges: BTreeSet<i64>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

#[derive(Debug)]
struct FaceBoundaryEdgeIndex<'ctx> {
    boundaries: BTreeMap<i64, FaceBoundaryEdges<'ctx>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

fn face_boundary_edge_index<'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<FaceBoundaryEdgeIndex<'ctx>, cadmpeg_core::CodecError> {
    let (face_loops, _face_loops_storage) = decode.unique_index(
        topology
            .face_loops
            .iter()
            .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
        "index F3D boundary face loops",
    )?;
    let (loop_coedges, _loop_coedges_storage) = decode.unique_index(
        topology
            .loop_coedges
            .iter()
            .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
        "index F3D boundary loop coedges",
    )?;
    let (coedge_edges, _coedge_edges_storage) = decode.unique_index(
        topology
            .coedge_topology
            .iter()
            .map(|coedge| (coedge.coedge, coedge.edge)),
        "index F3D boundary coedge edges",
    )?;
    let mut boundaries_storage = decode.reserve_scoped(0, "index F3D face boundaries")?;
    let mut boundaries = BTreeMap::new();
    let mut history_source = IntoIterator::into_iter(&topology.faces);
    'faces: while let Some(face) =
        decode.next_charged(&mut history_source, "scan F3D boundary faces")?
    {
        let Some(loops) = decode
            .get_hash_map(&face_loops, face, "find F3D history face loops")?
            .copied()
            .flatten()
        else {
            continue;
        };
        let mut edges_storage = decode.reserve_scoped(0, "index F3D face boundary edges")?;
        let mut edges = BTreeSet::new();
        let mut history_source = IntoIterator::into_iter(loops);
        while let Some(loop_slot) =
            decode.next_charged(&mut history_source, "scan F3D boundary face loops")?
        {
            let Some(coedges) = decode
                .get_hash_map(&loop_coedges, loop_slot, "find F3D history loop coedges")?
                .copied()
                .flatten()
            else {
                continue 'faces;
            };
            let mut history_source = IntoIterator::into_iter(coedges);
            while let Some(coedge) =
                decode.next_charged(&mut history_source, "scan F3D boundary loop coedges")?
            {
                let Some(edge) = decode
                    .get_hash_map(&coedge_edges, coedge, "find F3D history coedge edges")?
                    .copied()
                    .flatten()
                else {
                    continue 'faces;
                };
                edges_storage.with_storage(|| {
                    decode.insert_btree_set(&mut edges, edge, "index F3D face boundary edges")
                })?;
            }
        }
        let entry = FaceBoundaryEdges {
            edges,
            _storage: edges_storage,
        };
        boundaries_storage.with_storage(|| {
            decode
                .insert_btree_map(&mut boundaries, *face, entry, "index F3D face boundaries")
                .map(|_| ())
        })?;
    }
    Ok(FaceBoundaryEdgeIndex {
        boundaries,
        _storage: boundaries_storage,
    })
}

pub(super) fn affected_body_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    current: &AsmDeltaState,
    previous: Option<&AsmDeltaState>,
) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError> {
    let Some(transition) = current.transition.as_ref() else {
        return Ok(None);
    };
    if transition.previous_state_id != previous.map(|state| state.state_id) {
        return Ok(None);
    }
    let Some(current_topology) = current.topology() else {
        return Ok(None);
    };
    let mut affected_storage = ctx.reserve_scoped(0, "merge F3D affected history bodies")?;
    let (current_changes, _current_changes_storage) = ctx
        .with_scoped_storage("index F3D changed topology members", || {
            changed_family_refs(ctx, &transition.topology, false)
        })?;
    let Some(mut affected) = affected_storage
        .with_storage(|| bodies_intersecting(ctx, current_topology, &current_changes))?
    else {
        return Ok(None);
    };
    if let Some(previous) = previous {
        let Some(previous_topology) = previous.topology() else {
            return Ok(None);
        };
        let (deleted, _deleted_storage) = ctx
            .with_scoped_storage("index F3D changed topology members", || {
                changed_family_refs(ctx, &transition.topology, true)
            })?;
        let Some(previous_affected) = affected_storage
            .with_storage(|| bodies_intersecting(ctx, previous_topology, &deleted))?
        else {
            return Ok(None);
        };
        for body in ctx.admit_iter(
            previous_affected,
            "scan F3D previous affected history bodies",
        )? {
            ctx.insert_scoped_btree_set(
                &mut affected_storage,
                &mut affected,
                body,
                "find F3D affected history body",
                "merge F3D affected history bodies",
            )?;
        }
    }
    let mut bodies = ctx.collection_vec(affected.len(), "collect F3D affected history bodies")?;
    for &body in ctx.admit_iter(&affected, "scan F3D affected history bodies")? {
        bodies.push(body);
    }
    Ok(Some(bodies))
}

fn changed_family_refs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    delta: &AsmHistoricalTopologyDelta,
    deleted: bool,
) -> Result<BTreeSet<i64>, cadmpeg_core::CodecError> {
    let families = [
        &delta.bodies,
        &delta.regions,
        &delta.shells,
        &delta.faces,
        &delta.loops,
        &delta.coedges,
        &delta.edges,
        &delta.vertices,
        &delta.points,
        &delta.surfaces,
        &delta.curves,
        &delta.pcurves,
    ];
    let mut changed = BTreeSet::new();
    for family in families {
        let members: [&[i64]; 2] = if deleted {
            [&family.deleted, &[]]
        } else {
            [&family.inserted, &family.updated]
        };
        for members in members {
            for &member in ctx.admit_iter(members, "scan F3D changed topology family members")? {
                ctx.insert_btree_set(&mut changed, member, "index F3D changed topology members")?;
            }
        }
    }
    Ok(changed)
}

/// Visits every entity in each body's topology closure (regions, shells,
/// faces, loops, coedges, edges, vertices and their carriers) as
/// `visit(body, entity)`, an entity once per path that reaches it. Returns
/// `false` at the first missing closure link. The relation indexes are scratch
/// held for this call.
fn walk_body_closures(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    mut visit: impl FnMut(i64, i64) -> Result<(), cadmpeg_core::CodecError>,
) -> Result<bool, cadmpeg_core::CodecError> {
    macro_rules! linked {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(false),
            }
        };
    }
    struct EdgeLinks {
        edge_vertices: HashMap<i64, [i64; 2]>,
        vertex_points: HashMap<i64, i64>,
        edge_curves: HashMap<i64, Option<i64>>,
    }
    fn visit_edge(
        decode: &cadmpeg_core::decode::DecodeContext<'_>,
        links: &EdgeLinks,
        body: i64,
        edge: i64,
        visit: &mut impl FnMut(i64, i64) -> Result<(), cadmpeg_core::CodecError>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        visit(body, edge)?;
        let Some(vertices) = decode.get_hash_map(
            &links.edge_vertices,
            &edge,
            "find F3D history edge vertices",
        )?
        else {
            return Ok(false);
        };
        for &vertex in vertices {
            visit(body, vertex)?;
            let Some(&point) = decode.get_hash_map(
                &links.vertex_points,
                &vertex,
                "find F3D history vertex points",
            )?
            else {
                return Ok(false);
            };
            visit(body, point)?;
        }
        if let Some(curve) = decode
            .get_hash_map(&links.edge_curves, &edge, "find F3D history edge curves")?
            .copied()
            .flatten()
        {
            visit(body, curve)?;
        }
        Ok(true)
    }
    let mut index_storage = decode.reserve_scoped(0, "index F3D historical relations")?;
    let body_regions =
        index_storage.with_storage(|| relation_map(decode, &topology.body_regions))?;
    let region_shells =
        index_storage.with_storage(|| relation_map(decode, &topology.region_shells))?;
    let shell_faces = index_storage.with_storage(|| relation_map(decode, &topology.shell_faces))?;
    let shell_wire_edges =
        index_storage.with_storage(|| relation_map(decode, &topology.shell_wire_edges))?;
    let shell_free_vertices =
        index_storage.with_storage(|| relation_map(decode, &topology.shell_free_vertices))?;
    let face_loops = index_storage.with_storage(|| relation_map(decode, &topology.face_loops))?;
    let loop_coedges =
        index_storage.with_storage(|| relation_map(decode, &topology.loop_coedges))?;
    let coedge_edges = index_storage.with_storage(|| {
        decode.collect_hash_map(
            topology
                .coedge_topology
                .iter()
                .map(|coedge| (coedge.coedge, coedge.edge)),
            "index F3D historical coedges",
        )
    })?;
    let carrier = |items: &[AsmHistoricalCarrierBinding]| {
        decode.collect_hash_map(
            items
                .iter()
                .map(|binding| (binding.entity, binding.carrier)),
            "index F3D historical carriers",
        )
    };
    let optional_carrier = |items: &[AsmHistoricalOptionalCarrierBinding]| {
        decode.collect_hash_map(
            items
                .iter()
                .map(|binding| (binding.entity, binding.carrier)),
            "index F3D historical optional carriers",
        )
    };
    let links = EdgeLinks {
        edge_vertices: index_storage.with_storage(|| {
            decode.collect_hash_map(
                topology
                    .edge_vertices
                    .iter()
                    .map(|edge| (edge.edge, [edge.start_vertex, edge.end_vertex])),
                "index F3D historical edge vertices",
            )
        })?,
        vertex_points: index_storage.with_storage(|| carrier(&topology.vertex_points))?,
        edge_curves: index_storage.with_storage(|| optional_carrier(&topology.edge_curves))?,
    };
    let face_surfaces = index_storage.with_storage(|| carrier(&topology.face_surfaces))?;
    let coedge_pcurves =
        index_storage.with_storage(|| optional_carrier(&topology.coedge_pcurves))?;
    let mut history_source = IntoIterator::into_iter(&topology.bodies);
    while let Some(&body) = decode.next_charged(&mut history_source, "scan F3D topology bodies")? {
        visit(body, body)?;
        let mut history_source = IntoIterator::into_iter(*linked!(decode.get_hash_map(
            &body_regions,
            &body,
            "find F3D history body regions"
        )?));
        while let Some(&region) =
            decode.next_charged(&mut history_source, "scan F3D body regions")?
        {
            visit(body, region)?;
            let mut history_source = IntoIterator::into_iter(*linked!(decode.get_hash_map(
                &region_shells,
                &region,
                "find F3D history region shells"
            )?));
            while let Some(&shell) =
                decode.next_charged(&mut history_source, "scan F3D region shells")?
            {
                visit(body, shell)?;
                let wire_edges = *linked!(decode.get_hash_map(
                    &shell_wire_edges,
                    &shell,
                    "find F3D history shell wire edges"
                )?);
                let free_vertices = *linked!(decode.get_hash_map(
                    &shell_free_vertices,
                    &shell,
                    "find F3D history shell free vertices"
                )?);
                let mut history_source = IntoIterator::into_iter(*linked!(decode.get_hash_map(
                    &shell_faces,
                    &shell,
                    "find F3D history shell faces"
                )?));
                while let Some(&face) =
                    decode.next_charged(&mut history_source, "scan F3D shell faces")?
                {
                    visit(body, face)?;
                    visit(
                        body,
                        *linked!(decode.get_hash_map(
                            &face_surfaces,
                            &face,
                            "find F3D history face surfaces"
                        )?),
                    )?;
                    let mut history_source = IntoIterator::into_iter(*linked!(
                        decode.get_hash_map(&face_loops, &face, "find F3D history face loops")?
                    ));
                    while let Some(&loop_) =
                        decode.next_charged(&mut history_source, "scan F3D face loops")?
                    {
                        visit(body, loop_)?;
                        let mut history_source = IntoIterator::into_iter(*linked!(decode
                            .get_hash_map(
                                &loop_coedges,
                                &loop_,
                                "find F3D history loop coedges"
                            )?));
                        while let Some(&coedge) =
                            decode.next_charged(&mut history_source, "scan F3D loop coedges")?
                        {
                            visit(body, coedge)?;
                            let edge = *linked!(decode.get_hash_map(
                                &coedge_edges,
                                &coedge,
                                "find F3D history coedge edges"
                            )?);
                            if !visit_edge(decode, &links, body, edge, &mut visit)? {
                                return Ok(false);
                            }
                            if let Some(pcurve) = decode
                                .get_hash_map(
                                    &coedge_pcurves,
                                    &coedge,
                                    "find F3D history coedge pcurves",
                                )?
                                .copied()
                                .flatten()
                            {
                                visit(body, pcurve)?;
                            }
                        }
                    }
                }
                let mut history_source = IntoIterator::into_iter(wire_edges);
                while let Some(&edge) =
                    decode.next_charged(&mut history_source, "scan F3D shell edges")?
                {
                    if !visit_edge(decode, &links, body, edge, &mut visit)? {
                        return Ok(false);
                    }
                }
                for &vertex in
                    decode.admit_iter(free_vertices, "scan F3D historical shell vertices")?
                {
                    visit(body, vertex)?;
                    visit(
                        body,
                        *linked!(decode.get_hash_map(
                            &links.vertex_points,
                            &vertex,
                            "find F3D history vertex points"
                        )?),
                    )?;
                }
            }
        }
    }
    Ok(true)
}

/// Returns the bodies whose topology closure meets `changed`, or `None` when
/// any closure link is missing.
fn bodies_intersecting(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
    changed: &BTreeSet<i64>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    let mut affected = BTreeSet::new();
    let complete = walk_body_closures(decode, topology, |body, entity| {
        if decode.contains_btree_set(changed, &entity, "check F3D changed body closure")? {
            decode.insert_btree_set(&mut affected, body, "collect F3D affected topology bodies")?;
        }
        Ok(())
    })?;
    Ok(complete.then_some(affected))
}

/// The bodies whose closure holds each entity of one topology, for repeated
/// intersection queries. `None` when some body closure has a missing link.
pub(super) struct BodyClosures<'ctx> {
    owners: Option<HashMap<i64, Vec<i64>>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

pub(super) fn body_closures<'ctx>(
    decode: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    topology: &AsmHistoricalTopology,
) -> Result<BodyClosures<'ctx>, cadmpeg_core::CodecError> {
    let operation = "index F3D body closure owners";
    let mut storage = decode.reserve_scoped(0, operation)?;
    let mut owners = HashMap::<i64, Vec<i64>>::new();
    let complete = walk_body_closures(decode, topology, |body, entity| {
        // One body's closure is walked before the next body's, so a repeated
        // visit by the same body is the group's last owner.
        if let Some(group) = decode.get_hash_map(&owners, &entity, "find F3D history owners")? {
            if group.last() == Some(&body) {
                return Ok(());
            }
        }
        storage.with_storage(|| {
            decode.push_hash_group(&mut owners, entity, body, operation, operation)
        })
    })?;
    Ok(BodyClosures {
        owners: complete.then_some(owners),
        _storage: storage,
    })
}

/// Returns the bodies whose closure meets `changed`, or `None` when a closure
/// is incomplete.
pub(super) fn closures_intersecting(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    closures: &BodyClosures<'_>,
    changed: &BTreeSet<i64>,
) -> Result<Option<BTreeSet<i64>>, cadmpeg_core::CodecError> {
    let Some(owners) = &closures.owners else {
        return Ok(None);
    };
    let mut affected = BTreeSet::new();
    for entity in decode.admit_iter(changed, "scan F3D changed body closure entities")? {
        let Some(bodies) = decode.get_hash_map(owners, entity, "find F3D history owners")? else {
            continue;
        };
        for &body in decode.admit_iter(bodies, "scan F3D changed entity bodies")? {
            decode.insert_btree_set(&mut affected, body, "collect F3D affected topology bodies")?;
        }
    }
    Ok(Some(affected))
}

fn relation_map<'a>(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    items: &'a [AsmHistoricalRelation],
) -> Result<HashMap<i64, &'a [i64]>, cadmpeg_core::CodecError> {
    decode.collect_hash_map(
        items
            .iter()
            .map(|relation| (relation.owner_ref, relation.member_refs.as_slice())),
        "index F3D historical relations",
    )
}

pub(super) fn historical_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    macro_rules! topology_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }

    fn refs<'a, Owner>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        owners: &'a [Owner],
        id: impl Fn(&'a Owner) -> &'a str,
    ) -> Result<Option<Vec<i64>>, cadmpeg_core::CodecError>
    where
        Owner: 'a,
    {
        let mut refs = Vec::new();
        let mut owners = owners.iter();
        while let Some(owner) =
            ctx.next_charged(&mut owners, "scan F3D historical topology reference owners")?
        {
            let Some(reference) = stable_ref(ctx, id(owner))? else {
                return Ok(None);
            };
            ctx.push_vec(
                &mut refs,
                reference,
                "collect F3D historical topology references",
            )?;
        }
        Ok(Some(refs))
    }

    fn relations<'a, Owner, Member, OwnerMembers, MemberId>(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        owners: &'a [Owner],
        owner_members: OwnerMembers,
        member_id: MemberId,
    ) -> Result<Option<Vec<AsmHistoricalRelation>>, cadmpeg_core::CodecError>
    where
        Member: 'a,
        OwnerMembers: Fn(&'a Owner) -> (&'a str, &'a [Member]),
        MemberId: Fn(&'a Member) -> &'a str,
    {
        let mut relations = Vec::new();
        let mut owners = owners.iter();
        while let Some(owner) =
            ctx.next_charged(&mut owners, "scan F3D historical topology relation owners")?
        {
            let (owner, members) = owner_members(owner);
            let Some(owner_ref) = stable_ref(ctx, owner)? else {
                return Ok(None);
            };
            let mut member_refs = Vec::new();
            let mut members = members.iter();
            while let Some(member) = ctx.next_charged(
                &mut members,
                "scan F3D historical topology relation members",
            )? {
                let Some(member_ref) = stable_ref(ctx, member_id(member))? else {
                    return Ok(None);
                };
                ctx.push_vec(
                    &mut member_refs,
                    member_ref,
                    "collect F3D historical topology references",
                )?;
            }
            ctx.push_vec(
                &mut relations,
                AsmHistoricalRelation {
                    owner_ref,
                    member_refs,
                },
                "collect F3D historical topology relations",
            )?;
        }
        Ok(Some(relations))
    }

    fn face_loop_relations(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        faces: &[cadmpeg_ir::topology::Face],
    ) -> Result<Option<Vec<AsmHistoricalRelation>>, cadmpeg_core::CodecError> {
        let mut relations = Vec::new();
        let mut faces = faces.iter();
        while let Some(face) =
            ctx.next_charged(&mut faces, "scan F3D historical topology relation owners")?
        {
            let Some(owner_ref) = stable_ref(ctx, face.id.as_str())? else {
                return Ok(None);
            };
            let mut member_refs = Vec::new();
            match &face.loops {
                cadmpeg_ir::topology::FaceLoops::Unspecified { loops } => {
                    let mut history_source = IntoIterator::into_iter(loops);
                    while let Some(loop_id) = ctx.next_charged(
                        &mut history_source,
                        "scan F3D historical topology relation members",
                    )? {
                        let Some(loop_ref) = stable_ref(ctx, loop_id.as_str())? else {
                            return Ok(None);
                        };
                        ctx.push_vec(
                            &mut member_refs,
                            loop_ref,
                            "collect F3D historical topology references",
                        )?;
                    }
                }
                cadmpeg_ir::topology::FaceLoops::Classified { outer, inner } => {
                    let Some(outer_ref) = stable_ref(ctx, outer.as_str())? else {
                        return Ok(None);
                    };
                    ctx.push_vec(
                        &mut member_refs,
                        outer_ref,
                        "collect F3D historical topology references",
                    )?;
                    let mut history_source = IntoIterator::into_iter(inner);
                    while let Some(loop_id) = ctx.next_charged(
                        &mut history_source,
                        "scan F3D historical topology relation members",
                    )? {
                        let Some(loop_ref) = stable_ref(ctx, loop_id.as_str())? else {
                            return Ok(None);
                        };
                        ctx.push_vec(
                            &mut member_refs,
                            loop_ref,
                            "collect F3D historical topology references",
                        )?;
                    }
                }
            }
            ctx.push_vec(
                &mut relations,
                AsmHistoricalRelation {
                    owner_ref,
                    member_refs,
                },
                "collect F3D historical topology relations",
            )?;
        }
        Ok(Some(relations))
    }

    let mut topology_storage = ctx.reserve_scoped(0, "collect F3D historical topology")?;
    let topology = topology_storage.with_storage(
        || -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
            let mut surface_radii = Vec::new();
            for surface in ctx.admit_iter(&brep.surfaces, "scan F3D historical surface radii")? {
                use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
                let radius = match &surface.geometry {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                        cylinder_surface.radius().get()
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                        sphere_surface.radius().get()
                    }
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                        torus_surface.minor_radius().get()
                    }
                    _ => continue,
                };
                let Some(surface_ref) = stable_ref(ctx, surface.id.as_str())? else {
                    continue;
                };
                ctx.push_vec(
                    &mut surface_radii,
                    crate::history_records::AsmHistoricalSurfaceRadius {
                        surface: surface_ref,
                        radius: radius.abs(),
                    },
                    "collect F3D historical surface radii",
                )?;
            }
            for (owner, procedural) in ctx.admit_iter(
                &brep.procedural_surfaces,
                "scan F3D brep procedural surfaces",
            )? {
                let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend(definition_payload) =
                    procedural.definition()
                else {
                    continue;
                };
                let radius = definition_payload.radius();

                let cadmpeg_ir::geometry::BlendRadiusLaw::Constant { signed_radius } = radius
                else {
                    continue;
                };
                let Some(surface) = stable_ref(ctx, owner.as_str())? else {
                    continue;
                };
                ctx.retain_vec(
                    &mut surface_radii,
                    |candidate| Ok(candidate.surface != surface),
                    "retain F3D historical surface radii",
                )?;

                ctx.reserve_vec(
                    &mut surface_radii,
                    1,
                    "collect F3D historical surface radii",
                )?;
                surface_radii.push(crate::history_records::AsmHistoricalSurfaceRadius {
                    surface,
                    radius: signed_radius.get().abs(),
                });
            }
            ctx.stable_sort_by(
                &mut surface_radii,
                |value| &value.surface,
                Ord::cmp,
                "sort F3D historical surface radii",
            )?;
            let mut surface_cylinders = Vec::new();
            for surface in
                ctx.admit_iter(&brep.surfaces, "scan F3D historical surface cylinders")?
            {
                let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) =
                    surface.geometry.solved()
                else {
                    continue;
                };
                let Some(surface_ref) = stable_ref(ctx, surface.id.as_str())? else {
                    continue;
                };
                ctx.push_vec(
                    &mut surface_cylinders,
                    crate::history_records::AsmHistoricalCylinder {
                        surface: surface_ref,
                        origin: cylinder_surface.origin().get(),
                        axis: *cylinder_surface.frame().axis().as_raw(),
                        radius: cylinder_surface.radius().get().abs(),
                    },
                    "collect F3D historical surface cylinders",
                )?;
            }
            ctx.stable_sort_by(
                &mut surface_cylinders,
                |value| &value.surface,
                Ord::cmp,
                "sort F3D historical surface cylinders",
            )?;
            let mut surface_planes = Vec::new();
            for surface in ctx.admit_iter(&brep.surfaces, "scan F3D historical surface planes")? {
                let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved()
                else {
                    continue;
                };
                let Some(surface_ref) = stable_ref(ctx, surface.id.as_str())? else {
                    continue;
                };
                ctx.push_vec(
                    &mut surface_planes,
                    crate::history_records::AsmHistoricalPlane {
                        surface: surface_ref,
                        origin: plane_surface.origin().get(),
                        normal: *plane_surface.frame().axis().as_raw(),
                    },
                    "collect F3D historical surface planes",
                )?;
            }
            ctx.stable_sort_by(
                &mut surface_planes,
                |value| &value.surface,
                Ord::cmp,
                "sort F3D historical surface planes",
            )?;
            let mut surface_axes = Vec::new();
            for surface in ctx.admit_iter(&brep.surfaces, "scan F3D historical surface axes")? {
                use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
                let (origin, direction) = match surface.geometry {
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => (
                        cylinder_surface.origin().get(),
                        *cylinder_surface.frame().axis().as_raw(),
                    ),
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => (
                        cone_surface.origin().get(),
                        *cone_surface.frame().axis().as_raw(),
                    ),
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => (
                        torus_surface.center().get(),
                        *torus_surface.frame().axis().as_raw(),
                    ),
                    _ => continue,
                };
                let Some(surface_ref) = stable_ref(ctx, surface.id.as_str())? else {
                    continue;
                };
                ctx.push_vec(
                    &mut surface_axes,
                    crate::history_records::AsmHistoricalSurfaceAxis {
                        surface: surface_ref,
                        origin,
                        direction,
                    },
                    "collect F3D historical surface axes",
                )?;
            }
            ctx.stable_sort_by(
                &mut surface_axes,
                |value| &value.surface,
                Ord::cmp,
                "sort F3D historical surface axes",
            )?;

            Ok(Some(AsmHistoricalTopology {
                bodies: topology_some!(refs(ctx, &brep.bodies, |entity| entity.id.as_str())?),
                regions: topology_some!(refs(ctx, &brep.regions, |entity| entity.id.as_str())?),
                shells: topology_some!(refs(ctx, &brep.shells, |entity| entity.id.as_str())?),
                faces: topology_some!(refs(ctx, &brep.faces, |entity| entity.id.as_str())?),
                loops: topology_some!(refs(ctx, &brep.loops, |entity| entity.id.as_str())?),
                coedges: topology_some!(refs(ctx, &brep.coedges, |entity| entity.id.as_str())?),
                edges: topology_some!(refs(ctx, &brep.edges, |entity| entity.id.as_str())?),
                vertices: topology_some!(refs(ctx, &brep.vertices, |entity| entity.id.as_str())?),
                points: topology_some!(refs(ctx, &brep.points, |entity| entity.id.as_str())?),
                surfaces: topology_some!(refs(ctx, &brep.surfaces, |entity| entity.id.as_str())?),
                surface_radii,
                surface_cylinders,
                surface_planes,
                surface_axes,
                curves: topology_some!(refs(ctx, &brep.curves, |entity| entity.id.as_str())?),
                curve_axes: {
                    let mut curve_axes = Vec::new();
                    for curve in ctx.admit_iter(&brep.curves, "scan F3D historical curve axes")? {
                        use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
                        let (origin, direction) = match curve.geometry {
                            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                                (line_curve.origin().get(), *line_curve.direction().as_raw())
                            }
                            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => (
                                circle_curve.center().get(),
                                *circle_curve.frame().axis().as_raw(),
                            ),
                            CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => (
                                ellipse_curve.center().get(),
                                *ellipse_curve.frame().axis().as_raw(),
                            ),
                            _ => continue,
                        };
                        let Some(curve_ref) = stable_ref(ctx, curve.id.as_str())? else {
                            continue;
                        };
                        ctx.push_vec(
                            &mut curve_axes,
                            crate::history_records::AsmHistoricalCurveAxis {
                                curve: curve_ref,
                                origin,
                                direction,
                            },
                            "collect F3D historical curve axes",
                        )?;
                    }
                    curve_axes
                },
                pcurves: topology_some!(refs(ctx, &brep.pcurves, |entity| entity.id.as_str())?),
                persistent_subentity_tags: Vec::new(),
                body_regions: topology_some!(relations(
                    ctx,
                    &brep.bodies,
                    |body| (body.id.as_str(), body.regions.as_slice()),
                    cadmpeg_ir::ids::RegionId::as_str,
                )?),
                region_shells: topology_some!(relations(
                    ctx,
                    &brep.regions,
                    |region| (region.id.as_str(), region.shells.as_slice()),
                    cadmpeg_ir::ids::ShellId::as_str,
                )?),
                shell_faces: topology_some!(relations(
                    ctx,
                    &brep.shells,
                    |shell| (shell.id.as_str(), shell.faces()),
                    cadmpeg_ir::ids::FaceId::as_str,
                )?),
                shell_wire_edges: topology_some!(relations(
                    ctx,
                    &brep.shells,
                    |shell| (shell.id.as_str(), shell.wire_edges()),
                    cadmpeg_ir::ids::EdgeId::as_str,
                )?),
                shell_free_vertices: topology_some!(relations(
                    ctx,
                    &brep.shells,
                    |shell| (shell.id.as_str(), shell.free_vertices()),
                    cadmpeg_ir::ids::VertexId::as_str,
                )?),
                face_loops: topology_some!(face_loop_relations(ctx, &brep.faces)?),
                loop_coedges: topology_some!(relations(
                    ctx,
                    &brep.loops,
                    |loop_| (loop_.id.as_str(), loop_.coedges()),
                    cadmpeg_ir::ids::CoedgeId::as_str,
                )?),
                coedge_topology: {
                    let mut neighbor_storage =
                        ctx.reserve_scoped(0, "index F3D historical coedge ring neighbors")?;
                    let mut neighbors = HashMap::new();
                    if !brep.coedges.is_empty() {
                        for loop_ in
                            ctx.admit_iter(&brep.loops, "scan F3D historical coedge rings")?
                        {
                            let ring = loop_.coedges();
                            for (position, coedge) in ctx
                                .admit_iter(ring, "scan F3D historical coedge ring members")?
                                .enumerate()
                            {
                                neighbor_storage.with_storage(
                                    || -> Result<(), cadmpeg_core::CodecError> {
                                        if let std::collections::hash_map::Entry::Vacant(entry) =
                                            ctx.entry_hash_map(
                                                &mut neighbors,
                                                (loop_.id.as_str(), coedge.as_str()),
                                                "index F3D historical coedge ring neighbors",
                                            )?
                                        {
                                            let next = &ring[(position + 1) % ring.len()];
                                            let previous = &ring[if position == 0 {
                                                ring.len() - 1
                                            } else {
                                                position - 1
                                            }];
                                            entry.insert((next, previous));
                                        }
                                        Ok(())
                                    },
                                )?;
                            }
                        }
                    }
                    topology_some!(
                        ctx.collect_fallible_options(
                            brep.coedges.iter().map(
                                |coedge| -> Result<
                                    Option<AsmHistoricalCoedge>,
                                    cadmpeg_core::CodecError,
                                > {
                                    let Some(&(next, previous)) = ctx.get_hash_map(
                                        &neighbors,
                                        &(coedge.owner_loop.as_str(), coedge.id.as_str()),
                                        "find F3D historical coedge ring neighbors",
                                    )?
                                    else {
                                        return Ok(None);
                                    };
                                    let Some(coedge_ref) = stable_ref(ctx, coedge.id.as_str())?
                                    else {
                                        return Ok(None);
                                    };
                                    let Some(owner_loop_ref) =
                                        stable_ref(ctx, coedge.owner_loop.as_str())?
                                    else {
                                        return Ok(None);
                                    };
                                    let Some(edge_ref) = stable_ref(ctx, coedge.edge.as_str())?
                                    else {
                                        return Ok(None);
                                    };
                                    let Some(next_ref) = stable_ref(ctx, next.as_str())? else {
                                        return Ok(None);
                                    };
                                    let Some(previous_ref) = stable_ref(ctx, previous.as_str())?
                                    else {
                                        return Ok(None);
                                    };
                                    let Some(radial_next_ref) =
                                        stable_ref(ctx, coedge.radial_next.as_str())?
                                    else {
                                        return Ok(None);
                                    };
                                    Ok(Some(AsmHistoricalCoedge {
                                        coedge: coedge_ref,
                                        owner_loop: owner_loop_ref,
                                        edge: edge_ref,
                                        next: next_ref,
                                        previous: previous_ref,
                                        radial_next: radial_next_ref,
                                    }))
                                }
                            ),
                            "collect F3D historical coedges"
                        )?
                    )
                },
                edge_vertices: topology_some!(ctx.collect_fallible_options(
                    brep.edges.iter().map(
                        |edge| -> Result<Option<AsmHistoricalEdge>, cadmpeg_core::CodecError> {
                            let Some(edge_ref) = stable_ref(ctx, edge.id.as_str())? else {
                                return Ok(None);
                            };
                            let Some(start_vertex_ref) = stable_ref(ctx, edge.start.as_str())?
                            else {
                                return Ok(None);
                            };
                            let Some(end_vertex_ref) = stable_ref(ctx, edge.end.as_str())? else {
                                return Ok(None);
                            };
                            Ok(Some(AsmHistoricalEdge {
                                edge: edge_ref,
                                start_vertex: start_vertex_ref,
                                end_vertex: end_vertex_ref,
                            }))
                        }
                    ),
                    "collect F3D historical edges"
                )?),
                face_surfaces: topology_some!(ctx.collect_fallible_options(
                    brep.faces.iter().map(
                        |face| -> Result<
                            Option<AsmHistoricalCarrierBinding>,
                            cadmpeg_core::CodecError,
                        > {
                            let Some(entity) = stable_ref(ctx, face.id.as_str())? else {
                                return Ok(None);
                            };
                            let Some(carrier) = stable_ref(ctx, face.surface.as_str())? else {
                                return Ok(None);
                            };
                            Ok(Some(AsmHistoricalCarrierBinding { entity, carrier }))
                        }
                    ),
                    "collect F3D historical face surfaces"
                )?),
                edge_curves: topology_some!(ctx.collect_fallible_options(
                    brep.edges.iter().map(
                        |edge| -> Result<
                            Option<AsmHistoricalOptionalCarrierBinding>,
                            cadmpeg_core::CodecError,
                        > {
                            let Some(entity) = stable_ref(ctx, edge.id.as_str())? else {
                                return Ok(None);
                            };
                            let carrier = match edge.curve() {
                                Some(curve) => match stable_ref(ctx, curve.as_str())? {
                                    Some(carrier) => Some(carrier),
                                    None => return Ok(None),
                                },
                                None => None,
                            };
                            Ok(Some(AsmHistoricalOptionalCarrierBinding {
                                entity,
                                carrier,
                            }))
                        }
                    ),
                    "collect F3D historical edge curves"
                )?),
                coedge_pcurves: topology_some!(ctx.collect_fallible_options(
                    brep.coedges.iter().map(
                        |coedge| -> Result<
                            Option<AsmHistoricalOptionalCarrierBinding>,
                            cadmpeg_core::CodecError,
                        > {
                            let Some(entity) = stable_ref(ctx, coedge.id.as_str())? else {
                                return Ok(None);
                            };
                            let carrier = match coedge.pcurves.first() {
                                Some(use_) => match stable_ref(ctx, use_.pcurve.as_str())? {
                                    Some(carrier) => Some(carrier),
                                    None => return Ok(None),
                                },
                                None => None,
                            };
                            Ok(Some(AsmHistoricalOptionalCarrierBinding {
                                entity,
                                carrier,
                            }))
                        }
                    ),
                    "collect F3D historical coedge pcurves"
                )?),
                vertex_points: topology_some!(ctx.collect_fallible_options(
                    brep.vertices.iter().map(
                        |vertex| -> Result<
                            Option<AsmHistoricalCarrierBinding>,
                            cadmpeg_core::CodecError,
                        > {
                            let Some(entity) = stable_ref(ctx, vertex.id.as_str())? else {
                                return Ok(None);
                            };
                            let Some(carrier) = stable_ref(ctx, vertex.point.as_str())? else {
                                return Ok(None);
                            };
                            Ok(Some(AsmHistoricalCarrierBinding { entity, carrier }))
                        }
                    ),
                    "collect F3D historical vertex points"
                )?),
                point_positions: topology_some!(ctx.collect_fallible_options(
                    brep.points.iter().map(
                        |point| -> Result<Option<AsmHistoricalPoint>, cadmpeg_core::CodecError> {
                            let Some(point_ref) = stable_ref(ctx, point.id.as_str())? else {
                                return Ok(None);
                            };
                            Ok(Some(AsmHistoricalPoint {
                                point: point_ref,
                                position: point.position().get(),
                            }))
                        }
                    ),
                    "collect F3D historical point positions"
                )?),
            }))
        },
    )?;
    if topology.is_some() {
        topology_storage.commit()?;
    }
    Ok(topology)
}

#[cfg(test)]
mod tests;
