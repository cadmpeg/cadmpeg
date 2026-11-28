// SPDX-License-Identifier: Apache-2.0
//! Validate transition membership without materializing geometry rows.
use super::{check_delta_order, intersects, ordered_keys, strictly_sorted};
use crate::history_records::{
    AsmHistoricalEntityDelta, AsmHistoricalRow, AsmHistoricalRowDelta, AsmHistoricalTransition,
    AsmTransitionError,
};
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct Memberships {
    bodies: HashSet<i64>,
    regions: HashSet<i64>,
    shells: HashSet<i64>,
    faces: HashSet<i64>,
    loops: HashSet<i64>,
    coedges: HashSet<i64>,
    edges: HashSet<i64>,
    vertices: HashSet<i64>,
    points: HashSet<i64>,
    surfaces: HashSet<i64>,
    curves: HashSet<i64>,
    pcurves: HashSet<i64>,
    surface_repeats: HashSet<i64>,
    curve_repeats: HashSet<i64>,
    surface_radii: HashSet<i64>,
    surface_cylinders: HashSet<i64>,
    surface_planes: HashSet<i64>,
    surface_axes: HashSet<i64>,
    curve_axes: HashSet<i64>,
    persistent_subentity_tags: HashSet<i64>,
    body_regions: HashSet<i64>,
    region_shells: HashSet<i64>,
    shell_faces: HashSet<i64>,
    shell_wire_edges: HashSet<i64>,
    shell_free_vertices: HashSet<i64>,
    face_loops: HashSet<i64>,
    face_envelopes: HashSet<i64>,
    edge_envelopes: HashSet<i64>,
    loop_coedges: HashSet<i64>,
    coedge_topology: HashSet<i64>,
    edge_vertices: HashSet<i64>,
    face_surfaces: HashSet<i64>,
    face_senses: HashSet<i64>,
    vertex_points: HashSet<i64>,
    edge_curves: HashSet<i64>,
    coedge_pcurves: HashSet<i64>,
    point_positions: HashSet<i64>,
    edge_samples: HashSet<i64>,
    face_samples: HashSet<i64>,
}

fn apply_entities(
    current: &mut HashSet<i64>,
    delta: &AsmHistoricalEntityDelta,
    updated: &[i64],
) -> Result<(), AsmTransitionError> {
    if delta.inserted.is_empty() && delta.deleted.is_empty() && delta.updated.is_empty() {
        return Ok(());
    }
    check_delta_order(delta)?;
    let expected = updated
        .iter()
        .copied()
        .filter(|slot| current.contains(slot) && delta.deleted.binary_search(slot).is_err());
    if !expected.eq(delta.updated.iter().copied()) {
        return Err(AsmTransitionError::InvalidUpdate);
    }
    if intersects(&delta.inserted, &delta.deleted)
        || delta.inserted.iter().any(|slot| current.contains(slot))
    {
        return Err(AsmTransitionError::InsertedPresent);
    }
    for slot in &delta.deleted {
        if !current.remove(slot) {
            return Err(AsmTransitionError::DeletedAbsent);
        }
    }
    current.extend(&delta.inserted);
    Ok(())
}

fn apply_rows<R: AsmHistoricalRow>(
    current: &mut HashSet<i64>,
    delta: &AsmHistoricalRowDelta<R>,
) -> Result<(), AsmTransitionError> {
    if !ordered_keys(&delta.set) || !strictly_sorted(delta.removed.iter().copied()) {
        return Err(AsmTransitionError::UnsortedFamily);
    }
    for key in &delta.removed {
        if !current.remove(key) {
            return Err(AsmTransitionError::RemovedRowAbsent);
        }
    }
    current.extend(delta.set.iter().map(R::key));
    Ok(())
}

