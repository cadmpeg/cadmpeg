// SPDX-License-Identifier: Apache-2.0
//! Native topology records in the E5 `0D 03` stream family.

use cadmpeg_core::decode::u64_from_index;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem::size_of;

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};

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
        ctx: &DecodeContext<'_>,
        edge_ref: u32,
        representation: u32,
    ) -> Result<Option<[FiniteReal; 2]>, CodecError> {
        let Some(edge) = self.edges.get(&edge_ref) else {
            return Ok(None);
        };
        for _ in 0..2 {
            charge_bound_arc_scan(ctx, &self.pcurves, representation)?;
        }
        Ok(bound_representation_parameter(
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
        .map(|(start, end)| [start, end]))
    }
}

/// A class-`0xc0`/`0xc1` curve-support record: the pcurve(s) an edge curve
/// evaluates against and the surface parameter range they span ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum E5CurveSupportKind {
    /// Class-`0xc0` one-pcurve boundary support.
    Boundary(u32),
    /// Class-`0xc1` two-pcurve intersection support.
    Intersection([u32; 2]),
}

impl E5CurveSupportKind {
    fn from_parts(intersection: bool, pcurves: &[u32]) -> Option<Self> {
        match (intersection, pcurves) {
            (false, &[pcurve]) => Some(Self::Boundary(pcurve)),
            (true, &[left, right]) => Some(Self::Intersection([left, right])),
            _ => None,
        }
    }

    fn pcurves(&self) -> &[u32] {
        match self {
            Self::Boundary(pcurve) => std::slice::from_ref(pcurve),
            Self::Intersection(pcurves) => pcurves,
        }
    }

    fn is_intersection(&self) -> bool {
        matches!(self, Self::Intersection(_))
    }
}

/// A class-`0xc0`/`0xc1` curve-support record ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq)]
pub(super) struct E5CurveSupport {
    /// Boundary or intersection pcurve layout.
    pub(super) kind: E5CurveSupportKind,
    /// Raw mode byte following the pcurve reference lane; meaning not
    /// decoded further.
    pub(super) mode: u8,
    /// `[lo, hi]` parameter range on the support, stored as LE f64.
    pub(super) range: [FiniteReal; 2],
    /// Unparsed bytes after the fixed header; not interpreted.
    pub(super) tail: Vec<u8>,
}

impl E5CurveSupport {
    pub(super) fn pcurves(&self) -> &[u32] {
        self.kind.pcurves()
    }

