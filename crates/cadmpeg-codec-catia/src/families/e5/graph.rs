// SPDX-License-Identifier: Apache-2.0
//! Native topology records in the E5 `0D 03` stream family.

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
    /// orientation-sign tapes. Empty when the stream carries no `0x01`
    /// records (bodies are optional; face/loop/edge resolution does not
    /// require them).
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
    #[must_use]
    pub(super) fn edge_representation_parameters(
        &self,
        edge_ref: u32,
        representation: u32,
    ) -> Option<[FiniteReal; 2]> {
        let edge = self.edges.get(&edge_ref)?;
        [edge.parameter_start, edge.parameter_end]
            .map(|bound_ref| {
                bound_representation_parameter(&self.bounds, bound_ref, representation)
            })
            .into_iter()
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()
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
    /// Raw trailing `u32` code following the parameter; meaning not decoded
    /// further.
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
        /// Effective parameter domain of the expanded knot vector.
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
    pub(super) faces: Vec<u32>,
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
/// root sign-tape entry ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct E5Face {
    /// This face record's `record_id`.
    pub(super) record_id: u32,
    /// `record_id` of the face's surface carrier.
    pub(super) surface: u32,
    /// This face's entry in the class-`0x08` root sign tape (`+1` or
    /// `-1`), used by [`solve_absolute_orientation`] to fix each loop's
    /// global sense.
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
    /// Loop role bit from the trailing sign tape: `Some(true)` =
    /// `FACE_OUTER_BOUND`, `Some(false)` = `FACE_BOUND`, `None` when the
    /// loop carries no trailing role tape.
    pub(super) outer: Option<bool>,
    /// Exact global-sense anchor for a closed plane-cap split circle. This is
    /// present only when the two-edge loop has a complete role sign, two
    /// complementary intersection-support ranges, and occurrence parameter
    /// directions that determine one native-UV winding. Other loops use the
    /// shared-edge parity component anchor.
    pub(super) orientation_hint: Option<Sign>,
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
/// <param_end>`).
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
    /// Bytes following the five counted fields.
    pub(super) tail: Vec<u8>,
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
struct RawLoop {
    id: u32,
    surface: u32,
    pcurves: Vec<u32>,
    edges: Vec<u32>,
    outer: Option<bool>,
}

/// Resolve E5 face→loop→edge-use references and determine each serialized
/// loop occurrence's unique head-to-tail traversal from stored vertex refs.
pub(crate) fn parse_topology(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Option<E5Topology>, CodecError> {
    let mut admitted_records = Vec::new();
    for record in records(bytes) {
        crate::resource::push(ctx, &mut admitted_records, record, "catia_e5_graph_records")?;
    }
    (|| -> Option<Result<E5Topology, CodecError>> {
        let records = admitted_records;
        let mut by_id = HashMap::new();
        for record in &records {
            if let Err(error) = crate::resource::insert_map(ctx, &mut by_id, record.id, record, "catia_e5_records_by_id") {
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
                let edge = parse_edge(record)?;
                if let Err(error) = ctx.charge_collection_items(1, "catia_e5_topology_edges") {
                    return Some(Err(error));
                }
                edges.insert(record.id, edge);
            }
            if matches!(record.class, 0x96 | 0x97 | 0xa0 | 0xaa) {
                let pcurve = match parse_pcurve(ctx, record) {
                    Ok(Some(pcurve)) => pcurve,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                if let Err(error) = ctx.charge_collection_items(1, "catia_e5_topology_pcurves") {
                    return Some(Err(error));
                }
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
                    let value = match parse_bounds(ctx, record) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) = ctx.charge_collection_items(1, "catia_e5_topology_bounds") {
                        return Some(Err(error));
                    }
                    bounds.insert(record.id, value);
                }
                0xc0 | 0xc1 => {
                    let value = match parse_curve_support(ctx, record) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) = ctx.charge_collection_items(1, "catia_e5_curve_supports") {
                        return Some(Err(error));
                    }
                    curve_supports.insert(record.id, value);
                }
                0x09 => {
                    let value = match parse_loop(ctx, record) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) = crate::resource::insert_map(ctx, &mut loops, record.id, value, "catia_e5_raw_loops") {
                        return Some(Err(error));
                    }
                }
                0x00 => {
                    let value = match parse_face(ctx, record) {
                        Ok(Some(value)) => value,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                    if let Err(error) = crate::resource::push(ctx, &mut raw_faces, value, "catia_e5_raw_faces") {
                        return Some(Err(error));
                    }
                }
                0xfe => {
                    if let Err(error) = crate::resource::insert_set(ctx, &mut vertex_ids, record.id, "catia_e5_vertex_ids") {
                        return Some(Err(error));
                    }
                }
                _ => {}
            }
        }
        if raw_faces.is_empty() || loops.is_empty() || edges.is_empty() || vertex_ids.is_empty() {
            return None;
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
                if raw.outer.is_some_and(|outer| outer != (loop_position == 0)) {
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
                                bound_representation_parameter(&bounds, *bound_ref, *pcurve_id)
                                    .is_none()
                            })
                    {
                        return None;
                    }
                    let support = curve_supports.get(&edge.support)?;
                    for reference in support.pcurves() {
                        match curve_support_reference_closes(ctx, *reference, &pcurves, &curve_supports) {
                            Ok(true) => {}
                            Ok(false) => return None,
                            Err(error) => return Some(Err(error)),
                        }
                    }
                    if let Err(error) = crate::resource::insert_set(ctx, &mut reachable_edges, *edge_id, "catia_e5_reachable_edges") {
                        return Some(Err(error));
                    }
                }
                let orientation_hint = plane_digon_orientation_hint(
                    face.trailer_sign,
                    by_id.get(&face.surface).map(|record| record.class),
                    &raw.pcurves,
                    &raw.edges,
                    &reversed,
                    raw.outer,
                    &edges,
                    &pcurves,
                    &curve_supports,
                    &bounds,
                );
                let mut members = Vec::new();
                for ((&pcurve, &edge_use), &reversed) in raw.pcurves.iter().zip(&raw.edges).zip(&reversed) {
                    if let Err(error) = crate::resource::push(ctx, &mut members, E5LoopMember { pcurve, edge_use, reversed }, "catia_e5_loop_members") {
                        return Some(Err(error));
                    }
                }
                if let Err(error) = crate::resource::push(ctx, &mut resolved_loops, E5Loop {
                    record_id: raw.id,
                    surface: raw.surface,
                    members,
                    oriented_members: None,
                    outer: raw.outer,
                    orientation_hint,
                }, "catia_e5_resolved_loops") {
                    return Some(Err(error));
                }
            }
            if let Err(error) = crate::resource::push(ctx, &mut faces, E5Face {
                record_id: face.id,
                surface: face.surface,
                trailer_sign: face.trailer_sign,
                loops: resolved_loops,
            }, "catia_e5_topology_faces") {
                return Some(Err(error));
            }
        }
        let oriented = match solve_absolute_orientation(ctx, &mut faces) {
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
                if let Err(error) = crate::resource::push(ctx, &mut vertex_refs, vertex, "catia_e5_vertex_refs") {
                    return Some(Err(error));
                }
            }
        }
        vertex_refs.sort_unstable();
        vertex_refs.dedup();
        let bodies = match parse_bodies(ctx, &records, &by_id) {
            Ok(Some(bodies)) => bodies,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        if !bodies.is_empty() {
            let mut roster = Vec::new();
            for body in &bodies {
                for &face in &body.faces {
                    if let Err(error) = crate::resource::push(ctx, &mut roster, face, "catia_e5_body_face_roster") {
                        return Some(Err(error));
                    }
                }
            }
            let mut roster_set = HashSet::new();
            for &face in &roster {
                if let Err(error) = crate::resource::insert_set(ctx, &mut roster_set, face, "catia_e5_body_face_set") {
                    return Some(Err(error));
                }
            }
            let mut face_set = HashSet::new();
            for face in &faces {
                if let Err(error) = crate::resource::insert_set(ctx, &mut face_set, face.record_id, "catia_e5_topology_face_set") {
                    return Some(Err(error));
                }
            }
            if roster.len() != roster_set.len() || roster_set != face_set {
                return None;
            }
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
#[must_use]
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
    crate::resource::push(ctx, &mut stack, (reference, false), "catia_e5_support_stack")?;
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
        if !crate::resource::insert_set(ctx, &mut visiting, reference, "catia_e5_support_visiting")? {
            return Ok(false);
        }
        crate::resource::push(ctx, &mut stack, (reference, true), "catia_e5_support_stack")?;
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
            crate::resource::push(ctx, &mut stack, (*child, false), "catia_e5_support_stack")?;
        }
    }
    Ok(true)
}

