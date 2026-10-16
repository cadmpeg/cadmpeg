// SPDX-License-Identifier: Apache-2.0
//! Bind historical snapshots by replacing the dependency closure of each delta.

use super::{
    archived_active_record_count, decode_record_archive, historical_topology_with_tags,
    insert_only_active_record_count, walk_entity_versions, HistoricalRecordArchive,
};
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistoricalTransition};
use cadmpeg_asm::brep::attributes::{attribute_next, attribute_owner};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_asm::sab::{Record, Token};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::{Duration, Instant};

mod indexed;
use indexed::IndexedTopology;

#[derive(Debug, Default)]
pub(crate) struct Statistics {
    pub(crate) steps: usize,
    pub(crate) fallbacks: BTreeMap<&'static str, usize>,
    pub(crate) decoded_records: usize,
    pub(crate) full_records: usize,
    pub(crate) elapsed: Duration,
}

pub(crate) fn bind(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
) -> Result<Statistics, cadmpeg_core::CodecError> {
    let started = Instant::now();
    let mut stats = Statistics::default();
    let Some(archive) = decode_record_archive(ctx, states, bytes, width)? else {
        return Ok(stats);
    };
    let archived_ids: Vec<_> = states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let Some(active_count) = archived_active_record_count(states)?
        .or_else(|| insert_only_active_record_count(states))
        .and_then(|count| i64::try_from(count).ok())
    else {
        return Ok(stats);
    };
    let saved_limit = cadmpeg_asm::asm_header::parse(ctx, bytes)?
        .and_then(|header| header.metadata.entity_count)
        .and_then(|count| i64::try_from(count).ok());
    let mut binder = Binder::new(&archive, saved_limit);
    let mut newer_index: Option<usize> = None;
    let mut current = IndexedTopology::default();
    let mut newer_versions = BTreeMap::new();
    let mut transitions = Vec::new();
    let mut complete = true;
    let mut failure = None;
    let walked = walk_entity_versions(states, active_count, &archived_ids, |ordinal, versions| {
        if !complete {
            return;
        }
        stats.full_records += versions.len();
        let changed: Slots = if let Some(newer_index) = newer_index {
            states[newer_index]
                .bulletin_boards
                .iter()
                .flat_map(|board| &board.changes)
                .filter_map(|change| change.new_ref().or_else(|| change.old_ref()))
                .filter(|slot| {
                    match (
                        binder.live.table.get(slot),
                        versions
                            .get(slot)
                            .and_then(|revision| archive.get(revision)),
                    ) {
                        (Some(current), Some(next)) => !std::ptr::eq(*current, next),
                        (None, None) => false,
                        _ => true,
                    }
                })
                .collect()
        } else {
            versions.keys().copied().collect()
        };
        if let Some(index) = newer_index {
            stats.steps += 1;
            let (older_rows, newer_rows) = match binder.incremental(
                ctx,
                &mut current,
                &changed,
                versions,
                bytes,
                &mut stats,
            ) {
                Ok(Some(rows)) => rows,
                Ok(None) => {
                    complete = false;
                    return;
                }
                Err(error) => {
                    failure = Some(error);
                    complete = false;
                    return;
                }
            };
            let older = crate::history_records::AsmStateSnapshot {
                state_id: states[ordinal].state_id,
                topology: &older_rows,
                versions,
            };
            let new = crate::history_records::AsmStateSnapshot {
                state_id: states[index].state_id,
                topology: &newer_rows,
                versions: &newer_versions,
            };
            let Some(transition) = transition_for_changes(older, new, &changed) else {
                complete = false;
                return;
            };
            transitions.push((index, transition));
        } else {
            if binder.update(&changed, versions).is_none() {
                complete = false;
                return;
            }
            binder.rebuild_units();
            match binder.full(ctx, bytes, &mut stats) {
                Ok(Some(topology)) => current = topology.into(),
                Ok(None) => {
                    complete = false;
                    return;
                }
                Err(error) => {
                    failure = Some(error);
                    complete = false;
                    return;
                }
            }
        }
        for slot in &changed {
            if let Some(revision) = versions.get(slot) {
                newer_versions.insert(*slot, *revision);
            } else {
                newer_versions.remove(slot);
            }
        }
        newer_index = Some(ordinal);
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if !walked || !complete {
        return Ok(stats);
    }
    if let Some(ordinal) = newer_index {
        let topology = current.materialize();
        let Some(transition) = AsmHistoricalTransition::between(
            None,
            crate::history_records::AsmStateSnapshot {
                state_id: states[ordinal].state_id,
                topology: &topology,
                versions: &newer_versions,
            },
        ) else {
            return Ok(stats);
        };
        transitions.push((ordinal, transition));
    }
    for (ordinal, transition) in transitions {
        states[ordinal].transition = Some(transition);
    }
    stats.elapsed = started.elapsed();
    Ok(stats)
}

fn transition_for_changes(
    older: crate::history_records::AsmStateSnapshot<'_>,
    newer: crate::history_records::AsmStateSnapshot<'_>,
    changed: &Slots,
) -> Option<AsmHistoricalTransition> {
    let old_topology = older.topology;
    let new_topology = newer.topology;
    let mut slots = old_topology.slots();
    slots.extend(new_topology.slots());
    slots.extend(changed);
    let project = |versions: &BTreeMap<i64, i64>| -> BTreeMap<_, _> {
        slots
            .iter()
            .filter_map(|slot| versions.get(slot).map(|revision| (*slot, *revision)))
            .collect()
    };
    let old_versions = project(older.versions);
    let new_versions = project(newer.versions);
    AsmHistoricalTransition::between(
        Some(crate::history_records::AsmStateSnapshot {
            topology: old_topology,
            versions: &old_versions,
            ..older
        }),
        crate::history_records::AsmStateSnapshot {
            topology: new_topology,
            versions: &new_versions,
            ..newer
        },
    )
}

#[derive(Default)]
struct Subtypes {
    definitions: Vec<usize>,
    references: Vec<Option<usize>>,
}

impl Subtypes {
    fn of(record: &Record) -> Self {
        let mut out = Self::default();
        for (pos, token) in record.tokens.iter().enumerate() {
            if !matches!(token, Token::SubtypeOpen) {
                continue;
            }
            match record.tokens.get(pos + 1) {
                Some(Token::Ident(name) | Token::SubIdent(name)) if name != "ref" => {
                    out.definitions.push(pos);
                }
                Some(Token::Ident(name) | Token::SubIdent(name)) if name == "ref" => {
                    out.references.push(match record.tokens.get(pos + 2) {
                        Some(Token::Long(index)) => usize::try_from(*index).ok(),
                        _ => None,
                    });
                }
                Some(Token::Long(index)) => out.references.push(usize::try_from(*index).ok()),
                _ => {}
            }
        }
        out
    }
}

type Slots = BTreeSet<i64>;
type Table<'a> = BTreeMap<i64, &'a Record>;

struct FaceUnit {
    slots: Slots,
    reached_edges: Slots,
}

#[derive(Default)]
struct Units {
    faces: BTreeMap<i64, Slots>,
    reverse: BTreeMap<i64, Slots>,
    edges_by_face: BTreeMap<i64, Slots>,
    edge_users: BTreeMap<i64, Slots>,
}

impl Units {
    fn remove(&mut self, face: i64) {
        if let Some(edges) = self.edges_by_face.remove(&face) {
            for edge in edges {
                if let Some(faces) = self.edge_users.get_mut(&edge) {
                    faces.remove(&face);
                    if faces.is_empty() {
                        self.edge_users.remove(&edge);
                    }
                }
            }
        }
        if let Some(unit) = self.faces.remove(&face) {
            for slot in unit {
                if let Some(faces) = self.reverse.get_mut(&slot) {
                    faces.remove(&face);
                    if faces.is_empty() {
                        self.reverse.remove(&slot);
                    }
                }
            }
        }
    }

    fn insert(&mut self, face: i64, unit: FaceUnit) {
        self.remove(face);
        let FaceUnit {
            slots,
            reached_edges,
        } = unit;
        for &edge in &reached_edges {
            self.edge_users.entry(edge).or_default().insert(face);
        }
        self.edges_by_face.insert(face, reached_edges);
        for &slot in &slots {
            self.reverse.entry(slot).or_default().insert(face);
        }
        self.faces.insert(face, slots);
    }

    fn touching(&self, slots: &Slots) -> Slots {
        slots
            .iter()
            .filter_map(|slot| self.reverse.get(slot))
            .flatten()
            .copied()
            .collect()
    }

    fn slots(&self, faces: &Slots) -> Slots {
        faces
            .iter()
            .filter_map(|face| self.faces.get(face))
            .flatten()
            .copied()
            .collect()
    }

    fn neighbours(&self, faces: &Slots) -> Slots {
        self.touching(&self.slots(faces))
    }
}

/// Live indexes contain borrowed archive records. Only changed slots update them.
struct Live<'a> {
    table: Table<'a>,
    kinds: BTreeMap<&'a str, Slots>,
    attributes: BTreeMap<i64, Slots>,
    incoming: BTreeMap<i64, Slots>,
    subtype_references: BTreeMap<i64, Vec<Option<usize>>>,
}

impl<'a> Live<'a> {
    fn new() -> Self {
        Self {
            table: BTreeMap::new(),
            kinds: BTreeMap::new(),
            attributes: BTreeMap::new(),
            incoming: BTreeMap::new(),
            subtype_references: BTreeMap::new(),
        }
    }

    fn remove(&mut self, slot: i64) {
        if let Some(record) = self.table.remove(&slot) {
            if let Some(slots) = self.kinds.get_mut(record.head()) {
                slots.remove(&slot);
            }
            if let Some(owner) =
                attribute_owner(record).filter(|_| record.name.ends_with("-attrib"))
            {
                if let Some(slots) = self.attributes.get_mut(&owner) {
                    slots.remove(&slot);
                }
            }
            for token in record.tokens.iter() {
                if let Token::Ref(reference) = token {
                    if let Some(slots) = self.incoming.get_mut(reference) {
                        slots.remove(&slot);
                    }
                }
            }
            self.subtype_references.remove(&slot);
        }
    }

    fn insert(&mut self, slot: i64, record: &'a Record, references: Vec<Option<usize>>) {
        self.remove(slot);
        self.table.insert(slot, record);
        self.kinds.entry(record.head()).or_default().insert(slot);
        if record.name.ends_with("-attrib") {
            if let Some(owner) = attribute_owner(record) {
                self.attributes.entry(owner).or_default().insert(slot);
            }
        }
        for token in record.tokens.iter() {
            if let Token::Ref(reference) = token {
                if *reference >= 0 {
                    self.incoming.entry(*reference).or_default().insert(slot);
                }
            }
        }
        if !references.is_empty() {
            self.subtype_references.insert(slot, references);
        }
    }

    fn kind(&self, kind: &str) -> impl Iterator<Item = i64> + '_ {
        self.kinds.get(kind).into_iter().flatten().copied()
    }

    fn valid_changes(&self, changed: &Slots) -> bool {
        changed.iter().all(|slot| match self.table.get(slot) {
            Some(record) => record.tokens.iter().all(|token| !matches!(token, Token::Ref(reference) if *reference >= 0 && !self.table.contains_key(reference))),
            None => self.incoming.get(slot).is_none_or(BTreeSet::is_empty),
        })
    }
}

