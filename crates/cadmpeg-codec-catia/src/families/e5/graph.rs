// SPDX-License-Identifier: Apache-2.0
//! Native topology records in the E5 `0D 03` stream family.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use cadmpeg_core::decode::View;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};

use super::support::E5CurveSupport;
use crate::wire;

const EPS_PARAMETER_ENDPOINT: f64 = 1.0e-9;

/// Resolved graph of an E5 `0D 03` record stream: bodies, faces, edges, and
/// the geometry records they reference. Produced by [`parse_topology`], which
/// walks every class-tagged record, resolves cross-record references, and
/// returns `None` if the walk cannot be closed ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct E5Topology {
    /// Class-`0x01` body records with their resolved face rosters and
    /// orientation-sign relations. A transferred graph requires body roots.
    pub(super) bodies: Vec<E5Body>,
    /// Class-`0x00` advanced-face records, each resolved to its surface and
    /// loops.
    pub(super) faces: Vec<E5Face>,
    /// Class-`0xff` trimmed edge-use records, keyed by their `record_id`.
    /// Only edges reachable from a resolved face's loops are retained.
    pub(in crate::families) edges: BTreeMap<u32, E5Edge>,
    /// Class-`0x96` (line), `0x97` (circle), `0xa0` (spline jet), and
    /// `0xaa` (NURBS) pcurve records, keyed by `record_id`.
    pub(super) pcurves: BTreeMap<u32, E5Pcurve>,
    /// Class-`0x0e` parameter-bound records, keyed by `record_id`.
    pub(super) bounds: BTreeMap<u32, E5Bounds>,
    /// Class-`0xc0` (one-pcurve boundary) and `0xc1` (two-pcurve
    /// intersection) curve-support records, keyed by `record_id`.
    pub(super) curve_supports: BTreeMap<u32, E5CurveSupport>,
    /// Sorted, deduplicated `record_id`s of every class-`0xfe` vertex record
    /// referenced as an edge endpoint.
    pub(super) vertex_refs: Vec<u32>,
}

impl E5Topology {
    /// Resolve one edge's start/end parameter records for a referenced
    /// representation. Each bound must contain that representation exactly
    /// once.
    pub(super) fn edge_representation_parameters(
        &self,
        edge_ref: u32,
        representation: u32,
    ) -> Option<[FiniteReal; 2]> {
        let edge = self.edges.get(&edge_ref)?;

        bound_representation_parameter(
            &self.bounds,
            &self.pcurves,
            edge.parameter_start,
            representation,
        )
        .zip(bound_representation_parameter(
            &self.bounds,
            &self.pcurves,
            edge.parameter_end,
            representation,
        ))
        .map(|(start, end)| [start, end])
    }
}

/// A class-`0x0e` parameter-bound record: a list of representation
/// references each paired with a bound parameter ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct E5Bounds {
    /// Ordered `(representation, parameter, code)` entries, one per
    /// referenced representation.
    pub(super) entries: Vec<E5BoundEntry>,
}

/// One entry of an [`E5Bounds`] record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct E5BoundEntry {
    /// Referenced representation's `record_id`.
    pub(super) representation: u32,
    /// Finite LE-f64 bound parameter for this representation.
    pub(super) parameter: FiniteReal,
    /// Raw arc-number bits. Jet and NURBS arcs use one-based distinct
    /// knot spans; current line and circle productions use arc one.
    pub(super) code: u32,
}

/// One knot of a degree-5 E5 UV jet.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::families::e5) struct E5PcurveJetSite {
    /// Distinct knot.
    pub(super) knot: FiniteReal,
    /// Multiplicity of this distinct knot.
    multiplicity: u32,
    /// `(u, v)` position.
    pub(super) point: [FiniteReal; 2],
    /// `(u, v)` first derivative.
    pub(super) first_derivatives: [FiniteReal; 2],
    /// `(u, v)` second derivative.
    pub(super) second_derivatives: [FiniteReal; 2],
}

#[cfg(test)]
impl E5PcurveJetSite {
    pub(super) fn zip(
        knots: Vec<FiniteReal>,
        multiplicities: Vec<u32>,
        points: Vec<[FiniteReal; 2]>,
        first_derivatives: Vec<[FiniteReal; 2]>,
        second_derivatives: Vec<[FiniteReal; 2]>,
    ) -> Vec<Self> {
        knots
            .into_iter()
            .zip(multiplicities)
            .zip(points)
            .zip(first_derivatives)
            .zip(second_derivatives)
            .map(
                |((((knot, multiplicity), point), first_derivatives), second_derivatives)| Self {
                    knot,
                    multiplicity,
                    point,
                    first_derivatives,
                    second_derivatives,
                },
            )
            .collect()
    }
}

/// A resolved E5 pcurve: a 2D curve in a surface's parameter space, decoded
/// from a class-`0x96` (line), `0x97` (circle), `0xa0` (spline jet), or
/// `0xaa` (NURBS)
/// record ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum E5Pcurve {
    /// Class `0x96`: `<surface_ref>, origin_u, origin_v, dir_u, dir_v,
    /// param_lo, param_hi` stored as f64.
    Line {
        /// `record_id` of the owning surface carrier.
        surface: u32,
        /// `(u, v)` origin of the line in surface parameter space.
        origin: [FiniteReal; 2],
        /// `(u, v)` direction of the line in surface parameter space.
        direction: [FiniteReal; 2],
        /// `[param_lo, param_hi]` domain along `direction` from `origin`.
        range: [FiniteReal; 2],
    },
    /// Class `0x97`: `<surface_ref>, center_u, center_v, radius, param_lo,
    /// param_hi` with two intervening `u32` fields (`codes`).
    Circle {
        /// `record_id` of the owning surface carrier.
        surface: u32,
        /// `(u, v)` center of the circle in surface parameter space.
        center: [FiniteReal; 2],
        /// The two `u32` fields between `center` and `radius`; meaning not
        /// decoded further.
        codes: [u32; 2],
        /// Circle radius in surface parameter units.
        radius: PositiveReal,
        /// `[param_lo, param_hi]` angular domain.
        range: [FiniteReal; 2],
        /// Two trailing scalar fields following the parameter range.
        tail: [FiniteReal; 2],
    },
    /// Class `0xa0`: a nonperiodic degree-5 C2 B-spline p-curve encoded as a
    /// per-knot position/first-derivative/second-derivative jet ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
    Jet {
        /// `record_id` of the owning surface carrier.
        surface: u32,
        /// Knot-aligned UV jet samples.
        sites: Vec<E5PcurveJetSite>,
        /// `[0.0, knots.last()]` parameter range, validated against the
        /// knot span.
        range: [FiniteReal; 2],
    },
    /// Class `0xaa`: a tensor-product-free NURBS p-curve with one surface
    /// reference, distinct knots and multiplicities, and 2D control points.
    Nurbs {
        /// `record_id` of the owning surface carrier.
        surface: u32,
        /// B-spline degree.
        degree: u32,
        /// The full knot vector: each stored distinct knot, in strictly
        /// increasing order, repeated by its stored multiplicity.
        knots: Vec<FiniteReal>,
        /// `(u, v)` control points in parameter order.
        control_points: Vec<[FiniteReal; 2]>,
        /// Current parameter interval from the arc-local limit program.
        range: [FiniteReal; 2],
    },
}

impl E5Pcurve {
    pub(super) const JET_DEGREE: u32 = 5;

    /// The `record_id` of the surface carrier this p-curve lies on, which every
    /// variant states.
    #[must_use]
    pub(super) fn surface_record_id(&self) -> u32 {
        match self {
            Self::Line { surface, .. }
            | Self::Circle { surface, .. }
            | Self::Jet { surface, .. }
            | Self::Nurbs { surface, .. } => *surface,
        }
    }
}

/// A class-`0x01` body record resolved through its class-`0x08` root record:
/// the body's validated face roster
/// ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::families::e5) struct E5Body {
    /// This class-`0x01` body's stream-assigned `record_id`.
    pub(super) record_id: u32,
    /// Faces in root-record order.
    pub(super) faces: Vec<E5BodyFace>,
}

/// One face occurrence in a class-`0x08` root roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct E5BodyFace {
    /// Referenced class-`0x00` face identity.
    pub(super) record_id: u32,
    /// Face-aligned root sign composed with the final root sign.
    pub(super) orientation: Sign,
}

/// An orientation sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::families) enum Sign {
    /// Positive orientation.
    Positive,
    /// Negative orientation.
    Negative,
}

impl Sign {
    fn from_i16(value: i16) -> Option<Self> {
        match value {
            1 => Some(Self::Positive),
            -1 => Some(Self::Negative),
            _ => None,
        }
    }

    fn flipped(self) -> Self {
        match self {
            Self::Positive => Self::Negative,
            Self::Negative => Self::Positive,
        }
    }

    fn combine(self, other: Self) -> Self {
        match self {
            Self::Positive => other,
            Self::Negative => other.flipped(),
        }
    }
}

/// A resolved class-`0x00` advanced-face record: its surface, loops, and
/// face-local orientation sign ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct E5Face {
    /// This face record's `record_id`.
    pub(super) record_id: u32,
    /// `record_id` of the face's surface carrier.
    pub(super) surface: u32,
    /// The final signed word of this class-`0x00` face record.
    pub(super) trailer_sign: Sign,
    /// The face's loops, first entry outer-bounded, remaining entries
    /// holes.
    pub(super) loops: Vec<E5Loop>,
}

