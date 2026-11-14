// SPDX-License-Identifier: Apache-2.0
//! Edges that a `Fillet` feature rounds, from the blend faces of its transition.

use super::blend_spine::{
    blend_spines, cylinder_axes, edge_ends, edge_geometry, edge_points, plane_rows, BlendSpine,
    CylinderAxis, EdgeGeometry,
};
use crate::history_records::{AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory};
use crate::loss::F3dLossCode;
use cadmpeg_ir::features::edge_treatments::RadiusSpec;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::report::loss::LossNote;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// Millimetres between a blend radius and the radius of its group.
const FILLET_BLEND_RADIUS_TOLERANCE: f64 = 1.0e-6;
/// Millimetres by which an edge may pass the axial extent of a blend's
/// boundary and still lie within it.
const FILLET_BLEND_EXTENT_TOLERANCE: f64 = 1.0e-6;
/// Millimetres by which the span of an edge may fall short of an end of the
/// axial extent of a blend's boundary and still cover it.
const FILLET_BLEND_COVER_TOLERANCE: f64 = 1.0e-6;

/// One new blend face: its radius, the deleted edge along its spine, and how
/// the proof found that edge.
#[derive(Clone, Debug, PartialEq)]
struct Blend {
    radius: f64,
    edge: i64,
    found: Found,
}

/// How the proof found a blend's edge.
#[derive(Clone, Debug, PartialEq)]
enum Found {
    /// Two supports of the blend share the edge.
    Shared,
    /// Supports of the blend share several edges along its axis; `shared`
    /// lists them in slot order, and the edge is the one that covers the
    /// blend's axial extent.
    Covering { shared: Vec<SharedEdge> },
    /// The supports share no edge along the axis; `candidates` are the
    /// candidates of `BlendRows::rounded_edge` in rank order, and the edge is
    /// the best-ranked deleted one.
    Inferred { candidates: Vec<Candidate> },
}

/// One candidate edge of `BlendRows::rounded_edge`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Candidate {
    edge: i64,
    /// Whether the edge has two owner faces and its other owner is a removed
    /// face that `BlendRows::reaches` another support.
    linked: bool,
    /// Whether the transition deletes the edge.
    deleted: bool,
}

/// One edge along the axis of a cylinder blend that two of its supports share.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SharedEdge {
    edge: i64,
    /// Whether the span of the edge between its end points covers the axial
    /// extent of the blend's boundary.
    covers: bool,
    /// Whether the transition deletes the edge.
    deleted: bool,
}

/// The boundary edges of each face whose boundary rebuilds.
type Boundaries = HashMap<i64, HashSet<i64>>;

/// The spine, axis, radius, and axial extent of one cylinder blend.
#[derive(Clone, Copy)]
struct CylinderBlend {
    spine: BlendSpine,
    axis: CylinderAxis,
    radius: f64,
    low: f64,
    high: f64,
}

impl CylinderBlend {
    /// Whether `point` lies within the axial extent of the blend's boundary.
    fn spans(self, point: Point3) -> bool {
        let position = self.axis.position(point);
        position >= self.low - FILLET_BLEND_EXTENT_TOLERANCE
            && position <= self.high + FILLET_BLEND_EXTENT_TOLERANCE
    }

    /// Whether an edge runs along the axis: `None` when its rows cannot show
    /// it.
    fn runs_along(self, geometry: EdgeGeometry) -> Option<bool> {
        if self.spine.rules_out(geometry) {
            return Some(false);
        }
        geometry
            .is_classified()
            .then(|| self.spine.admits(geometry))
    }
}

