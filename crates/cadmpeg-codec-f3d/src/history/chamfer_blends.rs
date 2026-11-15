// SPDX-License-Identifier: Apache-2.0
//! Edges that a `Chamfer` feature bevels, from the planar faces of its transition.

use super::blend_spine::{edge_geometry, edge_points, plane_rows};
use super::fillet_blends::{
    agrees, replaceable_native, treatment_faces, GroupBinding, TreatmentFaces,
};
use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTransition};
use cadmpeg_ir::features::edge_treatments::ChamferSpec;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::HashSet;

/// Millimetres between a setback distance of a bevel and the distance of its
/// group.
const CHAMFER_SETBACK_TOLERANCE: f64 = 1.0e-6;
/// Dot product of the unit tangent of an edge and the unit normal of a bevel
/// plane that an edge along the bevel has.
const CHAMFER_ALONG_TOLERANCE: f64 = 1.0e-9;
/// Smallest dot product of the unit normal of a bevel plane and a unit
/// direction across a support from which a setback distance is read.
const CHAMFER_ACROSS_MINIMUM: f64 = 1.0e-6;

/// One bevel face: the setback distances on its two supports, sorted, and the
/// deleted edge along it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Bevel {
    setbacks: [f64; 2],
    edge: i64,
}

/// The distance from the line `edge` of a support plane to the line where a
/// bevel plane cuts that support, measured within the support, or `None` when
/// the bevel plane runs along the support direction across the edge.
fn setback(
    bevel: (Point3, Vector3),
    support_normal: Vector3,
    edge: (Point3, Vector3),
) -> Option<f64> {
    let (origin, normal) = bevel;
    let (point, tangent) = edge;
    let across = support_normal.cross(tangent).unit()?;
    let slope = normal.dot(across);
    if slope.abs() < CHAMFER_ACROSS_MINIMUM {
        return None;
    }
    Some((normal.dot(point.vector_from(origin)) / slope).abs())
}

/// The bevel faces that a transition inserts on new planar carriers, each
/// with the one deleted edge that it replaces, or `None` when
/// `treatment_faces` reads none, a bevel carrier is not a plane or has no
/// complete pair of planar supports, the supports of a bevel share no edge
/// along its plane or more than one, the edge has no axis row, tangent, or
/// interior point, the transition keeps the edge, or two bevels replace one
/// edge.
fn transition_bevels(
    transition: &AsmHistoricalTransition,
    input: &AsmHistoricalTopology,
    output: &AsmHistoricalTopology,
) -> Option<Vec<Bevel>> {
    let TreatmentFaces {
        input_boundaries,
        input_carriers,
        supports,
        ..
    } = treatment_faces(transition, input, output)?;
    let output_planes = plane_rows(output);
    let input_planes = plane_rows(input);
    let edges = edge_geometry(input);
    let points = edge_points(input);
    let deleted = transition
        .topology
        .edges
        .deleted
        .iter()
        .copied()
        .collect::<HashSet<_>>();
    let mut bevels = Vec::new();
    let mut replaced = HashSet::new();
    for (_, carrier, supports, complete) in supports {
        if !complete {
            return None;
        }
        let (origin, normal) = output_planes.get(&carrier).copied().flatten()?;
        let normal = normal.unit()?;
        let mut along = Vec::new();
        for (ordinal, left) in supports.iter().enumerate() {
            let left_edges = input_boundaries.get(left)?;
            for right in &supports[ordinal + 1..] {
                let right_edges = input_boundaries.get(right)?;
                for edge in left_edges.intersection(right_edges) {
                    let tangent = edges.get(edge).copied().unwrap_or_default().tangent()?;
                    let tangent = tangent.unit()?;
                    if normal.dot(tangent).abs() <= CHAMFER_ALONG_TOLERANCE {
                        along.push((*edge, *left, *right, tangent));
                    }
                }
            }
        }
        let [(edge, left, right, tangent)] = along.as_slice() else {
            return None;
        };
        if !deleted.contains(edge) || !replaced.insert(*edge) {
            return None;
        }
        let point = points.get(edge).copied().flatten()?;
        let support_normal = |support: &i64| {
            let carrier = input_carriers.get(support).copied().flatten()?;
            input_planes.get(&carrier).copied().flatten()?.1.unit()
        };
        let mut setbacks = [
            setback((origin, normal), support_normal(left)?, (point, *tangent))?,
            setback((origin, normal), support_normal(right)?, (point, *tangent))?,
        ];
        setbacks.sort_by(f64::total_cmp);
        bevels.push(Bevel {
            setbacks,
            edge: *edge,
        });
    }
    Some(bevels)
}