/// Mirror the History walk and emit rules. Dependencies include dangling slots.
fn face_unit(live: &Live<'_>, face: i64) -> FaceUnit {
    let table = &live.table;
    let mut unit = Slots::from([face]);
    let mut reached_edges = Slots::new();
    let Some(record) = table.get(&face) else {
        return FaceUnit {
            slots: unit,
            reached_edges,
        };
    };
    let mut walking = record.ref_at(5).is_some()
        && record
            .ref_at(7)
            .is_some_and(|surface| table.contains_key(&surface));
    // keep_faces_and_carriers reads the surface and tests the shell reference.
    // Keeping a superset also tracks children of currently omitted faces.
    let mut carriers = record.ref_at(7).into_iter().collect::<Vec<_>>();
    let mut loops = record.ref_at(4);
    let mut loop_guard = Slots::new();
    while let Some(slot) = loops {
        unit.insert(slot);
        if !loop_guard.insert(slot) {
            break;
        }
        let Some(lp) = table.get(&slot) else { break };
        // loop_chain follows next even after a wrong-kind loop. Track that tail.
        loops = lp.ref_at(3);
        if lp.head() != "loop" {
            walking = false;
            continue;
        }
        // emit_loops reads its owner face's kept status.
        unit.extend(lp.ref_at(5));
        let mut coedges = lp.ref_at(4);
        let mut guard = Slots::new();
        while let Some(ci) = coedges {
            unit.insert(ci);
            if !guard.insert(ci) {
                break;
            }
            let Some(ce) = table.get(&ci) else { break };
            if !matches!(ce.head(), "coedge" | "tcoedge") {
                break;
            }
            // emit_coedges tests next, previous, partner, edge and owner loop.
            for field in [3, 4, 5, 6, 8] {
                unit.extend(ce.ref_at(field));
            }
            coedges = ce.ref_at(3);
            // History pcurves are placeholders, but retain their carrier refs too.
            let pcurve_field = if matches!(ce.chunk(9), Some(Token::Long(_))) {
                10
            } else {
                9
            };
            carriers.extend(ce.ref_at(pcurve_field));
            if let Some(edge) = ce.ref_at(6).and_then(|ei| table.get(&ei)) {
                if matches!(edge.head(), "edge" | "tedge") {
                    if walking {
                        reached_edges.extend(ce.ref_at(6));
                    }
                    // emit_edges and emit_vertices read both endpoints and points.
                    for field in [3, 5] {
                        if let Some(vi) = edge.ref_at(field) {
                            unit.insert(vi);
                            if let Some(vertex) = table.get(&vi) {
                                if matches!(vertex.head(), "vertex" | "tvertex") {
                                    let point_field =
                                        if matches!(vertex.chunk(4), Some(Token::Long(_))) {
                                            5
                                        } else {
                                            4
                                        };
                                    unit.extend(vertex.ref_at(point_field));
                                }
                            }
                        }
                    }
                    carriers.extend(edge.ref_at(8));
                }
            }
        }
    }
    // Carrier decoders may read referenced records. Never follow topology owners
    // from here. A carrier that names topology makes that step unsupported.
    let mut visited = Slots::new();
    while let Some(slot) = carriers.pop() {
        unit.insert(slot);
        if !visited.insert(slot) {
            continue;
        }
        if let Some(record) = table.get(&slot) {
            if !is_topology(record) {
                carriers.extend(record.tokens.iter().filter_map(|token| match token {
                    Token::Ref(slot) if *slot >= 0 => Some(*slot),
                    _ => None,
                }));
            }
        }
    }
    // emit_attributes follows ref_at(0) and attribute_next, then scans attributes
    // whose owner chain reaches an emitted entity. The owner index reverses it.
    let mut pending = unit
        .iter()
        .copied()
        .map(|slot| (slot, false))
        .collect::<Vec<_>>();
    let mut attributes = BTreeSet::new();
    while let Some((slot, attribute)) = pending.pop() {
        if !attributes.insert((slot, attribute)) {
            continue;
        }
        unit.insert(slot);
        if let Some(record) = table.get(&slot) {
            let next = if attribute {
                attribute_next(record)
            } else {
                record.ref_at(0)
            };
            pending.extend(next.map(|slot| (slot, true)));
        }
        pending.extend(
            live.attributes
                .get(&slot)
                .into_iter()
                .flatten()
                .map(|&slot| (slot, true)),
        );
    }
    FaceUnit {
        slots: unit,
        reached_edges,
    }
}