/// Rows of both states that locate the edge a blend replaces.
struct BlendRows<'a> {
    edges: HashMap<i64, EdgeGeometry>,
    deleted: HashSet<i64>,
    input_boundaries: &'a Boundaries,
    output_boundaries: &'a Boundaries,
    input_carriers: &'a HashMap<i64, Option<i64>>,
    input_points: HashMap<i64, Option<Point3>>,
    input_ends: HashMap<i64, Option<[Point3; 2]>>,
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
            input_ends: edge_ends(input),
            output_points: edge_points(output),
            planes: plane_rows(input),
            owners,
            output_faces: output.faces.iter().copied().collect(),
        }
    }

    /// The lowest and highest positions along `axis` of the interior sample
    /// points of the boundary edges of `blend_face`. `None` when the face has
    /// no rebuilt boundary or no boundary edge, or a boundary edge has no
    /// single finite interior sample.
    fn extent(&self, blend_face: i64, axis: CylinderAxis) -> Option<(f64, f64)> {
        let mut extent = None::<(f64, f64)>;
        for edge in self.output_boundaries.get(&blend_face)? {
            let position = axis.position(self.output_points.get(edge).copied().flatten()?);
            let (low, high) = extent.unwrap_or((position, position));
            extent = Some((low.min(position), high.max(position)));
        }
        extent
    }

    /// The edges `shared` along the axis of a cylinder blend of `radius` that
    /// two of its supports share, in slot order. The reading takes the one
    /// edge whose span between its end points covers the axial extent of the
    /// blend's boundary. `None` when `BlendRows::extent` gives no extent, no
    /// edge or more than one covers the extent, an edge has no single pair of
    /// finite end points, or the covering edge has no single finite interior
    /// sample or its interior sample point lies at most `1e-6` mm beyond the
    /// radius from the axis line.
    fn covering_edge(
        &self,
        blend_face: i64,
        (axis, radius): (CylinderAxis, f64),
        shared: &BTreeSet<i64>,
    ) -> Option<Vec<SharedEdge>> {
        let (low, high) = self.extent(blend_face, axis)?;
        let mut edges = Vec::new();
        for edge in shared {
            let [start, end] = self.input_ends.get(edge).copied().flatten()?;
            let (start, end) = (axis.position(start), axis.position(end));
            edges.push(SharedEdge {
                edge: *edge,
                covers: start.min(end) <= low + FILLET_BLEND_COVER_TOLERANCE
                    && start.max(end) >= high - FILLET_BLEND_COVER_TOLERANCE,
                deleted: self.deleted.contains(edge),
            });
        }
        let mut covering = edges.iter().filter(|edge| edge.covers);
        let (Some(edge), None) = (covering.next(), covering.next()) else {
            return None;
        };
        let point = self.input_points.get(&edge.edge).copied().flatten()?;
        axis.clears(radius, point).then_some(edges)
    }

    /// The candidates of a cylinder blend whose supports share no edge along
    /// its axis, in rank order. A
    /// candidate lies on the preceding boundary of a support, runs along the
    /// axis, lies farther than the radius from the axis line and within the
    /// axial extent of the blend's boundary, and has a support owner whose
    /// plane touches the cylinder. Candidates rank first when their other
    /// owner is a face that the transition removes and that `reaches`
    /// another support, then by distance from the axis line, then by slot.
    /// The blend replaces the best-ranked deleted candidate. `None` when no
    /// candidate is deleted, or when a row that decides an edge is missing or
    /// unusable.
    fn rounded_edge(
        &self,
        blend_face: i64,
        spine: BlendSpine,
        axis: CylinderAxis,
        radius: f64,
        supports: &[i64],
    ) -> Option<Vec<Candidate>> {
        let (low, high) = self.extent(blend_face, axis)?;
        let blend = CylinderBlend {
            spine,
            axis,
            radius,
            low,
            high,
        };
        let mut pool = BTreeSet::new();
        for support in supports {
            pool.extend(self.input_boundaries.get(support)?);
        }
        let mut along = BTreeMap::new();
        for edge in pool {
            if blend.runs_along(self.edges.get(edge).copied().unwrap_or_default())? {
                along.insert(*edge, self.input_points.get(edge).copied().flatten()?);
            }
        }
        let mut candidates = Vec::new();
        for (edge, point) in &along {
            if !axis.clears(radius, *point) || !blend.spans(*point) {
                continue;
            }
            let owners = self.owners.get(edge).map_or(&[][..], Vec::as_slice);
            let (mut touching, mut unknown, mut linked) = (false, false, false);
            for near in owners.iter().filter(|owner| supports.contains(owner)) {
                let carrier = self.input_carriers.get(near).copied().flatten()?;
                let Some(plane) = self.planes.get(&carrier) else {
                    unknown = true;
                    continue;
                };
                if axis.touches(radius, (*plane)?)? {
                    touching = true;
                    let other = owners.iter().find(|owner| *owner != near);
                    linked |= owners.len() == 2
                        && other.is_some_and(|other| {
                            self.reaches(*other, *near, supports, (blend, &along))
                        });
                }
            }
            if !touching {
                if unknown {
                    return None;
                }
                continue;
            }
            candidates.push((!linked, axis.distance(*point), *edge));
        }
        candidates.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.total_cmp(&right.1))
                .then(left.2.cmp(&right.2))
        });
        let ranked = candidates
            .into_iter()
            .map(|(unlinked, _, edge)| Candidate {
                edge,
                linked: !unlinked,
                deleted: self.deleted.contains(&edge),
            })
            .collect::<Vec<_>>();
        ranked
            .iter()
            .any(|candidate| candidate.deleted)
            .then_some(ranked)
    }

    /// Whether the transition removes `face` and `face` meets a support other
    /// than `support` at an edge of `along` that lies on the blend's cylinder
    /// and within its axial extent: the edge that an overflow blend rolls
    /// over. `along` holds the point of each support edge that runs along the
    /// blend's axis.
    fn reaches(
        &self,
        face: i64,
        support: i64,
        supports: &[i64],
        (blend, along): (CylinderBlend, &BTreeMap<i64, Point3>),
    ) -> bool {
        !self.output_faces.contains(&face)
            && self.input_boundaries.get(&face).is_some_and(|boundary| {
                supports
                    .iter()
                    .filter(|other| **other != support)
                    .filter_map(|other| self.input_boundaries.get(other))
                    .flat_map(|edges| boundary.intersection(edges))
                    .filter_map(|edge| along.get(edge))
                    .any(|point| blend.axis.meets(blend.radius, *point) && blend.spans(*point))
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
/// share more than one edge along its spine and the blend is not a cylinder
/// whose row radius is its radius and for which `BlendRows::covering_edge`
/// reads an edge, or none and the blend is not such a cylinder for which
/// `BlendRows::rounded_edge` reads an edge, the transition keeps that edge,
/// two blends replace one edge, or a deleted edge that the covering reading
/// of a blend assumes the blend consumes is replaced by a blend or consumed
/// by another blend.
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
                let intersection = left_edges.intersection(right_edges);
                for edge in intersection {
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
        let axis = || {
            axes.get(&carrier)
                .copied()
                .filter(|axis| axis.has_radius(radius))
        };
        let (edge, found) = match candidates.len() {
            0 => {
                let candidates = rows.rounded_edge(blend, *spine, axis()?, radius, &supports)?;
                (
                    candidates.iter().find(|candidate| candidate.deleted)?.edge,
                    Found::Inferred { candidates },
                )
            }
            1 => (candidates.pop_first()?, Found::Shared),
            _ => {
                let shared = rows.covering_edge(blend, (axis()?, radius), &candidates)?;
                (
                    shared.iter().find(|edge| edge.covers)?.edge,
                    Found::Covering { shared },
                )
            }
        };
        if !rows.deleted.contains(&edge) || !replaced.insert(edge) {
            return None;
        }
        blends.push(Blend {
            radius,
            edge,
            found,
        });
    }
    (!consumption_conflicts(&blends, &replaced)).then_some(blends)
}

/// Whether a deleted edge that the covering reading of a blend assumes the
/// blend consumes is also `replaced` by a blend or consumed by another blend.
fn consumption_conflicts(blends: &[Blend], replaced: &HashSet<i64>) -> bool {
    let mut consumed = HashSet::new();
    blends.iter().any(|blend| match &blend.found {
        Found::Covering { shared } => shared
            .iter()
            .filter(|edge| !edge.covers && edge.deleted)
            .any(|edge| replaced.contains(&edge.edge) || !consumed.insert(edge.edge)),
        Found::Shared | Found::Inferred { .. } => false,
    })
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
/// the whole transition, so then no group binds. A cylinder blend whose
/// supports share no edge along its axis gives its best-ranked deleted
/// candidate. A group that binds such an edge gets a note that states the
/// assumptions of the reading and, when other candidates exist, a note that
/// states the rank and the status of every candidate. A slot that the
/// replaced partial selection already states gets no such note. A cylinder
/// blend whose supports share several edges along its axis gives the one
/// whose span covers the blend's axial extent, with a note that states the
/// status of every shared edge; a stated slot keeps this note, because the
/// edges that the blend consumes stay assumed. Returns whether a group binds, and whether a blend of the transition takes the
/// covering reading.
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
    notes: &mut Vec<LossNote>,
) -> GroupBinding {
    let FeatureDefinition::Operation(FeatureOperation::Fillet {
        groups: fillet_groups,
    }) = definition
    else {
        return GroupBinding::Unbound;
    };
    let (transition, input, output) = states;
    let Some(blends) = transition_blends(transition, input, output) else {
        return GroupBinding::Unbound;
    };
    let covering = blends
        .iter()
        .any(|blend| matches!(blend.found, Found::Covering { .. }));
    let mut rebound = false;
    let radii = fillet_groups
        .iter()
        .map(|group| match group.radius {
            RadiusSpec::Constant { radius } => Some(radius.get()),
            _ => None,
        })
        .collect::<Vec<_>>();
    let Some(edges) = group_edges(&radii, &blends) else {
        return GroupBinding::Unbound;
    };
    if !fillet_groups
        .iter()
        .zip(&edges)
        .all(|(group, slots)| agrees(&group.edges, feature_id, previous_state_id, slots))
    {
        return GroupBinding::Unbound;
    }
    let found = blends
        .iter()
        .map(|blend| (blend.edge, &blend.found))
        .collect::<HashMap<_, _>>();
    let stream = crate::ids::native_stream(&scope.id);
    'groups: for (index, (fillet_group, slots)) in fillet_groups.iter_mut().zip(edges).enumerate() {
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
        let stated = match &fillet_group.edges {
            EdgeSelection::HistoricalPartial { edges, .. } => edges
                .iter()
                .filter_map(|edge| {
                    crate::ids::feature_input_edge_slot(feature_id, previous_state_id, edge)
                })
                .collect(),
            _ => HashSet::new(),
        };
        let mut inferred = Vec::new();
        for slot in &slots {
            let at = format!(
                "Fillet group {index} of feature {}: the edge at slot {slot} of history state {previous_state_id}",
                feature_id.as_str()
            );
            match found.get(slot) {
                Some(Found::Inferred { candidates }) if !stated.contains(slot) => {
                    let Some(notes) = inferred_edge_notes(&at, candidates) else {
                        continue 'groups;
                    };
                    inferred.extend(notes);
                }
                Some(Found::Covering { shared }) => inferred.push(covering_edge_note(&at, shared)),
                Some(Found::Shared | Found::Inferred { .. }) | None => {}
            }
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
            notes.append(&mut inferred);
            rebound = true;
        }
    }
    match (rebound, covering) {
        (false, _) => GroupBinding::Unbound,
        (true, false) => GroupBinding::Bound,
        (true, true) => GroupBinding::BoundCovering,
    }
}

/// What `bind_fillet_groups` bound from one transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum GroupBinding {
    /// No group binds.
    Unbound,
    /// A group binds, and no blend of the transition takes the covering
    /// reading.
    Bound,
    /// A group binds, and a blend of the transition takes the covering
    /// reading.
    BoundCovering,
}

