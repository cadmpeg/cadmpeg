// SPDX-License-Identifier: Apache-2.0
//! Edge treatment radius, edge, and face support candidates.

use super::body_closures::stable_ref;
use super::treatment_index::TreatmentCache;
use crate::history_records::{AsmHistoricalRelation, AsmHistoricalTopology};
use std::collections::{HashMap, HashSet};

pub(super) fn terminal_edge_recipe_faces(
    primary: &[cadmpeg_ir::ids::FaceId],
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    let mut faces = (primary.iter().chain(reference_faces.iter().flatten()))
        .cloned()
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    faces
}

pub(super) fn terminal_edge_recipe_reference_faces(
    references: &[crate::records::dimensions::DesignRecipeReference],
    local_topology_references: Option<&[std::num::NonZeroU32]>,
) -> Result<Vec<Vec<cadmpeg_ir::ids::FaceId>>, cadmpeg_core::CodecError> {
    let mut selected = Vec::new();
    let mut append = |reference: &crate::records::dimensions::DesignRecipeReference| {
        let faces = if reference.candidate_faces.is_empty() {
            &reference.alternate_selector_faces
        } else {
            &reference.candidate_faces
        };
        let faces = faces.clone();

        selected.push(faces);
        Ok::<(), cadmpeg_core::CodecError>(())
    };
    if let Some(ordinals) = local_topology_references {
        for ordinal in ordinals {
            if let Some(reference) = usize::try_from(ordinal.get())
                .ok()
                .and_then(|ordinal| ordinal.checked_sub(1))
                .and_then(|ordinal| references.get(ordinal))
            {
                append(reference)?;
            }
        }
    } else {
        for reference in references {
            append(reference)?;
        }
    }
    Ok(selected)
}

pub(super) fn treatment_radius_candidates<'a>(
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &'a AsmHistoricalTopology,
    preceding: &'a AsmHistoricalTopology,
    deleted_edges: &[i64],
    treatment_cache: &mut TreatmentCache<'a>,
) -> Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate> {
    treatment_edge_candidates(
        result_candidate_faces,
        inserted_faces,
        result,
        preceding,
        deleted_edges,
        treatment_cache,
    )
    .0
}