fn is_topology(record: &Record) -> bool {
    matches!(
        record.head(),
        "body"
            | "lump"
            | "region"
            | "shell"
            | "subshell"
            | "face"
            | "loop"
            | "coedge"
            | "tcoedge"
            | "edge"
            | "tedge"
            | "vertex"
            | "tvertex"
            | "wire"
    )
}

/// Container closure follows complete chains, but does not enter face loops.
fn container_unit(live: &Live<'_>, body: i64) -> Slots {
    let mut unit = Slots::new();
    let mut pending = vec![body];
    while let Some(slot) = pending.pop() {
        if !unit.insert(slot) {
            continue;
        }
        let Some(record) = live.table.get(&slot) else {
            continue;
        };
        if let Some(incoming) = live.incoming.get(&slot) {
            pending.extend(incoming.iter().copied().filter(|child| {
                live.table
                    .get(child)
                    .is_some_and(|child| match (record.head(), child.head()) {
                        ("body", "lump" | "region") => child.ref_at(5) == Some(slot),
                        ("lump" | "region", "shell") => child.ref_at(7) == Some(slot),
                        ("shell" | "subshell", "subshell") => child.ref_at(3) == Some(slot),
                        _ => false,
                    })
            }));
        }
        // face_chain, shell_faces and face admission need next, owner and surface.
        if record.head() == "face" {
            for field in [3, 5, 7] {
                pending.extend(record.ref_at(field));
            }
        } else {
            // Includes region/shell chains, subshell ancestors, wires, their
            // coedge rings, endpoints, carriers, transforms and attribute chains.
            pending.extend(record.tokens.iter().filter_map(|token| match token {
                Token::Ref(slot) if *slot >= 0 => Some(*slot),
                _ => None,
            }));
        }
    }
    unit
}