    pub(super) fn is_intersection(&self) -> bool {
        self.kind.is_intersection()
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
pub(crate) fn parse_topology(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<E5Topology>, CodecError> {
    let mut admitted_records = Vec::new();
    for record in records(bytes) {
        ctx.push_vec(&mut admitted_records, record, "catia_e5_graph_records")?;
    }
    (|| -> Option<Result<E5Topology, CodecError>> {
        let records = admitted_records;
        let mut by_id = HashMap::new();
        for record in &records {
            if let Err(error) =
                ctx.insert_hash_map(&mut by_id, record.id, record, "catia_e5_records_by_id")
            {
                return Some(Err(error));
            }
        }
        if by_id.len() != records.len() {
            return None;
        }

        let mut edges = BTreeMap::new();
        let mut pcurves = BTreeMap::new();
        for record in &records {
            if record.class == 0xff {
                let edge = parse_edge(record.payload)?;
                if let Err(error) =
                    ctx.insert_btree_map(&mut edges, record.id, edge, "catia_e5_topology_edges")
                {
                    return Some(Err(error));
                }
            }
            if matches!(record.class, 0x96 | 0x97 | 0xa0 | 0xaa) {
                let pcurve = match parse_pcurve(ctx, record) {
                    Ok(Some(pcurve)) => pcurve,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                if let Err(error) = ctx.insert_btree_map(
                    &mut pcurves,
                    record.id,
                    pcurve,
                    "catia_e5_topology_pcurves",
                ) {
                    return Some(Err(error));
                }
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
                    let value = match parse_bounds(ctx, record) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) = ctx.insert_btree_map(
                        &mut bounds,
                        record.id,
                        value,
                        "catia_e5_topology_bounds",
                    ) {
                        return Some(Err(error));
                    }
                }
                0xc0 | 0xc1 => {
                    let value = match parse_curve_support(ctx, record) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) = ctx.insert_btree_map(
                        &mut curve_supports,
                        record.id,
                        value,
                        "catia_e5_curve_supports",
                    ) {
                        return Some(Err(error));
                    }
                }
                0x09 => {
                    let value = match parse_loop(ctx, record) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) =
                        ctx.insert_hash_map(&mut loops, record.id, value, "catia_e5_raw_loops")
                    {
                        return Some(Err(error));
                    }
                }
                0x00 => {
                    let value = match parse_face(ctx, record) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) = ctx.push_vec(&mut raw_faces, value, "catia_e5_raw_faces") {
                        return Some(Err(error));
                    }
                }
                0xfe => {
                    if let Err(error) =
                        ctx.insert_hash_set(&mut vertex_ids, record.id, "catia_e5_vertex_ids")
                    {
                        return Some(Err(error));
                    }
                }
                _ => {}
            }
        }
        if raw_faces.is_empty() || loops.is_empty() || edges.is_empty() || vertex_ids.is_empty() {
            return None;
        }

        let bodies = match parse_bodies(ctx, &records, &by_id) {
            Ok(Some(bodies)) => bodies,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let mut root_orientation = HashMap::new();
        for body in &bodies {
            for face in &body.faces {
                if root_orientation.contains_key(&face.record_id) {
                    return None;
                }
                if let Err(error) = ctx.insert_hash_map(
                    &mut root_orientation,
                    face.record_id,
                    face.orientation,
                    "catia_e5_root_face_orientations",
                ) {
                    return Some(Err(error));
                }
            }
        }
        let mut faces = Vec::new();
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
                let reversed = match solve_loop_chain(ctx, &raw.edges, &edges) {
                    Ok(Some(reversed)) => reversed,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
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
                    for _ in 0..2 {
                        if let Err(error) = charge_bound_arc_scan(ctx, &pcurves, *pcurve_id) {
                            return Some(Err(error));
                        }
                    }
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
                        match curve_support_reference_closes(
                            ctx,
                            *reference,
                            &pcurves,
                            &curve_supports,
                        ) {
                            Ok(true) => {}
                            Ok(false) => return None,
                            Err(error) => return Some(Err(error)),
                        }
                    }
                    if let Err(error) = ctx.insert_hash_set(
                        &mut reachable_edges,
                        *edge_id,
                        "catia_e5_reachable_edges",
                    ) {
                        return Some(Err(error));
                    }
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
                    if let Err(error) = ctx.push_vec(
                        &mut members,
                        E5LoopMember {
                            pcurve,
                            edge_use,
                            reversed,
                        },
                        "catia_e5_loop_members",
                    ) {
                        return Some(Err(error));
                    }
                }
                if let Err(error) = ctx.push_vec(
                    &mut resolved_loops,
                    E5Loop {
                        record_id: raw.id,
                        surface: raw.surface,
                        members,
                        oriented_members: None,
                        outer: raw.outer,
                        absolute_orientation,
                    },
                    "catia_e5_resolved_loops",
                ) {
                    return Some(Err(error));
                }
            }
            if let Err(error) = ctx.push_vec(
                &mut faces,
                E5Face {
                    record_id: face.id,
                    surface: face.surface,
                    trailer_sign: face.trailer_sign,
                    loops: resolved_loops,
                },
                "catia_e5_topology_faces",
            ) {
                return Some(Err(error));
            }
        }
        let oriented = match resolve_absolute_orientation(ctx, &mut faces) {
            Ok(oriented) => oriented,
            Err(error) => return Some(Err(error)),
        };
        if !oriented {
            return None;
        }
        edges.retain(|id, _| reachable_edges.contains(id));
        let mut vertex_refs = Vec::new();
        for edge in edges.values() {
            for vertex in [edge.start_vertex, edge.end_vertex] {
                if let Err(error) = ctx.push_vec(&mut vertex_refs, vertex, "catia_e5_vertex_refs") {
                    return Some(Err(error));
                }
            }
        }
        if let Err(error) = ctx.sort_unstable_by(
            &mut vertex_refs,
            Ord::cmp,
            |_| 0,
            "catia_e5_vertex_refs_sort",
        ) {
            return Some(Err(error));
        }
        vertex_refs.dedup();
        let mut roster = Vec::new();
        for body in &bodies {
            for face in &body.faces {
                if let Err(error) =
                    ctx.push_vec(&mut roster, face.record_id, "catia_e5_body_face_roster")
                {
                    return Some(Err(error));
                }
            }
        }
        let mut roster_set = HashSet::new();
        for &face in &roster {
            if let Err(error) = ctx.insert_hash_set(&mut roster_set, face, "catia_e5_body_face_set")
            {
                return Some(Err(error));
            }
        }
        let mut face_set = HashSet::new();
        for face in &faces {
            if let Err(error) =
                ctx.insert_hash_set(&mut face_set, face.record_id, "catia_e5_topology_face_set")
            {
                return Some(Err(error));
            }
        }
        if roster.len() != roster_set.len() || roster_set != face_set {
            return None;
        }
        Some(Ok(E5Topology {
            bodies,
            faces,
            edges,
            pcurves,
            bounds,
            curve_supports,
            vertex_refs,
        }))
    })()
    .transpose()
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
fn curve_support_reference_closes(
    ctx: &DecodeContext<'_>,
    reference: u32,
    pcurves: &BTreeMap<u32, E5Pcurve>,
    supports: &BTreeMap<u32, E5CurveSupport>,
) -> Result<bool, CodecError> {
    if pcurves.contains_key(&reference) {
        return Ok(true);
    }
    let mut visiting = HashSet::new();
    let mut stack = Vec::new();
    ctx.push_vec(&mut stack, (reference, false), "catia_e5_support_stack")?;
    while let Some((reference, leaving)) = stack.pop() {
        if pcurves.contains_key(&reference) {
            continue;
        }
        let Some(support) = supports
            .get(&reference)
            .filter(|support| support.is_intersection())
        else {
            return Ok(false);
        };
        if leaving {
            visiting.remove(&reference);
            continue;
        }
        if !ctx.insert_hash_set(&mut visiting, reference, "catia_e5_support_visiting")? {
            return Ok(false);
        }
        ctx.push_vec(&mut stack, (reference, true), "catia_e5_support_stack")?;
        for child in support.pcurves().iter().rev() {
            if pcurves.contains_key(child) {
                continue;
            }
            if !supports
                .get(child)
                .is_some_and(E5CurveSupport::is_intersection)
                || visiting.contains(child)
            {
                return Ok(false);
            }
            ctx.push_vec(&mut stack, (*child, false), "catia_e5_support_stack")?;
        }
    }
    Ok(true)
}

