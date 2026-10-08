// SPDX-License-Identifier: Apache-2.0
//! Native topology records in the E5 `0D 03` stream family.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{DecodeContext, ScopedReservation, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};

use crate::families::e5::records::e5_frames;
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
    pub(super) fn edge_representation_parameters(
        &self,
        ctx: &DecodeContext<'_>,
        edge_ref: u32,
        representation: u32,
        bound_parameters: &E5BoundParameters,
    ) -> Result<Option<[FiniteReal; 2]>, CodecError> {
        let Some(edge) = ctx.get_btree_map(&self.edges, &edge_ref, "catia_e5_edge_lookup")? else {
            return Ok(None);
        };
        let start = bound_representation_parameter(
            ctx,
            bound_parameters,
            edge.parameter_start,
            representation,
        )?;
        let end = bound_representation_parameter(
            ctx,
            bound_parameters,
            edge.parameter_end,
            representation,
        )?;
        Ok(start.zip(end).map(|(start, end)| [start, end]))
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

    /// Whether `pcurve` is one of the support's p-curve references.
    fn references(&self, pcurve: u32) -> bool {
        match self.kind {
            E5CurveSupportKind::Boundary(only) => only == pcurve,
            E5CurveSupportKind::Intersection([first, second]) => {
                first == pcurve || second == pcurve
            }
        }
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

#[derive(Debug, Clone, Copy)]
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
    // The record list, the id index, raw faces and loops, chain senses and
    // the reachability sets are scratch; only the resolved topology is kept.
    let mut scratch = ctx.reserve_scoped(0, "catia_e5_topology_scratch")?;
    let mut stream_records = Vec::new();
    for record in records(ctx, bytes)? {
        ctx.push_scoped_vec(
            &mut scratch,
            &mut stream_records,
            record,
            "catia_e5_graph_records",
        )?;
    }
    let mut by_id = HashMap::new();
    let mut steps = stream_records.iter();
    while let Some(record) = ctx.next_charged(&mut steps, "catia_e5_record_id_scan")? {
        let previous = scratch.with_storage(|| {
            ctx.insert_hash_map(&mut by_id, record.id, *record, "catia_e5_records_by_id")
        })?;
        // A repeated id makes every reference to it ambiguous.
        if previous.is_some() {
            return Ok(None);
        }
    }
    let class_of = |id: u32| -> Result<Option<u8>, CodecError> {
        Ok(ctx
            .get_hash_map(&by_id, &id, "catia_e5_record_lookup")?
            .map(|record| record.class))
    };

    let mut edges = BTreeMap::new();
    let mut pcurves = BTreeMap::new();
    let mut steps = stream_records.iter();
    while let Some(record) = ctx.next_charged(&mut steps, "catia_e5_edge_pcurve_record_scan")? {
        if record.class == 0xff {
            let Some(edge) = parse_edge(ctx, record)? else {
                return Ok(None);
            };
            ctx.insert_btree_map(&mut edges, record.id, edge, "catia_e5_topology_edges")?;
        }
        if matches!(record.class, 0x96 | 0x97 | 0xa0 | 0xaa) {
            let Some(pcurve) = parse_pcurve(ctx, record)? else {
                return Ok(None);
            };
            ctx.insert_btree_map(&mut pcurves, record.id, pcurve, "catia_e5_topology_pcurves")?;
        }
    }
    if !ctx.all_by(
        &pcurves,
        |(_, pcurve)| {
            Ok(class_of(pcurve.surface_record_id())?.is_some_and(is_surface_carrier_class))
        },
        "catia_e5_surface_pcurve_scan",
    )? {
        return Ok(None);
    }
    let mut bounds = BTreeMap::new();
    let mut curve_supports = BTreeMap::new();
    let mut loops = HashMap::new();
    let mut raw_faces = Vec::new();
    let mut has_vertex = false;
    let mut steps = stream_records.iter();
    while let Some(record) = ctx.next_charged(&mut steps, "catia_e5_topology_record_scan")? {
        match record.class {
            0x0e => {
                let Some(value) = parse_bounds(ctx, record)? else {
                    return Ok(None);
                };
                ctx.insert_btree_map(&mut bounds, record.id, value, "catia_e5_topology_bounds")?;
            }
            0xc0 | 0xc1 => {
                let Some(value) = parse_curve_support(ctx, record)? else {
                    return Ok(None);
                };
                ctx.insert_btree_map(
                    &mut curve_supports,
                    record.id,
                    value,
                    "catia_e5_curve_supports",
                )?;
            }
            0x09 => {
                let Some(value) = scratch.with_storage(|| parse_loop(ctx, record))? else {
                    return Ok(None);
                };
                scratch.with_storage(|| {
                    ctx.insert_hash_map(&mut loops, record.id, value, "catia_e5_raw_loops")
                })?;
            }
            0x00 => {
                let Some(value) = scratch.with_storage(|| parse_face(ctx, record))? else {
                    return Ok(None);
                };
                ctx.push_scoped_vec(&mut scratch, &mut raw_faces, value, "catia_e5_raw_faces")?;
            }
            0xfe => has_vertex = true,
            _ => {}
        }
    }
    if raw_faces.is_empty() || loops.is_empty() || edges.is_empty() || !has_vertex {
        return Ok(None);
    }

    let (bound_parameters, _bound_storage) = index_bound_parameters(ctx, &bounds)?;
    let mut faces = Vec::new();
    let mut reachable_edges = HashSet::new();
    let mut closed_supports = HashSet::new();
    let mut steps = raw_faces.iter();
    while let Some(face) = ctx.next_charged(&mut steps, "catia_e5_raw_face_scan")? {
        let surface_class = class_of(face.surface)?;
        if !surface_class.is_some_and(is_surface_carrier_class) {
            return Ok(None);
        }
        let mut resolved_loops = Vec::new();
        let mut loop_ids = face.loops.iter().enumerate();
        while let Some((loop_position, loop_id)) =
            ctx.next_charged(&mut loop_ids, "catia_e5_raw_face_loop_scan")?
        {
            let Some(raw) = ctx.get_hash_map(&loops, loop_id, "catia_e5_raw_loop_lookup")? else {
                return Ok(None);
            };
            if raw.surface != face.surface
                || raw.outer.is_some_and(|outer| outer != (loop_position == 0))
                || raw.pcurves.len() != raw.edges.len()
            {
                return Ok(None);
            }
            let Some(reversed) =
                scratch.with_storage(|| solve_loop_chain(ctx, &raw.edges, &edges))?
            else {
                return Ok(None);
            };
            if reversed.len() != raw.edges.len()
                || !ctx.all_by(
                    &raw.pcurves,
                    |pcurve_id| {
                        Ok(ctx
                            .get_btree_map(&pcurves, pcurve_id, "catia_e5_pcurve_lookup")?
                            .is_some_and(|pcurve| pcurve.surface_record_id() == raw.surface))
                    },
                    "catia_e5_raw_loop_pcurve_validation_scan",
                )?
            {
                return Ok(None);
            }
            let mut pcurve_ids = raw.pcurves.iter();
            let mut edge_ids = raw.edges.iter();
            while let Some(pcurve_id) =
                ctx.next_charged(&mut pcurve_ids, "catia_e5_raw_loop_pcurve_scan")?
            {
                let Some(edge_id) =
                    ctx.next_charged(&mut edge_ids, "catia_e5_raw_loop_edge_scan")?
                else {
                    return Ok(None);
                };
                let Some(edge) = ctx.get_btree_map(&edges, edge_id, "catia_e5_edge_lookup")? else {
                    return Ok(None);
                };
                if class_of(edge.start_vertex)? != Some(0xfe)
                    || class_of(edge.end_vertex)? != Some(0xfe)
                {
                    return Ok(None);
                }
                for bound_ref in [edge.parameter_start, edge.parameter_end] {
                    if bound_representation_parameter(
                        ctx,
                        &bound_parameters,
                        bound_ref,
                        *pcurve_id,
                    )?
                    .is_none()
                    {
                        return Ok(None);
                    }
                }
                let Some(support) =
                    ctx.get_btree_map(&curve_supports, &edge.support, "catia_e5_support_lookup")?
                else {
                    return Ok(None);
                };
                let sides = match support.kind {
                    E5CurveSupportKind::Boundary(pcurve) => [pcurve, pcurve],
                    E5CurveSupportKind::Intersection(sides) => sides,
                };
                for side in sides {
                    if !curve_support_reference_closes(
                        ctx,
                        side,
                        &pcurves,
                        &curve_supports,
                        &mut scratch,
                        &mut closed_supports,
                    )? {
                        return Ok(None);
                    }
                }
                scratch.with_storage(|| {
                    ctx.insert_hash_set(&mut reachable_edges, *edge_id, "catia_e5_reachable_edges")
                })?;
            }
            let orientation_hint = plane_digon_orientation_hint(
                ctx,
                PlaneDigonOrientationHintInputs {
                    face_trailer_sign: face.trailer_sign,
                    surface_class,
                    pcurve_ids: &raw.pcurves,
                    edge_ids: &raw.edges,
                    reversed: &reversed,
                    outer: raw.outer,
                    edges: &edges,
                    pcurves: &pcurves,
                    curve_supports: &curve_supports,
                    bound_parameters: &bound_parameters,
                },
            )?;
            let mut members = ctx.vector_storage(raw.pcurves.len(), "catia_e5_loop_members")?;
            for ((&pcurve, &edge_use), &reversed) in ctx
                .admit_iter(&raw.pcurves, "catia_e5_resolved_member_pcurve_scan")?
                .zip(ctx.admit_iter(&raw.edges, "catia_e5_resolved_member_edge_scan")?)
                .zip(ctx.admit_iter(&reversed, "catia_e5_resolved_member_orientation_scan")?)
            {
                ctx.push_vec(
                    &mut members,
                    E5LoopMember {
                        pcurve,
                        edge_use,
                        reversed,
                    },
                    "catia_e5_loop_members",
                )?;
            }
            ctx.push_vec(
                &mut resolved_loops,
                E5Loop {
                    record_id: raw.id,
                    surface: raw.surface,
                    members,
                    oriented_members: None,
                    outer: raw.outer,
                    orientation_hint,
                },
                "catia_e5_resolved_loops",
            )?;
        }
        ctx.push_vec(
            &mut faces,
            E5Face {
                record_id: face.id,
                surface: face.surface,
                trailer_sign: face.trailer_sign,
                loops: resolved_loops,
            },
            "catia_e5_topology_faces",
        )?;
    }
    if !solve_absolute_orientation(ctx, &mut faces)? {
        return Ok(None);
    }
    ctx.retain_btree_map(
        &mut edges,
        |id, _| ctx.contains_hash_set(&reachable_edges, id, "catia_e5_reachable_edge_lookup"),
        "catia_e5_reachable_edge_retain",
    )?;
    let mut vertex_refs = Vec::new();
    for (_, edge) in ctx.admit_iter(&edges, "catia_e5_vertex_edge_scan")? {
        for vertex in [edge.start_vertex, edge.end_vertex] {
            ctx.push_vec(&mut vertex_refs, vertex, "catia_e5_vertex_refs")?;
        }
    }
    ctx.sort_unstable_by(
        &mut vertex_refs,
        |value| value,
        Ord::cmp,
        "catia_e5_vertex_refs_sort",
    )?;
    ctx.dedup_vec(&mut vertex_refs, "catia_e5_vertex_refs_dedup")?;
    let Some(bodies) = parse_bodies(ctx, &stream_records, &by_id)? else {
        return Ok(None);
    };
    if !bodies.is_empty() && !body_rosters_cover_faces(ctx, &bodies, &faces)? {
        return Ok(None);
    }
    Ok(Some(E5Topology {
        bodies,
        faces,
        edges,
        pcurves,
        bounds,
        curve_supports,
        vertex_refs,
    }))
}

/// Whether the body face rosters name every resolved face exactly once.
/// Face record ids are distinct, so a duplicate-free roster of resolved faces
/// with one entry per face names the complete face set.
fn body_rosters_cover_faces(
    ctx: &DecodeContext<'_>,
    bodies: &[E5Body],
    faces: &[E5Face],
) -> Result<bool, CodecError> {
    let mut storage = ctx.reserve_scoped(0, "catia_e5_body_face_sets")?;
    let mut face_ids = HashSet::new();
    for face in ctx.admit_iter(faces, "catia_e5_topology_face_set_scan")? {
        storage.with_storage(|| {
            ctx.insert_hash_set(&mut face_ids, face.record_id, "catia_e5_topology_face_set")
        })?;
    }
    let mut named = HashSet::new();
    let mut roster_len = 0usize;
    let mut steps = bodies.iter();
    while let Some(body) = ctx.next_charged(&mut steps, "catia_e5_body_roster_scan")? {
        let mut steps = body.faces.iter();
        while let Some(face) = ctx.next_charged(&mut steps, "catia_e5_body_face_roster_scan")? {
            let known = ctx.contains_hash_set(&face_ids, face, "catia_e5_body_face_lookup")?;
            let first = storage.with_storage(|| {
                ctx.insert_hash_set(&mut named, *face, "catia_e5_body_face_set")
            })?;
            if !known || !first {
                return Ok(false);
            }
            roster_len += 1;
        }
    }
    Ok(roster_len == faces.len())
}

/// Return the serialized surface reference for each valid class-`0x00` face.
///
/// This is the narrow face-to-carrier relation used by standard freeform
/// aliases. It does not claim that the complete E5 topology graph is closed.
pub(in crate::families) fn face_surface_references<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<impl Iterator<Item = (u32, u32)> + 'a, CodecError> {
    Ok(records(ctx, bytes)?
        .filter(|record| record.class == 0x00)
        .filter_map(|record| {
            // At most 126 loop references, each at least one payload byte the
            // record scan admitted.
            let count = record.payload.first()?.checked_sub(0x81)?;
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
        }))
}

