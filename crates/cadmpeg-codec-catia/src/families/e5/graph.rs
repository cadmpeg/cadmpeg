// SPDX-License-Identifier: Apache-2.0
//! Native topology records in the E5 `0D 03` stream family.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_core::decode::{alloc_filled, View};

use crate::wire;

const EPS_PARAMETER_ENDPOINT: f64 = 1.0e-9;

/// Resolved graph of an E5 `0D 03` record stream: bodies, faces, edges, and
/// the geometry records they reference. Produced by [`parse_topology`], which
/// walks every class-tagged record, resolves cross-record references, and
/// returns `None` if the walk cannot be closed ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq)]
pub struct E5Topology {
    /// Class-`0x01` body records with their resolved face rosters and
    /// orientation-sign tapes. Empty when the stream carries no `0x01`
    /// records (bodies are optional; face/loop/edge resolution does not
    /// require them).
    pub bodies: Vec<E5Body>,
    /// Class-`0x00` advanced-face records, each resolved to its surface and
    /// loops.
    pub faces: Vec<E5Face>,
    /// Class-`0xff` trimmed edge-use records, keyed by their `record_id`.
    /// Only edges reachable from a resolved face's loops are retained.
    pub edges: BTreeMap<u32, E5Edge>,
    /// Class-`0x96` (line), `0x97` (circle), `0xa0` (spline jet), and
    /// `0xaa` (NURBS) pcurve records, keyed by `record_id`.
    pub pcurves: BTreeMap<u32, E5Pcurve>,
    /// Class-`0x0e` parameter-bound records, keyed by `record_id`.
    pub bounds: BTreeMap<u32, E5Bounds>,
    /// Class-`0xc0` (one-pcurve boundary) and `0xc1` (two-pcurve
    /// intersection) curve-support records, keyed by `record_id`.
    pub curve_supports: BTreeMap<u32, E5CurveSupport>,
    /// Sorted, deduplicated `record_id`s of every class-`0xfe` vertex record
    /// referenced as an edge endpoint.
    pub vertex_refs: Vec<u32>,
}