/// One pcurve/edge-use occurrence of a class-`0x09` loop, with its unique
/// head-to-tail traversal sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct E5LoopMember {
    /// `record_id` of the member pcurve.
    pub(super) pcurve: u32,
    /// `record_id` of the member edge-use.
    pub(super) edge_use: u32,
    /// Traversal sense from [`solve_loop_chain`]; `true` means the edge is
    /// traversed end-to-start.
    pub(super) reversed: bool,
}

/// A resolved class-`0x09` loop record: its member pcurve/edge-use pairs and
/// derived orientation ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct E5Loop {
    /// This loop record's `record_id`.
    pub(super) record_id: u32,
    /// `record_id` of the loop's surface, matched against the owning
    /// face's surface during resolution.
    pub(super) surface: u32,
    /// Pcurve/edge-use occurrences in serialized order, each with its
    /// unique head-to-tail traversal sense.
    pub(super) members: Vec<E5LoopMember>,
    /// Shell-consistent member order and traversal senses after folding in
    /// the loop's global orientation sign. `None` when the radial parity
    /// system is frustrated or ambiguous.
    pub(super) oriented_members: Option<Vec<E5OrientedMember>>,
    /// Loop role from the complete trailing sign tape: true for an outer
    /// boundary, false for an inner boundary.
    pub(super) outer: bool,
    /// Absolute traversal sign from the loop, face and owning root relation.
    pub(super) absolute_orientation: Sign,
}

impl E5Loop {
    /// Shell-consistent member order and senses when radial parity closes.
    #[must_use]
    pub(super) fn resolved_members(&self) -> Option<&[E5OrientedMember]> {
        self.oriented_members.as_deref()
    }
}

/// One E5 loop member in shell-consistent traversal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct E5OrientedMember {
    /// Index of the member in the serialized loop arrays.
    pub(super) serialized_index: usize,
    /// Whether the physical edge is traversed end-to-start.
    pub(super) reversed: bool,
}

/// A resolved class-`0xff` trimmed edge-use record ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant), grammar `85
/// <curve_support_ref> <start_vertex> <end_vertex> <param_start>
/// <param_end> <allocation_terminal:u8>`). The terminal is validated here;
/// the original bytes remain in the native record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::families) struct E5Edge {
    /// `record_id` of the owning [`E5CurveSupport`].
    pub(super) support: u32,
    /// `record_id` of the class-`0xfe` start vertex.
    pub(in crate::families) start_vertex: u32,
    /// `record_id` of the class-`0xfe` end vertex.
    pub(in crate::families) end_vertex: u32,
    /// Reference to the start-parameter representation on the curve
    /// support.
    pub(super) parameter_start: u32,
    /// Reference to the end-parameter representation on the curve support.
    pub(super) parameter_end: u32,
}

#[derive(Debug)]
struct Record<'a> {
    class: u8,
    id: u32,
    payload: &'a [u8],
}

#[derive(Debug)]
struct RawFace {
    id: u32,
    surface: u32,
    loops: Vec<u32>,
    trailer_sign: Sign,
}

#[derive(Debug)]
struct RawLoop<'a> {
    id: u32,
    surface: u32,
    pcurves: Vec<u32>,
    edges: Vec<u32>,
    outer: bool,
    trailer: &'a [u8],
}

/// Resolve E5 face→loop→edge-use references and determine each serialized
/// loop occurrence's unique head-to-tail traversal from stored vertex refs.
pub(crate) fn parse_topology(bytes: &[u8]) -> Option<E5Topology> {
    let mut admitted_records = Vec::new();
    for record in records(bytes) {
        admitted_records.push(record);
    }
    (|| -> Option<E5Topology> {
        let records = admitted_records;
        let mut by_id = HashMap::new();
        for record in &records {
            by_id.insert(record.id, record);
        }
        if by_id.len() != records.len() {
            return None;
        }

        let mut edges = BTreeMap::new();
        let mut pcurves = BTreeMap::new();
        for record in &records {
            if record.class == 0xff {
                let edge = parse_edge(record.payload)?;
                edges.insert(record.id, edge);
            }
            if matches!(record.class, 0x96 | 0x97 | 0xa0 | 0xaa) {
                let pcurve = parse_pcurve(record)?;
                pcurves.insert(record.id, pcurve);
            }
        }
        for pcurve in pcurves.values() {
            let surface = match pcurve {
                E5Pcurve::Line { surface, .. }
                | E5Pcurve::Circle { surface, .. }
                | E5Pcurve::Jet { surface, .. }
                | E5Pcurve::Nurbs { surface, .. } => *surface,
            };
            if !by_id
                .get(&surface)
                .is_some_and(|record| is_surface_carrier_class(record.class))
            {
                return None;
            }
        }
        let mut bounds = BTreeMap::new();
        let mut curve_supports = BTreeMap::new();
        let mut loops = HashMap::new();
        let mut raw_faces = Vec::new();
        let mut vertex_ids = HashSet::new();
        for record in &records {
            match record.class {
                0x0e => {
                    let value = parse_bounds(record)?;
                    bounds.insert(record.id, value);
                }
                0xc0 | 0xc1 => {
                    let value = super::support::parse(record.class, record.payload)?;
                    curve_supports.insert(record.id, value);
                }
                0x09 => {
                    let value = parse_loop(record)?;
                    loops.insert(record.id, value);
                }
                0x00 => {
                    let value = parse_face(record)?;
                    raw_faces.push(value);
                }
                0xfe => {
                    vertex_ids.insert(record.id);
                }
                _ => {}
            }
        }
        if raw_faces.is_empty() || loops.is_empty() || edges.is_empty() || vertex_ids.is_empty() {
            return None;
        }

        let bodies = parse_bodies(&records, &by_id)?;
        let mut root_orientation = HashMap::new();
        for body in &bodies {
            for face in &body.faces {
                if root_orientation.contains_key(&face.record_id) {
                    return None;
                }
                root_orientation.insert(face.record_id, face.orientation);
            }
        }
        let mut faces = Vec::new();
        let mut closed_curve_supports = HashSet::new();
        let mut reachable_edges = HashSet::new();
        for face in raw_faces {
            if !by_id
                .get(&face.surface)
                .is_some_and(|record| is_surface_carrier_class(record.class))
            {
                return None;
            }
            let mut resolved_loops = Vec::new();
            for (loop_position, loop_id) in face.loops.into_iter().enumerate() {
                let raw = loops.get(&loop_id)?;
                if raw.surface != face.surface {
                    return None;
                }
                if raw.outer != (loop_position == 0) {
                    return None;
                }
                if raw.pcurves.len() != raw.edges.len() {
                    return None;
                }
                let reversed = solve_loop_chain(&raw.edges, &edges)?;
                if reversed.len() != raw.edges.len() {
                    return None;
                }
                for pcurve_id in &raw.pcurves {
                    let pcurve = pcurves.get(pcurve_id)?;
                    let surface = match pcurve {
                        E5Pcurve::Line { surface, .. }
                        | E5Pcurve::Circle { surface, .. }
                        | E5Pcurve::Jet { surface, .. }
                        | E5Pcurve::Nurbs { surface, .. } => *surface,
                    };
                    if surface != raw.surface {
                        return None;
                    }
                }
                for (pcurve_id, edge_id) in raw.pcurves.iter().zip(&raw.edges) {
                    let edge = edges.get(edge_id)?;
                    if !vertex_ids.contains(&edge.start_vertex)
                        || !vertex_ids.contains(&edge.end_vertex)
                    {
                        return None;
                    }
                    if [edge.parameter_start, edge.parameter_end]
                        .iter()
                        .any(|bound_ref| !bounds.contains_key(bound_ref))
                        || [edge.parameter_start, edge.parameter_end]
                            .iter()
                            .any(|bound_ref| {
                                bound_representation_parameter(
                                    &bounds, &pcurves, *bound_ref, *pcurve_id,
                                )
                                .is_none()
                            })
                    {
                        return None;
                    }
                    let support = curve_supports.get(&edge.support)?;
                    for reference in support.pcurves() {
                        if !curve_support_reference_closes(
                            *reference,
                            &pcurves,
                            &curve_supports,
                            &mut closed_curve_supports,
                        ) {
                            return None;
                        }
                    }
                    reachable_edges.insert(*edge_id);
                }
                let root_sign = *root_orientation.get(&face.id)?;
                let mut local_orientation = None;
                for (index, &reversed) in reversed.iter().enumerate() {
                    let local_sign = View::i16_le_at(raw.trailer, 1 + 2 * (3 + 3 * index))
                        .and_then(Sign::from_i16)?;
                    let candidate = if reversed {
                        local_sign.flipped()
                    } else {
                        local_sign
                    };
                    if local_orientation.is_some_and(|existing| existing != candidate) {
                        return None;
                    }
                    local_orientation = Some(candidate);
                }
                let absolute_orientation = local_orientation?
                    .combine(face.trailer_sign)
                    .combine(root_sign);
                let mut members = Vec::new();
                for ((&pcurve, &edge_use), &reversed) in
                    raw.pcurves.iter().zip(&raw.edges).zip(&reversed)
                {
                    members.push(E5LoopMember {
                        pcurve,
                        edge_use,
                        reversed,
                    });
                }
                resolved_loops.push(E5Loop {
                    record_id: raw.id,
                    surface: raw.surface,
                    members,
                    oriented_members: None,
                    outer: raw.outer,
                    absolute_orientation,
                });
            }
            faces.push(E5Face {
                record_id: face.id,
                surface: face.surface,
                trailer_sign: face.trailer_sign,
                loops: resolved_loops,
            });
        }
        let oriented = resolve_absolute_orientation(&mut faces);
        if !oriented {
            return None;
        }
        edges.retain(|id, _| reachable_edges.contains(id));
        let mut vertex_refs = Vec::new();
        for edge in edges.values() {
            for vertex in [edge.start_vertex, edge.end_vertex] {
                vertex_refs.push(vertex);
            }
        }
        vertex_refs.sort_unstable_by(Ord::cmp);
        vertex_refs.dedup();
        let mut roster = Vec::new();
        for body in &bodies {
            for face in &body.faces {
                roster.push(face.record_id);
            }
        }
        let mut roster_set = HashSet::new();
        for &face in &roster {
            roster_set.insert(face);
        }
        let mut face_set = HashSet::new();
        for face in &faces {
            face_set.insert(face.record_id);
        }
        if roster.len() != roster_set.len() || roster_set != face_set {
            return None;
        }
        Some(E5Topology {
            bodies,
            faces,
            edges,
            pcurves,
            bounds,
            curve_supports,
            vertex_refs,
        })
    })()
}

