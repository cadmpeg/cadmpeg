// SPDX-License-Identifier: Apache-2.0
//! Edges that a `Fillet` feature rounds, from the blend faces of its transition.

use super::blend_spine::{
    blend_spines, cylinder_axes, edge_geometry, edge_points, plane_rows, BlendSpine, CylinderAxis,
    EdgeGeometry,
};
use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory};
use cadmpeg_ir::features::edge_treatments::RadiusSpec;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Millimetres between a blend radius and the radius of its group.
const FILLET_BLEND_RADIUS_TOLERANCE: f64 = 1.0e-6;
/// Millimetres by which an edge may pass the axial extent of a blend's
/// boundary and still lie within it.
const FILLET_BLEND_EXTENT_TOLERANCE: f64 = 1.0e-6;

/// One new blend face: its radius and the deleted edge along its spine.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Blend {
    radius: f64,
    edge: i64,
}

/// The boundary edges of each face whose boundary rebuilds.
type Boundaries = HashMap<i64, HashSet<i64>>;

/// Rows of both states that locate the edge a blend replaces.
struct BlendRows<'a> {
    edges: HashMap<i64, EdgeGeometry>,
    deleted: HashSet<i64>,
    input_boundaries: &'a Boundaries,
    output_boundaries: &'a Boundaries,
    input_carriers: &'a HashMap<i64, Option<i64>>,
    input_points: HashMap<i64, Option<Point3>>,
    output_points: HashMap<i64, Option<Point3>>,
    planes: HashMap<i64, Option<(Point3, Vector3)>>,
    owners: HashMap<i64, Vec<i64>>,
    output_faces: HashSet<i64>,
}

impl<'a> BlendRows<'a> {
    fn new(
        transition: &AsmHistoricalTransition,
        (input, output): (&AsmHistoricalTopology, &AsmHistoricalTopology),
        (input_boundaries, output_boundaries): (&'a Boundaries, &'a Boundaries),
        input_carriers: &'a HashMap<i64, Option<i64>>,
    ) -> Self {
        let mut owners = HashMap::<i64, Vec<i64>>::new();
        for (face, boundary) in input_boundaries {
            for edge in boundary {
                owners.entry(*edge).or_default().push(*face);
            }
        }
        Self {
            edges: edge_geometry(input),
            deleted: transition.topology.edges.deleted.iter().copied().collect(),
            input_boundaries,
            output_boundaries,
            input_carriers,
            input_points: edge_points(input),
            output_points: edge_points(output),
            planes: plane_rows(input),
            owners,
            output_faces: output.faces.iter().copied().collect(),
        }
    }

    /// The preceding edge that a cylinder blend replaces when its supports
    /// share no edge along its axis: the one edge on the preceding boundary of
    /// a support that runs along the axis, lies farther than the radius from
    /// the axis line and within the axial extent of the blend's boundary, and
    /// has two owner faces: a support whose plane touches the cylinder, and a
    /// face that the transition removes and that shares a preceding edge with
    /// another support. Kept and deleted edges qualify alike. `None` when no
    /// edge or more than one qualifies, or when a row that decides an edge is
    /// missing or unusable.
    fn rounded_edge(
        &self,
        blend: i64,
        spine: BlendSpine,
        axis: CylinderAxis,
        radius: f64,
        supports: &[i64],
    ) -> Option<i64> {
        let mut extent = None::<(f64, f64)>;
        for edge in self.output_boundaries.get(&blend)? {
            let position = axis.position(self.output_points.get(edge).copied().flatten()?);
            let (low, high) = extent.unwrap_or((position, position));
            extent = Some((low.min(position), high.max(position)));
        }
        let (low, high) = extent?;
        let mut pool = BTreeSet::new();
        for support in supports {
            pool.extend(self.input_boundaries.get(support)?);
        }
        let mut found = None;
        for edge in pool {
            let geometry = self.edges.get(edge).copied().unwrap_or_default();
            if spine.rules_out(geometry) {
                continue;
            }
            if !geometry.is_classified() {
                return None;
            }
            if !spine.admits(geometry) {
                continue;
            }
            let point = self.input_points.get(edge).copied().flatten()?;
            let position = axis.position(point);
            if !axis.clears(radius, point)
                || position < low - FILLET_BLEND_EXTENT_TOLERANCE
                || position > high + FILLET_BLEND_EXTENT_TOLERANCE
            {
                continue;
            }
            let Some([first, second]) = self.owners.get(edge).map(Vec::as_slice) else {
                continue;
            };
            let (mut touching, mut unknown) = (false, false);
            for (near, far) in [(first, second), (second, first)] {
                if !supports.contains(near) || !self.beside_support(*far, *near, supports) {
                    continue;
                }
                let carrier = self.input_carriers.get(near).copied().flatten()?;
                match self.planes.get(&carrier) {
                    Some(plane) => touching |= axis.touches(radius, (*plane)?)?,
                    None => unknown = true,
                }
            }
            if touching {
                if found.replace(*edge).is_some() {
                    return None;
                }
            } else if unknown {
                return None;
            }
        }
        found
    }