impl E5Topology {
    /// Resolve one edge's start/end parameter records for a referenced
    /// representation. Each bound must contain that representation exactly
    /// once.
    #[must_use]
    pub fn edge_representation_parameters(
        &self,
        edge_ref: u32,
        representation: u32,
    ) -> Option<[f64; 2]> {
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
pub enum E5CurveSupportKind {
    /// Class-`0xc0` one-pcurve boundary support.
    Boundary(u32),
    /// Class-`0xc1` two-pcurve intersection support.
    Intersection([u32; 2]),
}

impl E5CurveSupportKind {
    fn from_parts(intersection: bool, pcurves: Vec<u32>) -> Option<Self> {
        match (intersection, pcurves.as_slice()) {
            (false, &[pcurve]) => Some(Self::Boundary(pcurve)),
            (true, &[left, right]) => Some(Self::Intersection([left, right])),
            _ => None,
        }
    }

    pub fn pcurves(&self) -> &[u32] {
        match self {
            Self::Boundary(pcurve) => std::slice::from_ref(pcurve),
            Self::Intersection(pcurves) => pcurves,
        }
    }

    pub fn is_intersection(&self) -> bool {
        matches!(self, Self::Intersection(_))
    }
}

/// A class-`0xc0`/`0xc1` curve-support record ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq)]
pub struct E5CurveSupport {
    /// This record's stream-assigned `record_id`, used to resolve
    /// `E5Edge::support` references.
    pub record_id: u32,
    /// Boundary or intersection pcurve layout.
    pub kind: E5CurveSupportKind,
    /// Raw mode byte following the pcurve reference lane; meaning not
    /// decoded further.
    pub mode: u8,
    /// Finite `[lo, hi]` parameter range on the support, stored as LE f64.
    pub range: [f64; 2],
    /// Unparsed bytes after the fixed header; not interpreted.
    pub tail: Vec<u8>,
}

impl E5CurveSupport {
    pub fn pcurves(&self) -> &[u32] {
        self.kind.pcurves()
    }

    pub fn is_intersection(&self) -> bool {
        self.kind.is_intersection()
    }
}

/// A class-`0x0e` parameter-bound record: a list of representation
/// references each paired with a bound parameter ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq)]
pub struct E5Bounds {
    /// This record's stream-assigned `record_id`.
    pub record_id: u32,
    /// Ordered `(representation, parameter, code)` entries, one per
    /// referenced representation.
    pub entries: Vec<E5BoundEntry>,
}

/// One entry of an [`E5Bounds`] record.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct E5BoundEntry {
    /// Referenced representation's `record_id`.
    pub representation: u32,
    /// Finite LE-f64 bound parameter for this representation.
    pub parameter: f64,
    /// Raw trailing `u32` code following the parameter; meaning not decoded
    /// further.
    pub code: u32,
}

/// One knot of a degree-5 E5 UV jet.
#[derive(Debug, Clone, PartialEq)]
pub struct E5PcurveJetSite {
    /// Distinct knot.
    pub knot: f64,
    /// Multiplicity of this distinct knot.
    pub multiplicity: u32,
    /// `(u, v)` position.
    pub point: [f64; 2],
    /// `(u, v)` first derivative.
    pub first_derivatives: [f64; 2],
    /// `(u, v)` second derivative.
    pub second_derivatives: [f64; 2],
}

impl E5PcurveJetSite {
    pub(crate) fn zip(
        knots: Vec<f64>,
        multiplicities: Vec<u32>,
        points: Vec<[f64; 2]>,
        first_derivatives: Vec<[f64; 2]>,
        second_derivatives: Vec<[f64; 2]>,
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
pub enum E5Pcurve {
    /// Class `0x96`: `<surface_ref>, origin_u, origin_v, dir_u, dir_v,
    /// param_lo, param_hi` stored as f64.
    Line {
        /// `record_id` of the owning surface carrier.
        surface: u32,
        /// `(u, v)` origin of the line in surface parameter space.
        origin: [f64; 2],
        /// `(u, v)` direction of the line in surface parameter space.
        direction: [f64; 2],
        /// `[param_lo, param_hi]` domain along `direction` from `origin`.
        range: [f64; 2],
    },
    /// Class `0x97`: `<surface_ref>, center_u, center_v, radius, param_lo,
    /// param_hi` with two intervening `u32` fields (`codes`).
    Circle {
        /// `record_id` of the owning surface carrier.
        surface: u32,
        /// `(u, v)` center of the circle in surface parameter space.
        center: [f64; 2],
        /// The two `u32` fields between `center` and `radius`; meaning not
        /// decoded further.
        codes: [u32; 2],
        /// Positive circle radius in surface parameter units.
        radius: f64,
        /// `[param_lo, param_hi]` angular domain.
        range: [f64; 2],
        /// Two trailing scalar fields following the parameter range.
        tail: [f64; 2],
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
        range: [f64; 2],
    },
    /// Class `0xaa`: a tensor-product-free NURBS p-curve with one surface
    /// reference, distinct knots and multiplicities, and 2D control points.
    Nurbs {
        /// `record_id` of the owning surface carrier.
        surface: u32,
        /// B-spline degree.
        degree: u32,
        /// Distinct knot values in strictly increasing order.
        knots: Vec<f64>,
        /// Multiplicity for each distinct knot.
        multiplicities: Vec<u32>,
        /// `(u, v)` control points in parameter order.
        control_points: Vec<[f64; 2]>,
        /// Effective parameter domain of the expanded knot vector.
        range: [f64; 2],
    },
}

impl E5Pcurve {
    pub const JET_DEGREE: u32 = 5;
}

/// A class-`0x01` body record resolved through its class-`0x08` root record:
/// the body's validated face roster
/// ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E5Body {
    /// This class-`0x01` body's stream-assigned `record_id`.
    pub record_id: u32,
    /// `record_id`s of every class-`0x00` face in the body, in root-record
    /// order.
    pub faces: Vec<u32>,
    /// Root sign-tape entries aligned with [`Self::faces`].
    pub face_orientation_signs: Vec<i16>,
    /// Final two root sign-tape entries after the face-aligned population.
    pub extra_orientation_signs: [i16; 2],
}

/// A resolved class-`0x00` advanced-face record: its surface, loops, and
/// root sign-tape entry ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E5Face {
    /// This face record's `record_id`.
    pub record_id: u32,
    /// `record_id` of the face's surface carrier.
    pub surface: u32,
    /// This face's entry in the class-`0x08` root sign tape (`+1` or
    /// `-1`), used by [`solve_absolute_orientation`] to fix each loop's
    /// global sense.
    pub trailer_sign: i16,
    /// The face's loops, first entry outer-bounded, remaining entries
    /// holes.
    pub loops: Vec<E5Loop>,
}

/// A resolved class-`0x09` loop record: its member pcurve/edge-use pairs and
/// derived orientation ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E5Loop {
    /// This loop record's `record_id`.
    pub record_id: u32,
    /// `record_id` of the loop's surface, matched against the owning
    /// face's surface during resolution.
    pub surface: u32,
    /// `record_id`s of the loop's member pcurves, in serialized order.
    pub pcurves: Vec<u32>,
    /// `record_id`s of the loop's member edge-uses, in serialized order,
    /// index-aligned with `pcurves`.
    pub edge_uses: Vec<u32>,
    /// Per-edge-use traversal sense from the unique head-to-tail chain
    /// solved by [`solve_loop_chain`]; `true` means the edge is traversed
    /// end-to-start.
    pub reversed: Vec<bool>,
    /// Shell-consistent member order and traversal senses after folding in
    /// the loop's global orientation sign. `None` when the radial parity
    /// system is frustrated or ambiguous.
    pub(crate) oriented_members: Option<Vec<E5OrientedMember>>,
    /// Loop role bit from the trailing sign tape: `Some(true)` =
    /// `FACE_OUTER_BOUND`, `Some(false)` = `FACE_BOUND`, `None` when the
    /// loop carries no trailing role tape.
    pub outer: Option<bool>,
    /// Complete trailing signed relation tape in serialized order. Empty when
    /// the loop carries no tape.
    pub orientation_signs: Vec<i16>,
    /// Exact global-sense anchor for a closed plane-cap split circle. This is
    /// present only when the two-edge loop has a complete role sign, two
    /// complementary intersection-support ranges, and occurrence parameter
    /// directions that determine one native-UV winding. Other loops use the
    /// shared-edge parity component anchor.
    pub(crate) orientation_hint: Option<i8>,
}

impl E5Loop {
    /// Shell-consistent member order and senses when radial parity closes.
    #[must_use]
    pub fn resolved_members(&self) -> Option<&[E5OrientedMember]> {
        self.oriented_members.as_deref()
    }
}

/// One E5 loop member in shell-consistent traversal order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct E5OrientedMember {
    /// Index of the member in the serialized loop arrays.
    pub serialized_index: usize,
    /// Whether the physical edge is traversed end-to-start.
    pub reversed: bool,
}

/// A resolved class-`0xff` trimmed edge-use record ([spec §9](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/catia.md#9-e5-0d-03-stream-variant), grammar `85
/// <curve_support_ref> <start_vertex> <end_vertex> <param_start>
/// <param_end>`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct E5Edge {
    /// This edge-use record's `record_id`.
    pub record_id: u32,
    /// `record_id` of the owning [`E5CurveSupport`].
    pub support: u32,
    /// `record_id` of the class-`0xfe` start vertex.
    pub start_vertex: u32,
    /// `record_id` of the class-`0xfe` end vertex.
    pub end_vertex: u32,
    /// Reference to the start-parameter representation on the curve
    /// support.
    pub parameter_start: u32,
    /// Reference to the end-parameter representation on the curve support.
    pub parameter_end: u32,
    /// Bytes following the five counted fields.
    pub tail: Vec<u8>,
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
    trailer_sign: i16,
}

#[derive(Debug)]
struct RawLoop {
    id: u32,
    surface: u32,
    pcurves: Vec<u32>,
    edges: Vec<u32>,
    outer: Option<bool>,
    orientation_signs: Vec<i16>,
}

