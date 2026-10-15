// SPDX-License-Identifier: Apache-2.0
//! Sorted historical row diffs and lazy chain materialization.
use super::{
    AsmDeltaState, AsmEntityVersion, AsmHistoricalEntityDelta, AsmHistoricalEntityTags,
    AsmHistoricalRow, AsmHistoricalRowDelta, AsmHistoricalRowsDelta, AsmHistoricalTopology,
    AsmHistoricalTopologyDelta, AsmHistoricalTransition, AsmHistory, AsmTopologyScope,
    AsmTransitionError,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::OnceLock;

/// One complete state as the diff reads it.
#[derive(Clone, Copy)]
pub(crate) struct AsmStateSnapshot<'a> {
    pub(crate) state_id: i64,
    pub(crate) topology: &'a AsmHistoricalTopology,
    pub(crate) versions: &'a BTreeMap<i64, i64>,
}

/// Derived state topologies. Clones start without materializations.
#[derive(Default)]
pub(crate) struct MaterializedTopologies {
    states: OnceLock<Vec<OnceLock<Option<Box<MaterializedState>>>>>,
    order: OnceLock<Result<Option<Vec<usize>>, AsmTransitionError>>,
}

#[derive(Clone, Default)]
struct MaterializedState {
    topology: AsmHistoricalTopology,
    versions: Vec<AsmEntityVersion>,
}

impl Clone for MaterializedTopologies {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl PartialEq for MaterializedTopologies {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl std::fmt::Debug for MaterializedTopologies {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MaterializedTopologies")
    }
}

fn strictly_sorted(keys: impl IntoIterator<Item = i64>) -> bool {
    let mut previous = None;
    for key in keys {
        if previous.is_some_and(|previous| previous >= key) {
            return false;
        }
        previous = Some(key);
    }
    true
}

fn entity_delta(
    newer: &[i64],
    older: &[i64],
    new: &BTreeMap<i64, i64>,
    old: &BTreeMap<i64, i64>,
) -> Option<AsmHistoricalEntityDelta> {
    if !strictly_sorted(newer.iter().copied()) || !strictly_sorted(older.iter().copied()) {
        return None;
    }
    let new_keys: BTreeSet<_> = newer.iter().copied().collect();
    let old_keys: BTreeSet<_> = older.iter().copied().collect();
    Some(AsmHistoricalEntityDelta {
        inserted: new_keys.difference(&old_keys).copied().collect(),
        deleted: old_keys.difference(&new_keys).copied().collect(),
        updated: new_keys
            .intersection(&old_keys)
            .copied()
            .filter(|key| new.get(key) != old.get(key))
            .collect(),
    })
}

impl<R: AsmHistoricalRow> AsmHistoricalRowDelta<R> {
    fn between(older: &[R], newer: &[R]) -> Option<Self> {
        if !strictly_sorted(older.iter().map(R::key)) || !strictly_sorted(newer.iter().map(R::key))
        {
            return None;
        }
        let old: BTreeMap<_, _> = older.iter().map(|row| (row.key(), row)).collect();
        let new: BTreeMap<_, _> = newer.iter().map(|row| (row.key(), row)).collect();
        Some(Self {
            set: newer
                .iter()
                .filter(|row| old.get(&row.key()).copied() != Some(*row))
                .cloned()
                .collect(),
            removed: old
                .keys()
                .filter(|key| !new.contains_key(key))
                .copied()
                .collect(),
        })
    }