fn append_containers(target: &mut AsmHistoricalTopology, source: AsmHistoricalTopology) {
    target.bodies = source.bodies;
    target.regions = source.regions;
    target.shells = source.shells;
    target.body_regions = source.body_regions;
    target.region_shells = source.region_shells;
    target.shell_faces = source.shell_faces;
    target.shell_wire_edges = source.shell_wire_edges;
    target.shell_free_vertices = source.shell_free_vertices;
}

fn container_rows(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    live: &Live<'_>,
    slots: &Slots,
    bytes: &[u8],
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    use cadmpeg_asm::brep::{
        emit::{emit_attributes, emit_containers, ContainerInputs},
        AsmBrep, Reachable, WireShellTopology,
    };
    let records: Vec<_> = slots
        .iter()
        .filter_map(|slot| live.table.get(slot))
        .filter(|record| {
            matches!(
                record.head(),
                "body" | "lump" | "region" | "shell" | "subshell" | "face"
            ) || record.name.ends_with("-attrib")
        })
        .map(|record| (*record).clone())
        .collect();
    let by_index: HashMap<_, _> = records
        .iter()
        .filter_map(|record| {
            i64::try_from(record.index)
                .ok()
                .map(|index| (index, record))
        })
        .collect();
    let mut reach = Reachable::default();
    reach.faces = by_index
        .iter()
        .filter(|(_, record)| {
            record.head() == "face"
                && record.ref_at(5).is_some()
                && record
                    .ref_at(7)
                    .is_some_and(|surface| live.table.contains_key(&surface))
        })
        .map(|(&slot, _)| slot)
        .collect();
    let mut out = AsmBrep::default();
    emit_containers(
        ctx,
        &mut out,
        ContainerInputs {
            records: &records,
            bytes,
            by_index: &by_index,
            reach: &reach,
            wire: &WireShellTopology::default(),
            stream: "",
            header_scale: 1.0,
            format: crate::ids::ID_FORMAT,
        },
    )?;
    emit_attributes(&mut out, &records, &by_index, &reach, crate::ids::ID_FORMAT)?;
    Ok(super::historical_topology(&out)?.filter(IndexedTopology::ordered))
}

fn decode_slots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    live: &Live<'_>,
    slots: &Slots,
    bytes: &[u8],
    stats: &mut Statistics,
    subtypes: Option<&cadmpeg_asm::nurbs::toks::SubtypeTable>,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    let records = slots
        .iter()
        .filter_map(|slot| live.table.get(slot).map(|record| (*record).clone()))
        .collect::<Vec<_>>();
    stats.decoded_records += records.len();
    let result = match subtypes {
        Some(table) => crate::brep::decode_history_with_subtypes(ctx, &records, bytes, table),
        None => crate::brep::decode_history_topology(ctx, &records, bytes, crate::ids::ID_FORMAT),
    };
    let decoded = match result {
        Ok(decoded) => decoded,
        Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    Ok(historical_topology_with_tags(&decoded)?.filter(IndexedTopology::ordered))
}

struct Definition {
    owner: (i64, usize),
    tokens: std::sync::Arc<[Token]>,
    position: usize,
}

struct Binder<'a> {
    live: Live<'a>,
    archive: &'a HistoricalRecordArchive,
    units: Units,
    containers: BTreeMap<i64, Slots>,
    container_reverse: BTreeMap<i64, Slots>,
    orphan_containers: Slots,
    wires: Slots,
    revision_subtypes: HashMap<i64, Subtypes>,
    saved_limit: Option<i64>,
    subtype_table: Option<cadmpeg_asm::nurbs::toks::SubtypeTable>,
    definitions: Vec<Definition>,
}

impl<'a> Binder<'a> {
    fn new(archive: &'a HistoricalRecordArchive, saved_limit: Option<i64>) -> Self {
        Self {
            saved_limit,
            subtype_table: None,
            definitions: Vec::new(),
            archive,
            live: Live::new(),
            units: Units::default(),
            containers: BTreeMap::new(),
            container_reverse: BTreeMap::new(),
            orphan_containers: Slots::new(),
            wires: Slots::new(),
            revision_subtypes: archive
                .iter()
                .map(|(&revision, record)| (revision, Subtypes::of(record)))
                .collect(),
        }
    }

    fn definitions_changed(&self, changed: &Slots, versions: &BTreeMap<i64, i64>) -> bool {
        changed.iter().any(|slot| {
            self.definitions
                .binary_search_by_key(slot, |definition| definition.owner.0)
                .is_ok()
                || versions
                    .get(slot)
                    .and_then(|r| self.revision_subtypes.get(r))
                    .is_some_and(|s| !s.definitions.is_empty())
        })
    }