fn is_surface_carrier_class(class: u8) -> bool {
    matches!(class, 0xc8 | 0xc9 | 0xca | 0xcc | 0xe7)
}

/// Checks that a curve-support side resolves to a direct p-curve or to a
/// finite, acyclic chain of intersection-support wrappers.
///
/// `closed` holds the supports already proven to close, so each support is
/// walked once per topology however many edges reference it. The walk stack
/// and `closed` live in the caller's scratch reservation.
fn curve_support_reference_closes(
    ctx: &DecodeContext<'_>,
    reference: u32,
    pcurves: &BTreeMap<u32, E5Pcurve>,
    supports: &BTreeMap<u32, E5CurveSupport>,
    scratch: &mut ScopedReservation<'_>,
    closed: &mut HashSet<u32>,
) -> Result<bool, CodecError> {
    let resolved = |reference: &u32, closed: &HashSet<u32>| -> Result<bool, CodecError> {
        Ok(
            ctx.contains_key_btree_map(pcurves, reference, "catia_e5_support_pcurve_lookup")?
                || ctx.contains_hash_set(closed, reference, "catia_e5_support_closed_lookup")?,
        )
    };
    if resolved(&reference, closed)? {
        return Ok(true);
    }
    let mut visiting = HashSet::new();
    let mut stack = Vec::new();
    ctx.push_scoped_vec(
        scratch,
        &mut stack,
        (reference, false),
        "catia_e5_support_stack",
    )?;
    while let Some((reference, leaving)) = stack.pop() {
        if leaving {
            ctx.remove_hash_set(&mut visiting, &reference, "catia_e5_support_visiting")?;
            scratch.with_storage(|| {
                ctx.insert_hash_set(closed, reference, "catia_e5_support_closed")
            })?;
            continue;
        }
        if resolved(&reference, closed)? {
            continue;
        }
        let Some(E5CurveSupportKind::Intersection([first, second])) = ctx
            .get_btree_map(supports, &reference, "catia_e5_support_lookup")?
            .map(|support| &support.kind)
        else {
            return Ok(false);
        };
        let entered = scratch.with_storage(|| {
            ctx.insert_hash_set(&mut visiting, reference, "catia_e5_support_visiting")
        })?;
        if !entered {
            return Ok(false);
        }
        ctx.push_scoped_vec(
            scratch,
            &mut stack,
            (reference, true),
            "catia_e5_support_stack",
        )?;
        for child in [*second, *first] {
            if resolved(&child, closed)? {
                continue;
            }
            let intersection = ctx
                .get_btree_map(supports, &child, "catia_e5_support_lookup")?
                .is_some_and(E5CurveSupport::is_intersection);
            if !intersection
                || ctx.contains_hash_set(&visiting, &child, "catia_e5_support_visiting")?
            {
                return Ok(false);
            }
            ctx.push_scoped_vec(
                scratch,
                &mut stack,
                (child, false),
                "catia_e5_support_stack",
            )?;
        }
    }
    Ok(true)
}