/// The sorted setback distances of a chamfer specification, or `None` for a
/// specification that states no two distances.
fn spec_setbacks(spec: ChamferSpec) -> Option<[f64; 2]> {
    let mut setbacks = match spec {
        ChamferSpec::Distance { distance } => [distance.get(), distance.get()],
        ChamferSpec::TwoDistances { first, second } => [first.get(), second.get()],
        ChamferSpec::Unresolved { .. } | ChamferSpec::DistanceAngle { .. } => return None,
    };
    setbacks.sort_by(f64::total_cmp);
    Some(setbacks)
}

/// The edges each group bevels, sorted, by the setback distances of its
/// bevels, or `None` when a bevel matches no group or more than one.
fn group_edges(specs: &[Option<[f64; 2]>], bevels: &[Bevel]) -> Option<Vec<Vec<i64>>> {
    let mut edges = vec![Vec::new(); specs.len()];
    for bevel in bevels {
        let mut owners = specs.iter().enumerate().filter(|(_, spec)| {
            spec.is_some_and(|spec| {
                spec.iter()
                    .zip(bevel.setbacks)
                    .all(|(want, have)| (want - have).abs() <= CHAMFER_SETBACK_TOLERANCE)
            })
        });
        let (Some((owner, _)), None) = (owners.next(), owners.next()) else {
            return None;
        };
        edges.get_mut(owner)?.push(bevel.edge);
    }
    for group in &mut edges {
        group.sort_unstable();
    }
    Some(edges)
}

/// Bind each `Chamfer` group whose operand proofs leave it native or partial
/// to the edges its bevels replace.
///
/// A chamfer replaces each edge that it bevels with a planar face. When every
/// new carrier of the transition is such a face whose supports are planes that
/// share one edge along the face and the transition deletes that edge, and
/// the face cuts both supports at the setback distances of one group, those
/// edges hold every beveled edge: a beveled edge that kept its slot would
/// leave its face with a kept edge along it. The bevels at a group's distances
/// therefore name exactly its edges. A selection that disagrees with its
/// bevel edges contradicts that claim for the whole transition, so then no
/// group binds. Returns whether a group binds.
pub(super) fn bind_chamfer_groups(
    definition: &mut FeatureDefinition,
    feature_id: &cadmpeg_ir::features::FeatureId,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    previous_state_id: i64,
    states: (
        &AsmHistoricalTransition,
        &AsmHistoricalTopology,
        &AsmHistoricalTopology,
    ),
) -> GroupBinding {
    let FeatureDefinition::Operation(FeatureOperation::Chamfer {
        groups: chamfer_groups,
        ..
    }) = definition
    else {
        return GroupBinding::Unbound;
    };
    let (transition, input, output) = states;
    let Some(bevels) = transition_bevels(transition, input, output) else {
        return GroupBinding::Unbound;
    };
    let specs = chamfer_groups
        .iter()
        .map(|group| spec_setbacks(group.spec))
        .collect::<Vec<_>>();
    let Some(edges) = group_edges(&specs, &bevels) else {
        return GroupBinding::Unbound;
    };
    if !chamfer_groups
        .iter()
        .zip(&edges)
        .all(|(group, slots)| agrees(&group.edges, feature_id, previous_state_id, slots))
    {
        return GroupBinding::Unbound;
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut rebound = false;
    for (chamfer_group, slots) in chamfer_groups.iter_mut().zip(edges) {
        let Some(native) = replaceable_native(&chamfer_group.edges).map(str::to_owned) else {
            continue;
        };
        let mut matching = groups.iter().filter(|group| {
            group.id == native
                && group.scope_record_index == scope.record_index
                && crate::ids::native_stream(&group.id) == stream
        });
        let (Some(group), None) = (matching.next(), matching.next()) else {
            continue;
        };
        if group.members().is_empty() || slots.is_empty() {
            continue;
        }
        let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
        let selected = slots
            .into_iter()
            .map(|slot| crate::ids::history_input_edge_id(&prefix, slot))
            .collect();
        if let Ok(historical) = EdgeSelection::historical(
            crate::ids::feature_input_topology_id(feature_id, previous_state_id),
            selected,
            native,
        ) {
            chamfer_group.edges = historical;
            rebound = true;
        }
    }
    if rebound {
        GroupBinding::Bound
    } else {
        GroupBinding::Unbound
    }
}

#[cfg(test)]
mod tests;