fn charge_bound_arc_scan(
    ctx: &DecodeContext<'_>,
    pcurves: &BTreeMap<u32, E5Pcurve>,
    representation: u32,
) -> Result<(), CodecError> {
    if let Some(E5Pcurve::Nurbs { knots, .. }) = pcurves.get(&representation) {
        ctx.charge_work(u64_from_index(knots.len()), "catia_e5_bound_arc_scan")?;
    }
    Ok(())
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

fn parse_curve_support(
    ctx: &DecodeContext<'_>,
    record: &Record<'_>,
) -> Result<Option<E5CurveSupport>, CodecError> {
    let expected = if record.class == 0xc0 { 1 } else { 2 };
    if record.payload.first() != Some(&(0x80 + expected)) {
        return Ok(None);
    }
    let mut position = 1;
    let mut pcurves = [0u32; 2];
    for pcurve in pcurves.iter_mut().take(usize::from(expected)) {
        let Some(reference) = wire::tokens::object_ref(record.payload, &mut position, false) else {
            return Ok(None);
        };
        *pcurve = reference;
    }
    if record.payload.get(position) != Some(&0x81) {
        return Ok(None);
    }
    position += 1;
    let Some(&mode) = record.payload.get(position) else {
        return Ok(None);
    };
    position += 1;
    if record.payload.get(position) != Some(&0x00) {
        return Ok(None);
    }
    position += 1;
    let mut view = View::over_retained(record.payload);
    if view.seek(position).is_none() {
        return Ok(None);
    }
    let Some(range) = (|| Some([finite_f64_le(&mut view)?, finite_f64_le(&mut view)?]))() else {
        return Ok(None);
    };
    position = view.position();
    let Some(kind) =
        E5CurveSupportKind::from_parts(record.class == 0xc1, &pcurves[..usize::from(expected)])
    else {
        return Ok(None);
    };
    let tail = ctx.copy_slice(&record.payload[position..], "catia_e5_curve_support_tail")?;
    Ok(Some(E5CurveSupport {
        kind,
        mode,
        range,
        tail,
    }))
}

fn parse_bounds(
    ctx: &DecodeContext<'_>,
    record: &Record<'_>,
) -> Result<Option<E5Bounds>, CodecError> {
    let Some(count) = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x80))
        .map(usize::from)
    else {
        return Ok(None);
    };
    let mut position = 1;
    let mut representations = Vec::new();
    for _ in 0..count {
        let Some(reference) = wire::tokens::object_ref(record.payload, &mut position, false) else {
            return Ok(None);
        };
        ctx.push_vec(&mut representations, reference, "catia_e5_bound_references")?;
    }
    let Some(expected_head) = u8::try_from(count)
        .ok()
        .and_then(|count| 0x80u8.checked_add(count))
    else {
        return Ok(None);
    };
    if record.payload.get(position) != Some(&expected_head) {
        return Ok(None);
    }
    position += 1;
    let mut view = View::over_retained(record.payload);
    if view.seek(position).is_none() {
        return Ok(None);
    }
    let mut entries = Vec::new();
    for representation in representations {
        let Some((parameter, code)) =
            (|| Some((FiniteReal::new(view.f64_le()?)?, view.u32_le()?)))()
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut entries,
            E5BoundEntry {
                representation,
                parameter,
                code,
            },
            "catia_e5_bound_entries",
        )?;
    }
    Ok(view.is_empty().then_some(E5Bounds { entries }))
}

fn parse_pcurve(
    ctx: &DecodeContext<'_>,
    record: &Record<'_>,
) -> Result<Option<E5Pcurve>, CodecError> {
    if record.payload.first() != Some(&0x81) {
        return Ok(None);
    }
    let mut position = 1;
    let Some(surface) = wire::tokens::object_ref(record.payload, &mut position, false) else {
        return Ok(None);
    };
    let mut view = View::over_retained(record.payload);
    if view.seek(position).is_none() {
        return Ok(None);
    }
    match record.class {
        0x96 => {
            let Some(values) = (|| {
                Some([
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                    finite_f64_le(&mut view)?,
                ])
            })() else {
                return Ok(None);
            };
            if !view.is_empty() {
                return Ok(None);
            }
            Ok(Some(E5Pcurve::Line {
                surface,
                origin: [values[0], values[1]],
                direction: [values[2], values[3]],
                range: [values[4], values[5]],
            }))
        }
        0x97 => {
            let Some((center, codes, values)) = (|| {
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
            })() else {
                return Ok(None);
            };
            if !view.is_empty() {
                return Ok(None);
            }
            let Some(radius) = PositiveReal::new(values[0].get()) else {
                return Ok(None);
            };
            Ok(Some(E5Pcurve::Circle {
                surface,
                center: [center[0], center[1]],
                codes,
                radius,
                range: [values[1], values[2]],
                tail: [values[3], values[4]],
            }))
        }
        0xa0 => parse_jet_pcurve(ctx, record.payload, position, surface),
        0xaa => parse_nurbs_pcurve(ctx, record.payload, position, surface),
        _ => Ok(None),
    }
}

const E5_NURBS_PCURVE_TAIL_BYTES: usize = 37;

fn finite_f64_le(view: &mut View<'_>) -> Option<FiniteReal> {
    FiniteReal::new(view.f64_le()?)
}