    fn apply(&self, family: &mut Vec<R>) -> Result<(), AsmTransitionError> {
        if self.is_empty() {
            return Ok(());
        }
        if !strictly_sorted(self.set.iter().map(R::key))
            || !strictly_sorted(self.removed.iter().copied())
        {
            return Err(AsmTransitionError::UnsortedFamily);
        }
        let mut old = std::mem::take(family).into_iter().peekable();
        let mut set = self.set.iter().peekable();
        let mut removed = self.removed.iter().peekable();
        while old.peek().is_some() || set.peek().is_some() {
            let old_key = old.peek().map(R::key);
            let set_key = set.peek().map(|row| row.key());
            let key = match (old_key, set_key) {
                (Some(old), Some(new)) => old.min(new),
                (Some(key), None) | (None, Some(key)) => key,
                (None, None) => break,
            };
            if removed.peek().is_some_and(|&&removed| removed < key) {
                return Err(AsmTransitionError::RemovedRowAbsent);
            }
            let remove = removed.peek().is_some_and(|&&removed| removed == key);
            if remove {
                if old_key != Some(key) {
                    return Err(AsmTransitionError::RemovedRowAbsent);
                }
                removed.next();
            }
            let previous = if old_key == Some(key) {
                old.next()
            } else {
                None
            };
            if set_key == Some(key) {
                if let Some(row) = set.next() {
                    family.push(row.clone());
                }
            } else if !remove {
                family.extend(previous);
            }
        }
        if removed.next().is_some() {
            return Err(AsmTransitionError::RemovedRowAbsent);
        }
        Ok(())
    }
}

fn entity_tags(topology: &AsmHistoricalTopology) -> Vec<AsmHistoricalEntityTags> {
    let mut groups: Vec<AsmHistoricalEntityTags> = Vec::new();
    for tag in &topology.persistent_subentity_tags {
        if let Some(last) = groups
            .last_mut()
            .filter(|last| last.entity_ref == tag.entity_ref)
        {
            last.tags.push(tag.clone());
        } else {
            groups.push(AsmHistoricalEntityTags {
                entity_ref: tag.entity_ref,
                tags: vec![tag.clone()],
            });
        }
    }
    groups
}

impl AsmHistoricalTransition {
    /// Forward change from `older`, or empty before the tail, to `newer`.
    pub(crate) fn between(
        older: Option<AsmStateSnapshot<'_>>,
        newer: AsmStateSnapshot<'_>,
    ) -> Option<Self> {
        let empty = AsmHistoricalTopology::default();
        let empty_versions = BTreeMap::new();
        let old_topology = older.as_ref().map_or(&empty, |state| state.topology);
        let old_versions = older
            .as_ref()
            .map_or(&empty_versions, |state| state.versions);
        let new = newer.topology;
        let delta = |new: &[i64], old: &[i64]| entity_delta(new, old, newer.versions, old_versions);
        let records = delta(
            &newer.versions.keys().copied().collect::<Vec<_>>(),
            &old_versions.keys().copied().collect::<Vec<_>>(),
        )?;
        let revisions: BTreeSet<_> = records
            .inserted
            .iter()
            .chain(&records.updated)
            .copied()
            .collect();
        let record_revisions = revisions
            .into_iter()
            .map(|entity_ref| {
                Some(AsmEntityVersion {
                    entity_ref,
                    record_ref: *newer.versions.get(&entity_ref)?,
                })
            })
            .collect::<Option<_>>()?;
        Some(Self {
            previous_state_id: older.map(|state| state.state_id),
            records,
            record_revisions,
            topology: AsmHistoricalTopologyDelta {
                bodies: delta(&new.bodies, &old_topology.bodies)?,
                regions: delta(&new.regions, &old_topology.regions)?,
                shells: delta(&new.shells, &old_topology.shells)?,
                faces: delta(&new.faces, &old_topology.faces)?,
                loops: delta(&new.loops, &old_topology.loops)?,
                coedges: delta(&new.coedges, &old_topology.coedges)?,
                edges: delta(&new.edges, &old_topology.edges)?,
                vertices: delta(&new.vertices, &old_topology.vertices)?,
                points: delta(&new.points, &old_topology.points)?,
                surfaces: delta(&new.surfaces, &old_topology.surfaces)?,
                curves: delta(&new.curves, &old_topology.curves)?,
                pcurves: delta(&new.pcurves, &old_topology.pcurves)?,
            },
            rows: AsmHistoricalRowsDelta {
                surface_radii: AsmHistoricalRowDelta::between(
                    &old_topology.surface_radii,
                    &new.surface_radii,
                )?,
                surface_cylinders: AsmHistoricalRowDelta::between(
                    &old_topology.surface_cylinders,
                    &new.surface_cylinders,
                )?,
                surface_planes: AsmHistoricalRowDelta::between(
                    &old_topology.surface_planes,
                    &new.surface_planes,
                )?,
                surface_axes: AsmHistoricalRowDelta::between(
                    &old_topology.surface_axes,
                    &new.surface_axes,
                )?,
                curve_axes: AsmHistoricalRowDelta::between(
                    &old_topology.curve_axes,
                    &new.curve_axes,
                )?,
                persistent_subentity_tags: AsmHistoricalRowDelta::between(
                    &entity_tags(old_topology),
                    &entity_tags(new),
                )?,
                body_regions: AsmHistoricalRowDelta::between(
                    &old_topology.body_regions,
                    &new.body_regions,
                )?,
                region_shells: AsmHistoricalRowDelta::between(
                    &old_topology.region_shells,
                    &new.region_shells,
                )?,
                shell_faces: AsmHistoricalRowDelta::between(
                    &old_topology.shell_faces,
                    &new.shell_faces,
                )?,
                shell_wire_edges: AsmHistoricalRowDelta::between(
                    &old_topology.shell_wire_edges,
                    &new.shell_wire_edges,
                )?,
                shell_free_vertices: AsmHistoricalRowDelta::between(
                    &old_topology.shell_free_vertices,
                    &new.shell_free_vertices,
                )?,
                face_loops: AsmHistoricalRowDelta::between(
                    &old_topology.face_loops,
                    &new.face_loops,
                )?,
                loop_coedges: AsmHistoricalRowDelta::between(
                    &old_topology.loop_coedges,
                    &new.loop_coedges,
                )?,
                coedge_topology: AsmHistoricalRowDelta::between(
                    &old_topology.coedge_topology,
                    &new.coedge_topology,
                )?,
                edge_vertices: AsmHistoricalRowDelta::between(
                    &old_topology.edge_vertices,
                    &new.edge_vertices,
                )?,
                face_surfaces: AsmHistoricalRowDelta::between(
                    &old_topology.face_surfaces,
                    &new.face_surfaces,
                )?,
                vertex_points: AsmHistoricalRowDelta::between(
                    &old_topology.vertex_points,
                    &new.vertex_points,
                )?,
                edge_curves: AsmHistoricalRowDelta::between(
                    &old_topology.edge_curves,
                    &new.edge_curves,
                )?,
                coedge_pcurves: AsmHistoricalRowDelta::between(
                    &old_topology.coedge_pcurves,
                    &new.coedge_pcurves,
                )?,
                point_positions: AsmHistoricalRowDelta::between(
                    &old_topology.point_positions,
                    &new.point_positions,
                )?,
            },
        })
    }