pub(super) fn treatment_edge_candidates<'a>(
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &'a AsmHistoricalTopology,
    preceding: &'a AsmHistoricalTopology,
    deleted_edges: &[i64],
    treatment_cache: &mut TreatmentCache<'a>,
) -> (
    Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
    Vec<i64>,
) {
    let index = treatment_cache.get(result, preceding);
    let result_boundaries = &index.result_boundaries;
    let preceding_boundaries = &index.preceding_boundaries;
    let mut deleted_edge_set = HashSet::new();
    for edge in deleted_edges {
        deleted_edge_set.insert(*edge);
    }
    let mut candidate_edges = HashSet::new();
    for edge in result_candidate_faces
        .into_iter()
        .flatten()
        .filter_map(|face| stable_ref(face.as_str()))
        .filter_map(|face| result_boundaries.get(&face))
        .flatten()
        .copied()
    {
        candidate_edges.insert(edge);
    }
    let mut radii_out = Vec::new();
    let mut transitions_out = Vec::new();
    for inserted in inserted_faces {
        let Some((carrier, supports)) = index.supports.get(inserted) else {
            continue;
        };
        let Some(inserted_boundary) = result_boundaries.get(inserted) else {
            continue;
        };

        let radius = index
            .radii
            .get(carrier)
            .copied()
            .flatten()
            .and_then(cadmpeg_ir::scalar::PositiveReal::new)
            .filter(|_| {
                candidate_edges.is_empty() || !inserted_boundary.is_disjoint(&candidate_edges)
            });
        for (ordinal, left) in supports.iter().enumerate() {
            let Some(left_edges) = preceding_boundaries.get(left) else {
                continue;
            };
            for right in supports.iter().skip(ordinal + 1) {
                let Some(right_edges) = preceding_boundaries.get(right) else {
                    continue;
                };

                for edge in left_edges.intersection(right_edges).filter(|edge| {
                    deleted_edge_set.contains(edge)
                        && index.spines.get(carrier).is_none_or(|spine| {
                            spine.admits(
                                index.preceding_edges.get(edge).copied().unwrap_or_default(),
                            )
                        })
                }) {
                    transitions_out.push(*edge);
                    if let Some(radius) = radius {
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
    radii_out.sort_by(|left, right| {
        left.radius
            .get()
            .total_cmp(&right.radius.get())
            .then(left.edge_slot.cmp(&right.edge_slot))
    });
    radii_out
        .dedup_by(|left, right| left.radius == right.radius && left.edge_slot == right.edge_slot);
    transitions_out.sort_unstable_by(Ord::cmp);
    transitions_out.dedup();
    (radii_out, transitions_out)
}

pub(super) fn treatment_face_supports(
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    result_boundaries: &HashMap<i64, HashSet<i64>>,
) -> Vec<(i64, i64, Vec<i64>, bool)> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding.faces {
        preceding_faces.insert(*face);
    }
    let mut preceding_surfaces = HashSet::new();
    for surface in &preceding.surfaces {
        preceding_surfaces.insert(*surface);
    }
    let result_carriers = unique_face_carriers(result);
    let preceding_carriers = unique_face_carriers(preceding);
    let preceding_carrier_faces = unique_carrier_faces(preceding, &preceding_faces);
    let result_faces = result.faces.iter().copied().collect::<HashSet<_>>();
    let removed_faces = preceding_faces
        .iter()
        .copied()
        .filter(|face| !result_faces.contains(face))
        .collect::<HashSet<_>>();
    let mut preceding_loop_faces = HashMap::<i64, Option<i64>>::new();
    for relation in &preceding.face_loops {
        for loop_slot in &relation.member_refs {
            preceding_loop_faces
                .entry(*loop_slot)
                .and_modify(|face| *face = None)
                .or_insert(Some(relation.owner_ref));
        }
    }
    let mut result_face_loops = HashMap::<i64, Vec<i64>>::new();
    for relation in &result.face_loops {
        result_face_loops
            .entry(relation.owner_ref)
            .or_default()
            .extend(&relation.member_refs);
    }
    // The preceding owner of each loop of a result face that a preceding face
    // owned; `None` when a loop had two preceding owners.
    let loop_ancestry = |face: i64| -> Option<Vec<i64>> {
        result_face_loops
            .get(&face)
            .map_or(Some(Vec::new()), |loops| {
                loops
                    .iter()
                    .filter_map(|loop_slot| preceding_loop_faces.get(loop_slot).copied())
                    .collect()
            })
    };
    let mut adjacent_faces = HashMap::<i64, Vec<i64>>::new();
    for (face, edges) in result_boundaries {
        for edge in edges {
            adjacent_faces.entry(*edge).or_default().push(*face);
        }
    }
    // The face that owns each coedge of a result edge, through its loop, or
    // `None` when the coedge or its loop has no single owner.
    let mut loop_faces = HashMap::<i64, Option<i64>>::new();
    for relation in &result.face_loops {
        for loop_slot in &relation.member_refs {
            loop_faces
                .entry(*loop_slot)
                .and_modify(|face| *face = None)
                .or_insert(Some(relation.owner_ref));
        }
    }
    let mut coedge_loops = HashMap::<i64, Option<i64>>::new();
    for row in &result.coedge_topology {
        coedge_loops
            .entry(row.coedge)
            .and_modify(|owner| *owner = None)
            .or_insert(Some(row.owner_loop));
    }
    let mut edge_owners = HashMap::<i64, Vec<Option<i64>>>::new();
    for row in &result.coedge_topology {
        let owner = coedge_loops
            .get(&row.coedge)
            .copied()
            .flatten()
            .and_then(|loop_slot| loop_faces.get(&loop_slot).copied().flatten());
        edge_owners.entry(row.edge).or_default().push(owner);
    }
    let mut selected = Vec::new();
    for inserted in inserted_faces.iter().copied() {
        let Some(carrier) = result_carriers.get(&inserted).copied().flatten() else {
            continue;
        };
        if preceding_surfaces.contains(&carrier) {
            continue;
        }
        let Some(inserted_boundary) = result_boundaries.get(&inserted) else {
            continue;
        };
        // An adjacent face on a preceding carrier, or without one carrier, that
        // maps to no single preceding face leaves the supports incomplete, and
        // so does a boundary edge with no other face, or with a coedge whose
        // owner face is unknown or has no rebuilt boundary.
        let mut complete = inserted_boundary.iter().all(|edge| {
            edge_owners.get(edge).is_some_and(|owners| {
                owners.iter().any(|owner| *owner != Some(inserted))
                    && owners.iter().all(|owner| {
                        owner.is_some_and(|face| {
                            face == inserted || result_boundaries.contains_key(&face)
                        })
                    })
            })
        });
        let mut supports = Vec::new();
        for face in inserted_boundary
            .iter()
            .filter_map(|edge| adjacent_faces.get(edge))
            .flatten()
            .copied()
            .filter(|face| *face != inserted)
        {
            let Some(carrier) = result_carriers.get(&face).copied().flatten() else {
                complete = false;
                continue;
            };
            let persists = preceding_faces.contains(&face);
            let source = loop_ancestry(face).and_then(|ancestry| {
                let source = if persists {
                    // A persisting face whose carrier the transition recorded
                    // again with the same geometry keeps its support role.
                    preceding_carriers
                        .get(&face)
                        .copied()
                        .flatten()
                        .is_some_and(|before| {
                            before == carrier
                                || (!preceding_surfaces.contains(&carrier)
                                    && super::blend_spine::same_surface(
                                        (preceding, before),
                                        (result, carrier),
                                    ))
                        })
                        .then_some(face)?
                } else if let Some(only) = preceding_carrier_faces.get(&carrier).copied().flatten()
                {
                    only
                } else {
                    // A face that replaces a removed face keeps one of its loops.
                    let replaced = *ancestry.first()?;
                    (removed_faces.contains(&replaced)
                        && preceding_carriers.get(&replaced).copied().flatten() == Some(carrier))
                    .then_some(replaced)?
                };
                ancestry
                    .iter()
                    .all(|owner| *owner == source)
                    .then_some(source)
            });
            match source {
                Some(source) => supports.push(source),
                // A face new to the transition on a new carrier is another
                // blend, not a support.
                None if persists || preceding_surfaces.contains(&carrier) => complete = false,
                None => {}
            }
        }
        supports.sort_unstable_by(Ord::cmp);
        supports.dedup();

        selected.push((inserted, carrier, supports, complete));
    }
    selected
}

pub(super) fn face_boundary_edge_index(
    topology: &AsmHistoricalTopology,
) -> HashMap<i64, HashSet<i64>> {
    fn unique_relations(relations: &[AsmHistoricalRelation]) -> HashMap<i64, Option<&[i64]>> {
        let mut out = HashMap::<i64, Option<&[i64]>>::new();
        for relation in relations {
            out.entry(relation.owner_ref)
                .and_modify(|members| *members = None)
                .or_insert(Some(&relation.member_refs));
        }
        out
    }
    let face_loops = unique_relations(&topology.face_loops);
    let loop_coedges = unique_relations(&topology.loop_coedges);
    let mut coedge_edges = HashMap::<i64, Option<i64>>::new();
    for coedge in &topology.coedge_topology {
        coedge_edges
            .entry(coedge.coedge)
            .and_modify(|edge| *edge = None)
            .or_insert(Some(coedge.edge));
    }
    let mut boundaries = HashMap::new();
    'faces: for face in &topology.faces {
        let Some(loops) = face_loops.get(face).copied().flatten() else {
            continue;
        };
        let mut edges = HashSet::new();
        for loop_slot in loops {
            let Some(coedges) = loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            for coedge in coedges {
                let Some(edge) = coedge_edges.get(coedge).copied().flatten() else {
                    continue 'faces;
                };
                edges.insert(edge);
            }
        }
        boundaries.insert(*face, edges);
    }
    boundaries
}

pub(super) fn unique_face_carriers(topology: &AsmHistoricalTopology) -> HashMap<i64, Option<i64>> {
    let mut out = HashMap::new();
    for binding in &topology.face_surfaces {
        out.entry(binding.entity)
            .and_modify(|carrier| *carrier = None)
            .or_insert(Some(binding.carrier));
    }
    out
}

pub(super) fn unique_carrier_faces(
    topology: &AsmHistoricalTopology,
    included_faces: &HashSet<i64>,
) -> HashMap<i64, Option<i64>> {
    let mut out = HashMap::new();
    for binding in &topology.face_surfaces {
        if !included_faces.contains(&binding.entity) {
            continue;
        }
        out.entry(binding.carrier)
            .and_modify(|face| *face = None)
            .or_insert(Some(binding.entity));
    }
    out
}
