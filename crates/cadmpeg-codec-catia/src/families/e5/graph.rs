// SPDX-License-Identifier: Apache-2.0
//! Native topology records in the E5 `0D 03` stream family.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::le::{take_f64s, take_u32 as read_u32, u32_at};

use crate::wire;

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
    /// Class-`0x96` (line), `0x97` (circle), and `0xa0` (spline jet) pcurve
    /// records, keyed by `record_id`.
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
                let bounds = self.bounds.get(&bound_ref)?;
                let mut entries = bounds
                    .entries
                    .iter()
                    .filter(|entry| entry.representation == representation);
                let parameter = entries.next()?.parameter;
                entries.next().is_none().then_some(parameter)
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
pub struct E5CurveSupport {
    /// This record's stream-assigned `record_id`, used to resolve
    /// `E5Edge::support` references.
    pub record_id: u32,
    /// `true` for a class-`0xc1` two-pcurve intersection support, `false`
    /// for a class-`0xc0` one-pcurve boundary support.
    pub intersection: bool,
    /// Referenced pcurve `record_id`s: one entry for `0xc0`, two for
    /// `0xc1`.
    pub pcurves: Vec<u32>,
    /// Raw mode byte following the pcurve reference lane; meaning not
    /// decoded further.
    pub mode: u8,
    /// Finite `[lo, hi]` parameter range on the support, stored as LE f64.
    pub range: [f64; 2],
    /// Unparsed bytes after the fixed header; not interpreted.
    pub tail: Vec<u8>,
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

/// A resolved E5 pcurve: a 2D curve in a surface's parameter space, decoded
/// from a class-`0x96` (line), `0x97` (circle), or `0xa0` (spline jet)
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
        /// B-spline degree; always `5` (only degree-5 jets are accepted).
        degree: u32,
        /// Distinct knot values, starting at `0.0`.
        knots: Vec<f64>,
        /// Per-knot multiplicities: `degree + 1` at each end, `3` at
        /// interior knots (clamped-C2 policy).
        multiplicities: Vec<u32>,
        /// `(u, v)` jet-site positions, one per knot.
        points: Vec<[f64; 2]>,
        /// `(u, v)` first derivatives at each jet site.
        first_derivatives: Vec<[f64; 2]>,
        /// `(u, v)` second derivatives at each jet site.
        second_derivatives: Vec<[f64; 2]>,
        /// `[0.0, knots.last()]` parameter range, validated against the
        /// knot span.
        range: [f64; 2],
    },
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
    /// `FACE_OUTER_BOUND`, `Some(false)` = `FACE_BOUND`, `None` for a
    /// two-edge digon loop (no trailing role tape).
    pub outer: Option<bool>,
    /// Complete trailing signed relation tape in serialized order. Empty when
    /// the loop carries no tape.
    pub orientation_signs: Vec<i16>,
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
        .filter(|record| matches!(record.class, 0x96 | 0x97 | 0xa0))
        .map(|record| parse_pcurve(record).map(|pcurve| (record.id, pcurve)))
        .collect::<Option<_>>()?;
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
        .filter(|record| record.class == 0x09 && record.payload.len() != 43)
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
        by_id.get(&face.surface)?;
        let mut resolved_loops = Vec::with_capacity(face.loops.len());
        for loop_id in face.loops {
            let raw = loops.get(&loop_id)?;
            if raw.surface != face.surface {
                return None;
            }
            let reversed = solve_loop_chain(&raw.edges, &edges)?;
            for pcurve_id in &raw.pcurves {
                let pcurve = pcurves.get(pcurve_id)?;
                let surface = match pcurve {
                    E5Pcurve::Line { surface, .. }
                    | E5Pcurve::Circle { surface, .. }
                    | E5Pcurve::Jet { surface, .. } => *surface,
                };
                if surface != raw.surface {
                    return None;
                }
            }
            for edge_id in &raw.edges {
                let edge = edges.get(edge_id)?;
                if !vertex_ids.contains(&edge.start_vertex)
                    || !vertex_ids.contains(&edge.end_vertex)
                {
                    return None;
                }
                reachable_edges.insert(*edge_id);
                if !curve_supports.is_empty() && !curve_supports.contains_key(&edge.support) {
                    return None;
                }
            }
            resolved_loops.push(E5Loop {
                record_id: raw.id,
                surface: raw.surface,
                pcurves: raw.pcurves.clone(),
                edge_uses: raw.edges.clone(),
                reversed,
                oriented_members: None,
                outer: raw.outer,
                orientation_signs: raw.orientation_signs.clone(),
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
    let range = [
        f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        ),
        f64::from_le_bytes(
            record
                .payload
                .get(position + 8..position + 16)?
                .try_into()
                .ok()?,
        ),
    ];
    if range.iter().any(|value| !value.is_finite()) {
        return None;
    }
    position += 16;
    Some(E5CurveSupport {
        record_id: record.id,
        intersection: record.class == 0xc1,
        pcurves,
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
    let mut entries = Vec::with_capacity(representations.len());
    for representation in representations {
        let parameter = f64::from_le_bytes(
            record
                .payload
                .get(position..position + 8)?
                .try_into()
                .ok()?,
        );
        position += 8;
        let code = read_u32(record.payload, &mut position)?;
        if !parameter.is_finite() {
            return None;
        }
        entries.push(E5BoundEntry {
            representation,
            parameter,
            code,
        });
    }
    (position == record.payload.len()).then_some(E5Bounds {
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
    match record.class {
        0x96 => {
            let values = take_f64s(record.payload, &mut position, 6)?;
            if position != record.payload.len() || values.iter().any(|value| !value.is_finite()) {
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
            let center = take_f64s(record.payload, &mut position, 2)?;
            let codes = [
                read_u32(record.payload, &mut position)?,
                read_u32(record.payload, &mut position)?,
            ];
            let values = take_f64s(record.payload, &mut position, 5)?;
            if position != record.payload.len()
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
        _ => None,
    }
}

fn parse_jet_pcurve(payload: &[u8], mut position: usize, surface: u32) -> Option<E5Pcurve> {
    let degree = read_u32(payload, &mut position)?;
    let zero0 = read_u32(payload, &mut position)?;
    let zero1 = read_u32(payload, &mut position)?;
    let site_count = usize::try_from(read_u32(payload, &mut position)?).ok()?;
    let zero2 = read_u32(payload, &mut position)?;
    let zero3 = read_u32(payload, &mut position)?;
    let zero4 = read_u32(payload, &mut position)?;
    if degree != 5 || site_count == 0 || [zero0, zero1, zero2, zero3, zero4] != [0; 5] {
        return None;
    }
    let mut knots = vec![0.0];
    knots.extend(take_f64s(payload, &mut position, site_count - 1)?);
    let mut multiplicities = Vec::with_capacity(site_count);
    for _ in 0..site_count {
        multiplicities.push(read_u32(payload, &mut position)?);
    }
    if usize::try_from(read_u32(payload, &mut position)?).ok()? != site_count {
        return None;
    }
    let x = take_f64s(payload, &mut position, site_count)?;
    let y = take_f64s(payload, &mut position, site_count)?;
    let dx = take_f64s(payload, &mut position, site_count)?;
    let dy = take_f64s(payload, &mut position, site_count)?;
    if payload.get(position..position + 2) != Some(&1u16.to_le_bytes()) {
        return None;
    }
    position += 2;
    let ddx = take_f64s(payload, &mut position, site_count)?;
    let ddy = take_f64s(payload, &mut position, site_count)?;
    let range_values = take_f64s(payload, &mut position, 2)?;
    let expected_multiplicities: Vec<u32> = if site_count == 1 {
        vec![degree + 1]
    } else {
        std::iter::once(degree + 1)
            .chain(std::iter::repeat_n(3, site_count.saturating_sub(2)))
            .chain(std::iter::once(degree + 1))
            .collect()
    };
    if position != payload.len()
        || knots.windows(2).any(|pair| pair[0] >= pair[1])
        || multiplicities != expected_multiplicities
        || multiplicities.iter().sum::<u32>() != degree + 1 + 3 * u32::try_from(site_count).ok()?
        || range_values[0].abs() >= 1e-12
        || (range_values[1] - *knots.last()?).abs() >= 1e-9
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
        degree,
        knots,
        multiplicities,
        points: x.into_iter().zip(y).map(|(u, v)| [u, v]).collect(),
        first_derivatives: dx.into_iter().zip(dy).map(|(u, v)| [u, v]).collect(),
        second_derivatives: ddx.into_iter().zip(ddy).map(|(u, v)| [u, v]).collect(),
        range: [range_values[0], range_values[1]],
    })
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
    let mut adjacency = vec![Vec::<(usize, i8)>::new(); locations.len()];
    for uses in occurrences.values().filter(|uses| uses.len() == 2) {
        let [(left, left_r), (right, right_r)] = uses.as_slice() else {
            unreachable!("filtered to two occurrences");
        };
        let relation = -left_r * right_r;
        adjacency[*left].push((*right, relation));
        adjacency[*right].push((*left, relation));
    }
    let mut solved = vec![None; locations.len()];
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
            let (faces, mut position) = wire::counted_refs(root.payload, false)?;
            if root.payload.get(position)
                != Some(&(0x80u8.checked_add(u8::try_from(faces.len()).ok()?)?))
            {
                return None;
            }
            position += 1;
            let sign_bytes = root.payload.get(position..)?;
            if sign_bytes.len() != (faces.len() + 2) * 2 {
                return None;
            }
            let signs: Vec<i16> = sign_bytes
                .chunks_exact(2)
                .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
                .collect();
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
        let size = usize::from(u16::from_le_bytes([bytes[start + 5], bytes[start + 6]]));
        let Some(end) = start.checked_add(13 + size) else {
            break;
        };
        if end > bytes.len() {
            position = start + 1;
            continue;
        }
        records.push(Record {
            class: bytes[start + 3],
            id: u32_at(bytes, start + 9).expect("record header bounds were checked"),
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
    let trailer_sign = i16::from_le_bytes(
        record
            .payload
            .get(position..position + 2)?
            .try_into()
            .ok()?,
    );
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
        .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
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
    use super::{
        solve_absolute_orientation, solve_loop_chain, E5BoundEntry, E5Bounds, E5Edge, E5Face,
        E5Loop, E5Topology,
    };
    use std::collections::BTreeMap;

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
}