fn bound_representation_parameter(
    bounds: &BTreeMap<u32, E5Bounds>,
    bound_ref: u32,
    representation: u32,
) -> Option<FiniteReal> {
    let bounds = bounds.get(&bound_ref)?;
    let mut entries = bounds
        .entries
        .iter()
        .filter(|entry| entry.representation == representation);
    let parameter = entries.next()?.parameter;
    entries.next().is_none().then_some(parameter)
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
        let Some(reference) = wire::tokens::object_ref(record.payload, &mut position, false) else { return Ok(None); };
        *pcurve = reference;
    }
    if record.payload.get(position) != Some(&0x81) {
        return Ok(None);
    }
    position += 1;
    let Some(&mode) = record.payload.get(position) else { return Ok(None); };
    position += 1;
    if record.payload.get(position) != Some(&0x00) {
        return Ok(None);
    }
    position += 1;
    let mut view = View::over_retained(record.payload);
    if view.seek(position).is_none() { return Ok(None); }
    let Some(range) = (|| Some([finite_f64_le(&mut view)?, finite_f64_le(&mut view)?]))() else { return Ok(None); };
    position = view.position();
    let Some(kind) = E5CurveSupportKind::from_parts(record.class == 0xc1, &pcurves[..usize::from(expected)]) else { return Ok(None); };
    let tail = crate::resource::copy_retained_slice(ctx, &record.payload[position..], "catia_e5_curve_support_tail")?;
    Ok(Some(E5CurveSupport {
        kind,
        mode,
        range,
        tail,
    }))
}

fn parse_bounds(ctx: &DecodeContext<'_>, record: &Record<'_>) -> Result<Option<E5Bounds>, CodecError> {
    let Some(count) = record.payload.first().and_then(|lead| lead.checked_sub(0x80)).map(usize::from) else { return Ok(None); };
    let mut position = 1;
    let mut representations = Vec::new();
    for _ in 0..count {
        let Some(reference) = wire::tokens::object_ref(record.payload, &mut position, false) else { return Ok(None); };
        crate::resource::push(ctx, &mut representations, reference, "catia_e5_bound_references")?;
    }
    let Some(expected_head) = u8::try_from(count).ok().and_then(|count| 0x80u8.checked_add(count)) else { return Ok(None); };
    if record.payload.get(position) != Some(&expected_head) {
        return Ok(None);
    }
    position += 1;
    let mut view = View::over_retained(record.payload);
    if view.seek(position).is_none() { return Ok(None); }
    let mut entries = Vec::new();
    for representation in representations {
        let Some((parameter, code)) = (|| Some((FiniteReal::new(view.f64_le()?)?, view.u32_le()?)))() else { return Ok(None); };
        crate::resource::push(ctx, &mut entries, E5BoundEntry {
            representation,
            parameter,
            code,
        }, "catia_e5_bound_entries")?;
    }
    Ok(view.is_empty().then_some(E5Bounds { entries }))
}