    fn apply_topology(
        &self,
        topology: &mut AsmHistoricalTopology,
    ) -> Result<(), AsmTransitionError> {
        apply_membership(
            &self.topology.bodies,
            &mut topology.bodies,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.regions,
            &mut topology.regions,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.shells,
            &mut topology.shells,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.faces,
            &mut topology.faces,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.loops,
            &mut topology.loops,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.coedges,
            &mut topology.coedges,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.edges,
            &mut topology.edges,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.vertices,
            &mut topology.vertices,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.points,
            &mut topology.points,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.surfaces,
            &mut topology.surfaces,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.curves,
            &mut topology.curves,
            &self.records.updated,
        )?;
        apply_membership(
            &self.topology.pcurves,
            &mut topology.pcurves,
            &self.records.updated,
        )?;
        self.rows.surface_radii.apply(&mut topology.surface_radii)?;
        self.rows
            .surface_cylinders
            .apply(&mut topology.surface_cylinders)?;
        self.rows
            .surface_planes
            .apply(&mut topology.surface_planes)?;
        self.rows.surface_axes.apply(&mut topology.surface_axes)?;
        self.rows.curve_axes.apply(&mut topology.curve_axes)?;
        self.rows.body_regions.apply(&mut topology.body_regions)?;
        self.rows.region_shells.apply(&mut topology.region_shells)?;
        self.rows.shell_faces.apply(&mut topology.shell_faces)?;
        self.rows
            .shell_wire_edges
            .apply(&mut topology.shell_wire_edges)?;
        self.rows
            .shell_free_vertices
            .apply(&mut topology.shell_free_vertices)?;
        self.rows.face_loops.apply(&mut topology.face_loops)?;
        self.rows.loop_coedges.apply(&mut topology.loop_coedges)?;
        self.rows
            .coedge_topology
            .apply(&mut topology.coedge_topology)?;
        self.rows.edge_vertices.apply(&mut topology.edge_vertices)?;
        self.rows.face_surfaces.apply(&mut topology.face_surfaces)?;
        self.rows.vertex_points.apply(&mut topology.vertex_points)?;
        self.rows.edge_curves.apply(&mut topology.edge_curves)?;
        self.rows
            .coedge_pcurves
            .apply(&mut topology.coedge_pcurves)?;
        self.rows
            .point_positions
            .apply(&mut topology.point_positions)?;
        if !self.rows.persistent_subentity_tags.is_empty() {
            let mut tags = entity_tags(topology);
            if self.rows.persistent_subentity_tags.set.iter().any(|group| {
                group.tags.is_empty()
                    || group
                        .tags
                        .iter()
                        .any(|tag| tag.entity_ref != group.entity_ref)
            }) {
                return Err(AsmTransitionError::UnsortedFamily);
            }
            self.rows.persistent_subentity_tags.apply(&mut tags)?;
            topology.persistent_subentity_tags =
                tags.into_iter().flat_map(|group| group.tags).collect();
        }
        Ok(())
    }