/// Resolve E5 face→loop→edge-use references and determine each serialized
/// loop occurrence's unique head-to-tail traversal from stored vertex refs.
#[must_use]
pub fn parse_topology(bytes: &[u8]) -> Option<E5Topology> {
    let records = records(bytes);
    let by_id: HashMap<u32, &Record<'_>> =
        records.iter().map(|record| (record.id, record)).collect();
    if by_id.len() != records.len() {
        return None;
    }

    let edges: BTreeMap<u32, E5Edge> = records
        .iter()
        .filter(|record| record.class == 0xff)
        .map(|record| parse_edge(record).map(|edge| (record.id, edge)))
        .collect::<Option<_>>()?;
    let pcurves: BTreeMap<u32, E5Pcurve> = records
        .iter()
        .filter(|record| matches!(record.class, 0x96 | 0x97 | 0xa0 | 0xaa))
        .map(|record| parse_pcurve(record).map(|pcurve| (record.id, pcurve)))
        .collect::<Option<_>>()?;
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
    let bounds: BTreeMap<u32, E5Bounds> = records
        .iter()
        .filter(|record| record.class == 0x0e)
        .map(|record| parse_bounds(record).map(|bounds| (record.id, bounds)))
        .collect::<Option<_>>()?;
    let curve_supports: BTreeMap<u32, E5CurveSupport> = records
        .iter()
        .filter(|record| matches!(record.class, 0xc0 | 0xc1))
        .map(|record| parse_curve_support(record).map(|support| (record.id, support)))
        .collect::<Option<_>>()?;
    let loops: HashMap<u32, RawLoop> = records
        .iter()
        .filter(|record| record.class == 0x09)
        .map(|record| parse_loop(record).map(|loop_| (record.id, loop_)))
        .collect::<Option<_>>()?;
    let raw_faces: Vec<RawFace> = records
        .iter()
        .filter(|record| record.class == 0x00)
        .map(|record| parse_face(record))
        .collect::<Option<_>>()?;
    let vertex_ids: HashSet<u32> = records
        .iter()
        .filter(|record| record.class == 0xfe)
        .map(|record| record.id)
        .collect();
    if raw_faces.is_empty() || loops.is_empty() || edges.is_empty() || vertex_ids.is_empty() {
        return None;
    }

    let mut faces = Vec::with_capacity(raw_faces.len());
    let mut reachable_edges = HashSet::new();
    for face in raw_faces {
        if !by_id
            .get(&face.surface)
            .is_some_and(|record| is_surface_carrier_class(record.class))
        {
            return None;
        }
        let mut resolved_loops = Vec::with_capacity(face.loops.len());
        for (loop_position, loop_id) in face.loops.into_iter().enumerate() {
            let raw = loops.get(&loop_id)?;
            if raw.surface != face.surface {
                return None;
            }
            if raw.outer.is_some_and(|outer| outer != (loop_position == 0)) {
                return None;
            }
            let reversed = solve_loop_chain(&raw.edges, &edges)?;
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
                if support.pcurves().iter().any(|reference| {
                    !curve_support_reference_closes(*reference, &pcurves, &curve_supports)
                }) {
                    return None;
                }
                reachable_edges.insert(*edge_id);
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
            resolved_loops.push(E5Loop {
                record_id: raw.id,
                surface: raw.surface,
                pcurves: raw.pcurves.clone(),
                edge_uses: raw.edges.clone(),
                reversed,
                oriented_members: None,
                outer: raw.outer,
                orientation_signs: raw.orientation_signs.clone(),
                orientation_hint,
            });
        }
        faces.push(E5Face {
            record_id: face.id,
            surface: face.surface,
            trailer_sign: face.trailer_sign,
            loops: resolved_loops,
        });
    }
    if !solve_absolute_orientation(&mut faces) {
        return None;
    }
    let edges: BTreeMap<u32, E5Edge> = edges
        .into_iter()
        .filter(|(id, _)| reachable_edges.contains(id))
        .collect();
    let mut vertex_refs: Vec<u32> = edges
        .values()
        .flat_map(|edge| [edge.start_vertex, edge.end_vertex])
        .collect();
    vertex_refs.sort_unstable();
    vertex_refs.dedup();
    let bodies = parse_bodies(&records, &by_id)?;
    if !bodies.is_empty() {
        let roster: Vec<u32> = bodies
            .iter()
            .flat_map(|body| body.faces.iter().copied())
            .collect();
        let roster_set: HashSet<u32> = roster.iter().copied().collect();
        let face_set: HashSet<u32> = faces.iter().map(|face| face.record_id).collect();
        if roster.len() != roster_set.len() || roster_set != face_set {
            return None;
        }
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
}

/// Return the serialized surface reference for each valid class-`0x00` face.
///
/// This is the narrow face-to-carrier relation used by standard freeform
/// aliases. It does not claim that the complete E5 topology graph is closed.
#[must_use]
pub fn face_surface_references(bytes: &[u8]) -> Vec<(u32, u32)> {
    records(bytes)
        .into_iter()
        .filter(|record| record.class == 0x00)
        .filter_map(|record| parse_face(&record).map(|face| (face.id, face.surface)))
        .collect()
}

fn is_surface_carrier_class(class: u8) -> bool {
    matches!(class, 0xc8 | 0xc9 | 0xca | 0xcc | 0xe7)
}

/// Checks that a curve-support side resolves to a direct p-curve or to a
/// finite, acyclic chain of intersection-support wrappers.
fn curve_support_reference_closes(
    reference: u32,
    pcurves: &BTreeMap<u32, E5Pcurve>,
    supports: &BTreeMap<u32, E5CurveSupport>,
) -> bool {
    if pcurves.contains_key(&reference) {
        return true;
    }
    let mut visiting = HashSet::new();
    let mut stack = vec![(reference, false)];
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
            continue;
        }
        if !visiting.insert(reference) {
            return false;
        }
        stack.push((reference, true));
        for child in support.pcurves().iter().rev() {
            if pcurves.contains_key(child) {
                continue;
            }
            if !supports
                .get(child)
                .is_some_and(|support| support.is_intersection())
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
    bound_ref: u32,
    representation: u32,
) -> Option<f64> {
    let bounds = bounds.get(&bound_ref)?;
    let mut entries = bounds
        .entries
        .iter()
        .filter(|entry| entry.representation == representation);
    let parameter = entries.next()?.parameter;
    entries.next().is_none().then_some(parameter)
}

fn parse_curve_support(record: &Record<'_>) -> Option<E5CurveSupport> {
    let (pcurves, mut position) = wire::counted_refs(record.payload, false)?;
    let expected = if record.class == 0xc0 { 1 } else { 2 };
    if pcurves.len() != expected || record.payload.get(position) != Some(&0x81) {
        return None;
    }
    position += 1;
    let mode = *record.payload.get(position)?;
    position += 1;
    if record.payload.get(position) != Some(&0x00) {
        return None;
    }
    position += 1;
    let mut view = View::over_retained(record.payload);
    view.seek(position)?;
    let range = [view.f64_le()?, view.f64_le()?];
    if range.iter().any(|value| !value.is_finite()) {
        return None;
    }
    position = view.position();
    Some(E5CurveSupport {
        record_id: record.id,
        kind: E5CurveSupportKind::from_parts(record.class == 0xc1, pcurves)?,
        mode,
        range,
        tail: record.payload[position..].to_vec(),
    })
}

fn parse_bounds(record: &Record<'_>) -> Option<E5Bounds> {
    let (representations, mut position) = wire::counted_refs(record.payload, false)?;
    if record.payload.get(position)
        != Some(&(0x80u8.checked_add(u8::try_from(representations.len()).ok()?)?))
    {
        return None;
    }
    position += 1;
    let mut view = View::over_retained(record.payload);
    view.seek(position)?;
    let mut entries = Vec::with_capacity(representations.len());
    for representation in representations {
        let parameter = view.f64_le()?;
        let code = view.u32_le()?;
        if !parameter.is_finite() {
            return None;
        }
        entries.push(E5BoundEntry {
            representation,
            parameter,
            code,
        });
    }
    view.is_empty().then_some(E5Bounds {
        record_id: record.id,
        entries,
    })
}

fn parse_pcurve(record: &Record<'_>) -> Option<E5Pcurve> {
    if record.payload.first() != Some(&0x81) {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(record.payload, &mut position, false)?;
    let mut view = View::over_retained(record.payload);
    view.seek(position)?;
    match record.class {
        0x96 => {
            let values = view.read_counted(6, 8, View::f64_le)?;
            if !view.is_empty() || values.iter().any(|value| !value.is_finite()) {
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
            let center = view.read_counted(2, 8, View::f64_le)?;
            let codes = [view.u32_le()?, view.u32_le()?];
            let values = view.read_counted(5, 8, View::f64_le)?;
            if !view.is_empty()
                || center.iter().chain(&values).any(|value| !value.is_finite())
                || values[0] <= 0.0
            {
                return None;
            }
            Some(E5Pcurve::Circle {
                surface,
                center: [center[0], center[1]],
                codes,
                radius: values[0],
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

fn parse_nurbs_pcurve(payload: &[u8], position: usize, surface: u32) -> Option<E5Pcurve> {
    let mut view = View::over_retained(payload);
    view.seek(position)?;
    if view.u16_le()? != 0 {
        return None;
    }
    let degree = view.u32_le()?;
    let zero0 = view.u32_le()?;
    let zero1 = view.u32_le()?;
    let knot_count = usize::try_from(view.u32_le()?).ok()?;
    let zero2 = view.u32_le()?;
    if degree == 0 || knot_count == 0 || [zero0, zero1, zero2] != [0; 3] {
        return None;
    }
    let knot_count_u64 = u64::try_from(knot_count).ok()?;
    let knots = view.read_counted(knot_count_u64, 8, View::f64_le)?;
    let multiplicities = view.read_counted(knot_count_u64, 4, View::u32_le)?;
    let max_control_count = view.remaining().checked_sub(E5_NURBS_PCURVE_TAIL_BYTES)? / 16;
    let (expanded_knots, control_count) =
        expand_nurbs_knots_limited(degree, &knots, &multiplicities, max_control_count)?;
    let control_points = view.read_counted(u64::try_from(control_count).ok()?, 16, |view| {
        Some([view.f64_le()?, view.f64_le()?])
    })?;
    if view.remaining() != E5_NURBS_PCURVE_TAIL_BYTES
        || knots.iter().any(|knot| !knot.is_finite())
        || control_points
            .iter()
            .flatten()
            .copied()
            .any(|value| !value.is_finite())
    {
        return None;
    }
    let range = [
        *expanded_knots.get(usize::try_from(degree).ok()?)?,
        *expanded_knots.get(control_count)?,
    ];
    if !range.into_iter().all(f64::is_finite) || range[0] >= range[1] {
        return None;
    }
    view.skip(E5_NURBS_PCURVE_TAIL_BYTES)?;
    view.is_empty().then_some(E5Pcurve::Nurbs {
        surface,
        degree,
        knots,
        multiplicities,
        control_points,
        range,
    })
}

pub(crate) fn expand_nurbs_knots(
    degree: u32,
    knots: &[f64],
    multiplicities: &[u32],
) -> Option<(Vec<f64>, usize)> {
    expand_nurbs_knots_limited(degree, knots, multiplicities, usize::MAX)
}

fn expand_nurbs_knots_limited(
    degree: u32,
    knots: &[f64],
    multiplicities: &[u32],
    max_control_count: usize,
) -> Option<(Vec<f64>, usize)> {
    if knots.len() != multiplicities.len()
        || knots.is_empty()
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities.contains(&0)
    {
        return None;
    }
    let total = multiplicities
        .iter()
        .try_fold(0usize, |total, multiplicity| {
            total.checked_add(usize::try_from(*multiplicity).ok()?)
        })?;
    let degree = usize::try_from(degree).ok()?;
    let control_count = total.checked_sub(degree.checked_add(1)?)?;
    if control_count <= degree || control_count > max_control_count {
        return None;
    }
    let mut expanded = Vec::with_capacity(total);
    for (knot, multiplicity) in knots.iter().zip(multiplicities) {
        expanded.extend(std::iter::repeat_n(
            *knot,
            usize::try_from(*multiplicity).ok()?,
        ));
    }
    (expanded.len() == total).then_some((expanded, control_count))
}

fn parse_jet_pcurve(payload: &[u8], position: usize, surface: u32) -> Option<E5Pcurve> {
    let mut view = View::over_retained(payload);
    view.seek(position)?;
    let degree = view.u32_le()?;
    let zero0 = view.u32_le()?;
    let zero1 = view.u32_le()?;
    let site_count = usize::try_from(view.u32_le()?).ok()?;
    let zero2 = view.u32_le()?;
    let zero3 = view.u32_le()?;
    let zero4 = view.u32_le()?;
    if degree != 5 || site_count == 0 || [zero0, zero1, zero2, zero3, zero4] != [0; 5] {
        return None;
    }
    let site_count_u64 = u64::try_from(site_count).ok()?;
    let mut knots = vec![0.0];
    knots.extend(view.read_counted(site_count_u64.checked_sub(1)?, 8, View::f64_le)?);
    let multiplicities = view.read_counted(site_count_u64, 4, View::u32_le)?;
    if usize::try_from(view.u32_le()?).ok()? != site_count {
        return None;
    }
    let x = view.read_counted(site_count_u64, 8, View::f64_le)?;
    let y = view.read_counted(site_count_u64, 8, View::f64_le)?;
    let dx = view.read_counted(site_count_u64, 8, View::f64_le)?;
    let dy = view.read_counted(site_count_u64, 8, View::f64_le)?;
    if view.u16_le()? != 1 {
        return None;
    }
    let ddx = view.read_counted(site_count_u64, 8, View::f64_le)?;
    let ddy = view.read_counted(site_count_u64, 8, View::f64_le)?;
    let range_values = view.read_counted(2, 8, View::f64_le)?;
    let expected_multiplicities: Vec<u32> = if site_count == 1 {
        vec![degree + 1]
    } else {
        std::iter::once(degree + 1)
            .chain(std::iter::repeat_n(3, site_count.saturating_sub(2)))
            .chain(std::iter::once(degree + 1))
            .collect()
    };
    let final_knot = *knots.last()?;
    if !view.is_empty()
        || knots.iter().any(|value| !value.is_finite())
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities != expected_multiplicities
        || multiplicities.iter().sum::<u32>() != degree + 1 + 3 * u32::try_from(site_count).ok()?
        || range_values[0] != 0.0
        || (range_values[1] - final_knot).abs() > EPS_PARAMETER_ENDPOINT * final_knot.abs()
        || x.iter()
            .chain(&y)
            .chain(&dx)
            .chain(&dy)
            .chain(&ddx)
            .chain(&ddy)
            .chain(&range_values)
            .any(|value| !value.is_finite())
    {
        return None;
    }
    Some(E5Pcurve::Jet {
        surface,
        sites: E5PcurveJetSite::zip(
            knots,
            multiplicities,
            x.into_iter().zip(y).map(|(u, v)| [u, v]).collect(),
            dx.into_iter().zip(dy).map(|(u, v)| [u, v]).collect(),
            ddx.into_iter().zip(ddy).map(|(u, v)| [u, v]).collect(),
        ),
        range: [range_values[0], range_values[1]],
    })
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
    face_trailer_sign: i16,
    surface_class: Option<u8>,
    pcurve_ids: &[u32],
    edge_ids: &[u32],
    reversed: &[bool],
    outer: Option<bool>,
    edges: &BTreeMap<u32, E5Edge>,
    pcurves: &BTreeMap<u32, E5Pcurve>,
    curve_supports: &BTreeMap<u32, E5CurveSupport>,
    bounds: &BTreeMap<u32, E5Bounds>,
) -> Option<i8> {
    const EPS_PLANE_DIGON: f64 = 1.0e-8;
    if surface_class != Some(0xc8)
        || pcurve_ids.len() != 2
        || edge_ids.len() != 2
        || reversed.len() != 2
        || !matches!(outer, Some(true | false))
        || !matches!(face_trailer_sign, -1 | 1)
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
    let first_start = first_sites.first()?.point;
    let first_end = first_sites.last()?.point;
    let second_start = second_sites.first()?.point;
    let second_end = second_sites.last()?.point;
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
            (site.point[0] - center[0]).hypot(site.point[1] - center[1]),
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
            .then_some(if cross > 0.0 { 1i8 } else { -1i8 })
    };
    let signed_parameter_direction = |edge: &E5Edge, pcurve_id: u32, native_range: [f64; 2]| {
        let parameters = [edge.parameter_start, edge.parameter_end]
            .map(|bound_ref| bound_representation_parameter(bounds, bound_ref, pcurve_id));
        let [start, end] = parameters
            .into_iter()
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()?;
        let bound_span = end - start;
        let native_span = native_range[1] - native_range[0];
        if !bound_span.is_finite()
            || !native_span.is_finite()
            || bound_span.abs() <= EPS_PLANE_DIGON
            || native_span.abs() <= EPS_PLANE_DIGON
        {
            return None;
        }
        Some(if bound_span * native_span > 0.0 {
            1i8
        } else {
            -1i8
        })
    };
    let first_direction = signed_parameter_direction(first_edge, first_pcurve_id, *first_range)?
        * if reversed[0] { -1 } else { 1 };
    let second_direction =
        signed_parameter_direction(second_edge, second_pcurve_id, *second_range)?
            * if reversed[1] { -1 } else { 1 };
    let first_winding =
        native_arc_sign(first_start, first_sites.first()?.first_derivatives)? * first_direction;
    let second_winding =
        native_arc_sign(second_start, second_sites.first()?.first_derivatives)? * second_direction;
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
    let mut intervals = [first_support.range, second_support.range];
    for interval in &mut intervals {
        if !interval[0].is_finite() || !interval[1].is_finite() || interval[0] == interval[1] {
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

    let face_sign = i8::try_from(face_trailer_sign).ok()?;
    let role_sign = if outer == Some(true) { 1 } else { -1 };
    Some(face_sign * role_sign * first_winding)
}

fn solve_absolute_orientation(faces: &mut [E5Face]) -> bool {
    let mut locations = Vec::new();
    for (face_index, face) in faces.iter().enumerate() {
        for (loop_index, loop_) in face.loops.iter().enumerate() {
            if !loop_.edge_uses.is_empty() {
                locations.push((face_index, loop_index));
            }
        }
    }
    let mut occurrences = HashMap::<u32, Vec<(usize, i8)>>::new();
    for (node, &(face_index, loop_index)) in locations.iter().enumerate() {
        let loop_ = &faces[face_index].loops[loop_index];
        for (&edge, &reversed) in loop_.edge_uses.iter().zip(&loop_.reversed) {
            occurrences
                .entry(edge)
                .or_default()
                .push((node, if reversed { -1 } else { 1 }));
        }
    }
    let Ok(mut adjacency) = alloc_filled(
        locations.len(),
        Vec::<(usize, i8)>::new(),
        "catia e5 orientation adjacency",
    ) else {
        return false;
    };
    for uses in occurrences.values().filter(|uses| uses.len() == 2) {
        let [(left, left_r), (right, right_r)] = uses.as_slice() else {
            unreachable!("filtered to two occurrences");
        };
        let relation = -left_r * right_r;
        adjacency[*left].push((*right, relation));
        adjacency[*right].push((*left, relation));
    }
    let Ok(mut solved) = alloc_filled(locations.len(), None, "catia e5 orientation assignments")
    else {
        return false;
    };
    for root in 0..locations.len() {
        if solved[root].is_some() {
            continue;
        }
        solved[root] = Some(1i8);
        let mut component = vec![root];
        let mut cursor = 0;
        let mut consistent = true;
        while cursor < component.len() {
            let node = component[cursor];
            cursor += 1;
            let value = solved[node].expect("queued orientation");
            for &(neighbor, relation) in &adjacency[node] {
                let expected = value * relation;
                match solved[neighbor] {
                    Some(actual) if actual != expected => consistent = false,
                    Some(_) => {}
                    None => {
                        solved[neighbor] = Some(expected);
                        component.push(neighbor);
                    }
                }
            }
        }
        if !consistent {
            for &node in &component {
                solved[node] = None;
            }
            continue;
        }
        let mut exact_flip = None;
        for &node in &component {
            let (face_index, loop_index) = locations[node];
            let Some(hint) = faces[face_index].loops[loop_index].orientation_hint else {
                continue;
            };
            let candidate = hint * solved[node].expect("component value");
            match exact_flip {
                Some(existing) if existing != candidate => {
                    for &component_node in &component {
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
            for &node in &component {
                solved[node] = solved[node].map(|value| value * flip);
            }
        } else {
            let plus_matches = component
                .iter()
                .filter(|&&node| {
                    let (face, _) = locations[node];
                    i16::from(solved[node].expect("component value")) == faces[face].trailer_sign
                })
                .count();
            let minus_matches = component.len() - plus_matches;
            if minus_matches > plus_matches {
                for &node in &component {
                    solved[node] = solved[node].map(|value| -value);
                }
            }
        }
    }
    for (node, &(face_index, loop_index)) in locations.iter().enumerate() {
        let Some(g) = solved[node] else {
            continue;
        };
        let loop_ = &mut faces[face_index].loops[loop_index];
        let flip = g < 0;
        let mut indices: Vec<usize> = (0..loop_.reversed.len()).collect();
        if flip {
            indices.reverse();
        }
        loop_.oriented_members = Some(
            indices
                .into_iter()
                .map(|serialized_index| E5OrientedMember {
                    serialized_index,
                    reversed: loop_.reversed[serialized_index] ^ flip,
                })
                .collect(),
        );
    }
    solved.into_iter().all(|value| value.is_some())
}

fn parse_bodies(records: &[Record<'_>], by_id: &HashMap<u32, &Record<'_>>) -> Option<Vec<E5Body>> {
    records
        .iter()
        .filter(|record| record.class == 0x01)
        .map(|record| {
            let (roots, end) = wire::counted_refs(record.payload, false)?;
            if roots.len() != 1 || end != record.payload.len() {
                return None;
            }
            let root = *by_id.get(&roots[0])?;
            if root.class != 0x08 {
                return None;
            }
            let (faces, signs) = parse_body_root(root.payload)?;
            if signs.iter().any(|sign| !matches!(sign, -1 | 1))
                || faces
                    .iter()
                    .any(|face| by_id.get(face).is_none_or(|target| target.class != 0x00))
            {
                return None;
            }
            Some(E5Body {
                record_id: record.id,
                faces,
                face_orientation_signs: signs[..signs.len() - 2].to_vec(),
                extra_orientation_signs: signs[signs.len() - 2..].try_into().ok()?,
            })
        })
        .collect()
}

fn parse_body_root(payload: &[u8]) -> Option<(Vec<u32>, Vec<i16>)> {
    let (faces, mut position) = if payload.first() == Some(&0x08) {
        let count = usize::from(*payload.get(1)?);
        let mut position = 2;
        let mut faces = Vec::with_capacity(count);
        for _ in 0..count {
            let face = wire::object_ref(payload, &mut position, false)?;
            if face > u32::from(u16::MAX) {
                return None;
            }
            faces.push(face);
        }
        (faces, position)
    } else {
        wire::counted_refs(payload, false)?
    };
    let count = u8::try_from(faces.len()).ok()?;
    if payload.get(position..position + 2) == Some(&[0x08, count]) {
        position += 2;
    } else if faces.len() <= 0x7f && payload.get(position) == Some(&0x80u8.checked_add(count)?) {
        position += 1;
    } else {
        return None;
    }
    let sign_bytes = payload.get(position..)?;
    if sign_bytes.len() != (faces.len() + 2) * 2 {
        return None;
    }
    let signs = sign_bytes
        .chunks_exact(2)
        .map(|bytes| View::i16_le_at(bytes, 0))
        .collect::<Option<Vec<_>>>()?;
    Some((faces, signs))
}

fn records(bytes: &[u8]) -> Vec<Record<'_>> {
    let mut records = Vec::new();
    let mut position = 0;
    while position + 13 <= bytes.len() {
        let Some(relative) = bytes[position..]
            .windows(3)
            .position(|value| value == [0xe5, 0x0d, 0x03])
        else {
            break;
        };
        let start = position + relative;
        if start + 13 > bytes.len() {
            break;
        }
        let Some(size) = View::u16_le_at(bytes, start + 5).map(usize::from) else {
            break;
        };
        let Some(end) = start.checked_add(13 + size) else {
            break;
        };
        if end > bytes.len() {
            position = start + 1;
            continue;
        }
        records.push(Record {
            class: bytes[start + 3],
            id: View::u32_le_at(bytes, start + 9).expect("record header bounds were checked"),
            payload: &bytes[start + 13..end],
        });
        position = end;
    }
    records
}

fn parse_face(record: &Record<'_>) -> Option<RawFace> {
    let count = usize::from(record.payload.first()?.checked_sub(0x81)?);
    if count == 0 {
        return None;
    }
    let mut position = 1;
    let surface = wire::object_ref(record.payload, &mut position, false)?;
    let mut loops = Vec::with_capacity(count);
    for _ in 0..count {
        loops.push(wire::object_ref(record.payload, &mut position, false)?);
    }
    let trailer_sign = View::i16_le_at(record.payload, position)?;
    if !matches!(trailer_sign, -1 | 1) || position + 2 != record.payload.len() {
        return None;
    }
    Some(RawFace {
        id: record.id,
        surface,
        loops,
        trailer_sign,
    })
}

fn parse_loop(record: &Record<'_>) -> Option<RawLoop> {
    let member_count = usize::from(record.payload.first()?.checked_sub(0x81)?);
    if member_count == 0 || member_count % 2 != 0 {
        return None;
    }
    let mut position = 1;
    let mut pcurves = Vec::with_capacity(member_count / 2);
    let mut edges = Vec::with_capacity(member_count / 2);
    for _ in 0..member_count / 2 {
        pcurves.push(wire::object_ref(record.payload, &mut position, false)?);
        edges.push(wire::object_ref(record.payload, &mut position, false)?);
    }
    let surface = wire::object_ref(record.payload, &mut position, false)?;
    let (outer, orientation_signs) =
        parse_loop_signs(record.payload.get(position..)?, member_count / 2)?;
    Some(RawLoop {
        id: record.id,
        surface,
        pcurves,
        edges,
        outer,
        orientation_signs,
    })
}

fn parse_loop_signs(trailing: &[u8], edge_count: usize) -> Option<(Option<bool>, Vec<i16>)> {
    if trailing.is_empty() {
        return Some((None, Vec::new()));
    }
    let expected_head = u8::try_from(edge_count)
        .ok()
        .and_then(|n| 0x80u8.checked_add(n))?;
    if trailing.first() != Some(&expected_head) || trailing.len() != 1 + 2 * (3 * edge_count + 4) {
        return None;
    }
    let signs: Vec<i16> = trailing[1..]
        .chunks_exact(2)
        .map(|bytes| View::i16_le_at(bytes, 0))
        .collect::<Option<Vec<_>>>()?;
    if signs.iter().any(|sign| !matches!(sign, -1..=1)) || !matches!(signs[1], -1 | 1) {
        return None;
    }
    Some((Some(signs[1] == 1), signs))
}

fn parse_edge(record: &Record<'_>) -> Option<E5Edge> {
    if record.payload.first() != Some(&0x85) {
        return None;
    }
    let mut position = 1;
    let support = wire::object_ref(record.payload, &mut position, false)?;
    let start_vertex = wire::object_ref(record.payload, &mut position, false)?;
    let end_vertex = wire::object_ref(record.payload, &mut position, false)?;
    let parameter_start = wire::object_ref(record.payload, &mut position, false)?;
    let parameter_end = wire::object_ref(record.payload, &mut position, false)?;
    let tail = record.payload[position..].to_vec();
    Some(E5Edge {
        record_id: record.id,
        support,
        start_vertex,
        end_vertex,
        parameter_start,
        parameter_end,
        tail,
    })
}

fn solve_loop_chain(edge_ids: &[u32], edges: &BTreeMap<u32, E5Edge>) -> Option<Vec<bool>> {
    let first = edges.get(edge_ids.first()?)?;
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
        let mut senses = vec![first_reversed];
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
    use super::*;
    use std::collections::BTreeMap;

    fn append_e5_record(bytes: &mut Vec<u8>, class: u8, id: u32, payload: &[u8]) {
        bytes.extend_from_slice(&[0xe5, 0x0d, 0x03, class, 0]);
        bytes.extend_from_slice(&(payload.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes.extend_from_slice(payload);
    }

    #[test]
    fn topology_accepts_a_valid_43_byte_loop_payload() {
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

        let topology = parse_topology(&bytes).expect("closed E5 topology");
        assert_eq!(topology.faces[0].surface, 258);
        assert_eq!(topology.faces[0].loops[0].edge_uses, [110, 111, 112]);
        assert_eq!(topology.faces[0].loops[0].outer, Some(true));
    }

    #[test]
    fn curve_support_wrapper_cycles_are_rejected() {
        let pcurves = BTreeMap::from([(
            3,
            E5Pcurve::Line {
                surface: 10,
                origin: [0.0, 0.0],
                direction: [1.0, 0.0],
                range: [0.0, 1.0],
            },
        )]);
        let support = |record_id, pcurves: [u32; 2]| E5CurveSupport {
            record_id,
            kind: E5CurveSupportKind::Intersection(pcurves),
            mode: 0,
            range: [0.0, 1.0],
            tail: Vec::new(),
        };
        let supports = BTreeMap::from([(1, support(1, [2, 3])), (2, support(2, [1, 3]))]);
        assert!(!curve_support_reference_closes(1, &pcurves, &supports));
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
        assert!(parse_nurbs_pcurve(&truncated, 2, 7).is_none());

        let E5Pcurve::Nurbs {
            surface,
            degree,
            knots,
            multiplicities,
            control_points,
            range,
        } = parse_nurbs_pcurve(&payload, 2, 7).expect("AA NURBS pcurve")
        else {
            panic!("AA record did not produce a NURBS pcurve");
        };
        assert_eq!(surface, 7);
        assert_eq!(degree, 1);
        assert_eq!(knots, [0.0, 1.0]);
        assert_eq!(multiplicities, [2, 2]);
        assert_eq!(control_points, [[0.0, 0.0], [1.0, 1.0]]);
        assert_eq!(range, [0.0, 1.0]);
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
        let (faces, signs) = parse_body_root(&payload).expect("widened body root");
        assert_eq!(faces, [0x1600, 0x1601]);
        assert_eq!(signs, [1, -1, 1, -1]);
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
            Some(E5Pcurve::Jet {
                range: [0.0, value],
                ..
            }) if value == final_knot
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
                    record_id: 1,
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
                    record_id: 2,
                    support: 0,
                    start_vertex: 20,
                    end_vertex: 10,
                    parameter_start: 0,
                    parameter_end: 0,
                    tail: Vec::new(),
                },
            ),
        ]);
        assert_eq!(solve_loop_chain(&[1, 2], &edges), Some(vec![false, false]));
    }

    #[test]
    fn plane_digon_winding_anchors_absolute_orientation() {
        let jet = |points: Vec<[f64; 2]>, first_derivatives: Vec<[f64; 2]>| E5Pcurve::Jet {
            surface: 500,
            sites: points
                .into_iter()
                .zip(first_derivatives)
                .enumerate()
                .map(|(index, (point, first_derivatives))| E5PcurveJetSite {
                    knot: index as f64,
                    multiplicity: 6,
                    point,
                    first_derivatives,
                    second_derivatives: [0.0, 0.0],
                })
                .collect(),
            range: [0.0, 1.0],
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
                    record_id: 1,
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
                    record_id: 2,
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
                    record_id: 100,
                    kind: E5CurveSupportKind::Intersection([10, 12]),
                    mode: 0,
                    range: [0.0, 1.0],
                    tail: Vec::new(),
                },
            ),
            (
                101,
                E5CurveSupport {
                    record_id: 101,
                    kind: E5CurveSupportKind::Intersection([11, 13]),
                    mode: 0,
                    range: [1.0, 2.0],
                    tail: Vec::new(),
                },
            ),
        ]);
        let bounds = BTreeMap::from([
            (
                30,
                E5Bounds {
                    record_id: 30,
                    entries: vec![E5BoundEntry {
                        representation: 10,
                        parameter: 0.0,
                        code: 0,
                    }],
                },
            ),
            (
                31,
                E5Bounds {
                    record_id: 31,
                    entries: vec![E5BoundEntry {
                        representation: 10,
                        parameter: 1.0,
                        code: 0,
                    }],
                },
            ),
            (
                32,
                E5Bounds {
                    record_id: 32,
                    entries: vec![E5BoundEntry {
                        representation: 11,
                        parameter: 0.0,
                        code: 0,
                    }],
                },
            ),
            (
                33,
                E5Bounds {
                    record_id: 33,
                    entries: vec![E5BoundEntry {
                        representation: 11,
                        parameter: 1.0,
                        code: 0,
                    }],
                },
            ),
        ]);
        let hint = plane_digon_orientation_hint(
            1,
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
        assert_eq!(hint, Some(-1));

        let mut faces = vec![E5Face {
            record_id: 1,
            surface: 500,
            trailer_sign: 1,
            loops: vec![E5Loop {
                record_id: 2,
                surface: 500,
                pcurves: vec![10, 11],
                edge_uses: vec![1, 2],
                reversed: vec![false, false],
                oriented_members: None,
                outer: Some(true),
                orientation_signs: Vec::new(),
                orientation_hint: hint,
            }],
        }];
        assert!(solve_absolute_orientation(&mut faces));
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
            record_id: 1,
            support: 0,
            start_vertex: 0,
            end_vertex: 0,
            parameter_start: 10,
            parameter_end: 11,
            tail: Vec::new(),
        };
        let bound = |record_id, parameter| E5Bounds {
            record_id,
            entries: vec![E5BoundEntry {
                representation: 20,
                parameter,
                code: 7,
            }],
        };
        let mut topology = E5Topology {
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: BTreeMap::from([(1, edge)]),
            pcurves: BTreeMap::new(),
            bounds: BTreeMap::from([(10, bound(10, 0.25)), (11, bound(11, 0.75))]),
            curve_supports: BTreeMap::new(),
            vertex_refs: Vec::new(),
        };

        assert_eq!(
            topology.edge_representation_parameters(1, 20),
            Some([0.25, 0.75])
        );
        topology
            .bounds
            .get_mut(&11)
            .expect("end bound")
            .entries
            .push(E5BoundEntry {
                representation: 20,
                parameter: 1.0,
                code: 8,
            });
        assert_eq!(topology.edge_representation_parameters(1, 20), None);
    }

    #[test]
    fn radial_parity_rejects_frustration_and_reverses_negative_gauge() {
        let loop_ = |record_id, edge_uses| E5Loop {
            record_id,
            surface: record_id + 100,
            pcurves: vec![record_id + 200; 2],
            edge_uses,
            reversed: vec![false, false],
            oriented_members: None,
            outer: Some(true),
            orientation_signs: Vec::new(),
            orientation_hint: None,
        };
        let mut faces = vec![
            E5Face {
                record_id: 1,
                surface: 101,
                trailer_sign: 1,
                loops: vec![loop_(11, vec![1, 3])],
            },
            E5Face {
                record_id: 2,
                surface: 102,
                trailer_sign: 1,
                loops: vec![loop_(12, vec![1, 2])],
            },
            E5Face {
                record_id: 3,
                surface: 103,
                trailer_sign: 1,
                loops: vec![loop_(13, vec![2, 3])],
            },
        ];

        assert!(!solve_absolute_orientation(&mut faces));
        assert!(faces
            .iter()
            .flat_map(|face| &face.loops)
            .all(|loop_| loop_.oriented_members.is_none()));

        let mut faces = vec![
            E5Face {
                record_id: 1,
                surface: 101,
                trailer_sign: 1,
                loops: vec![loop_(11, vec![1, 2])],
            },
            E5Face {
                record_id: 2,
                surface: 102,
                trailer_sign: 1,
                loops: vec![loop_(12, vec![1, 3])],
            },
        ];
        assert!(solve_absolute_orientation(&mut faces));
        let second = faces[1].loops[0]
            .resolved_members()
            .expect("required invariant");
        assert_eq!(second[0].serialized_index, 1);
        assert_eq!(second[1].serialized_index, 0);
        assert!(second.iter().all(|member| member.reversed));
    }

    #[test]
    fn records_stop_at_a_marker_without_a_full_header() {
        let mut bytes = vec![0; 20];
        bytes.extend_from_slice(&[0xe5, 0x0d, 0x03, 0x00]);
        assert!(records(&bytes).is_empty());
        assert!(parse_topology(&bytes).is_none());
    }
}