    /// Whether the transition removes `face` and `face` shares a preceding
    /// edge with a support other than `support`.
    fn beside_support(&self, face: i64, support: i64, supports: &[i64]) -> bool {
        let Some(boundary) = self.input_boundaries.get(&face) else {
            return false;
        };
        !self.output_faces.contains(&face)
            && supports.iter().any(|other| {
                *other != support
                    && self
                        .input_boundaries
                        .get(other)
                        .is_some_and(|edges| !edges.is_disjoint(boundary))
            })
    }
}

/// The blend faces that a transition inserts on new carriers, each with the
/// one deleted edge that it replaces, or `None` when a face of either state
/// has no rebuilt boundary or no single carrier, a result face on a new
/// carrier persists or is not listed as inserted, a new carrier has no spine
/// or finite positive radius row, an adjacent face of a blend maps to no single input
/// face or has no rebuilt boundary, a support has no input boundary, an edge
/// that two supports share has no axis row or tangent, a blend's supports
/// share more than one edge along its spine, or none and the blend is not a
/// cylinder whose edge `BlendRows::rounded_edge` locates, the transition
/// keeps that edge, or two blends replace one edge.
fn transition_blends(
    transition: &AsmHistoricalTransition,
    input: &AsmHistoricalTopology,
    output: &AsmHistoricalTopology,
) -> Option<Vec<Blend>> {
    let inserted = &transition.topology.faces.inserted;
    let carriers = super::unique_face_carriers(output);
    let input_surfaces = input.surfaces.iter().copied().collect::<HashSet<_>>();
    // A result face on a new carrier that persists from the preceding
    // topology, or that the transition does not list as inserted, could be
    // an unlisted blend.
    let input_faces = input.faces.iter().copied().collect::<HashSet<_>>();
    let listed = inserted.iter().copied().collect::<HashSet<_>>();
    for face in &output.faces {
        let carrier = carriers.get(face).copied().flatten()?;
        if !input_surfaces.contains(&carrier)
            && (input_faces.contains(face) || !listed.contains(face))
        {
            return None;
        }
    }
    let mut blend_faces = Vec::new();
    for face in inserted {
        let carrier = carriers.get(face).copied().flatten()?;
        if !input_surfaces.contains(&carrier) {
            blend_faces.push(*face);
        }
    }
    if blend_faces.is_empty() {
        return None;
    }
    let output_boundaries = super::face_boundary_edge_index(output);
    let input_boundaries = super::face_boundary_edge_index(input);
    // A face whose boundary does not rebuild, or that has no single carrier,
    // could hide a support or a shared edge, in either state.
    let input_carriers = super::unique_face_carriers(input);
    let rebuilt = |faces: &[i64],
                   boundaries: &HashMap<i64, HashSet<i64>>,
                   carriers: &HashMap<i64, Option<i64>>| {
        faces.iter().all(|face| {
            boundaries.contains_key(face) && carriers.get(face).copied().flatten().is_some()
        })
    };
    if !rebuilt(&output.faces, &output_boundaries, &carriers)
        || !rebuilt(&input.faces, &input_boundaries, &input_carriers)
    {
        return None;
    }
    let supports = super::treatment_face_supports(&blend_faces, output, input, &output_boundaries);
    if supports.len() != blend_faces.len() {
        return None;
    }
    let spines = blend_spines(output);
    let mut radii = HashMap::<i64, Option<f64>>::new();
    for row in &output.surface_radii {
        radii
            .entry(row.surface)
            .and_modify(|radius| *radius = None)
            .or_insert(Some(row.radius));
    }
    let rows = BlendRows::new(
        transition,
        (input, output),
        (&input_boundaries, &output_boundaries),
        &input_carriers,
    );
    let axes = cylinder_axes(output);
    let mut blends = Vec::new();
    let mut replaced = HashSet::new();
    for (blend, carrier, supports, complete) in supports {
        if !complete {
            return None;
        }
        let spine = spines.get(&carrier)?;
        let radius = radii
            .get(&carrier)
            .copied()
            .flatten()
            .filter(|radius| radius.is_finite() && *radius > 0.0)?;
        let mut candidates = BTreeSet::new();
        for (ordinal, left) in supports.iter().enumerate() {
            let left_edges = input_boundaries.get(left)?;
            for right in &supports[ordinal + 1..] {
                let right_edges = input_boundaries.get(right)?;
                for edge in left_edges.intersection(right_edges) {
                    let geometry = rows.edges.get(edge).copied().unwrap_or_default();
                    if !geometry.is_classified() {
                        return None;
                    }
                    if spine.admits(geometry) {
                        candidates.insert(*edge);
                    }
                }
            }
        }
        let edge = if candidates.is_empty() {
            let axis = axes.get(&carrier).copied()?;
            rows.rounded_edge(blend, *spine, axis, radius, &supports)?
        } else {
            let mut candidates = candidates.into_iter();
            let (Some(edge), None) = (candidates.next(), candidates.next()) else {
                return None;
            };
            edge
        };
        if !rows.deleted.contains(&edge) || !replaced.insert(edge) {
            return None;
        }
        blends.push(Blend { radius, edge });
    }
    Some(blends)
}