/// Return the serialized surface reference for each valid class-`0x00` face.
///
/// This is the narrow face-to-carrier relation used by standard freeform
/// aliases. It does not claim that the complete E5 topology graph is closed.
pub(in crate::families) fn face_surface_references(
    bytes: &[u8],
) -> impl Iterator<Item = (u32, u32)> + '_ {
    records(bytes)
        .filter(|record| record.class == 0x00)
        .filter_map(|record| {
            let count = usize::from(record.payload.first()?.checked_sub(0x81)?);
            if count == 0 {
                return None;
            }
            let mut position = 1;
            let surface = wire::tokens::object_ref(record.payload, &mut position, false)?;
            for _ in 0..count {
                wire::tokens::object_ref(record.payload, &mut position, false)?;
            }
            Sign::from_i16(View::i16_le_at(record.payload, position)?)?;
            (position + 2 == record.payload.len()).then_some((record.id, surface))
        })
}

fn is_surface_carrier_class(class: u8) -> bool {
    matches!(class, 0xc8 | 0xc9 | 0xca | 0xcc | 0xe7)
}

/// Checks that a curve-support side resolves to a direct p-curve or to a
/// finite, acyclic chain of intersection-support wrappers.
///
/// `closed` holds the supports already proven to close. Calls that share it
/// walk each support once.
fn curve_support_reference_closes(
    reference: u32,
    pcurves: &BTreeMap<u32, E5Pcurve>,
    supports: &BTreeMap<u32, E5CurveSupport>,
    closed: &mut HashSet<u32>,
) -> bool {
    if pcurves.contains_key(&reference) || closed.contains(&reference) {
        return true;
    }
    let mut visiting = HashSet::new();
    let mut stack = Vec::new();
    stack.push((reference, false));
    while let Some((reference, leaving)) = stack.pop() {
        if pcurves.contains_key(&reference) {
            continue;
        }
        let Some(support) = supports
            .get(&reference)
            .filter(|support| support.is_intersection())
        else {
            return false;
        };
        if leaving {
            visiting.remove(&reference);
            closed.insert(reference);
            continue;
        }
        if closed.contains(&reference) {
            continue;
        }
        if !visiting.insert(reference) {
            return false;
        }
        stack.push((reference, true));
        for child in support.pcurves().iter().rev() {
            if pcurves.contains_key(child) || closed.contains(child) {
                continue;
            }
            if !supports
                .get(child)
                .is_some_and(E5CurveSupport::is_intersection)
                || visiting.contains(child)
            {
                return false;
            }
            stack.push((*child, false));
        }
    }
    true
}

fn bound_representation_parameter(
    bounds: &BTreeMap<u32, E5Bounds>,
    pcurves: &BTreeMap<u32, E5Pcurve>,
    bound_ref: u32,
    representation: u32,
) -> Option<FiniteReal> {
    let bounds = bounds.get(&bound_ref)?;
    let mut entries = bounds
        .entries
        .iter()
        .filter(|entry| entry.representation == representation);
    let entry = entries.next()?;
    if entries.next().is_some() {
        return None;
    }
    let span = usize::try_from(entry.code.checked_sub(1)?).ok()?;
    let (start, end) = match pcurves.get(&representation)? {
        E5Pcurve::Jet { sites, .. } => (
            sites.get(span)?.knot.get(),
            sites.get(span.checked_add(1)?)?.knot.get(),
        ),
        E5Pcurve::Nurbs { knots, .. } => {
            let interval = knots
                .windows(2)
                .filter(|pair| pair[0] < pair[1])
                .nth(span)?;
            (interval[0].get(), interval[1].get())
        }
        E5Pcurve::Line { .. } | E5Pcurve::Circle { .. } => {
            return (entry.code == 1).then_some(entry.parameter);
        }
    };
    super::parameters::arc_local_parameter(start, end, entry.parameter.get())
}

fn parse_bounds(record: &Record<'_>) -> Option<E5Bounds> {
    let count = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x80))
        .map(usize::from)?;
    let mut position = 1;
    let mut representations = Vec::new();
    for _ in 0..count {
        let reference = wire::tokens::object_ref(record.payload, &mut position, false)?;
        representations.push(reference);
    }
    let expected_head = u8::try_from(count)
        .ok()
        .and_then(|count| 0x80u8.checked_add(count))?;
    if record.payload.get(position) != Some(&expected_head) {
        return None;
    }
    position += 1;
    let mut view = View::over_retained(record.payload);
    view.seek(position)?;
    let mut entries = Vec::new();
    for representation in representations {
        let (parameter, code) = (|| Some((FiniteReal::new(view.f64_le()?)?, view.u32_le()?)))()?;
        entries.push(E5BoundEntry {
            representation,
            parameter,
            code,
        });
    }
    view.is_empty().then_some(E5Bounds { entries })
}

fn parse_pcurve(record: &Record<'_>) -> Option<E5Pcurve> {
    if record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::tokens::object_ref(record.payload, &mut position, false)?;
    let mut view = View::over_retained(record.payload);
    view.seek(position)?;
    match record.class {
        0x96 => {
            let values = (|| {
                Some([
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                ])
            })()?;
            if !view.is_empty() {
                return None;
            }
            Some(E5Pcurve::Line {
                surface,
                origin: [values[0], values[1]],
                direction: [values[2], values[3]],
                range: [values[4], values[5]],
            })
        }
        0x97 => {
            let (center, codes, values) = (|| {
                Some((
                    [finite_f64_le(&mut view)?, finite_f64_le(&mut view)?],
                    [view.u32_le()?, view.u32_le()?],
                    [
                        finite_f64_le(&mut view)?,
                        finite_f64_le(&mut view)?,
                        finite_f64_le(&mut view)?,
                        finite_f64_le(&mut view)?,
                        finite_f64_le(&mut view)?,
                    ],
                ))
            })()?;
            if !view.is_empty() {
                return None;
            }
            let radius = PositiveReal::new(values[0].get())?;
            Some(E5Pcurve::Circle {
                surface,
                center: [center[0], center[1]],
                codes,
                radius,
                range: [values[1], values[2]],
                tail: [values[3], values[4]],
            })
        }
        0xa0 => parse_jet_pcurve(record.payload, position, surface),
        0xaa => parse_nurbs_pcurve(record.payload, position, surface),
        _ => None,
    }
}

const E5_NURBS_PCURVE_TAIL_BYTES: usize = 37;

fn finite_f64_le(view: &mut View<'_>) -> Option<FiniteReal> {
    FiniteReal::new(view.f64_le()?)
}

fn parse_nurbs_pcurve(payload: &[u8], position: usize, surface: u32) -> Option<E5Pcurve> {
    let mut view = View::over_retained(payload);
    if view.seek(position).is_none() || view.u16_le() != Some(0) {
        return None;
    }
    let (degree, zero0, zero1, knot_count, zero2) = (|| {
        Some((
            view.u32_le()?,
            view.u32_le()?,
            view.u32_le()?,
            usize::try_from(view.u32_le()?).ok()?,
            view.u32_le()?,
        ))
    })()?;
    if degree == 0 || knot_count == 0 || [zero0, zero1, zero2] != [0; 3] {
        return None;
    }
    let knot_count_u64 = u64_from_index(knot_count);
    view.counted(knot_count_u64, 12)?;
    let mut knots = Vec::new();

    for _ in 0..knot_count {
        let knot = finite_f64_le(&mut view)?;
        knots.push(knot);
    }
    let mut multiplicities = Vec::new();

    for _ in 0..knot_count {
        let multiplicity = view.u32_le()?;
        multiplicities.push(multiplicity);
    }
    let max_control_count = view
        .remaining()
        .checked_sub(E5_NURBS_PCURVE_TAIL_BYTES)
        .map(|remaining| remaining / 16)?;
    let (expanded_knots, control_count) =
        expand_nurbs_knots_limited(degree, &knots, &multiplicities, max_control_count)?;
    let control_count_u64 = u64_from_index(control_count);
    view.counted(control_count_u64, 16)?;

    let mut control_points = Vec::new();

    for _ in 0..control_count {
        let point = (|| Some([finite_f64_le(&mut view)?, finite_f64_le(&mut view)?]))()?;
        control_points.push(point);
    }
    if view.remaining() != E5_NURBS_PCURVE_TAIL_BYTES {
        return None;
    }
    let (arcs, locals, coefficient, shift, extrapolation) = (|| {
        Some((
            [view.u16_le()?, view.u16_le()?],
            [view.f64_le()?, view.f64_le()?],
            view.f64_le()?,
            view.f64_le()?,
            view.u8()?,
        ))
    })()?;
    if coefficient != 1.0 || shift != 0.0 || extrapolation != 0 {
        return None;
    }
    let range = super::parameters::current_limits(arcs, locals, |arc| {
        Some([knots.get(arc)?.get(), knots.get(arc.checked_add(1)?)?.get()])
    })?;
    let degree_index = usize::try_from(degree).ok()?;
    let domain_start = expanded_knots[degree_index].get();
    let domain_end = expanded_knots[control_count].get();
    let tolerance = EPS_PARAMETER_ENDPOINT * (1.0 + (domain_end - domain_start).abs());
    if domain_start >= domain_end
        || !tolerance.is_finite()
        || range[0].get() < domain_start - tolerance
        || range[1].get() > domain_end + tolerance
    {
        return None;
    }
    view.is_empty().then_some(E5Pcurve::Nurbs {
        surface,
        degree,
        knots: expanded_knots,
        control_points,
        range,
    })
}