    fn update(&mut self, changed: &Slots, versions: &BTreeMap<i64, i64>) -> Option<()> {
        if self.definitions_changed(changed, versions) {
            self.subtype_table = None;
        }
        for &slot in changed {
            let start = self.definitions.partition_point(|d| d.owner.0 < slot);
            let end = self.definitions.partition_point(|d| d.owner.0 <= slot);
            let definitions = versions
                .get(&slot)
                .and_then(|revision| {
                    Some((
                        self.archive.get(revision)?,
                        self.revision_subtypes.get(revision)?,
                    ))
                })
                .into_iter()
                .flat_map(|(record, meta)| {
                    meta.definitions
                        .iter()
                        .enumerate()
                        .map(move |(ordinal, &position)| Definition {
                            owner: (slot, ordinal),
                            tokens: record.tokens.clone(),
                            position,
                        })
                });
            self.definitions.splice(start..end, definitions);
            self.live.remove(slot);
            if let Some(revision) = versions.get(&slot) {
                let record = self.archive.get(revision)?;
                if i64::try_from(record.index).ok() != Some(slot) {
                    return None;
                }
                self.live.insert(
                    slot,
                    record,
                    self.revision_subtypes.get(revision)?.references.clone(),
                );
            }
        }
        for &slot in changed {
            self.refresh_orphan(slot);
        }
        self.live.valid_changes(changed).then_some(())
    }

    fn resolution_changes(&self, changed: &Slots, versions: &BTreeMap<i64, i64>) -> Slots {
        if !self.definitions_changed(changed, versions) {
            return changed.clone();
        }
        let mut next: BTreeMap<_, _> = self
            .live
            .subtype_references
            .iter()
            .map(|(&slot, references)| (slot, references.as_slice()))
            .collect();
        for slot in changed {
            next.remove(slot);
            if let Some(meta) = versions
                .get(slot)
                .and_then(|r| self.revision_subtypes.get(r))
            {
                if !meta.references.is_empty() {
                    next.insert(*slot, meta.references.as_slice());
                }
            }
        }
        let old: Vec<_> = self
            .definitions
            .iter()
            .map(|definition| definition.owner)
            .collect();
        let mut new = old.clone();
        for &slot in changed {
            let start = new.partition_point(|&(owner, _)| owner < slot);
            let end = new.partition_point(|&(owner, _)| owner <= slot);
            let count = versions
                .get(&slot)
                .and_then(|revision| self.revision_subtypes.get(revision))
                .map_or(0, |meta| meta.definitions.len());
            new.splice(start..end, (0..count).map(|ordinal| (slot, ordinal)));
        }
        let mut out = changed.clone();
        let mut users: BTreeMap<i64, Slots> = BTreeMap::new();
        let resolution = |references: &[Option<usize>], defs: &[(i64, usize)]| -> Vec<_> {
            references
                .iter()
                .map(|index| index.and_then(|i| defs.get(i).copied()))
                .collect()
        };
        for (&slot, meta) in &next {
            let before = self
                .live
                .subtype_references
                .get(&slot)
                .map(|meta| resolution(meta, &old))
                .unwrap_or_default();
            let after = resolution(meta, &new);
            if before != after || before.iter().chain(&after).any(Option::is_none) {
                out.insert(slot);
            }
            for &(owner, _) in before.iter().chain(&after).flatten() {
                users.entry(owner).or_default().insert(slot);
            }
        }
        // A definition can itself refer to a changed definition. Propagate
        // through its owning record before choosing face and container units.
        let mut pending: Vec<_> = out.iter().copied().collect();
        while let Some(owner) = pending.pop() {
            for &slot in users.get(&owner).into_iter().flatten() {
                if out.insert(slot) {
                    pending.push(slot);
                }
            }
        }
        out
    }

    fn rebuild_units(&mut self) {
        self.units = Units::default();
        for face in self.live.kind("face") {
            self.units.insert(face, face_unit(&self.live, face));
        }
        self.wires = wire_slots(&self.live);
        self.containers.clear();
        self.container_reverse.clear();
        self.orphan_containers = self
            .live
            .kind("shell")
            .chain(self.live.kind("lump"))
            .chain(self.live.kind("region"))
            .collect();
        let bodies: Vec<_> = self.live.kind("body").collect();
        for body in bodies {
            self.replace_container(body, container_unit(&self.live, body));
        }
    }

    fn replace_container(&mut self, body: i64, unit: Slots) {
        if let Some(old) = self.containers.remove(&body) {
            for slot in old {
                if let Some(bodies) = self.container_reverse.get_mut(&slot) {
                    bodies.remove(&body);
                    if bodies.is_empty() {
                        self.container_reverse.remove(&slot);
                    }
                }
                self.refresh_orphan(slot);
            }
        }
        if self
            .live
            .table
            .get(&body)
            .is_some_and(|record| record.head() == "body")
        {
            for &slot in &unit {
                self.container_reverse.entry(slot).or_default().insert(body);
                self.refresh_orphan(slot);
            }
            self.containers.insert(body, unit);
        }
    }

    fn refresh_orphan(&mut self, slot: i64) {
        if self
            .live
            .table
            .get(&slot)
            .is_some_and(|record| matches!(record.head(), "shell" | "lump" | "region"))
            && !self.container_reverse.contains_key(&slot)
        {
            self.orphan_containers.insert(slot);
        } else {
            self.orphan_containers.remove(&slot);
        }
    }