/// The edges each group rounds, sorted, by the constant radius of its blends,
/// or `None` when a blend matches no group radius or more than one.
fn group_edges(radii: &[Option<f64>], blends: &[Blend]) -> Option<Vec<Vec<i64>>> {
    let mut edges = vec![Vec::new(); radii.len()];
    for blend in blends {
        let mut owners = radii.iter().enumerate().filter(|(_, radius)| {
            radius.is_some_and(|radius| {
                (radius - blend.radius).abs() <= FILLET_BLEND_RADIUS_TOLERANCE
            })
        });
        let (Some((owner, _)), None) = (owners.next(), owners.next()) else {
            return None;
        };
        edges.get_mut(owner)?.push(blend.edge);
    }
    for group in &mut edges {
        group.sort_unstable();
    }
    Some(edges)
}

/// Whether a group's selection agrees with the blend edges `slots` at its
/// radius. A native or unresolved selection states no edge. A partial
/// selection agrees when every edge it states is in `slots`, and a complete
/// selection when it states exactly `slots`. A selection of edges outside the
/// input topology states edges that `slots` cannot confirm.
fn agrees(
    selection: &EdgeSelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    slots: &[i64],
) -> bool {
    let slot = |edge: &cadmpeg_ir::ids::HistoricalEdgeId| {
        crate::ids::feature_input_edge_slot(feature_id, previous_state_id, edge)
    };
    match selection {
        EdgeSelection::Native(_) | EdgeSelection::Unresolved => true,
        EdgeSelection::HistoricalPartial { edges, .. } => edges
            .iter()
            .all(|edge| slot(edge).is_some_and(|slot| slots.contains(&slot))),
        EdgeSelection::Historical { edges, .. } => {
            edges.iter().map(slot).collect::<Option<HashSet<_>>>()
                == Some(slots.iter().copied().collect())
        }
        EdgeSelection::All
        | EdgeSelection::Edges(_)
        | EdgeSelection::Resolved { .. }
        | EdgeSelection::Generated { .. } => false,
    }
}