    fn apply_versions(
        &self,
        versions: &mut Vec<AsmEntityVersion>,
    ) -> Result<(), AsmTransitionError> {
        check_delta_order(&self.records)?;
        if !strictly_sorted(self.record_revisions.iter().map(|row| row.entity_ref)) {
            return Err(AsmTransitionError::UnsortedFamily);
        }
        let mut inserted = self.records.inserted.iter().peekable();
        let mut updated = self.records.updated.iter().peekable();
        for row in &self.record_revisions {
            match (inserted.peek().copied(), updated.peek().copied()) {
                (Some(a), Some(b)) if a == b => return Err(AsmTransitionError::RevisionCoverage),
                (Some(a), Some(b)) if a < b && *a == row.entity_ref => {
                    inserted.next();
                }
                (Some(_), Some(b)) if *b == row.entity_ref => {
                    updated.next();
                }
                (Some(a), None) if *a == row.entity_ref => {
                    inserted.next();
                }
                (None, Some(b)) if *b == row.entity_ref => {
                    updated.next();
                }
                _ => return Err(AsmTransitionError::RevisionCoverage),
            }
        }
        if inserted.next().is_some()
            || updated.next().is_some()
            || intersects(&self.records.deleted, &self.records.inserted)
            || intersects(&self.records.deleted, &self.records.updated)
        {
            return Err(AsmTransitionError::RevisionCoverage);
        }
        let mut old = versions.iter().peekable();
        let mut inserted = self.records.inserted.iter().peekable();
        for revision in &self.record_revisions {
            while old
                .peek()
                .is_some_and(|row| row.entity_ref < revision.entity_ref)
            {
                old.next();
            }
            let previous = old
                .peek()
                .filter(|row| row.entity_ref == revision.entity_ref);
            if inserted
                .peek()
                .is_some_and(|&&key| key == revision.entity_ref)
            {
                if previous.is_some() {
                    return Err(AsmTransitionError::InsertedPresent);
                }
                inserted.next();
            } else if previous.is_none_or(|row| row.record_ref == revision.record_ref) {
                return Err(AsmTransitionError::InvalidUpdate);
            }
        }
        let delta = AsmHistoricalRowDelta {
            set: self.record_revisions.clone(),
            removed: self.records.deleted.clone(),
        };
        delta.apply(versions).map_err(|error| match error {
            AsmTransitionError::RemovedRowAbsent => AsmTransitionError::DeletedAbsent,
            error => error,
        })
    }
}

impl AsmHistoricalRow for AsmEntityVersion {
    fn key(&self) -> i64 {
        self.entity_ref
    }
}

fn intersects(left: &[i64], right: &[i64]) -> bool {
    let mut right = right.iter().peekable();
    left.iter().any(|key| {
        while right.peek().is_some_and(|&other| other < key) {
            right.next();
        }
        right.peek().is_some_and(|&other| other == key)
    })
}

fn check_delta_order(delta: &AsmHistoricalEntityDelta) -> Result<(), AsmTransitionError> {
    if [&delta.inserted, &delta.deleted, &delta.updated]
        .iter()
        .any(|keys| !strictly_sorted(keys.iter().copied()))
    {
        return Err(AsmTransitionError::UnsortedFamily);
    }
    Ok(())
}

fn apply_membership(
    delta: &AsmHistoricalEntityDelta,
    family: &mut Vec<i64>,
    updated_records: &[i64],
) -> Result<(), AsmTransitionError> {
    if delta.inserted.is_empty() && delta.deleted.is_empty() && delta.updated.is_empty() {
        return Ok(());
    }
    check_delta_order(delta)?;
    let mut revisions = updated_records.iter().peekable();
    let mut deleted = delta.deleted.iter().peekable();
    let expected_updates = family.iter().copied().filter(|slot| {
        while revisions.peek().is_some_and(|&&key| key < *slot) {
            revisions.next();
        }
        while deleted.peek().is_some_and(|&&key| key < *slot) {
            deleted.next();
        }
        revisions.peek().is_some_and(|&&key| key == *slot)
            && deleted.peek().is_none_or(|&&key| key != *slot)
    });
    if !expected_updates.eq(delta.updated.iter().copied()) {
        return Err(AsmTransitionError::InvalidUpdate);
    }
    if intersects(&delta.inserted, &delta.deleted) {
        return Err(AsmTransitionError::InsertedPresent);
    }
    let mut old = std::mem::take(family).into_iter().peekable();
    let mut inserted = delta.inserted.iter().peekable();
    let mut deleted = delta.deleted.iter().peekable();
    while old.peek().is_some() || inserted.peek().is_some() {
        let key = match (old.peek(), inserted.peek()) {
            (Some(a), Some(b)) if a == *b => return Err(AsmTransitionError::InsertedPresent),
            (Some(a), Some(b)) => (*a).min(**b),
            (Some(key), None) => *key,
            (None, Some(key)) => **key,
            (None, None) => break,
        };
        if deleted.peek().is_some_and(|&&removed| removed < key) {
            return Err(AsmTransitionError::DeletedAbsent);
        }
        if inserted.peek().is_some_and(|&&added| added == key) {
            inserted.next();
            family.push(key);
        } else {
            old.next();
            if deleted.peek().is_some_and(|&&removed| removed == key) {
                deleted.next();
            } else {
                family.push(key);
            }
        }
    }
    if deleted.next().is_some() {
        return Err(AsmTransitionError::DeletedAbsent);
    }
    Ok(())
}

impl AsmHistoricalRowsDelta {
    pub(crate) fn retain_projection_rows(&mut self) {
        self.surface_radii = AsmHistoricalRowDelta::default();
        self.surface_cylinders = AsmHistoricalRowDelta::default();
        self.surface_axes = AsmHistoricalRowDelta::default();
        self.persistent_subentity_tags = AsmHistoricalRowDelta::default();
        self.body_regions = AsmHistoricalRowDelta::default();
        self.region_shells = AsmHistoricalRowDelta::default();
        self.shell_faces = AsmHistoricalRowDelta::default();
        self.shell_wire_edges = AsmHistoricalRowDelta::default();
        self.shell_free_vertices = AsmHistoricalRowDelta::default();
        self.edge_vertices = AsmHistoricalRowDelta::default();
        self.vertex_points = AsmHistoricalRowDelta::default();
        self.point_positions = AsmHistoricalRowDelta::default();
    }
}

impl AsmHistoricalTopology {
    fn has_retained_rows(&self) -> bool {
        !self.face_surfaces.is_empty()
            || !self.surface_planes.is_empty()
            || !self.loop_coedges.is_empty()
            || !self.coedge_topology.is_empty()
            || !self.face_loops.is_empty()
            || !self.edge_curves.is_empty()
            || !self.coedge_pcurves.is_empty()
            || !self.curve_axes.is_empty()
    }
    fn slots(&self) -> BTreeSet<i64> {
        [
            &self.bodies,
            &self.regions,
            &self.shells,
            &self.faces,
            &self.loops,
            &self.coedges,
            &self.edges,
            &self.vertices,
            &self.points,
            &self.surfaces,
            &self.curves,
            &self.pcurves,
        ]
        .into_iter()
        .flatten()
        .copied()
        .collect()
    }
}

impl AsmHistory {
    fn chain_order(&self) -> Result<Vec<usize>, AsmTransitionError> {
        let by_node: BTreeMap<_, _> = self
            .states
            .iter()
            .enumerate()
            .map(|(i, state)| (state.node_index, i))
            .collect();
        if by_node.len() != self.states.len() {
            return Err(AsmTransitionError::Chain);
        }
        let mut heads = self
            .states
            .iter()
            .enumerate()
            .filter(|(_, state)| state.previous_ref.is_none());
        let Some((head, _)) = heads.next() else {
            return Err(AsmTransitionError::Chain);
        };
        if heads.next().is_some() {
            return Err(AsmTransitionError::Chain);
        }
        let mut order = Vec::new();
        let mut seen = BTreeSet::new();
        let mut next = Some(head);
        while let Some(index) = next {
            if !seen.insert(index) {
                return Err(AsmTransitionError::Chain);
            }
            order.push(index);
            next = match self.states[index].next_ref {
                Some(node) => Some(*by_node.get(&node).ok_or(AsmTransitionError::Chain)?),
                None => None,
            };
        }
        if order.len() != self.states.len() {
            return Err(AsmTransitionError::Chain);
        }
        Ok(order)
    }

