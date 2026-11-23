// SPDX-License-Identifier: Apache-2.0
//! Support contexts of historical face selections in the preceding state.

use super::body_closures::stable_ref;
use super::face_boundaries::face_boundary_contexts_for_slots;
use crate::history::topology_cache::TopologyQueryCache;
use crate::history_records::{AsmHistoricalTopology, AsmHistory};
use std::collections::HashSet;

/// The plane or cylinder geometry that `topology` states once for `surface`.
fn historical_surface_geometry(
    topology: &AsmHistoricalTopology,
    surface: i64,
) -> Option<crate::records::topology::historical_context::DesignHistoricalSurfaceGeometry> {
    use crate::records::topology::historical_context::DesignHistoricalSurfaceGeometry;
    let mut planes = topology
        .surface_planes
        .iter()
        .filter(|row| row.surface == surface);
    let mut cylinders = topology
        .surface_cylinders
        .iter()
        .filter(|row| row.surface == surface);
    match (planes.next(), cylinders.next()) {
        (Some(plane), None) if planes.next().is_none() => {
            Some(DesignHistoricalSurfaceGeometry::Plane {
                origin: plane.origin,
                normal: plane.normal,
            })
        }
        (None, Some(cylinder)) if cylinders.next().is_none() => {
            Some(DesignHistoricalSurfaceGeometry::Cylinder {
                origin: cylinder.origin,
                axis: cylinder.axis,
                radius: cylinder.radius,
            })
        }
        _ => None,
    }
}

pub(in crate::history) fn historical_face_support_contexts<'a>(
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &'a AsmHistory,
    preceding_topology: &'a AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<crate::records::topology::historical_context::DesignHistoricalFaceSupportContext> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        preceding_faces.insert(*face);
    }
    let mut contexts = Vec::new();
    'candidates: for candidate in candidates {
        let Some(active_face_slot) = stable_ref(candidate.as_str()) else {
            continue;
        };
        let mut preceding_bindings = preceding_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == active_face_slot);
        let surface_slot = if let Some(binding) = preceding_bindings.next() {
            if preceding_bindings.next().is_some() {
                continue;
            }
            binding.carrier
        } else {
            let mut carriers = HashSet::new();
            for topology in history
                .states()
                .iter()
                .filter_map(|state| history.topology(state))
            {
                let mut bindings = topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.entity == active_face_slot);
                if let Some(binding) = bindings.next() {
                    if bindings.next().is_some() {
                        continue 'candidates;
                    }
                    carriers.insert(binding.carrier);
                }
            }
            if carriers.len() != 1 {
                continue;
            }
            let Some(carrier) = carriers.into_iter().next() else {
                continue;
            };
            carrier
        };
        let mut preceding_face_slots = preceding_topology
            .face_surfaces
            .iter()
            .filter(|binding| {
                binding.carrier == surface_slot && preceding_faces.contains(&binding.entity)
            })
            .map(|binding| binding.entity)
            .collect::<Vec<_>>();
        preceding_face_slots.sort_unstable_by(Ord::cmp);
        preceding_face_slots.dedup();
        if preceding_face_slots.is_empty() {
            continue;
        }
        let changed_preceding_face_slots = preceding_face_slots
            .iter()
            .copied()
            .filter(|face| changed_faces.contains(face))
            .collect::<Vec<_>>();
        let preceding_face_boundaries = face_boundary_contexts_for_slots(
            &preceding_face_slots,
            preceding_topology,
            topology_queries,
        );

        let preceding_surface = historical_surface_geometry(preceding_topology, surface_slot);
        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot,
                surface_slot,
                preceding_face_slots,
                preceding_face_boundaries,
                changed_preceding_face_slots,
                preceding_surface,
            },
        );
    }
    contexts
}