fn expand_nurbs_knots_limited(
    degree: u32,
    knots: &[FiniteReal],
    multiplicities: &[u32],
    max_control_count: usize,
) -> Option<(Vec<FiniteReal>, usize)> {
    if knots.len() != multiplicities.len()
        || knots.is_empty()
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.contains(&0)
    {
        return None;
    }

    let total = multiplicities
        .iter()
        .try_fold(0usize, |total, &multiplicity| {
            total.checked_add(index_from_u32(multiplicity))
        })?;
    let (degree, control_count) = usize::try_from(degree)
        .ok()
        .and_then(|degree| Some((degree, total.checked_sub(degree.checked_add(1)?)?)))?;
    if control_count <= degree || control_count > max_control_count {
        return None;
    }

    let mut expanded = Vec::new();

    for (knot, multiplicity) in knots.iter().zip(multiplicities) {
        expanded.extend(std::iter::repeat_with(|| *knot).take(index_from_u32(*multiplicity)));
    }
    (expanded.len() == total).then_some((expanded, control_count))
}

fn read_finite_lane(view: &mut View<'_>, count: usize) -> Option<Vec<FiniteReal>> {
    let count_u64 = u64_from_index(count);
    view.counted(count_u64, 8)?;
    let mut values = Vec::new();

    for _ in 0..count {
        let value = finite_f64_le(view)?;
        values.push(value);
    }
    Some(values)
}

fn read_u32_lane(view: &mut View<'_>, count: usize) -> Option<Vec<u32>> {
    let count_u64 = u64_from_index(count);
    view.counted(count_u64, 4)?;
    let mut values = Vec::new();

    for _ in 0..count {
        let value = view.u32_le()?;
        values.push(value);
    }
    Some(values)
}

fn parse_jet_pcurve(payload: &[u8], position: usize, surface: u32) -> Option<E5Pcurve> {
    let mut view = View::over_retained(payload);
    view.seek(position)?;
    let (degree, zero0, zero1, site_count, zero2, zero3, zero4) = (|| {
        Some((
            view.u32_le()?,
            view.u32_le()?,
            view.u32_le()?,
            usize::try_from(view.u32_le()?).ok()?,
            view.u32_le()?,
            view.u32_le()?,
            view.u32_le()?,
        ))
    })()?;
    if degree != 5 || site_count == 0 || [zero0, zero1, zero2, zero3, zero4] != [0; 5] {
        return None;
    }
    let knot_tail_count = site_count.checked_sub(1)?;
    let knot_tail = read_finite_lane(&mut view, knot_tail_count)?;
    let mut knots = Vec::new();

    knots.push(FiniteReal::ZERO);
    knots.extend(knot_tail);
    let multiplicities = read_u32_lane(&mut view, site_count)?;
    if view.u32_le().and_then(|count| usize::try_from(count).ok()) != Some(site_count) {
        return None;
    }
    let x = read_finite_lane(&mut view, site_count)?;
    let y = read_finite_lane(&mut view, site_count)?;
    let dx = read_finite_lane(&mut view, site_count)?;
    let dy = read_finite_lane(&mut view, site_count)?;
    if view.u16_le() != Some(1) {
        return None;
    }
    let ddx = read_finite_lane(&mut view, site_count)?;
    let ddy = read_finite_lane(&mut view, site_count)?;
    let range_values = (|| Some([finite_f64_le(&mut view)?, finite_f64_le(&mut view)?]))()?;
    let final_knot = knots.last().map(|knot| knot.get())?;
    let expected_sum = u32::try_from(site_count)
        .ok()
        .and_then(|count| count.checked_mul(3))
        .and_then(|interior| (degree + 1).checked_add(interior));
    if !view.is_empty()
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities
            .iter()
            .enumerate()
            .any(|(index, multiplicity)| {
                *multiplicity
                    != if site_count == 1 || index == 0 || index == site_count - 1 {
                        degree + 1
                    } else {
                        3
                    }
            })
        || multiplicities
            .iter()
            .try_fold(0_u32, |sum, multiplicity| sum.checked_add(*multiplicity))
            != expected_sum
        || range_values[0].get() != 0.0
        || (range_values[1].get() - final_knot).abs() > EPS_PARAMETER_ENDPOINT * final_knot.abs()
    {
        return None;
    }

    let mut sites = Vec::new();

    for ((((((knot, multiplicity), u), v), du), dv), (ddu, ddv)) in knots
        .into_iter()
        .zip(multiplicities)
        .zip(x)
        .zip(y)
        .zip(dx)
        .zip(dy)
        .zip(ddx.into_iter().zip(ddy))
    {
        sites.push(E5PcurveJetSite {
            knot,
            multiplicity,
            point: [u, v],
            first_derivatives: [du, dv],
            second_derivatives: [ddu, ddv],
        });
    }
    Some(E5Pcurve::Jet {
        surface,
        sites,
        range: [range_values[0], range_values[1]],
    })
}

/// Apply explicit root-context loop signs and verify every paired edge use.
fn resolve_absolute_orientation(faces: &mut [E5Face]) -> bool {
    let mut occurrences = HashMap::<u32, Vec<Sign>>::new();
    for face in faces.iter() {
        for loop_ in &face.loops {
            for member in &loop_.members {
                let sign = if member.reversed {
                    loop_.absolute_orientation.flipped()
                } else {
                    loop_.absolute_orientation
                };

                occurrences.entry(member.edge_use).or_default().push(sign);
            }
        }
    }
    if occurrences
        .values()
        .any(|uses| uses.len() == 2 && uses[0] == uses[1])
    {
        return false;
    }
    for face in faces {
        for loop_ in &mut face.loops {
            let flip = loop_.absolute_orientation == Sign::Negative;
            let mut oriented = Vec::new();
            for index in 0..loop_.members.len() {
                let serialized_index = if flip {
                    loop_.members.len() - 1 - index
                } else {
                    index
                };
                oriented.push(E5OrientedMember {
                    serialized_index,
                    reversed: loop_.members[serialized_index].reversed ^ flip,
                });
            }
            loop_.oriented_members = Some(oriented);
        }
    }
    true
}

fn parse_bodies(records: &[Record<'_>], by_id: &HashMap<u32, &Record<'_>>) -> Option<Vec<E5Body>> {
    let mut bodies = Vec::new();
    for record in records.iter().filter(|record| record.class == 0x01) {
        if record.payload.first() != Some(&0x81) {
            return None;
        }
        let mut position = 1;
        let root_id = wire::tokens::object_ref(record.payload, &mut position, false)?;
        if position != record.payload.len() {
            return None;
        }
        let root = by_id.get(&root_id)?;
        if root.class != 0x08 {
            return None;
        }
        let faces = parse_body_root(root.payload)?;
        if faces.iter().any(|face| {
            by_id
                .get(&face.record_id)
                .is_none_or(|target| target.class != 0x00)
        }) {
            return None;
        }
        bodies.push(E5Body {
            record_id: record.id,
            faces,
        });
    }
    (!bodies.is_empty()).then_some(bodies)
}

fn parse_body_root(payload: &[u8]) -> Option<Vec<E5BodyFace>> {
    let (count, mut position, narrow) = if payload.first() == Some(&0x08) {
        let count = payload.get(1)?;
        (usize::from(*count), 2, true)
    } else {
        let count = payload.first().and_then(|lead| lead.checked_sub(0x80))?;
        (usize::from(count), 1, false)
    };
    let mut faces = Vec::new();
    for _ in 0..count {
        let face = wire::tokens::object_ref(payload, &mut position, false)?;
        if narrow && face > u32::from(u16::MAX) {
            return None;
        }
        faces.push(face);
    }
    let count = u8::try_from(faces.len()).ok()?;
    if narrow && payload.get(position..position + 2) == Some(&[0x08, count]) {
        position += 2;
    } else if !narrow
        && faces.len() <= 0x7f
        && 0x80u8
            .checked_add(count)
            .is_some_and(|head| payload.get(position) == Some(&head))
    {
        position += 1;
    } else {
        return None;
    }
    let sign_bytes = payload.get(position..)?;
    if sign_bytes.len() != (faces.len() + 2) * 2 {
        return None;
    }
    if View::i16_le_at(sign_bytes, 0) != Some(1)
        || View::i16_le_at(sign_bytes, 2 * (faces.len() + 1)) != Some(-1)
    {
        return None;
    }
    let mut occurrences = Vec::new();
    for (index, record_id) in faces.into_iter().enumerate() {
        let sign = View::i16_le_at(sign_bytes, 2 * (index + 1)).and_then(Sign::from_i16)?;
        occurrences.push(E5BodyFace {
            record_id,
            orientation: sign.flipped(),
        });
    }
    Some(occurrences)
}

fn records(bytes: &[u8]) -> impl Iterator<Item = Record<'_>> + '_ {
    crate::container::all_e5_record_spans(bytes).filter_map(|span| {
        let start = span.start;
        Some(Record {
            class: bytes[start + 3],
            id: View::u32_le_at(bytes, start + 9)?,
            payload: &bytes[start + 13..span.end],
        })
    })
}

fn parse_face(record: &Record<'_>) -> Option<RawFace> {
    let count = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x81))
        .map(usize::from)?;
    if count == 0 {
        return None;
    }
    let mut position = 1;
    let surface = wire::tokens::object_ref(record.payload, &mut position, false)?;
    let mut loops = Vec::new();
    for _ in 0..count {
        let loop_id = wire::tokens::object_ref(record.payload, &mut position, false)?;
        loops.push(loop_id);
    }
    let trailer_sign = View::i16_le_at(record.payload, position).and_then(Sign::from_i16)?;
    if position + 2 != record.payload.len() {
        return None;
    }
    Some(RawFace {
        id: record.id,
        surface,
        loops,
        trailer_sign,
    })
}