/// The native group reference of a selection that the blend edges replace,
/// or `None` when the selection stays. A complete selection that agrees
/// already states the blend edges, in its operand order.
fn replaceable_native(selection: &EdgeSelection) -> Option<&str> {
    match selection {
        EdgeSelection::Native(native) => Some(native.as_str()),
        EdgeSelection::HistoricalPartial { native, .. } => Some(native.as_str()),
        EdgeSelection::Historical { .. }
        | EdgeSelection::Unresolved
        | EdgeSelection::All
        | EdgeSelection::Edges(_)
        | EdgeSelection::Resolved { .. }
        | EdgeSelection::Generated { .. } => None,
    }
}

/// Bind each `Fillet` group whose operand proofs leave it native or partial
/// to the edges its blends replace.
///
/// A constant-radius fillet replaces each edge that it rounds with a blend
/// face at its group's radius. When every new carrier of the transition is a
/// blend whose supports share one edge along its spine and the transition
/// deletes that edge, those edges hold every rounded edge: a rounded edge
/// that kept its slot would leave its blend with a kept edge along its spine.
/// The blends at a group's radius therefore name exactly its edges, and a
/// tangent chain can give the group more or fewer members than edges. A
/// selection that disagrees with its blend edges contradicts that claim for
/// the whole transition, so then no group binds.
pub(super) fn bind_fillet_groups(
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
) {
    let FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: fillet_groups,
    }) = definition
    else {
        return;
    };
    let (transition, input, output) = states;
    let Some(blends) = transition_blends(transition, input, output) else {
        return;
    };
    let radii = fillet_groups
        .iter()
        .map(|group| match group.radius {
            RadiusSpec::Constant { radius } => Some(radius.get()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let Some(edges) = group_edges(&radii, &blends) else {
        return;
    };
    if !fillet_groups
        .iter()
        .zip(&edges)
        .all(|(group, slots)| agrees(&group.edges, feature_id, previous_state_id, slots))
    {
        return;
    }
    let stream = crate::ids::native_stream(&scope.id);
    for (fillet_group, slots) in fillet_groups.iter_mut().zip(edges) {
        let Some(native) = replaceable_native(&fillet_group.edges).map(str::to_owned) else {
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
        if group.members().is_empty() {
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
            fillet_group.edges = historical;
        }
    }
}

/// Design records that `Fillet` binding reads.
pub(crate) struct FilletBlendInputs<'a> {
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    pub(crate) histories: &'a mut [AsmHistory],
}

/// Bind the edges of each `Fillet` feature from the blend faces of its
/// transition.
pub(crate) fn bind_fillet_blend_edges(
    features: &mut [cadmpeg_ir::features::Feature],
    input: FilletBlendInputs<'_>,
) {
    let FilletBlendInputs {
        scopes,
        groups,
        histories,
    } = input;
    let states = features
        .iter()
        .map(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
            )
            .then(|| super::face_selection_state(feature, scopes, histories))
            .flatten()
        })
        .collect::<Vec<_>>();
    let gates = states
        .iter()
        .enumerate()
        .filter_map(|(position, state)| {
            state.map(|state| (state.history, state.previous, position))
        })
        .collect::<Vec<_>>();
    let mut available = HashSet::new();
    super::visit_states_oldest_first(histories, gates, |history, previous, position| {
        if history.topology(previous).is_some() {
            available.insert(position);
        }
    });
    let histories = &*histories;
    for (position, (feature, state)) in features.iter_mut().zip(states).enumerate() {
        let Some(state) = state.filter(|_| available.contains(&position)) else {
            continue;
        };
        let Some(history) = histories.get(state.history) else {
            continue;
        };
        let (Some(output_state), Some(input_state)) = (
            history.states().get(state.state),
            history.states().get(state.previous),
        ) else {
            continue;
        };
        let (Some(transition), Some(input), Some(output)) = (
            output_state.transition.as_ref(),
            history.topology(input_state),
            history.topology(output_state),
        ) else {
            continue;
        };
        let feature_id = feature.id.clone();
        feature.evaluation.edit(|definition, _| {
            bind_fillet_groups(
                definition,
                &feature_id,
                state.scope,
                groups,
                state.previous_state_id,
                (transition, input, output),
            );
        });
    }
}

#[cfg(test)]
mod tests;