    fn container_targets(&self, changed: &Slots) -> Slots {
        let mut bodies = Slots::new();
        let mut pending: Vec<_> = changed.iter().copied().collect();
        let mut visited = Slots::new();
        while let Some(slot) = pending.pop() {
            if !visited.insert(slot) {
                continue;
            }
            bodies.extend(self.container_reverse.get(&slot).into_iter().flatten());
            if let Some(record) = self.live.table.get(&slot) {
                match record.head() {
                    "body" => {
                        bodies.insert(slot);
                    }
                    "lump" | "region" => pending.extend(record.ref_at(5)),
                    "shell" => pending.extend(record.ref_at(7)),
                    "subshell" => pending.extend(record.ref_at(3)),
                    _ => {}
                }
            }
        }
        bodies
    }

    fn attribute_targets(&self, changed: &Slots) -> Slots {
        let mut out = changed.clone();
        for slot in changed {
            let mut owner = Some(*slot);
            let mut visited = Slots::new();
            while let Some(slot) = owner {
                if !visited.insert(slot) {
                    break;
                }
                out.insert(slot);
                owner = self
                    .live
                    .table
                    .get(&slot)
                    .filter(|record| record.name.ends_with("-attrib"))
                    .and_then(|record| attribute_owner(record));
            }
        }
        out
    }