fn parse_loop<'a>(record: &Record<'a>) -> Option<RawLoop<'a>> {
    let member_count = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x81))
        .map(usize::from)?;
    if member_count == 0 || member_count % 2 != 0 {
        return None;
    }
    let mut position = 1;
    let mut pcurves = Vec::new();
    let mut edges = Vec::new();
    for _ in 0..member_count / 2 {
        let pcurve = wire::tokens::object_ref(record.payload, &mut position, false)?;
        let edge = wire::tokens::object_ref(record.payload, &mut position, false)?;
        pcurves.push(pcurve);
        edges.push(edge);
    }
    let surface = wire::tokens::object_ref(record.payload, &mut position, false)?;
    let outer = record
        .payload
        .get(position..)
        .and_then(|tail| parse_loop_signs(tail, member_count / 2).ok())?;
    Some(RawLoop {
        id: record.id,
        surface,
        pcurves,
        edges,
        outer,
        trailer: &record.payload[position..],
    })
}

#[derive(Debug)]
enum LoopSignError {
    Count,
    Frame,
    Sign,
}

fn parse_loop_signs(trailing: &[u8], edge_count: usize) -> Result<bool, LoopSignError> {
    let expected_head = u8::try_from(edge_count)
        .ok()
        .and_then(|n| 0x80u8.checked_add(n))
        .ok_or(LoopSignError::Count)?;
    if trailing.first() != Some(&expected_head) || trailing.len() != 1 + 2 * (3 * edge_count + 4) {
        return Err(LoopSignError::Frame);
    }
    if View::i16_le_at(trailing, 1)
        .and_then(Sign::from_i16)
        .is_none()
        || View::i16_le_at(trailing, 5) != Some(-1)
        || View::i16_le_at(trailing, trailing.len() - 2) != Some(0)
    {
        return Err(LoopSignError::Sign);
    }
    let mut outer = None;
    for (index, bytes) in trailing[1..].chunks_exact(2).enumerate() {
        let sign = View::i16_le_at(bytes, 0).ok_or(LoopSignError::Frame)?;
        if !matches!(sign, -1..=1) {
            return Err(LoopSignError::Sign);
        }
        if index == 1 {
            outer = Some(sign);
        }
    }
    let outer = outer.ok_or(LoopSignError::Frame)?;
    if !matches!(outer, -1 | 1) {
        return Err(LoopSignError::Sign);
    }
    Ok(outer == 1)
}

pub(super) fn parse_edge(payload: &[u8]) -> Option<E5Edge> {
    use wire::bytes::AllocationReferenceEncoding;

    if payload.first() != Some(&0x85) {
        return None;
    }
    let mut position = 1;
    let mut references = [0; 5];
    for reference in &mut references {
        *reference = if payload.get(position) == Some(&0x30) {
            let value = u32::from(View::u16_le_at(payload, position + 1)?) << 8;
            position += 3;
            value
        } else {
            wire::tokens::object_ref(payload, &mut position, false)?
        };
    }
    let terminal = wire::bytes::allocation_reference(payload, &mut position)?;
    if position != payload.len()
        || !matches!(
            (terminal.encoding, terminal.value),
            (
                AllocationReferenceEncoding::BackwardDistance,
                0 | 8 | 9 | 10
            ) | (AllocationReferenceEncoding::Selector2, 0 | 8 | 10)
        )
    {
        return None;
    }
    let [support, start_vertex, end_vertex, parameter_start, parameter_end] = references;
    Some(E5Edge {
        support,
        start_vertex,
        end_vertex,
        parameter_start,
        parameter_end,
    })
}