/// The notes of an edge that a blend reading infers from `candidates`, in
/// rank order: the assumptions of the reading and, when other candidates
/// exist, the rank and status of every candidate. `None` when no candidate
/// is deleted.
fn inferred_edge_notes(at: &str, candidates: &[Candidate]) -> Option<Vec<LossNote>> {
    let selected = candidates.iter().position(|candidate| candidate.deleted)?;
    let mut notes = vec![F3dLossCode::HistoricalFilletEdgeInferred.note(format!(
        "{at} is inferred from the blend geometry, because the blend's supports share no edge along its axis. The reading assumes the following about the rounded edge. It is an edge of a support in the preceding state. It runs along the axis. Its interior sample point lies farther than the radius from the axis line and within the blend's axial extent. A support that owns it lies on a plane that touches the blend cylinder. The transition deletes it."
    ))];
    if candidates.len() > 1 {
        let status = candidates
            .iter()
            .enumerate()
            .map(|(position, candidate)| {
                format!(
                    "slot {} ({}, {}{})",
                    candidate.edge,
                    if candidate.linked {
                        "linked"
                    } else {
                        "unlinked"
                    },
                    if candidate.deleted { "deleted" } else { "kept" },
                    if position == selected {
                        ", selected"
                    } else {
                        ""
                    }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let kept = candidates
            .iter()
            .take(selected)
            .filter(|candidate| !candidate.deleted)
            .map(|candidate| candidate.edge.to_string())
            .collect::<Vec<_>>();
        let skip = match kept.as_slice() {
            [] => String::new(),
            [slot] => format!(
                " The transition keeps the higher-ranked candidate at slot {slot}, and the reading skips it because it assumes that the transition deletes the rounded edge."
            ),
            slots => format!(
                " The transition keeps the higher-ranked candidates at slots {}, and the reading skips them because it assumes that the transition deletes the rounded edge.",
                slots.join(", ")
            ),
        };
        notes.push(F3dLossCode::HistoricalFilletEdgeAmbiguous.note(format!(
            "{at} is the best-ranked deleted candidate of {} candidates. The reading assumes that the rounded edge is the best-ranked deleted candidate. The rank puts linked candidates first, then candidates nearer the axis line, then lower slots. A linked candidate has two owner faces, and its other owner is a removed face that meets another support at an edge along the axis whose interior sample point lies on the blend cylinder and within the blend's axial extent.{skip} Candidates in rank order: {status}.",
            candidates.len()
        )));
    }
    Some(notes)
}

/// The note of an edge that a blend reading takes from the edges `shared`
/// along the blend's axis that its supports share, in slot order.
fn covering_edge_note(at: &str, shared: &[SharedEdge]) -> LossNote {
    let status = shared
        .iter()
        .map(|edge| {
            format!(
                "slot {} ({}{})",
                edge.edge,
                if edge.deleted { "deleted" } else { "kept" },
                if edge.covers {
                    ", covers the extent, selected"
                } else {
                    ""
                }
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    F3dLossCode::HistoricalFilletEdgeCovering.note(format!(
        "{at} is inferred from the blend geometry, because the blend's supports share {} edges along its axis. The reading assumes the following about the rounded edge. It is the one shared edge whose span between its end points covers the blend's axial extent within 1e-6 mm at each end. Its interior sample point lies more than 1e-6 mm beyond the radius from the axis line. The transition deletes it. The reading also assumes that the blend consumes the other shared edges that the transition deletes. Shared edges in slot order: {status}.",
        shared.len()
    ))
}

/// Design records that `Fillet` binding reads.
pub(crate) struct FilletBlendInputs<'a> {
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    pub(crate) histories: &'a mut [AsmHistory],
    pub(crate) scope_histories: &'a std::collections::HashMap<String, String>,
}

/// Loss notes and inferred history bindings of `bind_fillet_blend_edges`.
pub(crate) struct FilletBlendBindings {
    pub(crate) notes: Vec<LossNote>,
    /// Each unbound scope that took the one history in which its edges
    /// resolve, with that history's id.
    pub(crate) inferred_scope_histories: Vec<(String, String)>,
}

/// Bind the edges of each `Fillet` feature from the blend faces of its
/// transition, with a note for each edge that the binding infers.
pub(crate) fn bind_fillet_blend_edges(
    features: &mut [cadmpeg_ir::features::Feature],
    input: FilletBlendInputs<'_>,
) -> FilletBlendBindings {
    let FilletBlendInputs {
        scopes,
        groups,
        histories,
        scope_histories,
    } = input;
    let is_fillet = |feature: &cadmpeg_ir::features::Feature| {
        matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
        )
    };
    let states = features
        .iter()
        .map(|feature| {
            is_fillet(feature)
                .then(|| super::face_selection_state(feature, scopes, scope_histories, histories))
                .flatten()
        })
        .collect::<Vec<_>>();
    let candidates = features
        .iter()
        .zip(&states)
        .map(|(feature, state)| {
            if state.is_none() && is_fillet(feature) {
                super::unbound_face_selection_candidates(
                    feature,
                    scopes,
                    scope_histories,
                    histories,
                )
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>();
    let mut gates = states
        .iter()
        .enumerate()
        .filter_map(|(position, state)| {
            state.map(|state| (state.history, state.previous, (position, None)))
        })
        .collect::<Vec<_>>();
    for (position, states) in candidates.iter().enumerate() {
        for (candidate, state) in states.iter().enumerate() {
            gates.push((state.history, state.previous, (position, Some(candidate))));
        }
    }
    let mut available = HashSet::new();
    super::visit_states_oldest_first(histories, gates, |history, previous, job| {
        if history.topology(previous).is_some() {
            available.insert(job);
        }
    });
    let histories = &*histories;
    let mut notes = Vec::new();
    let mut inferred_scope_histories = Vec::new();
    for (position, ((feature, state), candidates)) in
        features.iter_mut().zip(states).zip(candidates).enumerate()
    {
        let feature_id = feature.id.clone();
        if let Some(state) = state.filter(|_| available.contains(&(position, None))) {
            let Some(states) = fillet_states(histories, &state) else {
                continue;
            };
            feature.evaluation.edit(|definition, _| {
                bind_fillet_groups(
                    definition,
                    &feature_id,
                    state.scope,
                    groups,
                    state.previous_state_id,
                    states,
                    &mut notes,
                );
            });
            continue;
        }
        // An unbound scope whose state pair several histories hold takes the
        // one history whose blends bind a group and leave every group
        // historical, ranking histories that bind through the covering
        // reading below the others.
        let mut resolved = Vec::new();
        for (candidate, state) in candidates.iter().enumerate() {
            if !available.contains(&(position, Some(candidate))) {
                continue;
            }
            let Some(states) = fillet_states(histories, state) else {
                continue;
            };
            let mut definition = feature.evaluation.definition().clone();
            let mut candidate_notes = Vec::new();
            let binding = bind_fillet_groups(
                &mut definition,
                &feature_id,
                state.scope,
                groups,
                state.previous_state_id,
                states,
                &mut candidate_notes,
            );
            if binding != GroupBinding::Unbound && every_fillet_group_historical(&definition) {
                resolved.push((
                    state.history,
                    state.scope,
                    definition,
                    candidate_notes,
                    binding == GroupBinding::BoundCovering,
                ));
            }
        }
        let covering_only = resolved.iter().filter(|resolved| resolved.4).count();
        if covering_only < resolved.len() {
            resolved.retain(|resolved| !resolved.4);
        }
        if let [(history, scope, definition, candidate_notes, covering)] = resolved.as_mut_slice() {
            let definition = definition.clone();
            feature
                .evaluation
                .edit(|current, _| *current = definition.clone());
            notes.append(candidate_notes);
            let history_id = histories
                .get(*history)
                .map_or("", |history| history.id.as_str());
            notes.push(F3dLossCode::HistoricalEdgeHistoryInferred.note(match covering_only {
                0 => format!(
                    "feature {}: its scope has no bound history and {} histories hold its state pair; its edges resolve only in {history_id}.",
                    feature_id.as_str(),
                    candidates.len(),
                ),
                _ if *covering => format!(
                    "feature {}: its scope has no bound history and {} histories hold its state pair; its edges resolve only in {history_id}, through the covering reading.",
                    feature_id.as_str(),
                    candidates.len(),
                ),
                _ => format!(
                    "feature {}: its scope has no bound history and {} histories hold its state pair; its edges resolve without the covering reading only in {history_id}, and through it in {covering_only} other {}.",
                    feature_id.as_str(),
                    candidates.len(),
                    if covering_only == 1 { "history" } else { "histories" },
                ),
            }));
            if let Some(history) = histories.get(*history) {
                inferred_scope_histories.push((scope.id.clone(), history.id.clone()));
            }
        }
    }
    FilletBlendBindings {
        notes,
        inferred_scope_histories,
    }
}

/// The transition, input topology and output topology of a fillet state.
fn fillet_states<'a>(
    histories: &'a [AsmHistory],
    state: &super::FaceSelectionState<'_>,
) -> Option<(
    &'a AsmHistoricalTransition,
    &'a AsmHistoricalTopology,
    &'a AsmHistoricalTopology,
)> {
    let history = histories.get(state.history)?;
    let output_state = history.states().get(state.state)?;
    let input_state = history.states().get(state.previous)?;
    Some((
        output_state.transition.as_ref()?,
        history.topology(input_state)?,
        history.topology(output_state)?,
    ))
}

/// Whether every group of a Fillet definition names historical edges.
fn every_fillet_group_historical(definition: &FeatureDefinition) -> bool {
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) = definition else {
        return false;
    };
    groups.iter().all(|group| {
        matches!(
            group.edges,
            EdgeSelection::Historical { .. } | EdgeSelection::HistoricalPartial { .. }
        )
    })
}

#[cfg(test)]
mod tests;