    /// Replace keyed rows and return the two sides of this transition.
    fn incremental(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        previous: &mut IndexedTopology,
        changed: &Slots,
        versions: &BTreeMap<i64, i64>,
        bytes: &[u8],
        stats: &mut Statistics,
    ) -> Result<Option<(AsmHistoricalTopology, AsmHistoricalTopology)>, cadmpeg_core::CodecError>
    {
        let revisions = changed;
        let expanded = self.resolution_changes(changed, versions);
        let changed = &expanded;
        let saved_containers = previous.bodies().any(|slot| {
            self.live
                .table
                .get(slot)
                .is_none_or(|record| record.head() != "body")
        });
        let orphan_containers = !self.orphan_containers.is_empty();
        let old_targets = self.attribute_targets(changed);
        let mut dirty = self.units.touching(&old_targets);
        let old_changed_edges: Slots = revisions
            .iter()
            .filter(|slot| {
                self.live
                    .table
                    .get(slot)
                    .is_some_and(|record| matches!(record.head(), "edge" | "tedge"))
            })
            .copied()
            .collect();
        let mut affected_bodies = self.container_targets(changed);
        let old_container_covered: Slots = changed
            .iter()
            .filter(|slot| self.container_reverse.contains_key(slot))
            .copied()
            .collect();
        let mut wire_changed = !changed.is_disjoint(&self.wires);
        if self.update(revisions, versions).is_none() {
            return Ok(None);
        }
        let targets = self.attribute_targets(changed);
        dirty.extend(self.units.touching(&targets));
        dirty.extend(
            targets
                .iter()
                .filter(|slot| {
                    self.live
                        .table
                        .get(slot)
                        .is_some_and(|record| record.head() == "face")
                })
                .copied(),
        );
        let mut next_units = Vec::new();
        for &face in &dirty {
            if self
                .live
                .table
                .get(&face)
                .is_some_and(|record| record.head() == "face")
            {
                next_units.push((face, face_unit(&self.live, face)));
            }
        }
        affected_bodies.extend(self.container_targets(changed));
        let mut container_keys = Slots::new();
        let mut container_input = Slots::new();
        for body in affected_bodies {
            if let Some(unit) = self.containers.get(&body) {
                container_keys.extend(unit);
            }
            let unit = if self
                .live
                .table
                .get(&body)
                .is_some_and(|record| record.head() == "body")
            {
                container_unit(&self.live, body)
            } else {
                Slots::new()
            };
            container_keys.extend(&unit);
            container_input.extend(&unit);
            self.replace_container(body, unit);
        }
        if wire_changed
            || revisions.iter().any(|slot| {
                self.live
                    .table
                    .get(slot)
                    .is_some_and(|record| record.head() == "wire")
            })
        {
            self.wires = wire_slots(&self.live);
            wire_changed |= !changed.is_disjoint(&self.wires);
        }
        let old_covered = changed.iter().all(|slot| {
            self.units.reverse.contains_key(slot)
                || old_container_covered.contains(slot)
                || self.container_reverse.contains_key(slot)
                || next_units.iter().any(|(_, unit)| unit.slots.contains(slot))
        });
        // Only a rebuilt face unit can change an edge's face ownership.
        let old_edge_ownership: BTreeMap<_, _> = dirty
            .iter()
            .filter_map(|face| self.units.edges_by_face.get(face))
            .flatten()
            .copied()
            .chain(
                next_units
                    .iter()
                    .flat_map(|(_, unit)| unit.reached_edges.iter().copied()),
            )
            .filter(|slot| {
                self.saved_limit
                    .is_some_and(|limit| (1..limit).contains(slot))
            })
            .filter(|slot| {
                if revisions.contains(slot) {
                    old_changed_edges.contains(slot)
                } else {
                    self.live
                        .table
                        .get(slot)
                        .is_some_and(|record| matches!(record.head(), "edge" | "tedge"))
                }
            })
            .map(|slot| (slot, self.units.edge_users.contains_key(&slot)))
            .collect();
        let mut first = dirty.clone();
        first.extend(self.units.neighbours(&dirty));
        let old_first = self.units.slots(&first);
        let mut old_second = self.units.neighbours(&first);
        for &face in &dirty {
            self.units.remove(face);
        }
        for (face, unit) in next_units {
            self.units.insert(face, unit);
        }
        first.extend(self.units.neighbours(&dirty));
        // A B-side neighbour may have A-side neighbours absent from the first
        // expansion. Its unchanged unit is already present in both indexes.
        let mut keys = old_first;
        keys.extend(self.units.slots(&first));
        old_second.extend(self.units.neighbours(&first));
        old_second.extend(first);
        let input = self.units.slots(&old_second);
        let saved_ownership_changed = old_edge_ownership.iter().any(|(slot, was_reached)| {
            self.live
                .table
                .get(slot)
                .is_some_and(|record| matches!(record.head(), "edge" | "tedge"))
                && *was_reached != self.units.edge_users.contains_key(slot)
        });
        let orphan_containers = orphan_containers || !self.orphan_containers.is_empty();
        let reason = if orphan_containers {
            Some("orphan-container")
        } else if saved_containers && !container_keys.is_empty() {
            Some("saved-free-containers")
        } else if saved_ownership_changed {
            Some("saved-edge-ownership")
        } else if wire_changed {
            Some("wire-change")
        } else if !old_covered {
            Some("uncovered-change")
        } else if input.iter().any(|slot| {
            self.live.table.get(slot).is_some_and(|record| {
                matches!(
                    record.head(),
                    "body" | "lump" | "region" | "shell" | "subshell" | "wire"
                )
            })
        }) || self.ambient_dependency(&input)
        {
            Some("global-dependency")
        } else if container_input.iter().any(|slot| {
            self.live.table.get(slot).is_some_and(|record| {
                record.head() == "shell"
                    && (record.ref_at(7).is_none() || !shell_nonempty(&self.live, record))
            })
        }) {
            Some("empty-shell")
        } else if container_input.iter().any(|slot| {
            self.saved_limit
                .is_some_and(|limit| (1..limit).contains(slot))
                && self
                    .live
                    .table
                    .get(slot)
                    .is_some_and(|record| matches!(record.head(), "edge" | "tedge"))
                && self.units.edge_users.get(slot).is_some_and(|faces| {
                    !faces.iter().any(|face| {
                        self.units
                            .faces
                            .get(face)
                            .is_some_and(|unit| unit.is_subset(&container_input))
                    })
                })
        }) {
            Some("container-saved-edge")
        } else {
            None
        };
        if let Some(reason) = reason {
            *stats.fallbacks.entry(reason).or_default() += 1;
            return self.replace_full(ctx, previous, bytes, stats);
        }
        if self.subtype_table.is_none() {
            let definitions = self
                .definitions
                .iter()
                .map(|definition| (definition.tokens.clone(), definition.position))
                .collect();
            let version = cadmpeg_asm::asm_header::parse(ctx, bytes)?
                .and_then(|h| h.metadata.save_format_version);
            self.subtype_table = Some(
                cadmpeg_asm::nurbs::toks::SubtypeTable::from_definitions(definitions)
                    .with_save_format_version(version),
            );
        }
        let Some(part) = decode_slots(
            ctx,
            &self.live,
            &input,
            bytes,
            stats,
            self.subtype_table.as_ref(),
        )?
        else {
            *stats.fallbacks.entry("subset-decode").or_default() += 1;
            return self.replace_full(ctx, previous, bytes, stats);
        };
        let fast_containers = self.wires.is_empty()
            && container_input.iter().all(|slot| {
                self.live.table.get(slot).is_none_or(|record| {
                    !matches!(
                        record.head(),
                        "wire"
                            | "loop"
                            | "coedge"
                            | "tcoedge"
                            | "edge"
                            | "tedge"
                            | "vertex"
                            | "tvertex"
                    ) && (record.head() != "face"
                        || record
                            .ref_at(3)
                            .and_then(|next| self.live.table.get(&next))
                            .is_none_or(|next| next.head() == "face"))
                        && record
                            .ref_at(0)
                            .and_then(|attribute| self.live.table.get(&attribute))
                            .is_none_or(|attribute| attribute.name.ends_with("-attrib"))
                })
            });
        let container_part = if container_keys.is_empty() {
            None
        } else if fast_containers {
            match container_rows(ctx, &self.live, &container_input, bytes) {
                Ok(Some(part)) => Some(part),
                Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
                Ok(None) | Err(_) => {
                    *stats.fallbacks.entry("container-decode").or_default() += 1;
                    return self.replace_full(ctx, previous, bytes, stats);
                }
            }
        } else {
            *stats
                .fallbacks
                .entry("container-complex-decode")
                .or_default() += 1;
            let Some(part) = decode_slots(
                ctx,
                &self.live,
                &container_input,
                bytes,
                stats,
                self.subtype_table.as_ref(),
            )?
            else {
                *stats.fallbacks.entry("container-decode").or_default() += 1;
                return self.replace_full(ctx, previous, bytes, stats);
            };
            Some(part)
        };
        let mut prior = previous.project(&keys, false);
        let containers = previous.project(&container_keys, true);
        append_containers(&mut prior, containers);
        previous.replace(part, &keys, false);
        if let Some(part) = container_part {
            previous.replace(part, &container_keys, true);
        }
        let mut older = previous.project(&keys, false);
        append_containers(&mut older, previous.project(&container_keys, true));
        Ok(Some((older, prior)))
    }

    fn replace_full(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        previous: &mut IndexedTopology,
        bytes: &[u8],
        stats: &mut Statistics,
    ) -> Result<Option<(AsmHistoricalTopology, AsmHistoricalTopology)>, cadmpeg_core::CodecError>
    {
        let Some(topology) = self.full(ctx, bytes, stats)? else {
            return Ok(None);
        };
        let prior = std::mem::replace(previous, topology.clone().into()).materialize();
        Ok(Some((topology, prior)))
    }