    fn bound_chain(&self) -> Result<Option<Vec<usize>>, AsmTransitionError> {
        let count = self
            .states
            .iter()
            .filter(|state| state.transition.is_some())
            .count();
        if count == 0 {
            return Ok(None);
        }
        if count != self.states.len() {
            return Err(AsmTransitionError::MissingTransition);
        }
        let order = self.chain_order()?;
        for (position, &index) in order.iter().enumerate() {
            let expected = order
                .get(position + 1)
                .map(|&older| self.states[older].state_id);
            let transition = self.states[index]
                .transition
                .as_ref()
                .ok_or(AsmTransitionError::MissingTransition)?;
            if transition.previous_state_id != expected {
                return Err(AsmTransitionError::PreviousState);
            }
        }
        Ok(Some(order))
    }

    /// Check chain links and all delta application invariants.
    // Snapshot consumers migrate to this entry point in the next unit.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn check_transitions(&self) -> Result<(), AsmTransitionError> {
        let Some(order) = self.bound_chain()? else {
            return Ok(());
        };
        let mut topology = AsmHistoricalTopology::default();
        let mut versions = Vec::new();
        for index in order.into_iter().rev() {
            let transition = self.states[index]
                .transition
                .as_ref()
                .ok_or(AsmTransitionError::MissingTransition)?;
            transition.apply_versions(&mut versions)?;
            transition.apply_topology(&mut topology)?;
        }
        Ok(())
    }