fn parse_pcurve(ctx: &DecodeContext<'_>, record: &Record<'_>) -> Result<Option<E5Pcurve>, CodecError> {
    if record.payload.first() != Some(&0x81) {
        return Ok(None);
    }
    let mut position = 1;
    let Some(surface) = wire::tokens::object_ref(record.payload, &mut position, false) else { return Ok(None); };
    let mut view = View::over_retained(record.payload);
    if view.seek(position).is_none() { return Ok(None); }
    match record.class {
        0x96 => {
            let Some(values) = (|| Some([
                finite_f64_le(&mut view)?, finite_f64_le(&mut view)?,
                finite_f64_le(&mut view)?, finite_f64_le(&mut view)?,
                finite_f64_le(&mut view)?, finite_f64_le(&mut view)?,
            ]))() else { return Ok(None); };
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
                    [finite_f64_le(&mut view)?, finite_f64_le(&mut view)?, finite_f64_le(&mut view)?, finite_f64_le(&mut view)?, finite_f64_le(&mut view)?],
                ))
            })() else { return Ok(None); };
            if !view.is_empty() {
                return Ok(None);
            }
            let Some(radius) = PositiveReal::new(values[0].get()) else { return Ok(None); };
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
        Some((view.u32_le()?, view.u32_le()?, view.u32_le()?, usize::try_from(view.u32_le()?).ok()?, view.u32_le()?))
    })() else { return Ok(None); };
    if degree == 0 || knot_count == 0 || [zero0, zero1, zero2] != [0; 3] {
        return Ok(None);
    }
    let Some(knot_count_u64) = u64::try_from(knot_count).ok() else { return Ok(None); };
    if view.counted(knot_count_u64, 12).is_none() { return Ok(None); }
    let mut knots = Vec::new();
    crate::resource::reserve_vec(ctx, &mut knots, knot_count, "catia_e5_pcurve_knots")?;
    for _ in 0..knot_count {
        let Some(knot) = finite_f64_le(&mut view) else { return Ok(None); };
        knots.push(knot);
    }
    let mut multiplicities = Vec::new();
    crate::resource::reserve_vec(ctx, &mut multiplicities, knot_count, "catia_e5_pcurve_multiplicities")?;
    for _ in 0..knot_count {
        let Some(multiplicity) = view.u32_le() else { return Ok(None); };
        multiplicities.push(multiplicity);
    }
    let Some(max_control_count) = view.remaining().checked_sub(E5_NURBS_PCURVE_TAIL_BYTES).map(|remaining| remaining / 16) else { return Ok(None); };
    let Some((expanded_knots, control_count)) = expand_nurbs_knots_limited(ctx, degree, &knots, &multiplicities, max_control_count)? else { return Ok(None); };
    let Some(control_count_u64) = u64::try_from(control_count).ok() else { return Ok(None); };
    if view.counted(control_count_u64, 16).is_none() { return Ok(None); }
    let Some(bytes) = control_count_u64.checked_mul(16) else {
        return Err(ctx.refuse_codec_limit("catia_e5_pcurve_controls", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_pcurve_controls")?;
    let mut control_points = Vec::new();
    crate::resource::reserve_vec(ctx, &mut control_points, control_count, "catia_e5_pcurve_controls")?;
    for _ in 0..control_count {
        let Some(point) = (|| Some([finite_f64_le(&mut view)?, finite_f64_le(&mut view)?]))() else { return Ok(None); };
        control_points.push(point);
    }
    if view.remaining() != E5_NURBS_PCURVE_TAIL_BYTES {
        return Ok(None);
    }
    let Some(range) = usize::try_from(degree).ok().and_then(|degree| Some([*expanded_knots.get(degree)?, *expanded_knots.get(control_count)?])) else { return Ok(None); };
    if range[0] >= range[1] {
        return Ok(None);
    }
    if view.skip(E5_NURBS_PCURVE_TAIL_BYTES).is_none() { return Ok(None); }
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
    if knots.len() != multiplicities.len()
        || knots.is_empty()
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.contains(&0)
    {
        return Ok(None);
    }
    let Some(total) = multiplicities
        .iter()
        .try_fold(0usize, |total, multiplicity| {
            total.checked_add(usize::try_from(*multiplicity).ok()?)
        }) else { return Ok(None); };
    let Some((degree, control_count)) = usize::try_from(degree).ok().and_then(|degree| Some((degree, total.checked_sub(degree.checked_add(1)?)?))) else { return Ok(None); };
    if control_count <= degree || control_count > max_control_count {
        return Ok(None);
    }
    let Some(bytes) = total.checked_mul(size_of::<FiniteReal>()).and_then(|bytes| u64::try_from(bytes).ok()) else {
        return Err(ctx.refuse_codec_limit("catia_e5_pcurve_expanded_knots", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_pcurve_expanded_knots")?;
    let mut expanded = Vec::new();
    crate::resource::reserve_vec(ctx, &mut expanded, total, "catia_e5_pcurve_expanded_knots")?;
    for (knot, multiplicity) in knots.iter().zip(multiplicities) {
        let Ok(count) = usize::try_from(*multiplicity) else { return Ok(None); };
        expanded.extend(std::iter::repeat_n(*knot, count));
    }
    Ok((expanded.len() == total).then_some((expanded, control_count)))
}

fn read_finite_lane(
    ctx: &DecodeContext<'_>,
    view: &mut View<'_>,
    count: usize,
    operation: &'static str,
) -> Result<Option<Vec<FiniteReal>>, CodecError> {
    let Some(count_u64) = u64::try_from(count).ok() else { return Ok(None); };
    if view.counted(count_u64, 8).is_none() { return Ok(None); }
    let mut values = Vec::new();
    crate::resource::reserve_vec(ctx, &mut values, count, operation)?;
    for _ in 0..count {
        let Some(value) = finite_f64_le(view) else { return Ok(None); };
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
    let Some(count_u64) = u64::try_from(count).ok() else { return Ok(None); };
    if view.counted(count_u64, 4).is_none() { return Ok(None); }
    let mut values = Vec::new();
    crate::resource::reserve_vec(ctx, &mut values, count, operation)?;
    for _ in 0..count {
        let Some(value) = view.u32_le() else { return Ok(None); };
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
    if view.seek(position).is_none() { return Ok(None); }
    let Some((degree, zero0, zero1, site_count, zero2, zero3, zero4)) = (|| {
        Some((view.u32_le()?, view.u32_le()?, view.u32_le()?, usize::try_from(view.u32_le()?).ok()?, view.u32_le()?, view.u32_le()?, view.u32_le()?))
    })() else { return Ok(None); };
    if degree != 5 || site_count == 0 || [zero0, zero1, zero2, zero3, zero4] != [0; 5] {
        return Ok(None);
    }
    let Some(knot_tail_count) = site_count.checked_sub(1) else { return Ok(None); };
    let Some(knot_tail) = read_finite_lane(ctx, &mut view, knot_tail_count, "catia_e5_jet_knots")? else { return Ok(None); };
    let mut knots = Vec::new();
    crate::resource::reserve_vec(ctx, &mut knots, site_count, "catia_e5_jet_knots")?;
    knots.push(FiniteReal::ZERO);
    knots.extend(knot_tail);
    let Some(multiplicities) = read_u32_lane(ctx, &mut view, site_count, "catia_e5_jet_multiplicities")? else { return Ok(None); };
    if view.u32_le().and_then(|count| usize::try_from(count).ok()) != Some(site_count) {
        return Ok(None);
    }
    let Some(x) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_x")? else { return Ok(None); };
    let Some(y) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_y")? else { return Ok(None); };
    let Some(dx) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_dx")? else { return Ok(None); };
    let Some(dy) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_dy")? else { return Ok(None); };
    if view.u16_le() != Some(1) {
        return Ok(None);
    }
    let Some(ddx) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_ddx")? else { return Ok(None); };
    let Some(ddy) = read_finite_lane(ctx, &mut view, site_count, "catia_e5_jet_ddy")? else { return Ok(None); };
    let Some(range_values) = (|| Some([finite_f64_le(&mut view)?, finite_f64_le(&mut view)?]))() else { return Ok(None); };
    let Some(final_knot) = knots.last().map(|knot| knot.get()) else { return Ok(None); };
    let expected_sum = u32::try_from(site_count)
        .ok()
        .and_then(|count| count.checked_mul(3))
        .and_then(|interior| (degree + 1).checked_add(interior));
    if !view.is_empty()
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.iter().enumerate().any(|(index, multiplicity)| *multiplicity != if site_count == 1 || index == 0 || index == site_count - 1 { degree + 1 } else { 3 })
        || multiplicities.iter().try_fold(0_u32, |sum, multiplicity| sum.checked_add(*multiplicity)) != expected_sum
        || range_values[0].get() != 0.0
        || (range_values[1].get() - final_knot).abs() > EPS_PARAMETER_ENDPOINT * final_knot.abs()
    {
        return Ok(None);
    }
    let Some(bytes) = site_count.checked_mul(size_of::<E5PcurveJetSite>()).and_then(|bytes| u64::try_from(bytes).ok()) else {
        return Err(ctx.refuse_codec_limit("catia_e5_jet_sites", u64::MAX, u64::MAX));
    };
    ctx.charge_retained(bytes, "catia_e5_jet_sites")?;
    let mut sites = Vec::new();
    crate::resource::reserve_vec(ctx, &mut sites, site_count, "catia_e5_jet_sites")?;
    for ((((((knot, multiplicity), u), v), du), dv), (ddu, ddv)) in knots.into_iter()
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

/// Derive the exact global-sense anchor for a plane-cap split circle.
///
/// A two-member plane-cap loop has two possible vertex-chain closures. The
/// class-`0x09` role sign identifies outer versus inner boundary, but it does
/// not choose between those closures. A strict source relation supplies the
/// missing sign: both members must be degree-5 jets on one plane, their
/// intersection supports must carry equal adjacent parameter intervals, and
/// the paired `0x0e` bounds must give a signed occurrence direction. The
/// start tangent of each jet then gives the native-UV winding of the canonical
/// chain. The face sign and boundary role convert that winding to the loop's
/// global sign.
///
/// This helper returns `None` for every incomplete or non-circular relation.
/// Such a loop remains on the shared-edge parity path instead of receiving a
/// geometric guess.
#[allow(clippy::too_many_arguments)]
fn plane_digon_orientation_hint(
    face_trailer_sign: Sign,
    surface_class: Option<u8>,
    pcurve_ids: &[u32],
    edge_ids: &[u32],
    reversed: &[bool],
    outer: Option<bool>,
    edges: &BTreeMap<u32, E5Edge>,
    pcurves: &BTreeMap<u32, E5Pcurve>,
    curve_supports: &BTreeMap<u32, E5CurveSupport>,
    bounds: &BTreeMap<u32, E5Bounds>,
) -> Option<Sign> {
    const EPS_PLANE_DIGON: f64 = 1.0e-8;
    if surface_class != Some(0xc8)
        || pcurve_ids.len() != 2
        || edge_ids.len() != 2
        || reversed.len() != 2
        || !matches!(outer, Some(true | false))
    {
        return None;
    }
    let [first_pcurve_id, second_pcurve_id] = *pcurve_ids else {
        return None;
    };
    let [first_edge_id, second_edge_id] = *edge_ids else {
        return None;
    };
    let first_edge = edges.get(&first_edge_id)?;
    let second_edge = edges.get(&second_edge_id)?;
    let same_endpoints = (first_edge.start_vertex == second_edge.start_vertex
        && first_edge.end_vertex == second_edge.end_vertex)
        || (first_edge.start_vertex == second_edge.end_vertex
            && first_edge.end_vertex == second_edge.start_vertex);
    if !same_endpoints || first_edge.support == second_edge.support {
        return None;
    }

    let [first_pcurve, second_pcurve] = [
        pcurves.get(&first_pcurve_id)?,
        pcurves.get(&second_pcurve_id)?,
    ];
    let [E5Pcurve::Jet {
        surface: first_surface,
        sites: first_sites,
        range: first_range,
    }, E5Pcurve::Jet {
        surface: second_surface,
        sites: second_sites,
        range: second_range,
    }] = [first_pcurve, second_pcurve]
    else {
        return None;
    };
    if first_surface != second_surface || first_sites.len() < 2 || second_sites.len() < 2 {
        return None;
    }

    let close = |left: f64, right: f64| {
        left.is_finite()
            && right.is_finite()
            && (left - right).abs() <= EPS_PLANE_DIGON * (1.0 + left.abs().max(right.abs()))
    };
    let close_point =
        |left: [f64; 2], right: [f64; 2]| close(left[0], right[0]) && close(left[1], right[1]);
    let first_start = first_sites.first()?.point.map(FiniteReal::get);
    let first_end = first_sites.last()?.point.map(FiniteReal::get);
    let second_start = second_sites.first()?.point.map(FiniteReal::get);
    let second_end = second_sites.last()?.point.map(FiniteReal::get);
    let same_endpoint_pair = (close_point(first_start, second_start)
        && close_point(first_end, second_end))
        || (close_point(first_start, second_end) && close_point(first_end, second_start));
    if !same_endpoint_pair {
        return None;
    }
    let center = [
        (first_start[0] + first_end[0]) * 0.5,
        (first_start[1] + first_end[1]) * 0.5,
    ];
    let first_start_radius = (first_start[0] - center[0]).hypot(first_start[1] - center[1]);
    let first_end_radius = (first_end[0] - center[0]).hypot(first_end[1] - center[1]);
    let second_center = [
        (second_start[0] + second_end[0]) * 0.5,
        (second_start[1] + second_end[1]) * 0.5,
    ];
    let second_start_radius =
        (second_start[0] - second_center[0]).hypot(second_start[1] - second_center[1]);
    let second_end_radius =
        (second_end[0] - second_center[0]).hypot(second_end[1] - second_center[1]);
    if !close_point(center, second_center)
        || !first_start_radius.is_finite()
        || !first_end_radius.is_finite()
        || !second_start_radius.is_finite()
        || !second_end_radius.is_finite()
        || first_start_radius <= EPS_PLANE_DIGON
        || !close(first_start_radius, first_end_radius)
        || !close(first_start_radius, second_start_radius)
        || !close(first_start_radius, second_end_radius)
    {
        return None;
    }
    let first_radius = first_start_radius;
    for site in first_sites.iter().chain(second_sites.iter()) {
        if !close(
            (site.point[0].get() - center[0]).hypot(site.point[1].get() - center[1]),
            first_radius,
        ) {
            return None;
        }
    }
    let native_arc_sign = |start: [f64; 2], derivative: [f64; 2]| {
        let radial = [start[0] - center[0], start[1] - center[1]];
        let derivative_norm = derivative[0].hypot(derivative[1]);
        let radial_norm = radial[0].hypot(radial[1]);
        let cross = radial[0] * derivative[1] - radial[1] * derivative[0];
        (derivative_norm.is_finite()
            && radial_norm.is_finite()
            && derivative_norm > EPS_PLANE_DIGON
            && radial_norm > EPS_PLANE_DIGON
            && cross.is_finite()
            && cross.abs() > EPS_PLANE_DIGON * radial_norm * derivative_norm)
            .then_some(if cross > 0.0 {
                Sign::Positive
            } else {
                Sign::Negative
            })
    };
    let signed_parameter_direction = |edge: &E5Edge, pcurve_id: u32, native_range: [f64; 2]| {
        let parameters = [edge.parameter_start, edge.parameter_end]
            .map(|bound_ref| bound_representation_parameter(bounds, bound_ref, pcurve_id));
        let [start, end] = parameters
            .into_iter()
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;
        let bound_span = end.get() - start.get();
        let native_span = native_range[1] - native_range[0];
        if bound_span.abs() <= EPS_PLANE_DIGON || native_span.abs() <= EPS_PLANE_DIGON {
            return None;
        }
        Some(if bound_span * native_span > 0.0 {
            Sign::Positive
        } else {
            Sign::Negative
        })
    };
    let first_direction = signed_parameter_direction(
        first_edge,
        first_pcurve_id,
        first_range.map(FiniteReal::get),
    )?
    .combine(if reversed[0] {
        Sign::Negative
    } else {
        Sign::Positive
    });
    let second_direction = signed_parameter_direction(
        second_edge,
        second_pcurve_id,
        second_range.map(FiniteReal::get),
    )?
    .combine(if reversed[1] {
        Sign::Negative
    } else {
        Sign::Positive
    });
    let first_winding = native_arc_sign(
        first_start,
        first_sites.first()?.first_derivatives.map(FiniteReal::get),
    )?
    .combine(first_direction);
    let second_winding = native_arc_sign(
        second_start,
        second_sites.first()?.first_derivatives.map(FiniteReal::get),
    )?
    .combine(second_direction);
    if first_winding != second_winding {
        return None;
    }

    let first_support = curve_supports.get(&first_edge.support)?;
    let second_support = curve_supports.get(&second_edge.support)?;
    if !first_support.is_intersection()
        || !second_support.is_intersection()
        || !first_support.pcurves().contains(&first_pcurve_id)
        || !second_support.pcurves().contains(&second_pcurve_id)
    {
        return None;
    }
    let mut intervals =
        [first_support.range, second_support.range].map(|range| range.map(FiniteReal::get));
    for interval in &mut intervals {
        if interval[0] == interval[1] {
            return None;
        }
        if interval[0] > interval[1] {
            interval.swap(0, 1);
        }
    }
    if intervals[0][0] > intervals[1][0] {
        intervals.swap(0, 1);
    }
    let first_span = intervals[0][1] - intervals[0][0];
    let second_span = intervals[1][1] - intervals[1][0];
    if !close(first_span, second_span) || !close(intervals[0][1], intervals[1][0]) {
        return None;
    }

    let role_sign = if outer == Some(true) {
        Sign::Positive
    } else {
        Sign::Negative
    };
    Some(face_trailer_sign.combine(role_sign).combine(first_winding))
}

fn solve_absolute_orientation(
    ctx: &DecodeContext<'_>,
    faces: &mut [E5Face],
) -> Result<bool, CodecError> {
    let location_count = faces
        .iter()
        .flat_map(|face| &face.loops)
        .filter(|loop_| !loop_.members.is_empty())
        .count();
    ctx.charge_collection_items(
        u64::try_from(location_count).map_err(|_| {
            ctx.refuse_codec_limit("catia e5 orientation locations", u64::MAX, u64::MAX)
        })?,
        "catia e5 orientation locations",
    )?;
    let mut locations = Vec::with_capacity(location_count);
    for (face_index, face) in faces.iter().enumerate() {
        for (loop_index, loop_) in face.loops.iter().enumerate() {
            if !loop_.members.is_empty() {
                locations.push((face_index, loop_index));
            }
        }
    }
    let mut occurrences = HashMap::<u32, Vec<(usize, Sign)>>::new();
    for (node, &(face_index, loop_index)) in locations.iter().enumerate() {
        let loop_ = &faces[face_index].loops[loop_index];
        for member in &loop_.members {
            ctx.charge_collection_items(1, "catia e5 orientation edge occurrences")?;
            occurrences.entry(member.edge_use).or_default().push((
                node,
                if member.reversed {
                    Sign::Negative
                } else {
                    Sign::Positive
                },
            ));
        }
    }
    let mut adjacency = ctx.alloc_filled(
        locations.len(),
        Vec::<(usize, Sign)>::new(),
        "catia e5 orientation adjacency",
    )?;
    for [(left, left_r), (right, right_r)] in occurrences
        .values()
        .filter_map(|uses| <&[_; 2]>::try_from(uses.as_slice()).ok())
    {
        let relation = left_r.flipped().combine(*right_r);
        ctx.charge_collection_items(2, "catia e5 orientation adjacent edges")?;
        adjacency[*left].push((*right, relation));
        adjacency[*right].push((*left, relation));
    }
    let mut solved = ctx.alloc_filled(locations.len(), None, "catia e5 orientation assignments")?;
    for root in 0..locations.len() {
        if solved[root].is_some() {
            continue;
        }
        solved[root] = Some(Sign::Positive);
        ctx.charge_collection_items(1, "catia e5 orientation component")?;
        let mut component = vec![(root, Sign::Positive)];
        let mut cursor = 0;
        let mut consistent = true;
        while cursor < component.len() {
            let (node, value) = component[cursor];
            cursor += 1;
            for &(neighbor, relation) in &adjacency[node] {
                let expected = value.combine(relation);
                match solved[neighbor] {
                    Some(actual) if actual != expected => consistent = false,
                    Some(_) => {}
                    None => {
                        solved[neighbor] = Some(expected);
                        ctx.charge_collection_items(1, "catia e5 orientation component")?;
                        component.push((neighbor, expected));
                    }
                }
            }
        }
        if !consistent {
            for &(node, _) in &component {
                solved[node] = None;
            }
            continue;
        }
        let mut exact_flip = None;
        for &(node, value) in &component {
            let (face_index, loop_index) = locations[node];
            let Some(hint) = faces[face_index].loops[loop_index].orientation_hint else {
                continue;
            };
            let candidate = hint.combine(value);
            match exact_flip {
                Some(existing) if existing != candidate => {
                    for &(component_node, _) in &component {
                        solved[component_node] = None;
                    }
                    consistent = false;
                    break;
                }
                Some(_) => {}
                None => exact_flip = Some(candidate),
            }
        }
        if !consistent {
            continue;
        }
        if let Some(flip) = exact_flip {
            for &(node, value) in &component {
                solved[node] = Some(value.combine(flip));
            }
        } else {
            let plus_matches = component
                .iter()
                .filter(|&&(node, value)| {
                    let (face, _) = locations[node];
                    value == faces[face].trailer_sign
                })
                .count();
            let minus_matches = component.len() - plus_matches;
            if minus_matches > plus_matches {
                for &(node, value) in &component {
                    solved[node] = Some(value.flipped());
                }
            }
        }
    }
    for (node, &(face_index, loop_index)) in locations.iter().enumerate() {
        let Some(g) = solved[node] else {
            continue;
        };
        let loop_ = &mut faces[face_index].loops[loop_index];
        let flip = g == Sign::Negative;
        ctx.charge_collection_items(
            u64::try_from(loop_.members.len()).map_err(|_| {
                ctx.refuse_codec_limit("catia e5 orientation member indices", u64::MAX, u64::MAX)
            })?,
            "catia e5 orientation member indices",
        )?;
        let mut indices: Vec<usize> = (0..loop_.members.len()).collect();
        if flip {
            indices.reverse();
        }
        ctx.charge_collection_items(
            u64::try_from(loop_.members.len()).map_err(|_| {
                ctx.refuse_codec_limit("catia e5 oriented members", u64::MAX, u64::MAX)
            })?,
            "catia e5 oriented members",
        )?;
        loop_.oriented_members = Some(
            indices
                .into_iter()
                .map(|serialized_index| E5OrientedMember {
                    serialized_index,
                    reversed: loop_.members[serialized_index].reversed ^ flip,
                })
                .collect(),
        );
    }
    Ok(solved.into_iter().all(|value| value.is_some()))
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
        let Some(root_id) = wire::tokens::object_ref(record.payload, &mut position, false) else { return Ok(None); };
        if position != record.payload.len() {
            return Ok(None);
        }
        let Some(root) = by_id.get(&root_id) else { return Ok(None); };
        if root.class != 0x08 {
            return Ok(None);
        }
        let Some(faces) = parse_body_root(ctx, root.payload)? else { return Ok(None); };
        if faces.iter().any(|face| by_id.get(face).is_none_or(|target| target.class != 0x00)) {
            return Ok(None);
        }
        crate::resource::push(ctx, &mut bodies, E5Body { record_id: record.id, faces }, "catia_e5_bodies")?;
    }
    Ok(Some(bodies))
}

fn parse_body_root(ctx: &DecodeContext<'_>, payload: &[u8]) -> Result<Option<Vec<u32>>, CodecError> {
    let (count, mut position, narrow) = if payload.first() == Some(&0x08) {
        let Some(count) = payload.get(1) else { return Ok(None); };
        (usize::from(*count), 2, true)
    } else {
        let Some(count) = payload.first().and_then(|lead| lead.checked_sub(0x80)) else { return Ok(None); };
        (usize::from(count), 1, false)
    };
    let mut faces = Vec::new();
    for _ in 0..count {
        let Some(face) = wire::tokens::object_ref(payload, &mut position, false) else { return Ok(None); };
        if narrow && face > u32::from(u16::MAX) {
            return Ok(None);
        }
        crate::resource::push(ctx, &mut faces, face, "catia_e5_body_root_faces")?;
    }
    let Some(count) = u8::try_from(faces.len()).ok() else { return Ok(None); };
    if payload.get(position..position + 2) == Some(&[0x08, count]) {
        position += 2;
    } else if faces.len() <= 0x7f
        && 0x80u8.checked_add(count).is_some_and(|head| payload.get(position) == Some(&head))
    {
        position += 1;
    } else {
        return Ok(None);
    }
    let Some(sign_bytes) = payload.get(position..) else { return Ok(None); };
    if sign_bytes.len() != (faces.len() + 2) * 2 {
        return Ok(None);
    }
    for bytes in sign_bytes.chunks_exact(2) {
        if View::i16_le_at(bytes, 0).and_then(Sign::from_i16).is_none() {
            return Ok(None);
        }
    }
    Ok(Some(faces))
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
        let Some(id) = View::u32_le_at(bytes, start + 9) else {
            return None;
        };
        let Some(size) = View::u16_le_at(bytes, start + 5).map(usize::from) else {
            return None;
        };
        let Some(end) = start.checked_add(13 + size) else {
            return None;
        };
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
    let Some(count) = record.payload.first().and_then(|lead| lead.checked_sub(0x81)).map(usize::from) else { return Ok(None); };
    if count == 0 {
        return Ok(None);
    }
    let mut position = 1;
    let Some(surface) = wire::tokens::object_ref(record.payload, &mut position, false) else { return Ok(None); };
    let mut loops = Vec::new();
    for _ in 0..count {
        let Some(loop_id) = wire::tokens::object_ref(
            record.payload,
            &mut position,
            false,
        ) else { return Ok(None); };
        crate::resource::push(ctx, &mut loops, loop_id, "catia_e5_face_loop_ids")?;
    }
    let Some(trailer_sign) = View::i16_le_at(record.payload, position).and_then(Sign::from_i16) else { return Ok(None); };
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

fn parse_loop(ctx: &DecodeContext<'_>, record: &Record<'_>) -> Result<Option<RawLoop>, CodecError> {
    let Some(member_count) = record.payload.first().and_then(|lead| lead.checked_sub(0x81)).map(usize::from) else { return Ok(None); };
    if member_count == 0 || member_count % 2 != 0 {
        return Ok(None);
    }
    let mut position = 1;
    let mut pcurves = Vec::new();
    let mut edges = Vec::new();
    for _ in 0..member_count / 2 {
        let Some(pcurve) = wire::tokens::object_ref(
            record.payload,
            &mut position,
            false,
        ) else { return Ok(None); };
        let Some(edge) = wire::tokens::object_ref(
            record.payload,
            &mut position,
            false,
        ) else { return Ok(None); };
        crate::resource::push(ctx, &mut pcurves, pcurve, "catia_e5_loop_pcurves")?;
        crate::resource::push(ctx, &mut edges, edge, "catia_e5_loop_edges")?;
    }
    let Some(surface) = wire::tokens::object_ref(record.payload, &mut position, false) else { return Ok(None); };
    let Some(outer) = record.payload.get(position..).and_then(|tail| parse_loop_signs(tail, member_count / 2).ok()) else { return Ok(None); };
    Ok(Some(RawLoop {
        id: record.id,
        surface,
        pcurves,
        edges,
        outer,
    }))
}

#[derive(Debug)]
enum LoopSignError {
    Count,
    Frame,
    Sign,
}

fn parse_loop_signs(trailing: &[u8], edge_count: usize) -> Result<Option<bool>, LoopSignError> {
    if trailing.is_empty() {
        return Ok(None);
    }
    let expected_head = u8::try_from(edge_count)
        .ok()
        .and_then(|n| 0x80u8.checked_add(n))
        .ok_or(LoopSignError::Count)?;
    if trailing.first() != Some(&expected_head) || trailing.len() != 1 + 2 * (3 * edge_count + 4) {
        return Err(LoopSignError::Frame);
    }
    let signs: Vec<i16> = trailing[1..]
        .chunks_exact(2)
        .map(|bytes| View::i16_le_at(bytes, 0))
        .collect::<Option<Vec<_>>>()
        .ok_or(LoopSignError::Frame)?;
    if signs.iter().any(|sign| !matches!(sign, -1..=1)) || !matches!(signs[1], -1 | 1) {
        return Err(LoopSignError::Sign);
    }
    Ok(Some(signs[1] == 1))
}

fn parse_edge(record: &Record<'_>) -> Option<E5Edge> {
    if record.payload.first() != Some(&0x85) {
        return None;
    }
    let mut position = 1;
    let support = wire::tokens::object_ref(record.payload, &mut position, false)?;
    let start_vertex = wire::tokens::object_ref(record.payload, &mut position, false)?;
    let end_vertex = wire::tokens::object_ref(record.payload, &mut position, false)?;
    let parameter_start = wire::tokens::object_ref(record.payload, &mut position, false)?;
    let parameter_end = wire::tokens::object_ref(record.payload, &mut position, false)?;
    let tail = record.payload[position..].to_vec();
    Some(E5Edge {
        support,
        start_vertex,
        end_vertex,
        parameter_start,
        parameter_end,
        tail,
    })
}

fn solve_loop_chain(
    ctx: &DecodeContext<'_>,
    edge_ids: &[u32],
    edges: &BTreeMap<u32, E5Edge>,
) -> Result<Option<Vec<bool>>, CodecError> {
    let Some(first) = edge_ids.first().and_then(|id| edges.get(id)) else { return Ok(None); };
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
        crate::resource::push(ctx, &mut senses, first_reversed, "catia_e5_chain_senses")?;
        let mut valid = true;
        for edge_id in &edge_ids[1..] {
            let Some(edge) = edges.get(edge_id) else { return Ok(None); };
            match (edge.start_vertex == current, edge.end_vertex == current) {
                (true, false) => {
                    crate::resource::push(ctx, &mut senses, false, "catia_e5_chain_senses")?;
                    current = edge.end_vertex;
                }
                (false, true) => {
                    crate::resource::push(ctx, &mut senses, true, "catia_e5_chain_senses")?;
                    current = edge.start_vertex;
                }
                _ => {
                    valid = false;
                    break;
                }
            }
        }
        if valid && current == initial {
            crate::resource::push(ctx, &mut solutions, senses, "catia_e5_chain_solutions")?;
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
        parse_pcurve, parse_topology, plane_digon_orientation_hint, records,
        solve_absolute_orientation, solve_loop_chain, E5BoundEntry, E5Bounds, E5CurveSupport,
        E5CurveSupportKind, E5Edge, E5Face, E5Loop, E5LoopMember, E5Pcurve, E5PcurveJetSite,
        E5Topology, Record, Sign,
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
                &[0x85, 0x08, 200, 0x08, start, 0x08, end, 0x80, 0x80],
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
            bound_payload.extend_from_slice(&0_u32.to_le_bytes());
        }
        append_e5_record(&mut bytes, 0x0e, 0, &bound_payload);

        let mut loop_payload = vec![0x87];
        for reference in [100, 110, 101, 111, 102, 112] {
            loop_payload.extend_from_slice(&[0x08, reference]);
        }
        loop_payload.extend_from_slice(&[0x18, 0x02, 0x01, 0x83]);
        for _ in 0..13 {
            loop_payload.extend_from_slice(&1_i16.to_le_bytes());
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
        assert_eq!(topology.faces[0].loops[0].outer, Some(true));
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
        assert!(!crate::test_support::with_service_context(|ctx| curve_support_reference_closes(ctx, 1, &pcurves, &supports)).expect("service resource budget"));
    }

    #[test]
    fn e5_support_walk_refuses_before_stack_and_set_growth() {
        let pcurves = BTreeMap::from([(3, E5Pcurve::Line {
            surface: 10,
            origin: finite_pair([0.0, 0.0]),
            direction: finite_pair([1.0, 0.0]),
            range: finite_pair([0.0, 1.0]),
        })]);
        let supports = BTreeMap::from([(1, E5CurveSupport {
            kind: E5CurveSupportKind::Intersection([3, 3]),
            mode: 0,
            range: finite_pair([0.0, 1.0]),
            tail: Vec::new(),
        })]);
        assert!(crate::test_support::with_service_context(|ctx| curve_support_reference_closes(ctx, 1, &pcurves, &supports)).expect("service resource budget"));
        for (cap, operation) in [(0, "catia_e5_support_stack"), (1, "catia_e5_support_visiting")] {
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
        payload.extend_from_slice(&[0; 37]);
        let mut truncated = payload.clone();
        truncated.pop();
        assert!(crate::test_support::with_service_context(|ctx| parse_nurbs_pcurve(ctx, &truncated, 2, 7))
            .expect("service resource budget").is_none());

        let E5Pcurve::Nurbs {
            surface,
            degree,
            knots,
            control_points,
            range,
        } = crate::test_support::with_service_context(|ctx| parse_nurbs_pcurve(ctx, &payload, 2, 7))
            .expect("service resource budget").expect("AA NURBS pcurve")
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
        payload.extend_from_slice(&[0; 37]);
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
            crate::test_support::with_service_context(|ctx| parse_nurbs_pcurve(ctx, &payload, 2, 7)),
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
            crate::test_support::with_service_context(|ctx| parse_pcurve(ctx, &Record {
                class: 0x97,
                id: 20,
                payload: &payload,
            })).expect("service resource budget")
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
        assert_eq!(faces, [0x1600, 0x1601]);
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
        assert!(crate::test_support::with_service_context(|ctx| parse_jet_pcurve(ctx, &nonzero_lower, 0, 7))
            .expect("service resource budget").is_none());

        let mut wrong_upper = payload.clone();
        let upper_offset = wrong_upper.len() - 8;
        wrong_upper[upper_offset..].copy_from_slice(&(2.0 * final_knot).to_le_bytes());
        assert!(crate::test_support::with_service_context(|ctx| parse_jet_pcurve(ctx, &wrong_upper, 0, 7))
            .expect("service resource budget").is_none());

        let mut nonfinite_knot = payload;
        nonfinite_knot[28..36].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(crate::test_support::with_service_context(|ctx| parse_jet_pcurve(ctx, &nonfinite_knot, 0, 7))
            .expect("service resource budget").is_none());
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
                    tail: Vec::new(),
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
                    tail: Vec::new(),
                },
            ),
        ]);
        assert_eq!(crate::test_support::with_service_context(|ctx| solve_loop_chain(ctx, &[1, 2], &edges)).expect("service resource budget"), Some(vec![false, false]));
    }

    #[test]
    fn plane_digon_winding_anchors_absolute_orientation() {
        e5_test_context!(ctx);
        let jet = |points: Vec<[f64; 2]>, first_derivatives: Vec<[f64; 2]>| E5Pcurve::Jet {
            surface: 500,
            sites: points
                .into_iter()
                .zip(first_derivatives)
                .enumerate()
                .map(|(index, (point, first_derivatives))| E5PcurveJetSite {
                    knot: finite(index as f64),
                    multiplicity: 6,
                    point: finite_pair(point),
                    first_derivatives: finite_pair(first_derivatives),
                    second_derivatives: finite_pair([0.0, 0.0]),
                })
                .collect(),
            range: finite_pair([0.0, 1.0]),
        };
        let pcurves = BTreeMap::from([
            (
                10,
                jet(
                    vec![[0.0, -1.0], [-1.0, 0.0], [0.0, 1.0]],
                    vec![[-1.0, 0.0], [-1.0, 0.0], [-1.0, 0.0]],
                ),
            ),
            (
                11,
                jet(
                    vec![[0.0, 1.0], [1.0, 0.0], [0.0, -1.0]],
                    vec![[1.0, 0.0], [1.0, 0.0], [1.0, 0.0]],
                ),
            ),
        ]);
        let edges = BTreeMap::from([
            (
                1,
                E5Edge {
                    support: 100,
                    start_vertex: 20,
                    end_vertex: 21,
                    parameter_start: 30,
                    parameter_end: 31,
                    tail: Vec::new(),
                },
            ),
            (
                2,
                E5Edge {
                    support: 101,
                    start_vertex: 21,
                    end_vertex: 20,
                    parameter_start: 32,
                    parameter_end: 33,
                    tail: Vec::new(),
                },
            ),
        ]);
        let supports = BTreeMap::from([
            (
                100,
                E5CurveSupport {
                    kind: E5CurveSupportKind::Intersection([10, 12]),
                    mode: 0,
                    range: finite_pair([0.0, 1.0]),
                    tail: Vec::new(),
                },
            ),
            (
                101,
                E5CurveSupport {
                    kind: E5CurveSupportKind::Intersection([11, 13]),
                    mode: 0,
                    range: finite_pair([1.0, 2.0]),
                    tail: Vec::new(),
                },
            ),
        ]);
        let bounds = BTreeMap::from([
            (
                30,
                E5Bounds {
                    entries: vec![E5BoundEntry {
                        representation: 10,
                        parameter: crate::test_support::test_b5::finite(0.0),
                        code: 0,
                    }],
                },
            ),
            (
                31,
                E5Bounds {
                    entries: vec![E5BoundEntry {
                        representation: 10,
                        parameter: crate::test_support::test_b5::finite(1.0),
                        code: 0,
                    }],
                },
            ),
            (
                32,
                E5Bounds {
                    entries: vec![E5BoundEntry {
                        representation: 11,
                        parameter: crate::test_support::test_b5::finite(0.0),
                        code: 0,
                    }],
                },
            ),
            (
                33,
                E5Bounds {
                    entries: vec![E5BoundEntry {
                        representation: 11,
                        parameter: crate::test_support::test_b5::finite(1.0),
                        code: 0,
                    }],
                },
            ),
        ]);
        let hint = plane_digon_orientation_hint(
            Sign::Positive,
            Some(0xc8),
            &[10, 11],
            &[1, 2],
            &[false, false],
            Some(true),
            &edges,
            &pcurves,
            &supports,
            &bounds,
        );
        assert_eq!(hint, Some(Sign::Negative));

        let mut wide_pcurves = pcurves.clone();
        for pcurve in wide_pcurves.values_mut() {
            if let E5Pcurve::Jet { range, .. } = pcurve {
                *range = finite_pair([-f64::MAX, f64::MAX]);
            }
        }
        let mut wide_bounds = bounds.clone();
        for (id, bound) in &mut wide_bounds {
            bound.entries[0].parameter = finite(if matches!(*id, 30 | 32) {
                -f64::MAX
            } else {
                f64::MAX
            });
        }
        assert_eq!(
            plane_digon_orientation_hint(
                Sign::Positive,
                Some(0xc8),
                &[10, 11],
                &[1, 2],
                &[false, false],
                Some(true),
                &edges,
                &wide_pcurves,
                &supports,
                &wide_bounds,
            ),
            hint
        );

        let mut faces = vec![E5Face {
            record_id: 1,
            surface: 500,
            trailer_sign: crate::families::e5::graph::Sign::Positive,
            loops: vec![E5Loop {
                record_id: 2,
                surface: 500,
                members: e5_loop_members(&[10, 11], &[1, 2], &[false, false]),
                oriented_members: None,
                outer: Some(true),
                orientation_hint: hint,
            }],
        }];
        assert!(solve_absolute_orientation(&ctx, &mut faces).expect("service decode"));
        let members = faces[0].loops[0]
            .resolved_members()
            .expect("exact digon anchor resolves the component");
        assert_eq!(
            members,
            [
                super::E5OrientedMember {
                    serialized_index: 1,
                    reversed: true,
                },
                super::E5OrientedMember {
                    serialized_index: 0,
                    reversed: true,
                },
            ]
        );
    }

    #[test]
    fn edge_parameters_resolve_one_entry_from_each_bound() {
        let edge = E5Edge {
            support: 0,
            start_vertex: 0,
            end_vertex: 0,
            parameter_start: 10,
            parameter_end: 11,
            tail: Vec::new(),
        };
        let bound = |parameter| E5Bounds {
            entries: vec![E5BoundEntry {
                representation: 20,
                parameter: crate::test_support::test_b5::finite(parameter),
                code: 7,
            }],
        };
        let mut topology = E5Topology {
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: BTreeMap::from([(1, edge)]),
            pcurves: BTreeMap::new(),
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
            outer: Some(true),
            orientation_hint: None,
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

        assert!(!solve_absolute_orientation(&ctx, &mut faces).expect("service decode"));
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
        assert!(solve_absolute_orientation(&ctx, &mut faces).expect("service decode"));
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
        assert!(crate::test_support::with_service_context(|ctx| parse_topology(ctx, &bytes))
            .expect("service resource budget")
            .is_some());
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
    fn e5_curve_support_tail_refuses_before_copy() {
        let mut payload = vec![0x81, 0x81, 0x81, 0, 0];
        payload.extend_from_slice(&0.0_f64.to_le_bytes());
        payload.extend_from_slice(&1.0_f64.to_le_bytes());
        payload.push(0xaa);
        let record = Record { class: 0xc0, id: 1, payload: &payload };
        assert_eq!(
            crate::test_support::with_service_context(|ctx| super::parse_curve_support(ctx, &record))
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
    fn e5_orientation_refuses_before_location_collection() {
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
                outer: Some(true),
                orientation_hint: None,
            }],
        }];
        e5_test_context!(service_ctx);
        assert!(solve_absolute_orientation(&service_ctx, &mut faces.clone())
            .expect("orientation fits the service profile"));

        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits the input limit");
        let error = solve_absolute_orientation(&ctx, &mut faces.clone())
            .expect_err("one orientation location exceeds the collection limit");
        assert!(matches!(error, CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::CollectionItems
                && limit.operation == "catia e5 orientation locations"));
    }

    #[test]
    fn e5_orientation_charges_adjacency_and_assignment_arrays() {
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
                outer: Some(true),
                orientation_hint: None,
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
            match solve_absolute_orientation(&ctx, &mut faces.clone()) {
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
        assert!(operations.contains("catia e5 orientation adjacency"));
        assert!(operations.contains("catia e5 orientation assignments"));
    }
}