    fn ambient_dependency(&self, input: &Slots) -> bool {
        input.iter().any(|slot| {
            let Some(record) = self.live.table.get(slot) else {
                return false;
            };
            if self
                .live
                .incoming
                .get(slot)
                .into_iter()
                .flatten()
                .any(|owner| {
                    !input.contains(owner)
                        && self
                            .live
                            .table
                            .get(owner)
                            .is_some_and(|candidate| candidate.ref_at(0) == Some(*slot))
                })
            {
                return true;
            }
            // Shared attributes must retain the global record-order winner.
            if record.name.ends_with("-attrib") {
                return self
                    .live
                    .incoming
                    .get(slot)
                    .into_iter()
                    .flatten()
                    .any(|owner| !input.contains(owner));
            }
            // classify_edge_curve_senses also sees saved and wire-only edges.
            self.live
                .incoming
                .get(slot)
                .into_iter()
                .flatten()
                .any(|owner| {
                    self.live.table.get(owner).is_some_and(|candidate| {
                        (matches!(candidate.head(), "edge" | "tedge")
                            && [3, 5, 8]
                                .iter()
                                .any(|&field| candidate.ref_at(field) == Some(*slot))
                            && !self.units.edge_users.contains_key(owner))
                            || (matches!(candidate.head(), "vertex" | "tvertex")
                                && !self.units.reverse.contains_key(owner))
                    })
                })
        })
    }

    fn full(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &[u8],
        stats: &mut Statistics,
    ) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
        decode_slots(
            ctx,
            &self.live,
            &self.live.table.keys().copied().collect(),
            bytes,
            stats,
            None,
        )
    }
}

/// Avoid scratch retention by proving that each shell emits a container row.
fn shell_nonempty(live: &Live<'_>, shell: &Record) -> bool {
    let mut face_roots = shell.ref_at(5).into_iter().collect::<Vec<_>>();
    let mut wires = shell.ref_at(6).into_iter().collect::<Vec<_>>();
    let mut subshells = shell.ref_at(4).into_iter().collect::<Vec<_>>();
    let mut visited = Slots::new();
    while let Some(slot) = subshells.pop() {
        if !visited.insert(slot) {
            return false;
        }
        if let Some(record) = live
            .table
            .get(&slot)
            .filter(|record| record.head() == "subshell")
        {
            face_roots.extend(record.ref_at(6));
            wires.extend(record.ref_at(7));
            subshells.extend(record.ref_at(4));
            subshells.extend(record.ref_at(5));
        } else {
            return false;
        }
    }
    for mut current in face_roots.into_iter().map(Some) {
        let mut visited = Slots::new();
        while let Some(slot) = current {
            if !visited.insert(slot) {
                break;
            }
            let Some(record) = live.table.get(&slot) else {
                break;
            };
            if record.head() == "face"
                && record.ref_at(5).is_some()
                && record
                    .ref_at(7)
                    .is_some_and(|surface| live.table.contains_key(&surface))
            {
                return true;
            }
            current = record.ref_at(3);
        }
    }
    for mut current in wires.into_iter().map(Some) {
        let mut visited = Slots::new();
        while let Some(slot) = current {
            if !visited.insert(slot) {
                break;
            }
            let Some(wire) = live
                .table
                .get(&slot)
                .filter(|record| record.head() == "wire")
            else {
                break;
            };
            let mut coedge = wire.ref_at(4);
            let mut coedges = Slots::new();
            while let Some(slot) = coedge {
                if !coedges.insert(slot) {
                    break;
                }
                let Some(record) = live
                    .table
                    .get(&slot)
                    .filter(|record| matches!(record.head(), "coedge" | "tcoedge"))
                else {
                    break;
                };
                if record.ref_at(6).is_some() {
                    return true;
                }
                coedge = record.ref_at(3);
            }
            if wire.ref_at(4).is_none()
                && wire
                    .ref_at(6)
                    .and_then(|slot| live.table.get(&slot))
                    .is_some_and(|record| matches!(record.head(), "vertex" | "tvertex"))
            {
                return true;
            }
            current = wire.ref_at(3);
        }
    }
    false
}

/// Wire reachability affects non-container rows, which the container splice omits.
fn wire_slots(live: &Live<'_>) -> Slots {
    let mut unit = Slots::new();
    let mut pending = live.kind("wire").collect::<Vec<_>>();
    if pending.is_empty() {
        return unit;
    }
    // A shell or subshell link can add or remove a complete wire walk.
    unit.extend(live.kind("shell"));
    unit.extend(live.kind("subshell"));
    while let Some(slot) = pending.pop() {
        if !unit.insert(slot) {
            continue;
        }
        let Some(record) = live.table.get(&slot) else {
            continue;
        };
        let fields: &[usize] = match record.head() {
            "wire" => &[0, 3, 4, 6],
            "coedge" | "tcoedge" => &[0, 3, 6],
            "edge" | "tedge" => &[0, 3, 5, 8],
            "vertex" | "tvertex" => {
                if matches!(record.chunk(4), Some(Token::Long(_))) {
                    &[0, 5]
                } else {
                    &[0, 4]
                }
            }
            "body" | "lump" | "region" | "shell" | "subshell" | "face" | "loop" => &[],
            _ => {
                pending.extend(record.tokens.iter().filter_map(|token| match token {
                    Token::Ref(slot) if *slot >= 0 => Some(*slot),
                    _ => None,
                }));
                &[]
            }
        };
        for &field in fields {
            pending.extend(record.ref_at(field));
        }
        pending.extend(live.attributes.get(&slot).into_iter().flatten().copied());
    }
    unit
}

#[cfg(test)]
mod tests;