fn parse_nurbs_pcurve(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    position: usize,
    surface: u32,
) -> Result<Option<E5Pcurve>, CodecError> {
    let mut view = View::over_retained(payload);
    if view.seek(position).is_none() || view.u16_le() != Some(0) {
        return Ok(None);
    }
    let Some((degree, zero0, zero1, knot_count, zero2)) = (|| {
        Some((
            view.u32_le()?,
            view.u32_le()?,
            view.u32_le()?,
            usize::try_from(view.u32_le()?).ok()?,
            view.u32_le()?,
        ))
    })() else {
        return Ok(None);
    };
    if degree == 0 || knot_count == 0 || [zero0, zero1, zero2] != [0; 3] {
        return Ok(None);
    }
    let knot_count_u64 = u64_from_index(knot_count);
    if view.counted(knot_count_u64, 12).is_none() {
        return Ok(None);
    }
    let mut knots = Vec::new();
    ctx.reserve_vec(&mut knots, knot_count, "catia_e5_pcurve_knots")?;
    for _ in 0..knot_count {
        let Some(knot) = finite_f64_le(&mut view) else {
            return Ok(None);
        };
        knots.push(knot);
    }
    let mut multiplicities = Vec::new();
    ctx.reserve_vec(
        &mut multiplicities,
        knot_count,
        "catia_e5_pcurve_multiplicities",
    )?;
    for _ in 0..knot_count {
        let Some(multiplicity) = view.u32_le() else {
            return Ok(None);
        };
        multiplicities.push(multiplicity);
    }
    let Some(max_control_count) = view
        .remaining()
        .checked_sub(E5_NURBS_PCURVE_TAIL_BYTES)
        .map(|remaining| remaining / 16)
    else {
        return Ok(None);
    };
    let Some((expanded_knots, control_count)) =
        expand_nurbs_knots_limited(ctx, degree, &knots, &multiplicities, max_control_count)?
    else {
        return Ok(None);
    };
    let control_count_u64 = u64_from_index(control_count);
    if view.counted(control_count_u64, 16).is_none() {
        return Ok(None);
    }
    let Some(bytes) = control_count_u64.checked_mul(16) else {
        return Err(ctx.refuse_codec_limit("catia_e5_pcurve_controls", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_pcurve_controls")?;
    let mut control_points = Vec::new();
    ctx.reserve_vec(
        &mut control_points,
        control_count,
        "catia_e5_pcurve_controls",
    )?;
    for _ in 0..control_count {
        let Some(point) = (|| Some([finite_f64_le(&mut view)?, finite_f64_le(&mut view)?]))()
        else {
            return Ok(None);
        };
        control_points.push(point);
    }
    if view.remaining() != E5_NURBS_PCURVE_TAIL_BYTES {
        return Ok(None);
    }
    let Some((arcs, locals, coefficient, shift, extrapolation)) = (|| {
        Some((
            [view.u16_le()?, view.u16_le()?],
            [view.f64_le()?, view.f64_le()?],
            view.f64_le()?,
            view.f64_le()?,
            view.u8()?,
        ))
    })() else {
        return Ok(None);
    };
    if coefficient != 1.0 || shift != 0.0 || extrapolation != 0 {
        return Ok(None);
    }
    let Some(range) = super::parameters::current_limits(arcs, locals, |arc| {
        Some([knots.get(arc)?.get(), knots.get(arc.checked_add(1)?)?.get()])
    }) else {
        return Ok(None);
    };
    let Some(degree_index) = usize::try_from(degree).ok() else {
        return Ok(None);
    };
    let domain_start = expanded_knots[degree_index].get();
    let domain_end = expanded_knots[control_count].get();
    let tolerance = EPS_PARAMETER_ENDPOINT * (1.0 + (domain_end - domain_start).abs());
    if domain_start >= domain_end
        || !tolerance.is_finite()
        || range[0].get() < domain_start - tolerance
        || range[1].get() > domain_end + tolerance
    {
        return Ok(None);
    }
    Ok(view.is_empty().then_some(E5Pcurve::Nurbs {
        surface,
        degree,
        knots: expanded_knots,
        control_points,
        range,
    }))
}

fn expand_nurbs_knots_limited(
    ctx: &DecodeContext<'_>,
    degree: u32,
    knots: &[FiniteReal],
    multiplicities: &[u32],
    max_control_count: usize,
) -> Result<Option<(Vec<FiniteReal>, usize)>, CodecError> {
    ctx.charge_work(u64_from_index(knots.len()), "catia_e5_knot_order_scan")?;
    ctx.charge_work(
        u64_from_index(multiplicities.len()),
        "catia_e5_multiplicity_scan",
    )?;
    if knots.len() != multiplicities.len()
        || knots.is_empty()
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.contains(&0)
    {
        return Ok(None);
    }
    ctx.charge_work(
        u64_from_index(multiplicities.len()),
        "catia_e5_knot_expansion_scan",
    )?;
    let total = multiplicities
        .iter()
        .try_fold(0usize, |total, &multiplicity| {
            let repeats = usize::try_from(multiplicity).map_err(|_| {
                ctx.refuse_codec_limit("catia_e5_pcurve_expanded_knots", u64::MAX - 1, u64::MAX)
            })?;
            total.checked_add(repeats).ok_or_else(|| {
                ctx.refuse_codec_limit("catia_e5_pcurve_expanded_knots", u64::MAX - 1, u64::MAX)
            })
        })?;
    let Some((degree, control_count)) = usize::try_from(degree)
        .ok()
        .and_then(|degree| Some((degree, total.checked_sub(degree.checked_add(1)?)?)))
    else {
        return Ok(None);
    };
    if control_count <= degree || control_count > max_control_count {
        return Ok(None);
    }
    let Some(bytes) = total
        .checked_mul(size_of::<FiniteReal>())
        .map(u64_from_index)
    else {
        return Err(ctx.refuse_codec_limit("catia_e5_pcurve_expanded_knots", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_pcurve_expanded_knots")?;
    ctx.charge_work(u64_from_index(total), "catia_e5_knot_expansion_emit")?;
    let mut expanded = Vec::new();
    ctx.reserve_vec(&mut expanded, total, "catia_e5_pcurve_expanded_knots")?;
    for (knot, multiplicity) in knots.iter().zip(multiplicities) {
        let count = usize::try_from(*multiplicity).map_err(|_| {
            ctx.refuse_codec_limit("catia_e5_pcurve_expanded_knots", u64::MAX - 1, u64::MAX)
        })?;
        expanded.extend(std::iter::repeat_with(|| *knot).take(count));
    }
    Ok((expanded.len() == total).then_some((expanded, control_count)))
}

fn read_finite_lane(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    let count_u64 = u64_from_index(count);
    if view.counted(count_u64, 8).is_none() {
        return Ok(None);
    }
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, count, operation)?;
    for _ in 0..count {
        let Some(value) = finite_f64_le(view) else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok(Some(values))
}

fn read_u32_lane(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Option<Vec<u32>>, CodecError> {
    let count_u64 = u64_from_index(count);
    if view.counted(count_u64, 4).is_none() {
        return Ok(None);
    }
    let mut values = Vec::new();
    ctx.reserve_vec(&mut values, count, operation)?;
    for _ in 0..count {
        let Some(value) = view.u32_le() else {
            return Ok(None);
        };
        values.push(value);
    }
    Ok(Some(values))
}

fn parse_jet_pcurve(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
    position: usize,
    surface: u32,
) -> Result<Option<E5Pcurve>, CodecError> {
    let mut view = View::over_retained(payload);
    if view.seek(position).is_none() {
        return Ok(None);
    }
    let Some((degree, zero0, zero1, site_count, zero2, zero3, zero4)) = (|| {
        Some((
            view.u32_le()?,
            view.u32_le()?,
            view.u32_le()?,
            usize::try_from(view.u32_le()?).ok()?,
            view.u32_le()?,
            view.u32_le()?,
            view.u32_le()?,
        ))
    })() else {
        return Ok(None);
    };
    if degree != 5 || site_count == 0 || [zero0, zero1, zero2, zero3, zero4] != [0; 5] {
        return Ok(None);
    }
    let Some(knot_tail_count) = site_count.checked_sub(1) else {
        return Ok(None);
    };
    let Some(knot_tail) = read_finite_lane(ctx, &mut view, knot_tail_count, "catia_e5_jet_knots")?
    else {
        return Ok(None);
    };
    let mut knots = Vec::new();
    ctx.reserve_vec(&mut knots, site_count, "catia_e5_jet_knots")?;
    knots.push(FiniteReal::ZERO);
    knots.extend(knot_tail);
    let Some(multiplicities) =
        read_u32_lane(ctx, &mut view, site_count, "catia_e5_jet_multiplicities")?
    else {
        return Ok(None);
    };
    if view.u32_le().and_then(|count| usize::try_from(count).ok()) != Some(site_count) {
        return Ok(None);
    }
    let Some(x) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_x")? else {
        return Ok(None);
    };
    let Some(y) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_y")? else {
        return Ok(None);
    };
    let Some(dx) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_dx")? else {
        return Ok(None);
    };
    let Some(dy) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_dy")? else {
        return Ok(None);
    };
    if view.u16_le() != Some(1) {
        return Ok(None);
    }
    let Some(ddx) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_ddx")? else {
        return Ok(None);
    };
    let Some(ddy) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_ddy")? else {
        return Ok(None);
    };
    let Some(range_values) = (|| Some([finite_f64_le(&mut view)?, finite_f64_le(&mut view)?]))()
    else {
        return Ok(None);
    };
    let Some(final_knot) = knots.last().map(|knot| knot.get()) else {
        return Ok(None);
    };
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
        return Ok(None);
    }
    let Some(bytes) = site_count
        .checked_mul(size_of::<E5PcurveJetSite>())
        .map(u64_from_index)
    else {
        return Err(ctx.refuse_codec_limit("catia_e5_jet_sites", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_jet_sites")?;
    let mut sites = Vec::new();
    ctx.reserve_vec(&mut sites, site_count, "catia_e5_jet_sites")?;
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
    Ok(Some(E5Pcurve::Jet {
        surface,
        sites,
        range: [range_values[0], range_values[1]],
    }))
}

/// Apply explicit root-context loop signs and verify every paired edge use.
fn resolve_absolute_orientation(
    ctx: &DecodeContext<'_>,
    faces: &mut [E5Face],
) -> Result<bool, CodecError> {
    let mut occurrences = HashMap::<u32, Vec<Sign>>::new();
    for face in faces.iter() {
        for loop_ in &face.loops {
            for member in &loop_.members {
                let sign = if member.reversed {
                    loop_.absolute_orientation.flipped()
                } else {
                    loop_.absolute_orientation
                };
                ctx.admit_hash_map_entry(
                    &mut occurrences,
                    &member.edge_use,
                    "catia e5 orientation edge occurrence keys",
                )?;
                ctx.push_vec(
                    occurrences.entry(member.edge_use).or_default(),
                    sign,
                    "catia e5 orientation edge occurrences",
                )?;
            }
        }
    }
    if occurrences
        .values()
        .any(|uses| uses.len() == 2 && uses[0] == uses[1])
    {
        return Ok(false);
    }
    for face in faces {
        for loop_ in &mut face.loops {
            let flip = loop_.absolute_orientation == Sign::Negative;
            let mut oriented = Vec::new();
            ctx.reserve_vec(
                &mut oriented,
                loop_.members.len(),
                "catia e5 oriented members",
            )?;
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
    Ok(true)
}

fn parse_bodies(
    ctx: &DecodeContext<'_>,
    records: &[Record<'_>],
    by_id: &HashMap<u32, &Record<'_>>,
) -> Result<Option<Vec<E5Body>>, CodecError> {
    let mut bodies = Vec::new();
    for record in records.iter().filter(|record| record.class == 0x01) {
        if record.payload.first() != Some(&0x81) {
            return Ok(None);
        }
        let mut position = 1;
        let Some(root_id) = wire::tokens::object_ref(record.payload, &mut position, false) else {
            return Ok(None);
        };
        if position != record.payload.len() {
            return Ok(None);
        }
        let Some(root) = by_id.get(&root_id) else {
            return Ok(None);
        };
        if root.class != 0x08 {
            return Ok(None);
        }
        let Some(faces) = parse_body_root(ctx, root.payload)? else {
            return Ok(None);
        };
        if faces.iter().any(|face| {
            by_id
                .get(&face.record_id)
                .is_none_or(|target| target.class != 0x00)
        }) {
            return Ok(None);
        }
        ctx.push_vec(
            &mut bodies,
            E5Body {
                record_id: record.id,
                faces,
            },
            "catia_e5_bodies",
        )?;
    }
    Ok((!bodies.is_empty()).then_some(bodies))
}

fn parse_body_root(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<Vec<E5BodyFace>>, CodecError> {
    let (count, mut position, narrow) = if payload.first() == Some(&0x08) {
        let Some(count) = payload.get(1) else {
            return Ok(None);
        };
        (usize::from(*count), 2, true)
    } else {
        let Some(count) = payload.first().and_then(|lead| lead.checked_sub(0x80)) else {
            return Ok(None);
        };
        (usize::from(count), 1, false)
    };
    let mut faces = Vec::new();
    for _ in 0..count {
        let Some(face) = wire::tokens::object_ref(payload, &mut position, false) else {
            return Ok(None);
        };
        if narrow && face > u32::from(u16::MAX) {
            return Ok(None);
        }
        ctx.push_vec(&mut faces, face, "catia_e5_body_root_faces")?;
    }
    let Some(count) = u8::try_from(faces.len()).ok() else {
        return Ok(None);
    };
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
        return Ok(None);
    }
    let Some(sign_bytes) = payload.get(position..) else {
        return Ok(None);
    };
    if sign_bytes.len() != (faces.len() + 2) * 2 {
        return Ok(None);
    }
    if View::i16_le_at(sign_bytes, 0) != Some(1)
        || View::i16_le_at(sign_bytes, 2 * (faces.len() + 1)) != Some(-1)
    {
        return Ok(None);
    }
    let mut occurrences = Vec::new();
    for (index, record_id) in faces.into_iter().enumerate() {
        let Some(sign) = View::i16_le_at(sign_bytes, 2 * (index + 1)).and_then(Sign::from_i16)
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut occurrences,
            E5BodyFace {
                record_id,
                orientation: sign.flipped(),
            },
            "catia_e5_body_root_occurrences",
        )?;
    }
    Ok(Some(occurrences))
}

fn records(bytes: &[u8]) -> impl Iterator<Item = Record<'_>> + '_ {
    let mut position = 0;
    std::iter::from_fn(move || loop {
        if position + 13 > bytes.len() {
            return None;
        }
        let relative = bytes[position..]
            .windows(3)
            .position(|value| value == [0xe5, 0x0d, 0x03])?;
        let start = position + relative;
        let id = View::u32_le_at(bytes, start + 9)?;
        let size = View::u16_le_at(bytes, start + 5).map(usize::from)?;
        let end = start.checked_add(13 + size)?;
        if end > bytes.len() {
            position = start + 1;
            continue;
        }
        position = end;
        return Some(Record {
            class: bytes[start + 3],
            id,
            payload: &bytes[start + 13..end],
        });
    })
}

fn parse_face(ctx: &DecodeContext<'_>, record: &Record<'_>) -> Result<Option<RawFace>, CodecError> {
    let Some(count) = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x81))
        .map(usize::from)
    else {
        return Ok(None);
    };
    if count == 0 {
        return Ok(None);
    }
    let mut position = 1;
    let Some(surface) = wire::tokens::object_ref(record.payload, &mut position, false) else {
        return Ok(None);
    };
    let mut loops = Vec::new();
    for _ in 0..count {
        let Some(loop_id) = wire::tokens::object_ref(record.payload, &mut position, false) else {
            return Ok(None);
        };
        ctx.push_vec(&mut loops, loop_id, "catia_e5_face_loop_ids")?;
    }
    let Some(trailer_sign) = View::i16_le_at(record.payload, position).and_then(Sign::from_i16)
    else {
        return Ok(None);
    };
    if position + 2 != record.payload.len() {
        return Ok(None);
    }
    Ok(Some(RawFace {
        id: record.id,
        surface,
        loops,
        trailer_sign,
    }))
}

fn parse_loop<'a>(
    ctx: &DecodeContext<'_>,
    record: &Record<'a>,
) -> Result<Option<RawLoop<'a>>, CodecError> {
    let Some(member_count) = record
        .payload
        .first()
        .and_then(|lead| lead.checked_sub(0x81))
        .map(usize::from)
    else {
        return Ok(None);
    };
    if member_count == 0 || member_count % 2 != 0 {
        return Ok(None);
    }
    let mut position = 1;
    let mut pcurves = Vec::new();
    let mut edges = Vec::new();
    for _ in 0..member_count / 2 {
        let Some(pcurve) = wire::tokens::object_ref(record.payload, &mut position, false) else {
            return Ok(None);
        };
        let Some(edge) = wire::tokens::object_ref(record.payload, &mut position, false) else {
            return Ok(None);
        };
        ctx.push_vec(&mut pcurves, pcurve, "catia_e5_loop_pcurves")?;
        ctx.push_vec(&mut edges, edge, "catia_e5_loop_edges")?;
    }
    let Some(surface) = wire::tokens::object_ref(record.payload, &mut position, false) else {
        return Ok(None);
    };
    let Some(outer) = record
        .payload
        .get(position..)
        .and_then(|tail| parse_loop_signs(tail, member_count / 2).ok())
    else {
        return Ok(None);
    };
    Ok(Some(RawLoop {
        id: record.id,
        surface,
        pcurves,
        edges,
        outer,
        trailer: &record.payload[position..],
    }))
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

fn solve_loop_chain(
    ctx: &DecodeContext<'_>,
    edge_ids: &[u32],
    edges: &BTreeMap<u32, E5Edge>,
) -> Result<Option<Vec<bool>>, CodecError> {
    let Some(first) = edge_ids.first().and_then(|id| edges.get(id)) else {
        return Ok(None);
    };
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
        ctx.push_vec(&mut senses, first_reversed, "catia_e5_chain_senses")?;
        let mut valid = true;
        for edge_id in &edge_ids[1..] {
            let Some(edge) = edges.get(edge_id) else {
                return Ok(None);
            };
            match (edge.start_vertex == current, edge.end_vertex == current) {
                (true, false) => {
                    ctx.push_vec(&mut senses, false, "catia_e5_chain_senses")?;
                    current = edge.end_vertex;
                }
                (false, true) => {
                    ctx.push_vec(&mut senses, true, "catia_e5_chain_senses")?;
                    current = edge.start_vertex;
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if valid && current == initial {
            ctx.push_vec(&mut solutions, senses, "catia_e5_chain_solutions")?;
        }
    }
    if solutions.len() == 1 {
        return Ok(solutions.pop());
    }
    if edge_ids.len() == 2
        && solutions.len() == 2
        && solutions[0]
            .iter()
            .zip(&solutions[1])
            .all(|(left, right)| left != right)
    {
        return Ok(solutions.into_iter().find(|solution| !solution[0]));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::{
        curve_support_reference_closes, parse_body_root, parse_jet_pcurve, parse_nurbs_pcurve,
        parse_pcurve, parse_topology, records, resolve_absolute_orientation, solve_loop_chain,
        E5BoundEntry, E5Bounds, E5CurveSupport, E5CurveSupportKind, E5Edge, E5Face, E5Loop,
        E5LoopMember, E5Pcurve, E5PcurveJetSite, E5Topology, Record, Sign,
    };
    use crate::families::e5::tests::e5_loop_members;
    use crate::test_support::test_b5::{finite, finite_lane, finite_pair};
    use crate::test_support::test_e5::append_e5_record;
    use std::collections::BTreeMap;

    macro_rules! e5_test_context {
        ($ctx:ident) => {
            let arena = cadmpeg_core::decode::DecodeArena::new();
            let policy = cadmpeg_core::decode::DecodePolicy::service();
            let ($ctx, _) =
                cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
                    .expect("service decode context");
        };
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
        append_e5_record(&mut bytes, 0xc1, 201, &wrapper_payload);
        let mut support_payload = vec![0x82, 0x08, 201, 0x08, 101, 0x81, 0, 0];
        support_payload.extend_from_slice(&0.0_f64.to_le_bytes());
        support_payload.extend_from_slice(&1.0_f64.to_le_bytes());
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
            parse_topology(&ctx, &bytes)
                .expect("service decode")
                .is_none(),
            "a face-local relation supplies no absolute body traversal without its root"
        );
        append_e5_record(
            &mut bytes,
            0x08,
            70,
            &[0x81, 0xbc, 0x81, 1, 0, 255, 255, 255, 255],
        );
        append_e5_record(&mut bytes, 0x01, 71, &[0x81, 0xc6]);

        let topology = parse_topology(&ctx, &bytes)
            .expect("service decode")
            .expect("closed E5 topology");
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
        let flipped = parse_topology(&ctx, &flipped)
            .expect("service decode")
            .expect("flipped root relation");
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
            assert!(parse_topology(&ctx, &unknown)
                .expect("service decode")
                .is_none());
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
        let support = |pcurves: [u32; 2]| E5CurveSupport {
            kind: E5CurveSupportKind::Intersection(pcurves),
            mode: 0,
            range: finite_pair([0.0, 1.0]),
            tail: Vec::new(),
        };
        let supports = BTreeMap::from([(1, support([2, 3])), (2, support([1, 3]))]);
        assert!(
            !crate::test_support::with_service_context(|ctx| curve_support_reference_closes(
                ctx, 1, &pcurves, &supports
            ))
            .expect("service resource budget")
        );
    }

    #[test]
    fn e5_support_walk_refuses_before_stack_and_set_growth() {
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
                kind: E5CurveSupportKind::Intersection([3, 3]),
                mode: 0,
                range: finite_pair([0.0, 1.0]),
                tail: Vec::new(),
            },
        )]);
        assert!(
            crate::test_support::with_service_context(|ctx| curve_support_reference_closes(
                ctx, 1, &pcurves, &supports
            ))
            .expect("service resource budget")
        );
        for (cap, operation) in [
            (0, "catia_e5_support_stack"),
            (1, "catia_e5_support_visiting"),
        ] {
            assert!(matches!(
                crate::test_support::with_collection_limit(cap, |ctx| curve_support_reference_closes(ctx, 1, &pcurves, &supports)),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation
            ));
        }
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
        assert!(
            crate::test_support::with_service_context(|ctx| parse_nurbs_pcurve(
                ctx, &truncated, 2, 7
            ))
            .expect("service resource budget")
            .is_none()
        );

        let E5Pcurve::Nurbs {
            surface,
            degree,
            knots,
            control_points,
            range,
        } = crate::test_support::with_service_context(|ctx| {
            parse_nurbs_pcurve(ctx, &payload, 2, 7)
        })
        .expect("service resource budget")
        .expect("AA NURBS pcurve")
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
            crate::test_support::with_service_context(|ctx| parse_nurbs_pcurve(ctx, &inset, 2, 7))
                .expect("service resource budget")
                .expect("inset current limits");
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
            assert!(crate::test_support::with_service_context(|ctx| {
                parse_nurbs_pcurve(ctx, &invalid, 2, 7)
            })
            .expect("service resource budget")
            .is_none());
        }
    }

    #[test]
    fn aa_nurbs_pcurve_refuses_before_each_counted_lane() {
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
        for (cap, operation) in [
            (0, "catia_e5_pcurve_knots"),
            (2, "catia_e5_pcurve_multiplicities"),
            (4, "catia_e5_pcurve_expanded_knots"),
            (8, "catia_e5_pcurve_controls"),
        ] {
            assert!(matches!(
                crate::test_support::with_collection_limit(cap, |ctx| parse_nurbs_pcurve(ctx, &payload, 2, 7)),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation
            ));
        }
        assert!(matches!(
            crate::test_support::with_service_context(|ctx| parse_nurbs_pcurve(
                ctx, &payload, 2, 7
            )),
            Ok(Some(E5Pcurve::Nurbs { .. }))
        ));
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
            crate::test_support::with_service_context(|ctx| {
                parse_pcurve(
                    ctx,
                    &Record {
                        class: 0x97,
                        id: 20,
                        payload: &payload,
                    },
                )
            })
            .expect("service resource budget")
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
        let faces = crate::test_support::with_service_context(|ctx| parse_body_root(ctx, &payload))
            .expect("service resource budget")
            .expect("widened body root");
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
        assert!(crate::test_support::with_service_context(|ctx| {
            parse_body_root(ctx, &mixed_frame)
        })
        .expect("service resource budget")
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
            crate::test_support::with_service_context(|ctx| parse_jet_pcurve(ctx, &payload, 0, 7))
                .expect("service resource budget"),
            Some(E5Pcurve::Jet { range, .. }) if range == finite_pair([0.0, final_knot])
        ));

        let mut nonzero_lower = payload.clone();
        let lower_offset = nonzero_lower.len() - 16;
        nonzero_lower[lower_offset..lower_offset + 8]
            .copy_from_slice(&(0.5 * final_knot).to_le_bytes());
        assert!(
            crate::test_support::with_service_context(|ctx| parse_jet_pcurve(
                ctx,
                &nonzero_lower,
                0,
                7
            ))
            .expect("service resource budget")
            .is_none()
        );

        let mut wrong_upper = payload.clone();
        let upper_offset = wrong_upper.len() - 8;
        wrong_upper[upper_offset..].copy_from_slice(&(2.0 * final_knot).to_le_bytes());
        assert!(
            crate::test_support::with_service_context(|ctx| parse_jet_pcurve(
                ctx,
                &wrong_upper,
                0,
                7
            ))
            .expect("service resource budget")
            .is_none()
        );

        let mut nonfinite_knot = payload;
        nonfinite_knot[28..36].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(
            crate::test_support::with_service_context(|ctx| parse_jet_pcurve(
                ctx,
                &nonfinite_knot,
                0,
                7
            ))
            .expect("service resource budget")
            .is_none()
        );
    }

    #[test]
    fn jet_pcurve_refuses_before_each_counted_lane() {
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
        for (cap, operation) in [
            (0, "catia_e5_jet_knots"),
            (3, "catia_e5_jet_multiplicities"),
            (5, "catia_e5_jet_x"),
            (7, "catia_e5_jet_y"),
            (9, "catia_e5_jet_dx"),
            (11, "catia_e5_jet_dy"),
            (13, "catia_e5_jet_ddx"),
            (15, "catia_e5_jet_ddy"),
            (17, "catia_e5_jet_sites"),
        ] {
            assert!(matches!(
                crate::test_support::with_collection_limit(cap, |ctx| parse_jet_pcurve(ctx, &payload, 0, 7)),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation
            ));
        }
        assert!(matches!(
            crate::test_support::with_service_context(|ctx| parse_jet_pcurve(ctx, &payload, 0, 7)),
            Ok(Some(E5Pcurve::Jet { .. }))
        ));
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
        assert_eq!(
            crate::test_support::with_service_context(|ctx| solve_loop_chain(ctx, &[1, 2], &edges))
                .expect("service resource budget"),
            Some(vec![false, false])
        );
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
            topology
                .edge_representation_parameters(&ctx, 1, 20)
                .expect("bound budget"),
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
        assert_eq!(
            topology
                .edge_representation_parameters(&ctx, 1, 20)
                .expect("bound budget"),
            None
        );
        let end = topology.bounds.get_mut(&11).expect("end bound");
        end.entries.truncate(1);
        end.entries[0].code = 0;
        assert_eq!(
            topology
                .edge_representation_parameters(&ctx, 1, 20)
                .expect("bound budget"),
            None
        );
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
            let error = crate::test_support::with_work_limit(0, |ctx| {
                super::charge_bound_arc_scan(ctx, &pcurves, 20)
            })
            .expect_err("expanded knot scan requires work");
            assert!(
                matches!(error, cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.operation == "catia_e5_bound_arc_scan")
            );
            assert!(crate::test_support::with_work_limit(5, |ctx| {
                super::charge_bound_arc_scan(ctx, &pcurves, 20)
            })
            .is_ok());
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

        assert!(!resolve_absolute_orientation(&ctx, &mut faces).expect("service decode"));
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
        assert!(!resolve_absolute_orientation(&ctx, &mut faces).expect("equal paired signs reject"));
        faces[1].loops[0].absolute_orientation = Sign::Negative;
        assert!(resolve_absolute_orientation(&ctx, &mut faces).expect("service decode"));
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
        assert!(parse_topology(&ctx, &bytes)
            .expect("service decode")
            .is_none());
    }

    #[test]
    fn e5_topology_collections_refuse_before_growth() {
        let bytes = crate::test_support::test_e5::e5_torus_topology_stream();
        assert!(
            crate::test_support::with_service_context(|ctx| parse_topology(ctx, &bytes))
                .expect("service resource budget")
                .is_some()
        );
        let mut operations = std::collections::HashSet::new();
        for cap in 0..256 {
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
                crate::test_support::with_collection_limit(cap, |ctx| parse_topology(ctx, &bytes))
            {
                operations.insert(limit.operation);
            }
        }
        for operation in [
            "catia_e5_graph_records",
            "catia_e5_records_by_id",
            "catia_e5_topology_edges",
            "catia_e5_topology_pcurves",
            "catia_e5_topology_bounds",
            "catia_e5_bound_references",
            "catia_e5_bound_entries",
            "catia_e5_curve_supports",
            "catia_e5_raw_loops",
            "catia_e5_raw_faces",
            "catia_e5_vertex_ids",
            "catia_e5_face_loop_ids",
            "catia_e5_loop_pcurves",
            "catia_e5_loop_edges",
            "catia_e5_chain_senses",
            "catia_e5_chain_solutions",
            "catia_e5_reachable_edges",
            "catia_e5_loop_members",
            "catia_e5_resolved_loops",
            "catia_e5_topology_faces",
            "catia_e5_vertex_refs",
            "catia_e5_body_root_faces",
            "catia_e5_bodies",
            "catia_e5_body_face_roster",
            "catia_e5_body_face_set",
            "catia_e5_topology_face_set",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
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
    fn e5_curve_support_tail_refuses_before_copy() {
        let mut payload = vec![0x81, 0x81, 0x81, 0, 0];
        payload.extend_from_slice(&0.0_f64.to_le_bytes());
        payload.extend_from_slice(&1.0_f64.to_le_bytes());
        payload.push(0xaa);
        let record = Record {
            class: 0xc0,
            id: 1,
            payload: &payload,
        };
        assert_eq!(
            crate::test_support::with_service_context(|ctx| super::parse_curve_support(
                ctx, &record
            ))
            .expect("service resource budget")
            .expect("valid curve support")
            .tail,
            [0xaa]
        );
        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| super::parse_curve_support(ctx, &record)),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_e5_curve_support_tail"
        ));
    }

    #[test]
    fn e5_orientation_refuses_before_occurrence_collection() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;

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
        assert!(
            resolve_absolute_orientation(&service_ctx, &mut faces.clone())
                .expect("orientation fits the service profile")
        );

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let error = resolve_absolute_orientation(&ctx, &mut faces.clone())
            .expect_err("one orientation location exceeds the collection limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia e5 orientation edge occurrence keys"));
    }

    #[test]
    fn e5_orientation_charges_occurrence_and_member_collections() {
        use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
        use cadmpeg_core::CodecError;
        use std::collections::BTreeSet;

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
        let mut operations = BTreeSet::new();
        let mut completed = false;
        for limit in 0..=16 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_collection_items = limit;
            let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
                .expect("fixture fits the input limit");
            match resolve_absolute_orientation(&ctx, &mut faces.clone()) {
                Err(CodecError::ResourceLimit(error)) => {
                    assert_eq!(error.dimension, ResourceDimension::CollectionItems);
                    operations.insert(error.operation);
                }
                Ok(true) => {
                    completed = true;
                    break;
                }
                Ok(false) => panic!("orientation fixture must solve"),
                Err(error) => panic!("unexpected orientation refusal: {error}"),
            }
        }
        assert!(
            completed,
            "collection limit 16 must admit the orientation fixture"
        );
        assert!(operations.contains("catia e5 orientation edge occurrence keys"));
        assert!(operations.contains("catia e5 orientation edge occurrences"));
        assert!(operations.contains("catia e5 oriented members"));
    }
}

#[cfg(test)]
mod knot_work_tests {
    #[test]
    fn e5_knot_expansion_refuses_scan_and_emission_work() {
        let knots = crate::test_support::test_b5::finite_lane(&[0.0, 1.0]);
        for (cap, operation) in [
            (0, "catia_e5_knot_order_scan"),
            (2, "catia_e5_multiplicity_scan"),
            (4, "catia_e5_knot_expansion_scan"),
            (9, "catia_e5_knot_expansion_emit"),
        ] {
            let result = crate::test_support::with_work_limit(cap, |ctx| {
                super::expand_nurbs_knots_limited(ctx, 1, &knots, &[2, 2], 2)
            });
            assert!(
                matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == cadmpeg_core::decode::ResourceDimension::WorkUnits
                    && limit.operation == operation)
            );
        }
        assert_eq!(
            crate::test_support::with_work_limit(10, |ctx| {
                super::expand_nurbs_knots_limited(ctx, 1, &knots, &[2, 2], 2)
            })
            .expect("scan and emission work"),
            Some((
                crate::test_support::test_b5::finite_lane(&[0.0, 0.0, 1.0, 1.0]),
                2
            ))
        );
    }
}