impl Memberships {
    pub(super) fn has_retained_rows(&self) -> bool {
        !self.face_surfaces.is_empty()
            || !self.surface_planes.is_empty()
            || !self.surface_cylinders.is_empty()
            || !self.face_loops.is_empty()
            || !self.loop_coedges.is_empty()
            || !self.coedge_topology.is_empty()
            || !self.edge_curves.is_empty()
            || !self.coedge_pcurves.is_empty()
            || !self.curve_axes.is_empty()
    }
    pub(super) fn apply(
        &mut self,
        transition: &AsmHistoricalTransition,
    ) -> Result<(), AsmTransitionError> {
        let topology = &transition.topology;
        apply_entities(
            &mut self.bodies,
            &topology.bodies,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.regions,
            &topology.regions,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.shells,
            &topology.shells,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.faces,
            &topology.faces,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.loops,
            &topology.loops,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.coedges,
            &topology.coedges,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.edges,
            &topology.edges,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.vertices,
            &topology.vertices,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.points,
            &topology.points,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.surfaces,
            &topology.surfaces,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.curves,
            &topology.curves,
            &transition.records.updated,
        )?;
        apply_entities(
            &mut self.pcurves,
            &topology.pcurves,
            &transition.records.updated,
        )?;
        let rows = &transition.rows;
        apply_rows(&mut self.surface_repeats, &rows.surface_repeats)?;
        apply_rows(&mut self.curve_repeats, &rows.curve_repeats)?;
        apply_rows(&mut self.surface_radii, &rows.surface_radii)?;
        apply_rows(&mut self.surface_cylinders, &rows.surface_cylinders)?;
        apply_rows(&mut self.surface_planes, &rows.surface_planes)?;
        apply_rows(&mut self.surface_axes, &rows.surface_axes)?;
        apply_rows(&mut self.curve_axes, &rows.curve_axes)?;
        apply_rows(
            &mut self.persistent_subentity_tags,
            &rows.persistent_subentity_tags,
        )?;
        apply_rows(&mut self.body_regions, &rows.body_regions)?;
        apply_rows(&mut self.region_shells, &rows.region_shells)?;
        apply_rows(&mut self.shell_faces, &rows.shell_faces)?;
        apply_rows(&mut self.shell_wire_edges, &rows.shell_wire_edges)?;
        apply_rows(&mut self.shell_free_vertices, &rows.shell_free_vertices)?;
        apply_rows(&mut self.face_loops, &rows.face_loops)?;
        apply_rows(&mut self.face_envelopes, &rows.face_envelopes)?;
        apply_rows(&mut self.edge_envelopes, &rows.edge_envelopes)?;
        apply_rows(&mut self.loop_coedges, &rows.loop_coedges)?;
        apply_rows(&mut self.coedge_topology, &rows.coedge_topology)?;
        apply_rows(&mut self.edge_vertices, &rows.edge_vertices)?;
        apply_rows(&mut self.face_surfaces, &rows.face_surfaces)?;
        apply_rows(&mut self.face_senses, &rows.face_senses)?;
        apply_rows(&mut self.vertex_points, &rows.vertex_points)?;
        apply_rows(&mut self.edge_curves, &rows.edge_curves)?;
        apply_rows(&mut self.coedge_pcurves, &rows.coedge_pcurves)?;
        apply_rows(&mut self.point_positions, &rows.point_positions)?;
        apply_rows(&mut self.edge_samples, &rows.edge_samples)?;
        apply_rows(&mut self.face_samples, &rows.face_samples)?;

        for (members, repeats, delta, changes) in [
            (
                &self.surfaces,
                &self.surface_repeats,
                &rows.surface_repeats,
                &topology.surfaces,
            ),
            (
                &self.curves,
                &self.curve_repeats,
                &rows.curve_repeats,
                &topology.curves,
            ),
        ] {
            if delta
                .set
                .iter()
                .any(|row| row.count < 2 || !members.contains(&row.slot))
                || changes.deleted.iter().any(|slot| repeats.contains(slot))
            {
                return Err(AsmTransitionError::MembershipCoverage);
            }
        }
        if rows.persistent_subentity_tags.set.iter().any(|group| {
            group.tags.is_empty()
                || group
                    .tags
                    .iter()
                    .any(|tag| tag.entity_ref != group.entity_ref)
        }) {
            return Err(AsmTransitionError::UnsortedFamily);
        }
        Ok(())
    }
}
