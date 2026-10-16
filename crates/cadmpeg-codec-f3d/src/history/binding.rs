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
    let mut newer: Option<(usize, AsmHistoricalTopology, BTreeMap<i64, i64>)> = None;
    let mut transitions = Vec::new();
    let mut complete = true;
    let mut failure = None;
    let walked = walk_entity_versions(states, active_count, &archived_ids, |ordinal, versions| {
        if !complete {
            return;
        }
        stats.full_records += versions.len();
        let decoded = if let Some((newer_index, topology, _)) = &newer {
            stats.steps += 1;
            let changed = states[*newer_index]
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
                .collect();
            binder.incremental(ctx, topology, &changed, versions, bytes, &mut stats)
        } else {
            let slots = versions.keys().copied().collect();
            if binder.update(&slots, versions).is_some() {
                binder.rebuild_units();
                binder.full(ctx, bytes, &mut stats)
            } else {
                Ok(None)
            }
        };
        let topology = match decoded {
            Ok(Some(topology)) => topology,
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
        if let Some((newer_index, newer_topology, newer_versions)) = newer.take() {
            let older = crate::history_records::AsmStateSnapshot {
                state_id: states[ordinal].state_id,
                topology: &topology,
                versions,
            };
            let new = crate::history_records::AsmStateSnapshot {
                state_id: states[newer_index].state_id,
                topology: &newer_topology,
                versions: &newer_versions,
            };
            let Some(transition) = AsmHistoricalTransition::between(Some(older), new) else {
                complete = false;
                return;
            };
            transitions.push((newer_index, transition));
        }
        newer = Some((ordinal, topology, versions.clone()));
    });
    if let Some(error) = failure {
        return Err(error);
    }
    if !walked || !complete {
        return Ok(stats);
    }
    if let Some((ordinal, topology, versions)) = newer {
        let Some(transition) = AsmHistoricalTransition::between(
            None,
            crate::history_records::AsmStateSnapshot {
                state_id: states[ordinal].state_id,
                topology: &topology,
                versions: &versions,
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

#[derive(Clone, Copy, Default)]
struct Subtypes {
    definitions: bool,
    references: bool,
}

impl Subtypes {
    fn of(record: &Record) -> Self {
        let mut flags = Self::default();
        for pair in record.tokens.windows(2) {
            if matches!(pair[0], Token::SubtypeOpen) {
                match &pair[1] {
                    Token::Ident(name) | Token::SubIdent(name) => {
                        flags.references |= name == "ref";
                        flags.definitions |= name != "ref";
                    }
                    Token::Long(_) => flags.references = true,
                    _ => {}
                }
            }
        }
        flags
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
    subtype_references: Slots,
    subtype_definitions: Slots,
}

impl<'a> Live<'a> {
    fn new() -> Self {
        Self {
            table: BTreeMap::new(),
            kinds: BTreeMap::new(),
            attributes: BTreeMap::new(),
            incoming: BTreeMap::new(),
            subtype_references: Slots::new(),
            subtype_definitions: Slots::new(),
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
            self.subtype_definitions.remove(&slot);
        }
    }

    fn insert(&mut self, slot: i64, record: &'a Record, flags: Subtypes) {
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
        if flags.references {
            self.subtype_references.insert(slot);
        }
        if flags.definitions {
            self.subtype_definitions.insert(slot);
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

fn replace_rows<T: Clone>(old: &mut Vec<T>, new: &[T], keys: &Slots, key: impl Fn(&T) -> i64) {
    let retained = std::mem::take(old);
    let mut left = retained
        .into_iter()
        .filter(|row| !keys.contains(&key(row)))
        .peekable();
    let mut right = new.iter().filter(|row| keys.contains(&key(row))).peekable();
    while let (Some(a), Some(b)) = (left.peek(), right.peek()) {
        if key(a) <= key(b) {
            if let Some(row) = left.next() {
                old.push(row);
            }
        } else if let Some(row) = right.next() {
            old.push(row.clone());
        }
    }
    old.extend(left);
    old.extend(right.cloned());
}

fn splice(
    topology: &mut AsmHistoricalTopology,
    part: &AsmHistoricalTopology,
    keys: &Slots,
    containers: bool,
) {
    macro_rules! members {
        ($($field:ident),* $(,)?) => { $(replace_rows(&mut topology.$field, &part.$field, keys, |slot| *slot);)* };
    }
    macro_rules! rows {
        ($($field:ident => $key:ident),* $(,)?) => { $(replace_rows(&mut topology.$field, &part.$field, keys, |row| row.$key);)* };
    }
    if containers {
        members!(bodies, regions, shells);
        rows!(body_regions => owner_ref, region_shells => owner_ref, shell_faces => owner_ref,
            shell_wire_edges => owner_ref, shell_free_vertices => owner_ref);
    } else {
        members!(faces, loops, coedges, edges, vertices, points, surfaces, curves, pcurves);
        rows!(surface_radii => surface, surface_cylinders => surface, surface_planes => surface,
            surface_axes => surface, curve_axes => curve, persistent_subentity_tags => entity_ref,
            face_loops => owner_ref, loop_coedges => owner_ref, coedge_topology => coedge,
            edge_vertices => edge, face_surfaces => entity, edge_curves => entity,
            coedge_pcurves => entity, vertex_points => entity, point_positions => point);
    }
}

fn decode_slots(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    live: &Live<'_>,
    slots: &Slots,
    bytes: &[u8],
    stats: &mut Statistics,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    let records = slots
        .iter()
        .filter_map(|slot| live.table.get(slot).map(|record| (*record).clone()))
        .collect::<Vec<_>>();
    stats.decoded_records += records.len();
    let decoded =
        match crate::brep::decode_history_topology(ctx, &records, bytes, crate::ids::ID_FORMAT) {
            Ok(decoded) => decoded,
            Err(error @ cadmpeg_core::CodecError::ResourceLimit(_)) => return Err(error),
            Err(_) => return Ok(None),
        };
    historical_topology_with_tags(&decoded)
}

struct Binder<'a> {
    live: Live<'a>,
    archive: &'a HistoricalRecordArchive,
    units: Units,
    containers: BTreeMap<i64, Slots>,
    flags: HashMap<i64, Subtypes>,
    saved_limit: Option<i64>,
}

impl<'a> Binder<'a> {
    fn new(archive: &'a HistoricalRecordArchive, saved_limit: Option<i64>) -> Self {
        Self {
            saved_limit,
            archive,
            live: Live::new(),
            units: Units::default(),
            containers: BTreeMap::new(),
            flags: archive
                .iter()
                .map(|(&revision, record)| (revision, Subtypes::of(record)))
                .collect(),
        }
    }

    fn update(&mut self, changed: &Slots, versions: &BTreeMap<i64, i64>) -> Option<()> {
        for &slot in changed {
            self.live.remove(slot);
            if let Some(revision) = versions.get(&slot) {
                let record = self.archive.get(revision)?;
                if i64::try_from(record.index).ok() != Some(slot) {
                    return None;
                }
                self.live.insert(slot, record, *self.flags.get(revision)?);
            }
        }
        self.live.valid_changes(changed).then_some(())
    }

    fn rebuild_units(&mut self) {
        self.units = Units::default();
        for face in self.live.kind("face") {
            self.units.insert(face, face_unit(&self.live, face));
        }
        self.containers = self
            .live
            .kind("body")
            .map(|body| (body, container_unit(&self.live, body)))
            .collect();
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

    fn incremental(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        previous: &AsmHistoricalTopology,
        changed: &Slots,
        versions: &BTreeMap<i64, i64>,
        bytes: &[u8],
        stats: &mut Statistics,
    ) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
        let saved_containers = previous.bodies.iter().any(|slot| {
            self.live
                .table
                .get(slot)
                .is_none_or(|record| record.head() != "body")
        });
        let orphan_containers = self
            .live
            .kind("shell")
            .chain(self.live.kind("lump"))
            .chain(self.live.kind("region"))
            .any(|slot| !self.containers.values().any(|unit| unit.contains(&slot)));
        let old_targets = self.attribute_targets(changed);
        let mut dirty = self.units.touching(&old_targets);
        let mut definition_changed = false;
        for slot in changed {
            definition_changed |= self.live.subtype_definitions.contains(slot);
            if let Some(flags) = versions
                .get(slot)
                .and_then(|revision| self.flags.get(revision))
            {
                definition_changed |= flags.definitions;
            }
        }
        let old_face_edges: Slots = self
            .units
            .edge_users
            .keys()
            .filter(|&&slot| {
                self.saved_limit
                    .is_some_and(|limit| (1..limit).contains(&slot))
                    && self
                        .live
                        .table
                        .get(&slot)
                        .is_some_and(|record| matches!(record.head(), "edge" | "tedge"))
            })
            .copied()
            .collect();
        let old_eligible_edges: Slots = self
            .live
            .kind("edge")
            .chain(self.live.kind("tedge"))
            .filter(|slot| {
                self.saved_limit
                    .is_some_and(|limit| (1..limit).contains(slot))
            })
            .collect();
        let old_wire_slots = wire_slots(&self.live);
        if self.update(changed, versions).is_none() {
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
        let old_container_slots: Slots = self.containers.values().flatten().copied().collect();
        let next_containers: BTreeMap<_, _> = self
            .live
            .kind("body")
            .map(|body| (body, container_unit(&self.live, body)))
            .collect();
        let new_container_slots: Slots = next_containers.values().flatten().copied().collect();
        let mut container_keys = Slots::new();
        let affected_bodies: Slots = self
            .containers
            .iter()
            .chain(&next_containers)
            .filter(|(_, unit)| !unit.is_disjoint(changed))
            .map(|(&body, _)| body)
            .collect();
        let mut container_input = Slots::new();
        for body in &affected_bodies {
            if let Some(unit) = self.containers.get(body) {
                container_keys.extend(unit);
            }
            if let Some(unit) = next_containers.get(body) {
                container_keys.extend(unit);
                container_input.extend(unit);
            }
        }
        let wire_changed =
            !changed.is_disjoint(&old_wire_slots) || !changed.is_disjoint(&wire_slots(&self.live));
        let old_covered = changed.iter().all(|slot| {
            self.units.reverse.contains_key(slot)
                || old_container_slots.contains(slot)
                || new_container_slots.contains(slot)
                || next_units.iter().any(|(_, unit)| unit.slots.contains(slot))
        });
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
        self.containers = next_containers;
        let saved_ownership_changed = old_eligible_edges.iter().any(|slot| {
            self.live
                .table
                .get(slot)
                .is_some_and(|record| matches!(record.head(), "edge" | "tedge"))
                && old_face_edges.contains(slot) != self.units.edge_users.contains_key(slot)
        });
        let orphan_containers = orphan_containers
            || self
                .live
                .kind("shell")
                .chain(self.live.kind("lump"))
                .chain(self.live.kind("region"))
                .any(|slot| !self.containers.values().any(|unit| unit.contains(&slot)));
        let reason = if orphan_containers {
            Some("orphan-container")
        } else if saved_containers && !container_keys.is_empty() {
            Some("saved-free-containers")
        } else if saved_ownership_changed {
            Some("saved-edge-ownership")
        } else if wire_changed {
            Some("wire-change")
        } else if definition_changed && !self.live.subtype_references.is_empty() {
            Some("changed-subtype-definition")
        } else if !input.is_disjoint(&self.live.subtype_references)
            || !container_input.is_disjoint(&self.live.subtype_references)
        {
            Some("subset-subtype-reference")
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
            return self.full(ctx, bytes, stats);
        }
        let Some(part) = decode_slots(ctx, &self.live, &input, bytes, stats)? else {
            *stats.fallbacks.entry("subset-decode").or_default() += 1;
            return self.full(ctx, bytes, stats);
        };
        let mut topology = previous.clone();
        splice(&mut topology, &part, &keys, false);
        if !container_keys.is_empty() {
            let Some(part) = decode_slots(ctx, &self.live, &container_input, bytes, stats)? else {
                *stats.fallbacks.entry("container-decode").or_default() += 1;
                return self.full(ctx, bytes, stats);
            };
            splice(&mut topology, &part, &container_keys, true);
        }
        Ok(Some(topology))
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