/// Unique parameters by bound and representation; duplicates remain ambiguous.
pub(super) type E5BoundParameters = HashMap<(u32, u32), Option<FiniteReal>>;

pub(super) fn index_bound_parameters<'storage>(
    ctx: &'storage DecodeContext<'_>,
    bounds: &BTreeMap<u32, E5Bounds>,
) -> Result<(E5BoundParameters, ScopedReservation<'storage>), CodecError> {
    const OPERATION: &str = "catia_e5_bound_parameter_index";
    let mut storage = ctx.reserve_scoped(0, OPERATION)?;
    let parameters = storage.with_storage(|| {
        let mut parameters = HashMap::new();
        for (&bound, entries) in ctx.admit_iter(bounds, OPERATION)? {
            for entry in ctx.admit_iter(&entries.entries, OPERATION)? {
                match ctx.entry_hash_map(
                    &mut parameters,
                    (bound, entry.representation),
                    OPERATION,
                )? {
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(Some(entry.parameter));
                    }
                    std::collections::hash_map::Entry::Occupied(mut slot) => {
                        slot.insert(None);
                    }
                }
            }
        }
        Ok::<_, CodecError>(parameters)
    })?;
    Ok((parameters, storage))
}

/// The parameter stated exactly once for a bound and representation.
fn bound_representation_parameter(
    ctx: &DecodeContext<'_>,
    parameters: &E5BoundParameters,
    bound_ref: u32,
    representation: u32,
) -> Result<Option<FiniteReal>, CodecError> {
    Ok(ctx
        .get_hash_map(
            parameters,
            &(bound_ref, representation),
            "catia_e5_bound_parameter_lookup",
        )?
        .copied()
        .flatten())
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
    // The reference lane precedes the parameter lane, so each entry is
    // created with its reference and completed from the second lane.
    let mut position = 1;
    let mut entries = ctx.collection_vec(count, "catia_e5_bound_entries")?;
    let mut steps = 0..count;
    while let Some(_) = ctx.next_charged(&mut steps, "catia_e5_bound_reference_scan")? {
        let Some(representation) = wire::tokens::object_ref(record.payload, &mut position, false)
        else {
            return Ok(None);
        };
        entries.push(E5BoundEntry {
            representation,
            parameter: FiniteReal::ZERO,
            code: 0,
        });
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
    let mut steps = entries.iter_mut();
    while let Some(entry) =
        ctx.next_charged(&mut steps, "catia_e5_bound_entry_representation_scan")?
    {
        let Some((parameter, code)) =
            (|| Some((FiniteReal::new(view.f64_le()?)?, view.u32_le()?)))()
        else {
            return Ok(None);
        };
        entry.parameter = parameter;
        entry.code = code;
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

/// The finite little-endian `f64` at `index` of an eight-byte lane.
fn lane_real(lane: &[u8], index: usize) -> Option<FiniteReal> {
    wire::bytes::f64_le(lane, index.checked_mul(8)?)
}

/// The little-endian `u32` at `index` of a four-byte lane.
fn lane_u32(lane: &[u8], index: usize) -> Option<u32> {
    View::u32_le_at(lane, index.checked_mul(4)?)
}

/// Takes a lane of `count` items of `width` bytes.
fn take_lane<'a>(view: &mut View<'a>, count: usize, width: usize) -> Option<&'a [u8]> {
    view.take(count.checked_mul(width)?)
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
    let (Some(knots), Some(multiplicities)) = (
        take_lane(&mut view, knot_count, 8),
        take_lane(&mut view, knot_count, 4),
    ) else {
        return Ok(None);
    };
    let Some(max_control_count) = view
        .remaining()
        .checked_sub(E5_NURBS_PCURVE_TAIL_BYTES)
        .map(|remaining| remaining / 16)
    else {
        return Ok(None);
    };
    let Some((expanded_knots, control_count)) = expand_nurbs_knots_limited(
        ctx,
        degree,
        knot_count,
        knots,
        multiplicities,
        max_control_count,
    )?
    else {
        return Ok(None);
    };
    let Some(control_bytes) = take_lane(&mut view, control_count, 16) else {
        return Ok(None);
    };
    if view.remaining() != E5_NURBS_PCURVE_TAIL_BYTES {
        return Ok(None);
    }
    let Some(range) = usize::try_from(degree).ok().and_then(|degree| {
        Some([
            *expanded_knots.get(degree)?,
            *expanded_knots.get(control_count)?,
        ])
    }) else {
        return Ok(None);
    };
    if range[0] >= range[1] {
        return Ok(None);
    }
    let mut control_points = ctx.vector_storage(control_count, "catia_e5_pcurve_controls")?;
    let mut steps = 0..control_count;
    while let Some(index) = ctx.next_charged(&mut steps, "catia_e5_pcurve_control_scan")? {
        let (Some(u), Some(v)) = (
            lane_real(control_bytes, index * 2),
            lane_real(control_bytes, index * 2 + 1),
        ) else {
            return Ok(None);
        };
        ctx.push_vec(&mut control_points, [u, v], "catia_e5_pcurve_controls")?;
    }
    Ok(Some(E5Pcurve::Nurbs {
        surface,
        degree,
        knots: expanded_knots,
        control_points,
        range,
    }))
}

/// Expands strictly increasing finite knots by their nonzero multiplicities
/// when the implied control count exceeds the degree and fits
/// `max_control_count`.
fn expand_nurbs_knots_limited(
    ctx: &DecodeContext<'_>,
    degree: u32,
    knot_count: usize,
    knots: &[u8],
    multiplicities: &[u8],
    max_control_count: usize,
) -> Result<Option<(Vec<FiniteReal>, usize)>, CodecError> {
    let multiplicity = |index: usize| {
        lane_u32(multiplicities, index)
            .and_then(|multiplicity| usize::try_from(multiplicity).ok())
            .filter(|multiplicity| *multiplicity != 0)
    };
    let mut previous = None;
    let mut total = 0usize;
    let valid = ctx.all_by(
        0..knot_count,
        |index| {
            let (Some(knot), Some(count)) = (lane_real(knots, index), multiplicity(index)) else {
                return Ok(false);
            };
            let ordered = previous.is_none_or(|previous| previous < knot);
            previous = Some(knot);
            let Some(next) = total.checked_add(count) else {
                return Ok(false);
            };
            total = next;
            Ok(ordered)
        },
        "catia_e5_knot_order_scan",
    )?;
    if !valid {
        return Ok(None);
    }
    let Some((degree, control_count)) = usize::try_from(degree)
        .ok()
        .and_then(|degree| Some((degree, total.checked_sub(degree.checked_add(1)?)?)))
    else {
        return Ok(None);
    };
    if control_count <= degree || control_count > max_control_count {
        return Ok(None);
    }
    let mut expanded = ctx.vector_storage(total, "catia_e5_pcurve_expanded_knots")?;
    for index in ctx.admit_iter(0..knot_count, "catia_e5_knot_expansion_scan")? {
        let (Some(knot), Some(count)) = (lane_real(knots, index), multiplicity(index)) else {
            return Ok(None);
        };
        let length = expanded.len() + count;
        ctx.resize_vec(
            &mut expanded,
            length,
            knot,
            "catia_e5_pcurve_expanded_knots",
        )?;
    }
    Ok(Some((expanded, control_count)))
}

/// The borrowed per-knot lanes of a class-`0xa0` degree-5 UV jet.
struct JetLanes<'a> {
    /// Every knot after the implicit leading zero.
    knot_tail: &'a [u8],
    multiplicities: &'a [u8],
    x: &'a [u8],
    y: &'a [u8],
    dx: &'a [u8],
    dy: &'a [u8],
    ddx: &'a [u8],
    ddy: &'a [u8],
}