fn solve_loop_chain(edge_ids: &[u32], edges: &BTreeMap<u32, E5Edge>) -> Option<Vec<bool>> {
    let first = edge_ids.first().and_then(|id| edges.get(id))?;
    let mut solutions = Vec::new();
    for first_reversed in [false, true] {
        let initial = if first_reversed {
            first.end_vertex
        } else {
            first.start_vertex
        };
        let mut current = if first_reversed {
            first.start_vertex
        } else {
            first.end_vertex
        };
        let mut senses = Vec::new();
        senses.push(first_reversed);
        let mut valid = true;
        for edge_id in &edge_ids[1..] {
            let edge = edges.get(edge_id)?;
            match (edge.start_vertex == current, edge.end_vertex == current) {
                (true, false) => {
                    senses.push(false);
                    current = edge.end_vertex;
                }
                (false, true) => {
                    senses.push(true);
                    current = edge.start_vertex;
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if valid && current == initial {
            solutions.push(senses);
        }
    }
    if solutions.len() == 1 {
        return solutions.pop();
    }
    if edge_ids.len() == 2
        && solutions.len() == 2
        && solutions[0]
            .iter()
            .zip(&solutions[1])
            .all(|(left, right)| left != right)
    {
        return solutions.into_iter().find(|solution| !solution[0]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::support::{E5CurveSupport, E5CurveSupportKind};
    use super::{
        curve_support_reference_closes, parse_body_root, parse_jet_pcurve, parse_nurbs_pcurve,
        parse_pcurve, parse_topology, records, resolve_absolute_orientation, solve_loop_chain,
        E5BoundEntry, E5Bounds, E5Edge, E5Face, E5Loop, E5LoopMember, E5Pcurve, E5PcurveJetSite,
        E5Topology, Record, Sign,
    };
    use crate::families::e5::tests::e5_loop_members;
    use crate::test_support::test_b5::{finite, finite_lane, finite_pair};
    use crate::test_support::test_e5::{affine_support, append_e5_record};
    use std::collections::{BTreeMap, HashSet};

    macro_rules! e5_test_context {
        ($ctx:ident) => {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let policy = cadmpeg_core::decode::DecodePolicy::service();
            let ($ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
                    .expect("build the decode context");
            let _ = &$ctx;
        };
    }

    fn correspondence() -> super::super::support::E5EquivalentParameters {
        use super::super::support::{E5EquivalentParameters, E5ParameterProgram};
        E5EquivalentParameters {
            map_accuracy: cadmpeg_ir::scalar::NonNegativeLength::new(0.0)
                .expect("zero is nonnegative"),
            co_domain: finite_pair([0.0, 1.0]),
            program: E5ParameterProgram::Affine {
                scale: cadmpeg_ir::scalar::NonZeroReal::new(1.0).expect("one is nonzero"),
                offset: cadmpeg_ir::scalar::FiniteReal::ZERO,
            },
            active_reference_limits: finite_pair([0.0, 1.0]),
            fit_tolerance: cadmpeg_ir::scalar::NonNegativeLength::new(0.0)
                .expect("zero is nonnegative"),
        }
    }

    #[test]
    fn topology_accepts_a_valid_43_byte_loop_payload() {
        e5_test_context!(ctx);
        let mut bytes = Vec::new();
        for vertex in [10, 11, 12] {
            append_e5_record(&mut bytes, 0xfe, vertex, &[]);
        }
        for (edge, start, end) in [(110, 10, 11), (111, 11, 12), (112, 12, 10)] {
            append_e5_record(
                &mut bytes,
                0xff,
                edge,
                &[0x85, 0x08, 200, 0x08, start, 0x08, end, 0x80, 0x80, 0x21],
            );
        }
        for pcurve in [100, 101, 102] {
            let mut payload = vec![0x81, 0x18, 0x02, 0x01];
            for value in [0.0_f64, 0.0, 1.0, 0.0, 0.0, 1.0] {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            append_e5_record(&mut bytes, 0x96, pcurve, &payload);
        }
        let mut wrapper_payload = vec![0x82, 0x08, 100, 0x08, 102, 0x81, 0, 0];
        wrapper_payload.extend_from_slice(&0.0_f64.to_le_bytes());
        wrapper_payload.extend_from_slice(&1.0_f64.to_le_bytes());
        wrapper_payload[6] = 3;
        for value in [0.0_f64, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0] {
            wrapper_payload.extend_from_slice(&value.to_le_bytes());
        }
        wrapper_payload.extend_from_slice(&0_u32.to_le_bytes());
        append_e5_record(&mut bytes, 0xc1, 201, &wrapper_payload);
        let mut support_payload = vec![0x82, 0x08, 201, 0x08, 101, 0x81, 0, 0];
        support_payload.extend_from_slice(&0.0_f64.to_le_bytes());
        support_payload.extend_from_slice(&1.0_f64.to_le_bytes());
        support_payload[6] = 3;
        for value in [0.0_f64, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0] {
            support_payload.extend_from_slice(&value.to_le_bytes());
        }
        support_payload.extend_from_slice(&0_u32.to_le_bytes());
        append_e5_record(&mut bytes, 0xc1, 200, &support_payload);

        let mut bound_payload = vec![0x83];
        for pcurve in [100_u16, 101, 102] {
            bound_payload.extend_from_slice(&[0x18]);
            bound_payload.extend_from_slice(&pcurve.to_le_bytes());
        }
        bound_payload.push(0x83);
        for parameter in [0.0_f64, 0.5, 1.0] {
            bound_payload.extend_from_slice(&parameter.to_le_bytes());
            bound_payload.extend_from_slice(&1_u32.to_le_bytes());
        }
        append_e5_record(&mut bytes, 0x0e, 0, &bound_payload);

        let mut loop_payload = vec![0x87];
        for reference in [100, 110, 101, 111, 102, 112] {
            loop_payload.extend_from_slice(&[0x08, reference]);
        }
        loop_payload.extend_from_slice(&[0x18, 0x02, 0x01, 0x83]);
        for sign in [1_i16, 1, -1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0] {
            loop_payload.extend_from_slice(&sign.to_le_bytes());
        }
        assert_eq!(loop_payload.len(), 43);
        append_e5_record(&mut bytes, 0x09, 40, &loop_payload);
        append_e5_record(&mut bytes, 0xc8, 258, &[]);
        append_e5_record(
            &mut bytes,
            0x00,
            60,
            &[0x82, 0x18, 0x02, 0x01, 0x08, 40, 0x01, 0x00],
        );

        assert!(
            parse_topology(&bytes).is_none(),
            "a face-local relation supplies no absolute body traversal without its root"
        );
        append_e5_record(
            &mut bytes,
            0x08,
            70,
            &[0x81, 0xbc, 0x81, 1, 0, 255, 255, 255, 255],
        );
        append_e5_record(&mut bytes, 0x01, 71, &[0x81, 0xc6]);

        let topology = parse_topology(&bytes).expect("closed E5 topology");
        assert_eq!(topology.faces[0].surface, 258);
        assert_eq!(
            topology.faces[0].loops[0]
                .members
                .iter()
                .map(|member| member.edge_use)
                .collect::<Vec<_>>(),
            [110, 111, 112]
        );
        assert!(topology.faces[0].loops[0].outer);
        assert_eq!(
            topology.faces[0].loops[0].absolute_orientation,
            Sign::Positive
        );
        let root = bytes
            .windows(4)
            .position(|window| window == [0xe5, 0x0d, 0x03, 0x08])
            .expect("root record");
        let mut flipped = bytes.clone();
        flipped[root + 18..root + 20].copy_from_slice(&1_i16.to_le_bytes());
        let flipped = parse_topology(&flipped).expect("flipped root relation");
        assert_eq!(
            flipped.faces[0].loops[0].absolute_orientation,
            Sign::Negative
        );
        assert!(flipped.faces[0].loops[0]
            .resolved_members()
            .expect("rooted orientation")
            .iter()
            .all(|member| member.reversed));
        for (offset, sign) in [(16, -1_i16), (20, 1_i16)] {
            let mut unknown = bytes.clone();
            unknown[root + offset..root + offset + 2].copy_from_slice(&sign.to_le_bytes());
            assert!(parse_topology(&unknown).is_none());
        }
    }

    #[test]
    fn curve_support_wrapper_cycles_are_rejected() {
        let pcurves = BTreeMap::from([(
            3,
            E5Pcurve::Line {
                surface: 10,
                origin: finite_pair([0.0, 0.0]),
                direction: finite_pair([1.0, 0.0]),
                range: finite_pair([0.0, 1.0]),
            },
        )]);
        let support = |pcurves: [u32; 2]| affine_support(pcurves, finite_pair([0.0, 1.0]));
        let supports = BTreeMap::from([(1, support([2, 3])), (2, support([1, 3]))]);
        assert!(!curve_support_reference_closes(
            1,
            &pcurves,
            &supports,
            &mut HashSet::new()
        ));
    }

    #[test]
    fn doubled_curve_support_chain_walks_each_support_once() {
        const DEPTH: u32 = 20;
        let pcurves = BTreeMap::from([(
            1_000,
            E5Pcurve::Line {
                surface: 10,
                origin: finite_pair([0.0, 0.0]),
                direction: finite_pair([1.0, 0.0]),
                range: finite_pair([0.0, 1.0]),
            },
        )]);
        let support = |pcurves: [u32; 2]| E5CurveSupport {
            kind: E5CurveSupportKind::Intersection {
                curves: pcurves,
                correspondence: correspondence(),
            },
            range: finite_pair([0.0, 1.0]),
        };
        let chain = |last: [u32; 2]| {
            let mut supports = BTreeMap::new();
            for id in 1..DEPTH {
                supports.insert(id, support([id + 1, id + 1]));
            }
            supports.insert(DEPTH, support(last));
            supports
        };

        let closing = chain([1_000, 1_000]);
        let mut closed = HashSet::new();
        assert!(curve_support_reference_closes(
            1,
            &pcurves,
            &closing,
            &mut closed
        ));
        assert_eq!(closed.len(), closing.len());
        assert!(curve_support_reference_closes(
            1,
            &pcurves,
            &closing,
            &mut closed
        ));

        let cyclic = chain([1_000, DEPTH / 2]);
        assert!(!curve_support_reference_closes(
            1,
            &pcurves,
            &cyclic,
            &mut HashSet::new()
        ));
        let dangling = chain([1_000, 2_000]);
        assert!(!curve_support_reference_closes(
            1,
            &pcurves,
            &dangling,
            &mut HashSet::new()
        ));
    }

    #[test]
    fn aa_nurbs_pcurve_decodes_distinct_knots_and_poles() {
        let mut payload = vec![0x81, 0x87, 0, 0];
        for value in [1_u32, 0, 0, 2, 0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0_f64, 1.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for value in [2_u32, 2] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for point in [[0.0_f64, 0.0], [1.0, 1.0]] {
            for value in point {
                payload.extend_from_slice(&value.to_le_bytes());
            }
        }
        payload.extend_from_slice(&1_u16.to_le_bytes());
        payload.extend_from_slice(&1_u16.to_le_bytes());
        for value in [0.0_f64, 1.0, 1.0, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(0);
        let mut truncated = payload.clone();
        truncated.pop();
        assert!(parse_nurbs_pcurve(&truncated, 2, 7).is_none());

        let E5Pcurve::Nurbs {
            surface,
            degree,
            knots,
            control_points,
            range,
        } = parse_nurbs_pcurve(&payload, 2, 7).expect("AA NURBS pcurve")
        else {
            panic!("AA record did not produce a NURBS pcurve");
        };
        assert_eq!(surface, 7);
        assert_eq!(degree, 1);
        assert_eq!(knots, finite_lane(&[0.0, 0.0, 1.0, 1.0]));
        assert_eq!(
            control_points,
            [finite_pair([0.0, 0.0]), finite_pair([1.0, 1.0])]
        );
        assert_eq!(range, finite_pair([0.0, 1.0]));
        let tail = payload.len() - super::E5_NURBS_PCURVE_TAIL_BYTES;
        let mut inset = payload.clone();
        inset[tail + 4..tail + 12].copy_from_slice(&0.125_f64.to_le_bytes());
        let decoded =
            crate::test_support::with_service_context(|_ctx| parse_nurbs_pcurve(&inset, 2, 7))
                .expect("service resource budget");
        assert!(
            matches!(decoded, E5Pcurve::Nurbs { range, .. } if range == finite_pair([0.125, 1.0]))
        );
        for (offset, bytes) in [
            (0, vec![0, 0]),
            (2, vec![2, 0]),
            (4, f64::NAN.to_le_bytes().to_vec()),
            (4, (-0.25_f64).to_le_bytes().to_vec()),
            (12, 1.25_f64.to_le_bytes().to_vec()),
            (20, 2.0_f64.to_le_bytes().to_vec()),
            (28, 0.5_f64.to_le_bytes().to_vec()),
            (36, vec![1]),
        ] {
            let mut invalid = payload.clone();
            invalid[tail + offset..tail + offset + bytes.len()].copy_from_slice(&bytes);
            assert!(crate::test_support::with_service_context(|_ctx| {
                parse_nurbs_pcurve(&invalid, 2, 7)
            })
            .is_none());
        }
    }

    #[test]
    fn circle_pcurve_admits_only_a_finite_positive_radius() {
        let parse = |radius: f64| {
            let mut payload = vec![0x81, 0x18];
            payload.extend_from_slice(&7_u16.to_le_bytes());
            for value in [0.5_f64, 0.0] {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            payload.extend_from_slice(&[0; 8]);
            for value in [radius, 0.0, 1.0, 0.0, 0.0] {
                payload.extend_from_slice(&value.to_le_bytes());
            }
            parse_pcurve(&Record {
                class: 0x97,
                id: 20,
                payload: &payload,
            })
        };
        assert!(matches!(
            parse(0.5),
            Some(E5Pcurve::Circle { surface: 7, radius, .. }) if radius.get() == 0.5
        ));
        for refused in [0.0, -0.5, f64::INFINITY, f64::NAN] {
            assert!(parse(refused).is_none());
        }
    }

    #[test]
    fn body_root_accepts_widened_u16_face_roster() {
        let mut payload = vec![0x08, 2];
        payload.extend_from_slice(&[0x10, 0x16]);
        payload.extend_from_slice(&[0x18, 0x01, 0x16]);
        payload.extend_from_slice(&[0x08, 2]);
        payload.extend_from_slice(
            &[
                1_i16.to_le_bytes(),
                (-1_i16).to_le_bytes(),
                1_i16.to_le_bytes(),
                (-1_i16).to_le_bytes(),
            ]
            .concat(),
        );
        let faces = crate::test_support::with_service_context(|_ctx| parse_body_root(&payload))
            .expect("service resource budget");
        assert_eq!(
            faces.iter().map(|face| face.record_id).collect::<Vec<_>>(),
            [0x1600, 0x1601]
        );
        assert_eq!(
            faces
                .iter()
                .map(|face| face.orientation)
                .collect::<Vec<_>>(),
            [Sign::Positive, Sign::Negative]
        );
        let mut mixed_frame = payload.clone();
        mixed_frame.splice(7..9, [0x82]);
        assert!(crate::test_support::with_service_context(|_ctx| {
            parse_body_root(&mixed_frame)
        })
        .is_none());
    }

    #[test]
    fn jet_range_trailer_is_scale_relative_and_knots_are_finite() {
        let final_knot = 1e-200_f64;
        let mut payload = Vec::new();
        for value in [5_u32, 0, 0, 2, 0, 0, 0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&final_knot.to_le_bytes());
        for value in [6_u32, 6, 2] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for _ in 0..8 {
            payload.extend_from_slice(&0.0_f64.to_le_bytes());
        }
        payload.extend_from_slice(&1_u16.to_le_bytes());
        for _ in 0..4 {
            payload.extend_from_slice(&0.0_f64.to_le_bytes());
        }
        payload.extend_from_slice(&0.0_f64.to_le_bytes());
        payload.extend_from_slice(&final_knot.to_le_bytes());

        assert!(matches!(
            parse_jet_pcurve(&payload, 0, 7),
            Some(E5Pcurve::Jet { range, .. }) if range == finite_pair([0.0, final_knot])
        ));

        let mut nonzero_lower = payload.clone();
        let lower_offset = nonzero_lower.len() - 16;
        nonzero_lower[lower_offset..lower_offset + 8]
            .copy_from_slice(&(0.5 * final_knot).to_le_bytes());
        assert!(parse_jet_pcurve(&nonzero_lower, 0, 7).is_none());

        let mut wrong_upper = payload.clone();
        let upper_offset = wrong_upper.len() - 8;
        wrong_upper[upper_offset..].copy_from_slice(&(2.0 * final_knot).to_le_bytes());
        assert!(parse_jet_pcurve(&wrong_upper, 0, 7).is_none());

        let mut nonfinite_knot = payload;
        nonfinite_knot[28..36].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(parse_jet_pcurve(&nonfinite_knot, 0, 7).is_none());
    }

    #[test]
    fn digon_uses_forward_first_edge_as_relative_gauge() {
        let edges = BTreeMap::from([
            (
                1,
                E5Edge {
                    support: 0,
                    start_vertex: 10,
                    end_vertex: 20,
                    parameter_start: 0,
                    parameter_end: 0,
                },
            ),
            (
                2,
                E5Edge {
                    support: 0,
                    start_vertex: 20,
                    end_vertex: 10,
                    parameter_start: 0,
                    parameter_end: 0,
                },
            ),
        ]);
        assert_eq!(solve_loop_chain(&[1, 2], &edges), Some(vec![false, false]));
    }

    #[test]
    fn edge_parameters_resolve_one_entry_from_each_bound() {
        e5_test_context!(ctx);
        let edge = E5Edge {
            support: 0,
            start_vertex: 0,
            end_vertex: 0,
            parameter_start: 10,
            parameter_end: 11,
        };
        let bound = |parameter| E5Bounds {
            entries: vec![E5BoundEntry {
                representation: 20,
                parameter: crate::test_support::test_b5::finite(parameter),
                code: 1,
            }],
        };
        let mut topology = E5Topology {
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: BTreeMap::from([(1, edge)]),
            pcurves: BTreeMap::from([(
                20,
                E5Pcurve::Line {
                    surface: 1,
                    origin: finite_pair([0.0, 0.0]),
                    direction: finite_pair([1.0, 0.0]),
                    range: finite_pair([0.0, 1.0]),
                },
            )]),
            bounds: BTreeMap::from([(10, bound(0.25)), (11, bound(0.75))]),
            curve_supports: BTreeMap::new(),
            vertex_refs: Vec::new(),
        };

        assert_eq!(
            topology.edge_representation_parameters(1, 20),
            Some(crate::test_support::test_b5::finite_pair([0.25, 0.75]))
        );
        topology
            .bounds
            .get_mut(&11)
            .expect("end bound")
            .entries
            .push(E5BoundEntry {
                representation: 20,
                parameter: crate::test_support::test_b5::finite(1.0),
                code: 8,
            });
        assert_eq!(topology.edge_representation_parameters(1, 20), None);
        let end = topology.bounds.get_mut(&11).expect("end bound");
        end.entries.truncate(1);
        end.entries[0].code = 0;
        assert_eq!(topology.edge_representation_parameters(1, 20), None);
    }

    #[test]
    fn jet_bound_codes_resolve_span_local_parameters() {
        use super::bound_representation_parameter;
        let sites = [0.0, 3.0, 7.0, 9.0, 13.0, 15.0, 19.0, 23.0]
            .into_iter()
            .map(|knot| E5PcurveJetSite {
                knot: finite(knot),
                multiplicity: 3,
                point: finite_pair([0.0, 0.0]),
                first_derivatives: finite_pair([0.0, 0.0]),
                second_derivatives: finite_pair([0.0, 0.0]),
            })
            .collect();
        let pcurves = BTreeMap::from([(
            20,
            E5Pcurve::Jet {
                surface: 1,
                sites,
                range: finite_pair([0.0, 23.0]),
            },
        )]);
        let resolve = |local, code| {
            let bounds = BTreeMap::from([(
                10,
                E5Bounds {
                    entries: vec![E5BoundEntry {
                        representation: 20,
                        parameter: finite(local),
                        code,
                    }],
                },
            )]);
            bound_representation_parameter(&bounds, &pcurves, 10, 20)
                .map(cadmpeg_ir::scalar::FiniteReal::get)
        };
        // Raw scalars decrease from 2 to 1; resolved native parameters
        // increase from 2 to 4 across the knot boundary.
        assert!(resolve(2.0, 1).expect("start span") < resolve(1.0, 2).expect("end span"));
        assert_eq!(resolve(2.0, 1), Some(2.0));
        assert_eq!(resolve(1.0, 2), Some(4.0));
        assert_eq!(resolve(4.0, 2), Some(7.0));
        assert_eq!(resolve(4.0, 7), Some(23.0));
        assert_eq!(resolve(0.0, 0), None);
        assert_eq!(resolve(0.0, 8), None);
        assert_eq!(resolve(-1.0, 2), None);
        assert_eq!(resolve(5.0, 2), None);
    }

    #[test]
    fn nurbs_bounds_use_distinct_arcs_and_nonzero_first_knot() {
        for first_knot in [-23.0, 1.0e12] {
            let pcurves = BTreeMap::from([(
                20,
                E5Pcurve::Nurbs {
                    surface: 1,
                    degree: 1,
                    knots: finite_lane(&[
                        first_knot,
                        first_knot,
                        first_knot + 0.5,
                        first_knot + 1.0,
                        first_knot + 1.0,
                    ]),
                    control_points: vec![finite_pair([0.0, 0.0]); 3],
                    range: finite_pair([first_knot, first_knot + 1.0]),
                },
            )]);
            let resolve = |local, code, representation| {
                let bounds = BTreeMap::from([(
                    10,
                    E5Bounds {
                        entries: vec![E5BoundEntry {
                            representation,
                            parameter: finite(local),
                            code,
                        }],
                    },
                )]);
                super::bound_representation_parameter(&bounds, &pcurves, 10, representation)
                    .map(cadmpeg_ir::scalar::FiniteReal::get)
            };
            assert_eq!(resolve(0.0, 1, 20), Some(first_knot));
            assert_eq!(resolve(0.5, 1, 20), Some(first_knot + 0.5));
            assert_eq!(resolve(0.5, 2, 20), Some(first_knot + 1.0));
            assert_eq!(resolve(0.0, 2, 20), Some(first_knot + 0.5));
            assert_eq!(resolve(0.0, 0, 20), None);
            assert_eq!(resolve(0.0, 3, 20), None);
            assert_eq!(resolve(0.0, u32::MAX, 20), None);
            assert_eq!(resolve(0.6, 2, 20), None);
            assert_eq!(resolve(0.0, 1, 21), None);
            assert_eq!(resolve(-0.25, 1, 20), None);
        }
    }

    #[test]
    fn radial_parity_rejects_frustration_and_reverses_negative_gauge() {
        e5_test_context!(ctx);
        let loop_ = |record_id, edge_uses: Vec<u32>| E5Loop {
            record_id,
            surface: record_id + 100,
            members: edge_uses
                .into_iter()
                .map(|edge_use| E5LoopMember {
                    pcurve: record_id + 200,
                    edge_use,
                    reversed: false,
                })
                .collect(),
            oriented_members: None,
            outer: true,
            absolute_orientation: Sign::Positive,
        };
        let mut faces = vec![
            E5Face {
                record_id: 1,
                surface: 101,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![loop_(11, vec![1, 3])],
            },
            E5Face {
                record_id: 2,
                surface: 102,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![loop_(12, vec![1, 2])],
            },
            E5Face {
                record_id: 3,
                surface: 103,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![loop_(13, vec![2, 3])],
            },
        ];

        assert!(!resolve_absolute_orientation(&mut faces));
        assert!(faces
            .iter()
            .flat_map(|face| &face.loops)
            .all(|loop_| loop_.oriented_members.is_none()));

        let mut faces = vec![
            E5Face {
                record_id: 1,
                surface: 101,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![loop_(11, vec![1, 2])],
            },
            E5Face {
                record_id: 2,
                surface: 102,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![loop_(12, vec![1, 3])],
            },
        ];
        assert!(!resolve_absolute_orientation(&mut faces));
        faces[1].loops[0].absolute_orientation = Sign::Negative;
        assert!(resolve_absolute_orientation(&mut faces));
        let second = faces[1].loops[0]
            .resolved_members()
            .expect("required invariant");
        assert_eq!(second[0].serialized_index, 1);
        assert_eq!(second[1].serialized_index, 0);
        assert!(second.iter().all(|member| member.reversed));
    }

    #[test]
    fn records_stop_at_a_marker_without_a_full_header() {
        e5_test_context!(ctx);
        let mut bytes = vec![0; 20];
        bytes.extend_from_slice(&[0xe5, 0x0d, 0x03, 0x00]);
        assert_eq!(records(&bytes).count(), 0);
        assert!(parse_topology(&bytes).is_none());
    }

    #[test]
    fn e5_edge_allocation_terminal_is_separate_from_object_references() {
        for terminal in [0x01, 0x02, 0x21, 0x22, 0x25, 0x29, 0x2a] {
            let payload = [0x85, 0x8d, 0x8a, 0x90, 0x8e, 0x8f, terminal];
            let record = Record {
                class: 0xff,
                id: 12,
                payload: &payload,
            };
            let edge = super::parse_edge(record.payload).expect("admitted allocation terminal");
            assert_eq!(
                (
                    edge.support,
                    edge.start_vertex,
                    edge.end_vertex,
                    edge.parameter_start,
                    edge.parameter_end
                ),
                (13, 10, 16, 14, 15)
            );
        }
        for tail in [vec![], vec![0x26], vec![0x7f], vec![0x21, 0], vec![0x08, 8]] {
            let mut payload = vec![0x85, 0x8d, 0x8a, 0x90, 0x8e, 0x8f];
            payload.extend(tail);
            let record = Record {
                class: 0xff,
                id: 12,
                payload: &payload,
            };
            assert!(super::parse_edge(record.payload).is_none());
        }
    }

    #[test]
    fn e5_edge_accepts_shifted_u16_reference_without_b5_only_leads() {
        let mut payload = vec![
            0x85, 0x38, 0x9a, 0x10, 1, 0x30, 0x11, 1, 0x38, 0xfc, 0x10, 1, 0x38, 0x9b, 0x10, 1,
            0x38, 0x9c, 0x10, 1, 0x21,
        ];
        let record = Record {
            class: 0xff,
            id: 69785,
            payload: &payload,
        };
        let edge = super::parse_edge(record.payload).expect("shifted u16 reference");
        assert_eq!(
            (
                edge.support,
                edge.start_vertex,
                edge.end_vertex,
                edge.parameter_start,
                edge.parameter_end
            ),
            (69786, 69888, 69884, 69787, 69788)
        );
        for lead in [0x20, 0x28] {
            payload[5] = lead;
            let record = Record {
                class: 0xff,
                id: 69785,
                payload: &payload,
            };
            assert!(super::parse_edge(record.payload).is_none());
        }
        payload.truncate(7);
        payload[5] = 0x30;
        let record = Record {
            class: 0xff,
            id: 69785,
            payload: &payload,
        };
        assert!(super::parse_edge(record.payload).is_none());
    }

    #[test]
    fn e5_loop_sign_scan_preserves_role_and_rejection() {
        let mut trailing = vec![0x82];
        for sign in [1_i16, 1, -1, 1, 1, 1, 1, 1, 1, 0] {
            trailing.extend_from_slice(&sign.to_le_bytes());
        }
        assert!(matches!(super::parse_loop_signs(&trailing, 2), Ok(true)));
        trailing[3..5].copy_from_slice(&(-1_i16).to_le_bytes());
        assert!(matches!(super::parse_loop_signs(&trailing, 2), Ok(false)));
        trailing[9..11].copy_from_slice(&2_i16.to_le_bytes());
        assert!(matches!(
            super::parse_loop_signs(&trailing, 2),
            Err(super::LoopSignError::Sign)
        ));
    }

    #[test]
    fn e5_orientation_admits_one_occurrence() {
        let faces = vec![E5Face {
            record_id: 1,
            surface: 2,
            trailer_sign: Sign::Positive,
            loops: vec![E5Loop {
                record_id: 3,
                surface: 2,
                members: e5_loop_members(&[4], &[5], &[false]),
                oriented_members: None,
                outer: true,
                absolute_orientation: Sign::Positive,
            }],
        }];
        e5_test_context!(service_ctx);
        assert!(resolve_absolute_orientation(&mut faces.clone()));
    }

    #[test]
    fn intersection_support_on_one_line_pcurve_closes() {
        let pcurves = BTreeMap::from([(
            3,
            E5Pcurve::Line {
                surface: 10,
                origin: finite_pair([0.0, 0.0]),
                direction: finite_pair([1.0, 0.0]),
                range: finite_pair([0.0, 1.0]),
            },
        )]);
        let supports = BTreeMap::from([(
            1,
            E5CurveSupport {
                kind: E5CurveSupportKind::Intersection {
                    curves: [3, 3],
                    correspondence: correspondence(),
                },
                range: finite_pair([0.0, 1.0]),
            },
        )]);
        assert!(curve_support_reference_closes(
            1,
            &pcurves,
            &supports,
            &mut HashSet::new()
        ));
    }

    #[test]
    fn nurbs_pcurve_payload_parses_to_a_nurbs_pcurve() {
        let mut payload = vec![0x81, 0x87, 0, 0];
        for value in [1_u32, 0, 0, 2, 0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for value in [0.0_f64, 1.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for value in [2_u32, 2] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for point in [[0.0_f64, 0.0], [1.0, 1.0]] {
            for value in point {
                payload.extend_from_slice(&value.to_le_bytes());
            }
        }
        payload.extend_from_slice(&1_u16.to_le_bytes());
        payload.extend_from_slice(&1_u16.to_le_bytes());
        for value in [0.0_f64, 1.0, 1.0, 0.0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.push(0);
        assert!(matches!(
            parse_nurbs_pcurve(&payload, 2, 7),
            Some(E5Pcurve::Nurbs { .. })
        ));
    }

    #[test]
    fn jet_pcurve_payload_parses_to_a_jet_pcurve() {
        let mut payload = Vec::new();
        for value in [5_u32, 0, 0, 2, 0, 0, 0] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        payload.extend_from_slice(&1.0_f64.to_le_bytes());
        for value in [6_u32, 6, 2] {
            payload.extend_from_slice(&value.to_le_bytes());
        }
        for _ in 0..8 {
            payload.extend_from_slice(&0.0_f64.to_le_bytes());
        }
        payload.extend_from_slice(&1_u16.to_le_bytes());
        for _ in 0..4 {
            payload.extend_from_slice(&0.0_f64.to_le_bytes());
        }
        payload.extend_from_slice(&0.0_f64.to_le_bytes());
        payload.extend_from_slice(&1.0_f64.to_le_bytes());
        assert!(matches!(
            parse_jet_pcurve(&payload, 0, 7),
            Some(E5Pcurve::Jet { .. })
        ));
    }

    #[test]
    fn torus_topology_stream_parses() {
        let bytes = crate::test_support::test_e5::e5_torus_topology_stream();
        assert!(crate::test_support::with_service_context(|_ctx| parse_topology(&bytes)).is_some());
    }

    #[test]
    fn single_outer_loop_face_solves_absolute_orientation() {
        let faces = vec![E5Face {
            record_id: 1,
            surface: 2,
            trailer_sign: Sign::Positive,
            loops: vec![E5Loop {
                record_id: 3,
                surface: 2,
                members: e5_loop_members(&[4], &[5], &[false]),
                oriented_members: None,
                outer: true,
                absolute_orientation: crate::families::e5::graph::Sign::Positive,
            }],
        }];
        e5_test_context!(service_ctx);
        assert!(resolve_absolute_orientation(&mut faces.clone()));
    }

    #[test]
    fn stream_with_only_an_edge_record_has_no_topology() {
        let mut bytes = Vec::new();
        append_e5_record(
            &mut bytes,
            0xff,
            110,
            &[0x85, 0x08, 200, 0x08, 10, 0x08, 11, 0x80, 0x80, 0x7f],
        );
        let service = crate::test_support::with_service_context(|_ctx| parse_topology(&bytes));
        assert!(service.is_none());
    }
}