    /// Rebuild this member state's topology and cache only that state.
    // Snapshot consumers migrate to this entry point in the next unit.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn topology(&self, state: &AsmDeltaState) -> Option<&AsmHistoricalTopology> {
        let index = self
            .states
            .iter()
            .position(|member| std::ptr::eq(member, state))?;
        let order = self
            .materialized
            .order
            .get_or_init(|| self.bound_chain())
            .as_ref()
            .ok()?
            .as_ref()?;
        let position = order.iter().position(|&ordinal| ordinal == index)?;
        let cache = self
            .materialized
            .states
            .get_or_init(|| self.states.iter().map(|_| OnceLock::new()).collect());
        cache
            .get(index)?
            .get_or_init(|| {
                let mut materialized = MaterializedState::default();
                let mut older_position = order.len();
                for (offset, &older) in order.iter().enumerate().skip(position + 1) {
                    if let Some(Some(cached)) = cache.get(older)?.get() {
                        materialized = cached.as_ref().clone();
                        older_position = offset;
                        break;
                    }
                }
                for &ordinal in order.get(position..older_position)?.iter().rev() {
                    let transition = self.states.get(ordinal)?.transition.as_ref()?;
                    transition.apply_versions(&mut materialized.versions).ok()?;
                    transition.apply_topology(&mut materialized.topology).ok()?;
                }
                Some(Box::new(materialized))
            })
            .as_deref()
            .filter(|state| {
                self.topology_scope != AsmTopologyScope::Retained
                    || state.topology.has_retained_rows()
            })
            .map(|state| &state.topology)
    }

    /// Revisions of the topology's slots, sorted by stable slot.
    // Snapshot consumers migrate to this entry point in the next unit.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn entity_versions(&self, state: &AsmDeltaState) -> Vec<AsmEntityVersion> {
        self.topology_versions(state).unwrap_or_default()
    }

    fn topology_versions(&self, state: &AsmDeltaState) -> Option<Vec<AsmEntityVersion>> {
        let topology = self.topology(state)?;
        let index = self
            .states
            .iter()
            .position(|member| std::ptr::eq(member, state))?;
        let cached = self
            .materialized
            .states
            .get()?
            .get(index)?
            .get()?
            .as_ref()?;
        let slots = topology.slots();
        Some(
            cached
                .versions
                .iter()
                .filter(|row| slots.contains(&row.entity_ref))
                .cloned()
                .collect(),
        )
    }

    /// Drop all cached materializations.
    pub(crate) fn release_topologies(&mut self) {
        self.materialized = MaterializedTopologies::default();
    }
}
#[cfg(test)]
mod tests;