impl JetLanes<'_> {
    fn knot(&self, index: usize) -> Option<FiniteReal> {
        match index.checked_sub(1) {
            None => Some(FiniteReal::ZERO),
            Some(tail_index) => lane_real(self.knot_tail, tail_index),
        }
    }

    fn site(&self, index: usize) -> Option<E5PcurveJetSite> {
        Some(E5PcurveJetSite {
            knot: self.knot(index)?,
            multiplicity: lane_u32(self.multiplicities, index)?,
            point: [lane_real(self.x, index)?, lane_real(self.y, index)?],
            first_derivatives: [lane_real(self.dx, index)?, lane_real(self.dy, index)?],
            second_derivatives: [lane_real(self.ddx, index)?, lane_real(self.ddy, index)?],
        })
    }
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
    if degree != E5Pcurve::JET_DEGREE
        || site_count == 0
        || [zero0, zero1, zero2, zero3, zero4] != [0; 5]
    {
        return Ok(None);
    }
    let Some((lanes, range)) = (|| {
        let knot_tail = take_lane(&mut view, site_count - 1, 8)?;
        let multiplicities = take_lane(&mut view, site_count, 4)?;
        if usize::try_from(view.u32_le()?).ok()? != site_count {
            return None;
        }
        let x = take_lane(&mut view, site_count, 8)?;
        let y = take_lane(&mut view, site_count, 8)?;
        let dx = take_lane(&mut view, site_count, 8)?;
        let dy = take_lane(&mut view, site_count, 8)?;
        if view.u16_le()? != 1 {
            return None;
        }
        let ddx = take_lane(&mut view, site_count, 8)?;
        let ddy = take_lane(&mut view, site_count, 8)?;
        let range = [finite_f64_le(&mut view)?, finite_f64_le(&mut view)?];
        view.is_empty().then_some((
            JetLanes {
                knot_tail,
                multiplicities,
                x,
                y,
                dx,
                dy,
                ddx,
                ddy,
            },
            range,
        ))
    })() else {
        return Ok(None);
    };
    let Some(final_knot) = lanes.knot(site_count - 1).map(FiniteReal::get) else {
        return Ok(None);
    };
    if range[0].get() != 0.0
        || (range[1].get() - final_knot).abs() > EPS_PARAMETER_ENDPOINT * final_knot.abs()
    {
        return Ok(None);
    }
    let Some(expected_sum) = u32::try_from(site_count)
        .ok()
        .and_then(|count| count.checked_mul(3))
        .and_then(|interior| (degree + 1).checked_add(interior))
    else {
        return Ok(None);
    };
    // The sites are built under a scoped reservation and retained only when
    // every knot, multiplicity and channel is admissible.
    let mut storage = ctx.reserve_scoped(0, "catia_e5_jet_sites")?;
    let mut sites =
        storage.with_storage(|| ctx.vector_storage(site_count, "catia_e5_jet_sites"))?;
    let mut previous_knot = None;
    let mut sum = 0_u32;
    let complete = ctx.all_by(
        0..site_count,
        |index| {
            let Some(site) = lanes.site(index) else {
                return Ok(false);
            };
            let end = index == 0 || index == site_count - 1;
            let expected = if end { degree + 1 } else { 3 };
            let ordered = previous_knot.is_none_or(|previous| previous < site.knot);
            previous_knot = Some(site.knot);
            let Some(next) = sum.checked_add(site.multiplicity) else {
                return Ok(false);
            };
            sum = next;
            if !ordered || site.multiplicity != expected {
                return Ok(false);
            }
            ctx.push_vec(&mut sites, site, "catia_e5_jet_sites")?;
            Ok(true)
        },
        "catia_e5_jet_site_scan",
    )?;
    if !complete || sum != expected_sum {
        return Ok(None);
    }
    storage.commit()?;
    Ok(Some(E5Pcurve::Jet {
        surface,
        sites,
        range,
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
#[derive(Clone, Copy)]
struct PlaneDigonOrientationHintInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input6,
> {
    face_trailer_sign: Sign,
    surface_class: Option<u8>,
    pcurve_ids: &'input0 [u32],
    edge_ids: &'input1 [u32],
    reversed: &'input2 [bool],
    outer: Option<bool>,
    edges: &'input3 BTreeMap<u32, E5Edge>,
    pcurves: &'input4 BTreeMap<u32, E5Pcurve>,
    curve_supports: &'input5 BTreeMap<u32, E5CurveSupport>,
    bound_parameters: &'input6 E5BoundParameters,
}

fn plane_digon_orientation_hint(
    ctx: &DecodeContext<'_>,
    inputs: PlaneDigonOrientationHintInputs<'_, '_, '_, '_, '_, '_, '_>,
) -> Result<Option<Sign>, CodecError> {
    const EPS_PLANE_DIGON: f64 = 1.0e-8;

    let PlaneDigonOrientationHintInputs {
        face_trailer_sign,
        surface_class,
        pcurve_ids,
        edge_ids,
        reversed,
        outer,
        edges,
        pcurves,
        curve_supports,
        bound_parameters,
    } = inputs;

    let (
        Some(0xc8),
        Some(outer),
        &[first_pcurve_id, second_pcurve_id],
        &[first_edge_id, second_edge_id],
        &[first_reversed, second_reversed],
    ) = (surface_class, outer, pcurve_ids, edge_ids, reversed)
    else {
        return Ok(None);
    };
    let (Some(first_edge), Some(second_edge)) = (
        ctx.get_btree_map(edges, &first_edge_id, "catia_e5_plane_digon_edge_lookup")?,
        ctx.get_btree_map(edges, &second_edge_id, "catia_e5_plane_digon_edge_lookup")?,
    ) else {
        return Ok(None);
    };
    let same_endpoints = (first_edge.start_vertex == second_edge.start_vertex
        && first_edge.end_vertex == second_edge.end_vertex)
        || (first_edge.start_vertex == second_edge.end_vertex
            && first_edge.end_vertex == second_edge.start_vertex);
    if !same_endpoints || first_edge.support == second_edge.support {
        return Ok(None);
    }

    let (
        Some(E5Pcurve::Jet {
            surface: first_surface,
            sites: first_sites,
            range: first_range,
        }),
        Some(E5Pcurve::Jet {
            surface: second_surface,
            sites: second_sites,
            range: second_range,
        }),
    ) = (
        ctx.get_btree_map(
            pcurves,
            &first_pcurve_id,
            "catia_e5_plane_digon_pcurve_lookup",
        )?,
        ctx.get_btree_map(
            pcurves,
            &second_pcurve_id,
            "catia_e5_plane_digon_pcurve_lookup",
        )?,
    )
    else {
        return Ok(None);
    };
    if first_surface != second_surface || first_sites.len() < 2 || second_sites.len() < 2 {
        return Ok(None);
    }

    let close = |left: f64, right: f64| {
        left.is_finite()
            && right.is_finite()
            && (left - right).abs() <= EPS_PLANE_DIGON * (1.0 + left.abs().max(right.abs()))
    };
    let close_point =
        |left: [f64; 2], right: [f64; 2]| close(left[0], right[0]) && close(left[1], right[1]);
    let (
        Some(first_start_site),
        Some(first_end_site),
        Some(second_start_site),
        Some(second_end_site),
    ) = (
        first_sites.first(),
        first_sites.last(),
        second_sites.first(),
        second_sites.last(),
    )
    else {
        return Ok(None);
    };
    let first_start = first_start_site.point.map(FiniteReal::get);
    let first_end = first_end_site.point.map(FiniteReal::get);
    let second_start = second_start_site.point.map(FiniteReal::get);
    let second_end = second_end_site.point.map(FiniteReal::get);
    let same_endpoint_pair = (close_point(first_start, second_start)
        && close_point(first_end, second_end))
        || (close_point(first_start, second_end) && close_point(first_end, second_start));
    if !same_endpoint_pair {
        return Ok(None);
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
        return Ok(None);
    }
    let first_radius = first_start_radius;
    if !ctx.all_by(
        first_sites.iter().chain(second_sites),
        |site| {
            Ok(close(
                (site.point[0].get() - center[0]).hypot(site.point[1].get() - center[1]),
                first_radius,
            ))
        },
        "catia_e5_plane_digon_site_scan",
    )? {
        return Ok(None);
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
    let signed_parameter_direction = |edge: &E5Edge,
                                      pcurve_id: u32,
                                      native_range: [f64; 2],
                                      reversed: bool|
     -> Result<Option<Sign>, CodecError> {
        let (Some(start), Some(end)) = (
            bound_representation_parameter(ctx, bound_parameters, edge.parameter_start, pcurve_id)?,
            bound_representation_parameter(ctx, bound_parameters, edge.parameter_end, pcurve_id)?,
        ) else {
            return Ok(None);
        };
        let bound_span = end.get() - start.get();
        let native_span = native_range[1] - native_range[0];
        if bound_span.abs() <= EPS_PLANE_DIGON || native_span.abs() <= EPS_PLANE_DIGON {
            return Ok(None);
        }
        let direction = if bound_span * native_span > 0.0 {
            Sign::Positive
        } else {
            Sign::Negative
        };
        Ok(Some(direction.combine(if reversed {
            Sign::Negative
        } else {
            Sign::Positive
        })))
    };
    let (Some(first_direction), Some(second_direction)) = (
        signed_parameter_direction(
            first_edge,
            first_pcurve_id,
            first_range.map(FiniteReal::get),
            first_reversed,
        )?,
        signed_parameter_direction(
            second_edge,
            second_pcurve_id,
            second_range.map(FiniteReal::get),
            second_reversed,
        )?,
    ) else {
        return Ok(None);
    };
    let (Some(first_winding), Some(second_winding)) = (
        native_arc_sign(
            first_start,
            first_start_site.first_derivatives.map(FiniteReal::get),
        ),
        native_arc_sign(
            second_start,
            second_start_site.first_derivatives.map(FiniteReal::get),
        ),
    ) else {
        return Ok(None);
    };
    let first_winding = first_winding.combine(first_direction);
    if first_winding != second_winding.combine(second_direction) {
        return Ok(None);
    }

    let (Some(first_support), Some(second_support)) = (
        ctx.get_btree_map(
            curve_supports,
            &first_edge.support,
            "catia_e5_plane_digon_support_lookup",
        )?,
        ctx.get_btree_map(
            curve_supports,
            &second_edge.support,
            "catia_e5_plane_digon_support_lookup",
        )?,
    ) else {
        return Ok(None);
    };
    if !first_support.is_intersection()
        || !second_support.is_intersection()
        || !first_support.references(first_pcurve_id)
        || !second_support.references(second_pcurve_id)
    {
        return Ok(None);
    }
    let mut intervals =
        [first_support.range, second_support.range].map(|range| range.map(FiniteReal::get));
    for interval in &mut intervals {
        if interval[0] == interval[1] {
            return Ok(None);
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
        return Ok(None);
    }

    let role_sign = if outer {
        Sign::Positive
    } else {
        Sign::Negative
    };
    Ok(Some(
        face_trailer_sign.combine(role_sign).combine(first_winding),
    ))
}

fn solve_absolute_orientation(
    ctx: &DecodeContext<'_>,
    faces: &mut [E5Face],
) -> Result<bool, CodecError> {
    // Locations, occurrences, adjacency, assignments and components are
    // scratch; only each loop's oriented member list is retained.
    let mut scratch = ctx.reserve_scoped(0, "catia e5 orientation scratch")?;
    let mut location_count = 0usize;
    for face in ctx.admit_iter(&*faces, "catia_e5_orientation_location_count_face_scan")? {
        for loop_ in ctx.admit_iter(&face.loops, "catia_e5_orientation_location_count_loop_scan")? {
            if !loop_.members.is_empty() {
                location_count = location_count.checked_add(1).ok_or_else(|| {
                    ctx.refuse_codec_limit("catia_e5_orientation_locations", u64::MAX, u64::MAX)
                })?;
            }
        }
    }
    let mut locations = Vec::new();
    ctx.reserve_scoped_vec(
        &mut scratch,
        &mut locations,
        location_count,
        "catia e5 orientation locations",
    )?;
    for (face_index, face) in ctx
        .admit_iter(&*faces, "catia_e5_orientation_location_face_scan")?
        .enumerate()
    {
        for (loop_index, loop_) in ctx
            .admit_iter(&face.loops, "catia_e5_orientation_location_loop_scan")?
            .enumerate()
        {
            if !loop_.members.is_empty() {
                locations.push((face_index, loop_index));
            }
        }
    }
    let mut occurrences = BTreeMap::<u32, Vec<(usize, Sign)>>::new();
    for (node, &(face_index, loop_index)) in ctx
        .admit_iter(&locations, "catia_e5_orientation_location_scan")?
        .enumerate()
    {
        let loop_ = &faces[face_index].loops[loop_index];
        for member in ctx.admit_iter(&loop_.members, "catia_e5_orientation_loop_member_scan")? {
            let sign = if member.reversed {
                Sign::Negative
            } else {
                Sign::Positive
            };
            scratch.with_storage(|| {
                ctx.push_btree_group(
                    &mut occurrences,
                    member.edge_use,
                    (node, sign),
                    "catia e5 orientation edge occurrence keys",
                    "catia e5 orientation edge occurrences",
                )
            })?;
        }
    }
    let mut adjacency = scratch.with_storage(|| {
        ctx.collect_indexed_vec(locations.len(), "catia e5 orientation adjacency", |_| {
            Ok(Vec::<(usize, Sign)>::new())
        })
    })?;
    for [(left, left_r), (right, right_r)] in ctx
        .admit_iter(&occurrences, "catia_e5_orientation_occurrence_group_scan")?
        .map(|(_, uses)| uses)
        .filter_map(|uses| <&[_; 2]>::try_from(uses.as_slice()).ok())
    {
        let relation = left_r.flipped().combine(*right_r);
        scratch.with_storage(|| {
            ctx.push_vec(
                &mut adjacency[*left],
                (*right, relation),
                "catia e5 orientation adjacent edges",
            )?;
            ctx.push_vec(
                &mut adjacency[*right],
                (*left, relation),
                "catia e5 orientation adjacent edges",
            )
        })?;
    }
    let mut solved = scratch.with_storage(|| {
        ctx.alloc_filled(locations.len(), None, "catia e5 orientation assignments")
    })?;
    let mut component = Vec::new();
    for root in ctx.admit_iter(0..locations.len(), "catia_e5_orientation_root_scan")? {
        if solved[root].is_some() {
            continue;
        }
        solved[root] = Some(Sign::Positive);
        ctx.clear_vec(&mut component, "catia e5 orientation component")?;
        ctx.push_scoped_vec(
            &mut scratch,
            &mut component,
            (root, Sign::Positive),
            "catia e5 orientation component",
        )?;
        let mut cursor = 0;
        let mut consistent = true;
        while cursor < component.len() {
            let (node, value) = component[cursor];
            cursor += 1;
            for &(neighbor, relation) in
                ctx.admit_iter(&adjacency[node], "catia_e5_orientation_neighbor_scan")?
            {
                let expected = value.combine(relation);
                match solved[neighbor] {
                    Some(actual) if actual != expected => consistent = false,
                    Some(_) => {}
                    None => {
                        solved[neighbor] = Some(expected);
                        ctx.push_scoped_vec(
                            &mut scratch,
                            &mut component,
                            (neighbor, expected),
                            "catia e5 orientation component",
                        )?;
                    }
                }
            }
        }
        let mut exact_flip = None;
        if consistent {
            let mut steps = component.iter();
            while let Some(&(node, value)) =
                ctx.next_charged(&mut steps, "catia_e5_orientation_hint_scan")?
            {
                let (face_index, loop_index) = locations[node];
                let Some(hint) = faces[face_index].loops[loop_index].orientation_hint else {
                    continue;
                };
                let candidate = hint.combine(value);
                match exact_flip {
                    Some(existing) if existing != candidate => {
                        consistent = false;
                        break;
                    }
                    Some(_) => {}
                    None => exact_flip = Some(candidate),
                }
            }
        }
        if !consistent {
            for &(node, _) in ctx.admit_iter(
                &component,
                "catia_e5_orientation_inconsistent_component_scan",
            )? {
                solved[node] = None;
            }
            continue;
        }
        let flip = match exact_flip {
            Some(flip) => flip,
            None => {
                let plus_matches = ctx
                    .admit_iter(&component, "catia_e5_orientation_trailer_match_scan")?
                    .filter(|&&(node, value)| {
                        let (face, _) = locations[node];
                        value == faces[face].trailer_sign
                    })
                    .count();
                if component.len() - plus_matches > plus_matches {
                    Sign::Negative
                } else {
                    Sign::Positive
                }
            }
        };
        if flip == Sign::Negative {
            for &(node, value) in ctx.admit_iter(&component, "catia_e5_orientation_flip_scan")? {
                solved[node] = Some(value.flipped());
            }
        }
    }
    for (node, &(face_index, loop_index)) in ctx
        .admit_iter(&locations, "catia_e5_orientation_apply_scan")?
        .enumerate()
    {
        let Some(g) = solved[node] else {
            continue;
        };
        let loop_ = &mut faces[face_index].loops[loop_index];
        let flip = g == Sign::Negative;
        let count = loop_.members.len();
        let mut oriented_members = ctx.vector_storage(count, "catia e5 oriented members")?;
        for position in ctx.admit_iter(0..count, "catia_e5_orientation_member_index_scan")? {
            let serialized_index = if flip { count - 1 - position } else { position };
            ctx.push_vec(
                &mut oriented_members,
                E5OrientedMember {
                    serialized_index,
                    reversed: loop_.members[serialized_index].reversed ^ flip,
                },
                "catia e5 oriented members",
            )?;
        }
        loop_.oriented_members = Some(oriented_members);
    }
    ctx.all_by(
        &solved,
        |assignment| Ok(assignment.is_some()),
        "catia_e5_orientation_consistency_scan",
    )
}

fn parse_bodies(
    ctx: &DecodeContext<'_>,
    records: &[Record<'_>],
    by_id: &HashMap<u32, Record<'_>>,
) -> Result<Option<Vec<E5Body>>, CodecError> {
    let mut bodies = Vec::new();
    let mut records = records.iter();
    while let Some(record) = ctx.next_charged(&mut records, "catia_e5_body_record_scan")? {
        if record.class != 0x01 {
            continue;
        }
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
        let Some(root) = ctx.get_hash_map(by_id, &root_id, "catia_e5_record_lookup")? else {
            return Ok(None);
        };
        if root.class != 0x08 {
            return Ok(None);
        }
        let Some(faces) = parse_body_root(ctx, root.payload)? else {
            return Ok(None);
        };
        if !ctx.all_by(
            &faces,
            |face| {
                Ok(ctx
                    .get_hash_map(by_id, face, "catia_e5_record_lookup")?
                    .is_some_and(|target| target.class == 0x00))
            },
            "catia_e5_body_face_class_scan",
        )? {
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
    Ok(Some(bodies))
}

fn parse_body_root(
    ctx: &DecodeContext<'_>,
    payload: &[u8],
) -> Result<Option<Vec<u32>>, CodecError> {
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
    let mut steps = 0..count;
    while let Some(_) = ctx.next_charged(&mut steps, "catia_e5_body_root_face_scan")? {
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
    if payload.get(position..position + 2) == Some(&[0x08, count]) {
        position += 2;
    } else if faces.len() <= 0x7f
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
    let signed = ctx.all_by(
        sign_bytes.chunks(2),
        |bytes| Ok(View::i16_le_at(bytes, 0).and_then(Sign::from_i16).is_some()),
        "catia_e5_body_sign_scan",
    )?;
    Ok(signed.then_some(faces))
}

fn records<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
) -> Result<impl Iterator<Item = Record<'a>> + 'a, CodecError> {
    Ok(e5_frames(ctx, bytes)?.map(|frame| Record {
        class: frame.class,
        id: frame.record_id(bytes),
        payload: frame.payload(bytes),
    }))
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
    let mut steps = 0..count;
    while let Some(_) = ctx.next_charged(&mut steps, "catia_e5_face_loop_scan")? {
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

fn parse_loop(ctx: &DecodeContext<'_>, record: &Record<'_>) -> Result<Option<RawLoop>, CodecError> {
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
    let mut steps = 0..member_count / 2;
    while let Some(_) = ctx.next_charged(&mut steps, "catia_e5_loop_member_scan")? {
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
    let Some(trailing) = record.payload.get(position..) else {
        return Ok(None);
    };
    let outer = match parse_loop_signs(ctx, trailing, member_count / 2) {
        Ok(Ok(outer)) => outer,
        Ok(Err(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
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

fn parse_loop_signs(
    ctx: &DecodeContext<'_>,
    trailing: &[u8],
    edge_count: usize,
) -> Result<Result<Option<bool>, LoopSignError>, CodecError> {
    if trailing.is_empty() {
        return Ok(Ok(None));
    }
    let expected_head = u8::try_from(edge_count)
        .ok()
        .and_then(|n| 0x80u8.checked_add(n))
        .ok_or(LoopSignError::Count);
    let expected_head = match expected_head {
        Ok(expected_head) => expected_head,
        Err(error) => return Ok(Err(error)),
    };
    if trailing.first() != Some(&expected_head) || trailing.len() != 1 + 2 * (3 * edge_count + 4) {
        return Ok(Err(LoopSignError::Frame));
    }
    let mut outer = None;
    let mut signs = trailing[1..].chunks(2).enumerate();
    while let Some((index, bytes)) = ctx.next_charged(&mut signs, "catia_e5_loop_sign_scan")? {
        let Some(sign) = View::i16_le_at(bytes, 0) else {
            return Ok(Err(LoopSignError::Frame));
        };
        if !matches!(sign, -1..=1) {
            return Ok(Err(LoopSignError::Sign));
        }
        if index == 1 {
            outer = Some(sign);
        }
    }
    let Some(outer) = outer else {
        return Ok(Err(LoopSignError::Frame));
    };
    if !matches!(outer, -1 | 1) {
        return Ok(Err(LoopSignError::Sign));
    }
    Ok(Ok(Some(outer == 1)))
}

fn parse_edge(ctx: &DecodeContext<'_>, record: &Record<'_>) -> Result<Option<E5Edge>, CodecError> {
    if record.payload.first() != Some(&0x85) {
        return Ok(None);
    }
    let mut position = 1;
    let Some(support) = wire::tokens::object_ref(record.payload, &mut position, false) else {
        return Ok(None);
    };
    let Some(start_vertex) = wire::tokens::object_ref(record.payload, &mut position, false) else {
        return Ok(None);
    };
    let Some(end_vertex) = wire::tokens::object_ref(record.payload, &mut position, false) else {
        return Ok(None);
    };
    let Some(parameter_start) = wire::tokens::object_ref(record.payload, &mut position, false)
    else {
        return Ok(None);
    };
    let Some(parameter_end) = wire::tokens::object_ref(record.payload, &mut position, false) else {
        return Ok(None);
    };
    let tail = ctx.copy_slice(&record.payload[position..], "catia_e5_edge_tail")?;
    Ok(Some(E5Edge {
        support,
        start_vertex,
        end_vertex,
        parameter_start,
        parameter_end,
        tail,
    }))
}

fn solve_loop_chain(
    ctx: &DecodeContext<'_>,
    edge_ids: &[u32],
    edges: &BTreeMap<u32, E5Edge>,
) -> Result<Option<Vec<bool>>, CodecError> {
    let Some((first_id, rest)) = edge_ids.split_first() else {
        return Ok(None);
    };
    let Some(first) = ctx.get_btree_map(edges, first_id, "catia_e5_chain_edge_lookup")? else {
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
        let mut missing = false;
        let chained = ctx.all_by(
            rest,
            |edge_id| {
                let Some(edge) = ctx.get_btree_map(edges, edge_id, "catia_e5_chain_edge_lookup")?
                else {
                    missing = true;
                    return Ok(false);
                };
                let reversed = match (edge.start_vertex == current, edge.end_vertex == current) {
                    (true, false) => false,
                    (false, true) => true,
                    _ => return Ok(false),
                };
                current = if reversed {
                    edge.start_vertex
                } else {
                    edge.end_vertex
                };
                ctx.push_vec(&mut senses, reversed, "catia_e5_chain_senses")?;
                Ok(true)
            },
            "catia_e5_chain_edge_scan",
        )?;
        if missing {
            return Ok(None);
        }
        if chained && current == initial {
            ctx.push_vec(&mut solutions, senses, "catia_e5_chain_solutions")?;
        }
    }
    match solutions.as_slice() {
        [_] => Ok(solutions.pop()),
        // A two-edge loop closes both ways with opposite senses; the solution
        // that traverses the first edge forward is the relative gauge.
        [left, right] if edge_ids.len() == 2 => {
            let (&[left_first, left_second], &[right_first, right_second]) =
                (left.as_slice(), right.as_slice())
            else {
                return Ok(None);
            };
            if left_first == right_first || left_second == right_second {
                return Ok(None);
            }
            let forward = usize::from(left_first);
            Ok(Some(solutions.swap_remove(forward)))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        curve_support_reference_closes, index_bound_parameters, parse_body_root, parse_bounds,
        parse_jet_pcurve, parse_nurbs_pcurve, parse_pcurve, parse_topology,
        plane_digon_orientation_hint, records, solve_absolute_orientation, solve_loop_chain,
        E5BoundEntry, E5Bounds, E5CurveSupport, E5CurveSupportKind, E5Edge, E5Face, E5Loop,
        E5LoopMember, E5Pcurve, E5PcurveJetSite, E5Topology, Record, Sign,
    };
    use crate::families::e5::tests::e5_loop_members;
    use crate::test_support::test_b5::{finite, finite_lane, finite_pair};
    use crate::test_support::test_e5::append_e5_record;
    use std::collections::BTreeMap;

    fn support_closes(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        reference: u32,
        pcurves: &BTreeMap<u32, E5Pcurve>,
        supports: &BTreeMap<u32, E5CurveSupport>,
    ) -> Result<bool, cadmpeg_core::CodecError> {
        let mut scratch = ctx.reserve_scoped(0, "catia_e5_support_test_scratch")?;
        let mut closed = std::collections::HashSet::new();
        curve_support_reference_closes(ctx, reference, pcurves, supports, &mut scratch, &mut closed)
    }

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
    fn e5_bound_entry_scan_propagates_caller_work_refusal() {
        let mut payload = vec![0x81, 0x85, 0x81];
        payload.extend_from_slice(&1.0_f64.to_le_bytes());
        payload.extend_from_slice(&7_u32.to_le_bytes());
        let record = Record {
            class: 0x01,
            id: 1,
            payload: &payload,
        };
        assert!(matches!(
            crate::test_support::with_work_refusal(
                "catia_e5_bound_entry_representation_scan",
                |ctx| parse_bounds(ctx, &record)
            ),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_e5_bound_entry_representation_scan"
        ));
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
            !crate::test_support::with_service_context(|ctx| support_closes(
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
            crate::test_support::with_service_context(|ctx| support_closes(
                ctx, 1, &pcurves, &supports
            ))
            .expect("service resource budget")
        );
        for (cap, operation) in [
            (0, "catia_e5_support_stack"),
            (1, "catia_e5_support_visiting"),
        ] {
            assert!(matches!(
                crate::test_support::with_collection_limit(cap, |ctx| support_closes(ctx, 1, &pcurves, &supports)),
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
        // The distinct-knot and multiplicity lanes are read in place; only
        // the kept expanded knots and control points are collections.
        let mut refused = std::collections::BTreeSet::new();
        for cap in 0..16 {
            if let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
                crate::test_support::with_collection_limit(cap, |ctx| {
                    parse_nurbs_pcurve(ctx, &payload, 2, 7)
                })
            {
                refused.insert(limit.operation);
            }
        }
        assert_eq!(
            refused,
            std::collections::BTreeSet::from([
                "catia_e5_pcurve_expanded_knots",
                "catia_e5_pcurve_controls",
            ])
        );
        for operation in [
            "catia_e5_knot_order_scan",
            "catia_e5_knot_expansion_scan",
            "catia_e5_pcurve_control_scan",
        ] {
            let refusal = crate::test_support::with_work_refusal(operation, |ctx| {
                parse_nurbs_pcurve(ctx, &payload, 2, 7)
            });
            assert!(matches!(
                refusal,
                Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                    if limit.operation == operation
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
        // The lanes are read in place; the kept sites are the one collection.
        assert!(matches!(
            crate::test_support::with_collection_limit(0, |ctx| parse_jet_pcurve(ctx, &payload, 0, 7)),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "catia_e5_jet_sites"
        ));
        assert!(matches!(
            crate::test_support::with_work_refusal("catia_e5_jet_site_scan", |ctx| {
                parse_jet_pcurve(ctx, &payload, 0, 7)
            }),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == "catia_e5_jet_site_scan"
        ));
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
        assert_eq!(
            crate::test_support::with_service_context(|ctx| solve_loop_chain(ctx, &[1, 2], &edges))
                .expect("service resource budget"),
            Some(vec![false, false])
        );
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
                    knot: finite(
                        cadmpeg_core::convert::f64_from_index(index)
                            .expect("fixture index is exactly representable"),
                    ),
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
        let (bound_parameters, _bound_storage) =
            index_bound_parameters(&ctx, &bounds).expect("bound index");
        let hint = plane_digon_orientation_hint(
            &ctx,
            crate::families::e5::graph::PlaneDigonOrientationHintInputs {
                face_trailer_sign: Sign::Positive,
                surface_class: Some(0xc8),
                pcurve_ids: &[10, 11],
                edge_ids: &[1, 2],
                reversed: &[false, false],
                outer: Some(true),
                edges: &edges,
                pcurves: &pcurves,
                curve_supports: &supports,
                bound_parameters: &bound_parameters,
            },
        )
        .expect("service resource budget");
        assert_eq!(hint, Some(Sign::Negative));
        assert!(matches!(
            crate::test_support::with_work_refusal("catia_e5_plane_digon_site_scan", |limited_ctx| {
                plane_digon_orientation_hint(
                    limited_ctx,
                    crate::families::e5::graph::PlaneDigonOrientationHintInputs {
                        face_trailer_sign: Sign::Positive,
                        surface_class: Some(0xc8),
                        pcurve_ids: &[10, 11],
                        edge_ids: &[1, 2],
                        reversed: &[false, false],
                        outer: Some(true),
                        edges: &edges,
                        pcurves: &pcurves,
                        curve_supports: &supports,
                        bound_parameters: &bound_parameters,
                    },
                )
            }),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_e5_plane_digon_site_scan"
        ));

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
        let (wide_parameters, _wide_storage) =
            index_bound_parameters(&ctx, &wide_bounds).expect("wide bound index");
        assert_eq!(
            plane_digon_orientation_hint(
                &ctx,
                crate::families::e5::graph::PlaneDigonOrientationHintInputs {
                    face_trailer_sign: Sign::Positive,
                    surface_class: Some(0xc8),
                    pcurve_ids: &[10, 11],
                    edge_ids: &[1, 2],
                    reversed: &[false, false],
                    outer: Some(true),
                    edges: &edges,
                    pcurves: &wide_pcurves,
                    curve_supports: &supports,
                    bound_parameters: &wide_parameters
                }
            )
            .expect("service resource budget"),
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
            crate::test_support::with_service_context(|ctx| {
                let (parameters, _storage) = index_bound_parameters(ctx, &topology.bounds)?;
                topology.edge_representation_parameters(ctx, 1, 20, &parameters)
            })
            .expect("service resource budget"),
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
            crate::test_support::with_service_context(|ctx| {
                let (parameters, _storage) = index_bound_parameters(ctx, &topology.bounds)?;
                topology.edge_representation_parameters(ctx, 1, 20, &parameters)
            })
            .expect("service resource budget"),
            None
        );
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
        assert_eq!(records(&ctx, &bytes).expect("service decode").count(), 0);
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
            "catia_e5_bound_entries",
            "catia_e5_curve_supports",
            "catia_e5_raw_loops",
            "catia_e5_raw_faces",
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
            "catia_e5_body_face_set",
            "catia_e5_topology_face_set",
        ] {
            assert!(operations.contains(operation), "no refusal at {operation}");
        }
    }

    #[test]
    fn e5_edge_tail_refuses_retained_limit_before_absent_topology() {
        let mut bytes = Vec::new();
        append_e5_record(
            &mut bytes,
            0xff,
            110,
            &[0x85, 0x08, 200, 0x08, 10, 0x08, 11, 0x80, 0x80, 0x7f],
        );
        let service = crate::test_support::with_service_context(|ctx| parse_topology(ctx, &bytes));
        assert!(matches!(service, Ok(None)));
        let limited =
            crate::test_support::with_retained_refusal(&[], "catia_e5_edge_tail", |ctx| {
                parse_topology(ctx, &bytes)
            });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_edge_tail")
        );
    }

    #[test]
    fn e5_loop_sign_scan_preserves_role_and_rejection() {
        e5_test_context!(ctx);
        let mut trailing = vec![0x82];
        for sign in [0_i16, 1, 0, 0, 0, 0, 0, 0, 0, 0] {
            trailing.extend_from_slice(&sign.to_le_bytes());
        }
        assert!(matches!(
            super::parse_loop_signs(&ctx, &trailing, 2),
            Ok(Ok(Some(true)))
        ));
        trailing[3..5].copy_from_slice(&(-1_i16).to_le_bytes());
        assert!(matches!(
            super::parse_loop_signs(&ctx, &trailing, 2),
            Ok(Ok(Some(false)))
        ));
        trailing[9..11].copy_from_slice(&2_i16.to_le_bytes());
        assert!(matches!(
            super::parse_loop_signs(&ctx, &trailing, 2),
            Ok(Err(super::LoopSignError::Sign))
        ));
    }

    #[test]
    fn e5_loop_sign_scan_propagates_caller_refusal() {
        let mut trailing = vec![0x82];
        for sign in [0_i16, 1, 0, 0, 0, 0, 0, 0, 0, 0] {
            trailing.extend_from_slice(&sign.to_le_bytes());
        }
        assert!(matches!(
            crate::test_support::with_work_limit(0, |ctx| {
                super::parse_loop_signs(ctx, &trailing, 2)
            }),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_e5_loop_sign_scan"
        ));
    }

    #[test]
    fn e5_record_scanner_propagates_caller_refusal() {
        let mut bytes = Vec::new();
        append_e5_record(&mut bytes, 0xfe, 10, &[]);
        assert!(matches!(
            crate::test_support::with_work_limit(0, |ctx| records(ctx, &bytes)),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia_e5_record_scan"
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
        assert!(operations.contains("catia e5 orientation edge occurrence keys"));
        assert!(operations.contains("catia e5 orientation edge occurrences"));
        assert!(operations.contains("catia e5 orientation component"));
        assert!(operations.contains("catia e5 oriented members"));
    }
}

#[cfg(test)]
mod knot_work_tests {
    fn lanes() -> (Vec<u8>, Vec<u8>) {
        let knots = [0.0_f64, 1.0]
            .iter()
            .flat_map(|knot| knot.to_le_bytes())
            .collect();
        let multiplicities = [2_u32, 2]
            .iter()
            .flat_map(|count| count.to_le_bytes())
            .collect();
        (knots, multiplicities)
    }

    #[test]
    fn e5_knot_expansion_refuses_scan_and_emission_work() {
        let (knots, multiplicities) = lanes();
        for operation in [
            "catia_e5_knot_order_scan",
            "catia_e5_knot_expansion_scan",
            "catia_e5_pcurve_expanded_knots",
        ] {
            let result = crate::test_support::with_work_refusal(operation, |ctx| {
                super::expand_nurbs_knots_limited(ctx, 1, 2, &knots, &multiplicities, 2)
            });
            assert!(
                matches!(result, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == operation)
            );
        }
        assert!(matches!(crate::test_support::with_work_limit(8, |ctx| {
            super::expand_nurbs_knots_limited(ctx, 1, 2, &knots, &multiplicities, 2)
        }), Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_pcurve_expanded_knots"));
        assert_eq!(
            crate::test_support::with_work_limit(9, |ctx| {
                super::expand_nurbs_knots_limited(ctx, 1, 2, &knots, &multiplicities, 2)
            })
            .expect("3 order-search steps + 2 expansion visits + 4 emissions"),
            Some((
                crate::test_support::test_b5::finite_lane(&[0.0, 0.0, 1.0, 1.0]),
                2
            ))
        );
    }
}

#[cfg(test)]
mod budget_tests;

#[cfg(test)]
mod index_tests;
