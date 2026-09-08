// SPDX-License-Identifier: Apache-2.0
//! Typed topology record parsing ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
//!
//! Six fixed-width record families live at Parasolid stream scope and form the
//! B-rep chain
//!
//! ```text
//! bridge 00 0e .refs[4] -> compact surface carrier
//!              .refs[2] -> loop head 00 0f
//!                            .refs[1] -> coedge 00 11 ring (via .refs[3] = next)
//!                                          .refs[6] -> edge-use 00 10 .refs[3] -> compact curve
//!                                          .refs[4] -> vertex-use 00 12 .refs[4] -> world point 00 1d
//! ```
//!
//! Every record opens with `00 TT`, an optional `0xff`, then a big-endian `attr`
//! (u16) and, for most families, an `ordinal`/`seq` (u32). The magic
//! `c2 bc 92 8f 99 6e 00 00` anchors the bridge, edge-use, and vertex-use
//! parses. Records are keyed by `attr` within one stream (one site); attribute
//! ids collide across sites, so this codec resolves references only within the
//! single active partition stream it decodes.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::Sense;

use crate::layout::world_point as world_pt;

/// The magic anchoring magic-bearing topology records ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
pub const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

#[derive(Debug, Clone, PartialEq)]
pub struct Bridge {
    pub attr: u16,
    pub refs: [u16; 5],
    pub sequence: u32,
    pub sense: Sense,
    pub owner: Option<u16>,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub attr: u16,
    pub refs: [u16; 4],
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EdgeUse {
    pub attr: u16,
    pub references: EdgeReferences,
    pub sequence: u32,
    pub offset: usize,
}

/// Bare edge-use cells or the curve-only compact layout.
#[derive(Debug, Clone, Eq)]
pub enum EdgeReferences {
    Bare([u16; 6]),
    Compact { curve: u16 },
}

impl PartialEq for EdgeReferences {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Bare(left), Self::Bare(right)) => left == right,
            (Self::Compact { curve: left }, Self::Compact { curve: right }) => left == right,
            (Self::Bare(refs), Self::Compact { curve })
            | (Self::Compact { curve }, Self::Bare(refs)) => {
                // Candidate equivalence treats absent compact cells as null bare cells.
                refs[3] == *curve && [refs[0], refs[1], refs[2], refs[4], refs[5]] == [0; 5]
            }
        }
    }
}

impl EdgeReferences {
    pub fn canonical(&self) -> Option<u16> {
        match self {
            Self::Bare(refs) => Some(refs[0]),
            Self::Compact { .. } => None,
        }
    }

    pub fn curve(&self) -> u16 {
        match self {
            Self::Bare(refs) => refs[3],
            Self::Compact { curve } => *curve,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Coedge {
    pub attr: u16,
    pub refs: [u16; 9],
    pub sense: Sense,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VertexUse {
    pub attr: u16,
    pub refs: [u16; 5],
    pub sequence: u32,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Point {
    pub attr: u16,
    pub refs: Vec<u16>,
    pub xyz_m: [f64; 3],
    pub xyz_offset: usize,
    pub offset: usize,
}

fn parse_sense(marker: u8) -> Option<Sense> {
    match marker {
        0x2b => Some(Sense::Forward),
        0x2d => Some(Sense::Reversed),
        _ => None,
    }
}

/// Read the fixed reference cell count of a topology family.
fn refs_be<const N: usize>(buf: &[u8], at: usize) -> Option<[u16; N]> {
    let mut out = [0; N];
    for (index, reference) in out.iter_mut().enumerate() {
        *reference = View::u16_be_at(buf, at + 2 * index)?;
    }
    Some(out)
}

fn refs_tripled<const N: usize>(buf: &[u8], at: usize) -> Option<[u16; N]> {
    let mut out = [0; N];
    for (index, reference) in out.iter_mut().enumerate() {
        let p = at + index * 3;
        if buf.get(p + 2) != Some(&1) {
            return None;
        }
        *reference = View::u16_be_at(buf, p)?;
    }
    Some(out)
}

/// Advance past the tag and an optional `0xff` byte, returning the body start.
fn body_start(buf: &[u8], off: usize, tag_lo: u8) -> Option<usize> {
    if buf.get(off) != Some(&0x00) || buf.get(off + 1) != Some(&tag_lo) {
        return None;
    }
    let mut p = off + 2;
    if buf.get(p) == Some(&0xff) {
        p += 1;
    }
    Some(p)
}

fn attr_at(buf: &[u8], p: usize) -> Option<u16> {
    let a = View::u16_be_at(buf, p)?;
    if a == 0 {
        None
    } else {
        Some(a)
    }
}

/// Bridge `00 0e`: 37-byte body, magic at body+8, `refs[5]` at body+16,
/// marker at body+26. `refs[4]` = surface carrier, `refs[2]` = loop head.
/// The deltas form stores the owner as a `[hi][lo][01]` triple, so the magic
/// sits at body+9 and the five refs follow as triples with the marker after.
fn parse_bridge(buf: &[u8], off: usize) -> Option<Bridge> {
    let p = body_start(buf, off, 0x0e)?;
    if buf.get(p + 8) == Some(&1) && buf.get(p + 9..p + 17) == Some(MAGIC.as_slice()) {
        let attr = attr_at(buf, p)?;
        let sequence = View::u32_be_at(buf, p + 2)?;
        let owner = View::u16_be_at(buf, p + 6)?;
        let refs = refs_tripled::<5>(buf, p + 17)?;
        let marker = *buf.get(p + 32)?;
        return Some(Bridge {
            attr,
            sequence,
            refs,
            sense: parse_sense(marker)?,
            owner: (owner > 1).then_some(owner),
            offset: off,
        });
    }
    if p + 37 > buf.len() || buf.get(p + 8..p + 16)? != MAGIC {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let sequence = View::u32_be_at(buf, p + 2)?;
    let owner = View::u16_be_at(buf, p + 6)?;
    let tripled = (0..5).all(|index| buf.get(p + 18 + index * 3) == Some(&1));
    let (refs, marker) = if tripled {
        (refs_tripled::<5>(buf, p + 16)?, *buf.get(p + 31)?)
    } else {
        (refs_be::<5>(buf, p + 16)?, *buf.get(p + 26)?)
    };
    Some(Bridge {
        attr,
        sequence,
        refs,
        sense: parse_sense(marker)?,
        owner: (owner > 1).then_some(owner),
        offset: off,
    })
}

/// Loop head `00 0f`: minimal 14-byte body, no magic, `refs[4]` at body+6.
/// `refs[1]` = first coedge, `refs[2]` = owning bridge, `refs[3]` = next sibling.
fn parse_loop(buf: &[u8], off: usize) -> Option<Loop> {
    let p = body_start(buf, off, 0x0f)?;
    if p + 14 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let refs = refs_tripled::<4>(buf, p + 6).or_else(|| refs_be::<4>(buf, p + 6))?;
    Some(Loop {
        attr,
        refs,
        offset: off,
    })
}

/// Return all syntactically valid edge-use readings at one offset.
///
/// A prefixed edge-use does not carry the complete six-cell array in the
/// compact form. The third post-magic cell is the support-curve carrier, so
/// preserve that field in the compact variant. The missing
/// canonical-coedge slot is resolved from the coedge table by the graph walk.
fn parse_edge_use_candidates(buf: &[u8], off: usize) -> Vec<EdgeUse> {
    let Some(p) = body_start(buf, off, 0x10) else {
        return Vec::new();
    };
    if p + 28 > buf.len() {
        return Vec::new();
    }
    let Some(attr) = attr_at(buf, p) else {
        return Vec::new();
    };
    let Some(sequence) = View::u32_be_at(buf, p + 2) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if buf.get(p + 8..p + 16) == Some(MAGIC.as_slice()) {
        if let Some(refs) = refs_be::<6>(buf, p + 16) {
            out.push(EdgeUse {
                attr,
                sequence,
                references: EdgeReferences::Bare(refs),
                offset: off,
            });
        }
    }

    let magic_end = (p + 16).min(buf.len().saturating_sub(MAGIC.len()));
    for magic in p + 9..=magic_end {
        if buf.get(magic..magic + MAGIC.len()) != Some(MAGIC.as_slice()) {
            continue;
        }
        let q = magic + MAGIC.len();
        for prefix_first in [true, false] {
            let mut decoded = Vec::new();
            let mut at = q;
            while decoded.len() < 8 {
                let (matches, reference_at) = if prefix_first {
                    // `[01][hi][lo]` triples.
                    (buf.get(at) == Some(&1), at + 1)
                } else {
                    // `[hi][lo][01]` triples.
                    (buf.get(at + 2) == Some(&1), at)
                };
                if !matches {
                    break;
                }
                let Some(reference) = View::u16_be_at(buf, reference_at) else {
                    break;
                };
                decoded.push(reference);
                at += 3;
            }
            if decoded.len() >= 3 {
                out.push(EdgeUse {
                    attr,
                    sequence,
                    references: EdgeReferences::Compact { curve: decoded[2] },
                    offset: off,
                });
            }
        }
    }
    out.dedup();
    out
}

/// Edge-use `00 10`: 28-byte body, magic at body+8, `refs[6]` at body+16.
/// `refs[0]` = canonical forward coedge when the bare record stores one;
/// `refs[3]` = support curve carrier. The compact layout has no canonical-coedge slot.
///
/// Coedge `00 11`: 21-byte body, no magic, `refs[9]` at body+2, marker at
/// body+20. `refs[1]` = owning loop, `refs[3]` = next coedge, `refs[4]` = start
/// vertex-use, `refs[5]` = twin coedge, `refs[6]` = edge-use.
fn parse_coedge_candidates(buf: &[u8], off: usize) -> Vec<Coedge> {
    let Some(p) = body_start(buf, off, 0x11) else {
        return Vec::new();
    };
    if p + 21 > buf.len() {
        return Vec::new();
    }
    let Some(attr) = attr_at(buf, p) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let (Some(refs), Some(marker)) = (refs_be::<9>(buf, p + 2), buf.get(p + 20).copied()) {
        if let Some(sense) = parse_sense(marker) {
            out.push(Coedge {
                attr,
                refs,
                sense,
                offset: off,
            });
        }
    }
    if let (Some(refs), Some(marker)) = (refs_tripled::<9>(buf, p + 2), buf.get(p + 29).copied()) {
        if let Some(sense) = parse_sense(marker) {
            out.push(Coedge {
                attr,
                refs,
                sense,
                offset: off,
            });
        }
    }
    out.dedup();
    out
}

/// Vertex-use `00 12`: 24-byte body, magic at body+16, `refs[5]` at body+6.
/// `refs[4]` = world-point attr.
fn parse_vertex_use(buf: &[u8], off: usize) -> Option<VertexUse> {
    let p = body_start(buf, off, 0x12)?;
    if p + 24 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let sequence = View::u32_be_at(buf, p + 2)?;
    let refs = if buf.get(p + 16..p + 24) == Some(MAGIC.as_slice()) {
        refs_be::<5>(buf, p + 6)?
    } else {
        let magic = (p + 21..=(p + 32).min(buf.len().saturating_sub(MAGIC.len())))
            .find(|at| buf.get(*at..*at + MAGIC.len()) == Some(MAGIC.as_slice()))?;
        let count = (magic.checked_sub(p + 6)?) / 3;
        if count < 5 || p + 6 + count * 3 != magic {
            return None;
        }
        if !(0..count).all(|index| buf.get(p + 8 + index * 3) == Some(&1)) {
            return None;
        }
        refs_tripled::<5>(buf, p + 6)?
    };
    Some(VertexUse {
        attr,
        sequence,
        refs,
        offset: off,
    })
}

/// World point `00 1d`: 38-byte body, no magic, `refs[4]` at body+6, xyz as
/// three big-endian f64 (metres) at body+14.
fn parse_point(buf: &[u8], off: usize, prefixed: bool) -> Option<Point> {
    let p = body_start(buf, off, 0x1d)?;
    if p + world_pt::LEN > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let (refs, xyz_at) = if prefixed {
        let mut refs = Vec::new();
        let mut cursor = p + world_pt::REFS;
        while buf.get(cursor + 2) == Some(&1) && refs.len() < 16 {
            refs.push(View::u16_be_at(buf, cursor)?);
            cursor += 3;
        }
        if refs.is_empty() {
            return None;
        }
        (refs, cursor)
    } else {
        (
            refs_be::<4>(buf, p + world_pt::REFS)?.to_vec(),
            p + world_pt::XYZ,
        )
    };
    if refs.first().is_none_or(|reference| *reference > 1) {
        return None;
    }
    let x = View::f64_be_at(buf, xyz_at)?;
    let y = View::f64_be_at(buf, xyz_at + 8)?;
    let z = View::f64_be_at(buf, xyz_at + 16)?;
    for v in [x, y, z] {
        // Reject exponent-poisoned reads from a misaligned candidate: real part
        // coordinates in metres sit well under this cap.
        if !v.is_finite() || v.abs() > 1e4 {
            return None;
        }
    }
    Some(Point {
        attr,
        refs,
        xyz_m: [x, y, z],
        xyz_offset: xyz_at,
        offset: off,
    })
}

/// The topology record tables of one stream, each keyed by `attr`.
#[derive(Default)]
pub(crate) struct Tables {
    bridges: HashMap<u16, Bridge>,
    loops: HashMap<u16, Loop>,
    edge_uses: HashMap<u16, EdgeUse>,
    coedges: HashMap<u16, Coedge>,
    vertex_uses: HashMap<u16, VertexUse>,
    points: HashMap<u16, Point>,
}

impl Tables {
    pub(crate) fn bridges(&self) -> &HashMap<u16, Bridge> {
        &self.bridges
    }

    pub(crate) fn insert_bridge(&mut self, record: Bridge) {
        self.bridges.insert(record.attr, record);
    }

    pub(crate) fn loops(&self) -> &HashMap<u16, Loop> {
        &self.loops
    }

    pub(crate) fn insert_loop(&mut self, record: Loop) {
        self.loops.insert(record.attr, record);
    }

    pub(crate) fn edge_uses(&self) -> &HashMap<u16, EdgeUse> {
        &self.edge_uses
    }

    pub(crate) fn insert_edge_use(&mut self, record: EdgeUse) {
        self.edge_uses.insert(record.attr, record);
    }

    pub(crate) fn coedges(&self) -> &HashMap<u16, Coedge> {
        &self.coedges
    }

    pub(crate) fn insert_coedge(&mut self, record: Coedge) {
        self.coedges.insert(record.attr, record);
    }

    pub(crate) fn vertex_uses(&self) -> &HashMap<u16, VertexUse> {
        &self.vertex_uses
    }

    pub(crate) fn insert_vertex_use(&mut self, record: VertexUse) {
        self.vertex_uses.insert(record.attr, record);
    }

    pub(crate) fn points(&self) -> &HashMap<u16, Point> {
        &self.points
    }

    pub(crate) fn insert_point(&mut self, record: Point) {
        self.points.insert(record.attr, record);
    }

    /// Merge deltas without replacing partition topology membership.
    ///
    /// Preserve partition topology for shared identities and add only deltas
    /// bridges selected by the typed FACE ownership set.
    pub fn merge_deltas(&mut self, mut deltas: Self, selected_bridge_attrs: Option<&HashSet<u16>>) {
        if self.bridges.is_empty() {
            if let Some(selected_bridge_attrs) = selected_bridge_attrs {
                retain_selected_bridges(&mut deltas.bridges, selected_bridge_attrs);
            }
            self.bridges = deltas.bridges;
        } else if let Some(selected_bridge_attrs) = selected_bridge_attrs {
            retain_selected_bridges(&mut deltas.bridges, selected_bridge_attrs);
            merge_missing(&mut self.bridges, deltas.bridges);
        }
        merge_missing(&mut self.loops, deltas.loops);
        merge_missing(&mut self.edge_uses, deltas.edge_uses);
        merge_missing(&mut self.coedges, deltas.coedges);
        merge_missing(&mut self.vertex_uses, deltas.vertex_uses);
        self.points.extend(deltas.points.drain());
    }
}

fn retain_selected_bridges(
    bridges: &mut HashMap<u16, Bridge>,
    selected_bridge_attrs: &HashSet<u16>,
) {
    bridges.retain(|attr, record| {
        selected_bridge_attrs.contains(attr)
            || record
                .owner
                .is_some_and(|owner| selected_bridge_attrs.contains(&owner))
    });
}

fn merge_missing<T>(target: &mut HashMap<u16, T>, source: HashMap<u16, T>) {
    for (attr, record) in source {
        target.entry(attr).or_insert(record);
    }
}

type CandidateMap<T> = HashMap<u16, Vec<T>>;

trait Candidate: PartialEq {
    fn attr(&self) -> u16;
    fn offset(&self) -> usize;
}

impl Candidate for EdgeUse {
    fn attr(&self) -> u16 {
        self.attr
    }
    fn offset(&self) -> usize {
        self.offset
    }
}

impl Candidate for Coedge {
    fn attr(&self) -> u16 {
        self.attr
    }
    fn offset(&self) -> usize {
        self.offset
    }
}
#[derive(Clone, Copy)]
struct CoedgeEvidence {
    owner_valid: bool,
    start_valid: bool,
    edge_valid: bool,
    next_owner_valid: Option<bool>,
    previous_valid: bool,
    loop_head_valid: bool,
}

impl CoedgeEvidence {
    fn flags(self) -> [bool; 7] {
        [
            self.owner_valid,
            self.start_valid,
            self.edge_valid,
            self.next_owner_valid == Some(true),
            self.next_owner_valid.is_some(),
            self.previous_valid,
            self.loop_head_valid,
        ]
    }
}

/// Keep the latest record occurrence for each attribute while retaining all
/// frame readings at that occurrence. A stream can contain overlapping payload
/// bytes, and a later complete record has the same override semantics as the
/// ordinary topology tables.
fn insert_candidates<T: Candidate>(target: &mut CandidateMap<T>, records: Vec<T>) {
    let Some(first) = records.first() else {
        return;
    };
    let attr = first.attr();
    match target.entry(attr) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(records);
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            let current = entry.get();
            if current
                .first()
                .is_some_and(|record| record.offset() < first.offset())
            {
                entry.insert(records);
            } else if current
                .first()
                .is_some_and(|record| record.offset() == first.offset())
            {
                let candidates = entry.get_mut();
                candidates.extend(records);
                candidates.dedup();
            }
        }
    }
}

fn loop_is_owned(record: &Loop, bridges: &HashMap<u16, Bridge>) -> bool {
    record.refs[2] != 0 && bridges.contains_key(&record.refs[2])
}

/// Collect independent graph invariants for one coedge frame. The first four
/// fields identify the owner, vertex-use, edge-use, and ring; reciprocal links
/// and loop-head membership provide additional confirmation. No field is a
/// byte-position discriminator.
fn coedge_evidence(
    candidate: &Coedge,
    loops: &[Loop],
    bridges: &HashMap<u16, Bridge>,
    vertex_uses: &HashMap<u16, VertexUse>,
    edge_candidates: &CandidateMap<EdgeUse>,
    coedge_candidates: &CandidateMap<Coedge>,
) -> CoedgeEvidence {
    let owner = candidate.refs[1];
    let owner_valid = owner != 0
        && loops
            .iter()
            .rev()
            .find(|loop_| loop_.attr == owner)
            .is_some_and(|loop_| loop_is_owned(loop_, bridges));
    let start = candidate.refs[4];
    let start_valid = start != 0 && vertex_uses.contains_key(&start);
    let edge = candidate.refs[6];
    let edge_valid = edge != 0 && edge_candidates.contains_key(&edge);
    let next = candidate.refs[3];
    let next_owner_valid = coedge_candidates
        .get(&next)
        .filter(|_| next != 0)
        .map(|candidates| {
            candidates
                .iter()
                .any(|next_candidate| next_candidate.refs[1] == owner)
        });
    let previous = candidate.refs[2];
    let previous_valid = previous == 0
        || coedge_candidates.get(&previous).is_some_and(|candidates| {
            candidates.iter().any(|previous_candidate| {
                previous_candidate.refs[3] == candidate.attr && previous_candidate.refs[1] == owner
            })
        });
    let loop_head_valid = loops.iter().rev().any(|loop_| {
        loop_.attr == owner && loop_is_owned(loop_, bridges) && loop_.refs[1] == candidate.attr
    });
    CoedgeEvidence {
        owner_valid,
        start_valid,
        edge_valid,
        next_owner_valid,
        previous_valid,
        loop_head_valid,
    }
}

fn evidence_dominates(left: CoedgeEvidence, right: CoedgeEvidence) -> bool {
    let left = left.flags();
    let right = right.flags();
    let at_least_as_supported = left.iter().zip(right).all(|(left, right)| *left || !right);
    let strictly_more_supported = left.iter().zip(right).any(|(left, right)| *left && !right);
    at_least_as_supported && strictly_more_supported
}

fn select_coedge(
    candidates: &[Coedge],
    loops: &[Loop],
    bridges: &HashMap<u16, Bridge>,
    vertex_uses: &HashMap<u16, VertexUse>,
    edge_candidates: &CandidateMap<EdgeUse>,
    coedge_candidates: &CandidateMap<Coedge>,
) -> Option<Coedge> {
    if candidates.len() == 1 {
        return candidates.first().cloned();
    }
    let evidence: Vec<CoedgeEvidence> = candidates
        .iter()
        .map(|candidate| {
            coedge_evidence(
                candidate,
                loops,
                bridges,
                vertex_uses,
                edge_candidates,
                coedge_candidates,
            )
        })
        .collect();
    let maximal: Vec<usize> = (0..candidates.len())
        .filter(|&index| {
            !evidence.iter().enumerate().any(|(other, other_evidence)| {
                other != index && evidence_dominates(*other_evidence, evidence[index])
            })
        })
        .collect();
    if maximal.len() == 1 {
        candidates.get(maximal[0]).cloned()
    } else {
        None
    }
}

fn edge_candidate_is_valid(
    candidate: &EdgeUse,
    coedges: &HashMap<u16, Coedge>,
    curve_attrs: Option<&HashSet<u16>>,
) -> bool {
    let canonical = candidate.references.canonical();
    let curve = candidate.references.curve();
    let canonical_valid =
        canonical.is_none_or(|canonical| canonical == 0 || coedges.contains_key(&canonical));
    let curve_valid = curve == 0 || curve_attrs.is_none_or(|attrs| attrs.contains(&curve));
    canonical_valid && curve_valid
}

fn select_edge_use(
    candidates: &[EdgeUse],
    coedges: &HashMap<u16, Coedge>,
    curve_attrs: Option<&HashSet<u16>>,
) -> Option<EdgeUse> {
    if candidates.len() == 1 {
        return candidates.first().cloned();
    }
    let mut valid = candidates
        .iter()
        .filter(|candidate| edge_candidate_is_valid(candidate, coedges, curve_attrs));
    let first = valid.next()?.clone();
    if valid.next().is_none() {
        Some(first)
    } else {
        None
    }
}

fn xyz_bytes(xyz_m: [f64; 3]) -> [u8; 24] {
    let mut bytes = [0; 24];
    for (slot, value) in bytes.chunks_exact_mut(8).zip(xyz_m) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    bytes
}

/// Replace coordinates only when the old bytes still match the parsed record.
pub(crate) fn patch_point_values(
    buf: &mut [u8],
    xyz_at: usize,
    old_xyz_m: [f64; 3],
    new_xyz_m: [f64; 3],
) -> bool {
    let old_bytes = xyz_bytes(old_xyz_m);
    if buf.get(xyz_at..xyz_at + old_bytes.len()) != Some(old_bytes.as_slice()) {
        return false;
    }
    let new_bytes = xyz_bytes(new_xyz_m);
    let Some(bytes) = buf.get_mut(xyz_at..xyz_at + new_bytes.len()) else {
        return false;
    };
    bytes.copy_from_slice(&new_bytes);
    true
}

/// Replace one world-point record while preserving its framing.
pub(crate) fn patch_point(buf: &mut [u8], attr: u16, xyz_m: [f64; 3]) -> bool {
    let Some(record) = scan(buf).points.remove(&attr) else {
        return false;
    };
    patch_point_values(buf, record.xyz_offset, record.xyz_m, xyz_m)
}

/// Scan the stream body for every typed topology record. Successful records do
/// not advance the scan past their extent because valid records can overlap an
/// enclosing payload. Family-specific framing gates reject payload coincidences.
/// Later full records replace earlier records with the same `attr`, matching
/// partition-base plus deltas-override merge order.
pub fn scan(body: &[u8]) -> Tables {
    scan_with_point_framing(body, false, None, None)
}

/// Scan a partition stream while admitting typed FACE offsets that carry a
/// loop head.  A typed FACE and a compact bridge share the `00 0e` framing.
/// An ownership-only typed FACE has a null loop field and must not replace a
/// separate compact bridge for the same attribute; a shared FACE/bridge record
/// has a non-null loop field and is the topology record as well.
pub(crate) fn scan_with_curve_attrs_excluding(
    body: &[u8],
    curve_attrs: &HashSet<u16>,
    excluded_bridge_offsets: &HashSet<usize>,
) -> Tables {
    scan_with_point_framing(
        body,
        false,
        Some(curve_attrs),
        Some(excluded_bridge_offsets),
    )
}

/// Scan a deltas stream with the typed FACE/compact-bridge overlap rule.
pub(crate) fn scan_deltas_with_curve_attrs_excluding(
    body: &[u8],
    curve_attrs: &HashSet<u16>,
    excluded_bridge_offsets: &HashSet<usize>,
) -> Tables {
    scan_with_point_framing(body, true, Some(curve_attrs), Some(excluded_bridge_offsets))
}

fn scan_with_point_framing(
    body: &[u8],
    prefixed_points: bool,
    curve_attrs: Option<&HashSet<u16>>,
    excluded_bridge_offsets: Option<&HashSet<usize>>,
) -> Tables {
    let mut t = Tables::default();
    let mut loop_candidates = Vec::new();
    let mut edge_candidates = CandidateMap::new();
    let mut coedge_candidates = CandidateMap::new();
    let mut i = 0usize;
    while i + 14 <= body.len() {
        if body[i] != 0x00 {
            i += 1;
            continue;
        }
        match body[i + 1] {
            0x0e => {
                if let Some(record) = parse_bridge(body, i) {
                    let excluded =
                        excluded_bridge_offsets.is_some_and(|offsets| offsets.contains(&i));
                    let carries_loop = record.refs[2] > 1;
                    if !excluded || carries_loop {
                        t.insert_bridge(record);
                    }
                }
            }
            0x0f => {
                if let Some(record) = parse_loop(body, i) {
                    loop_candidates.push(record);
                }
            }
            0x10 => insert_candidates(&mut edge_candidates, parse_edge_use_candidates(body, i)),
            0x11 => insert_candidates(&mut coedge_candidates, parse_coedge_candidates(body, i)),
            0x12 => {
                if let Some(record) = parse_vertex_use(body, i) {
                    t.insert_vertex_use(record);
                }
            }
            0x1d => {
                let record = if prefixed_points {
                    parse_point(body, i, true).or_else(|| parse_point(body, i, false))
                } else {
                    parse_point(body, i, false)
                };
                if let Some(record) = record {
                    t.insert_point(record);
                }
            }
            _ => {}
        }
        i += 1;
    }

    for candidates in coedge_candidates.values() {
        if let Some(record) = select_coedge(
            candidates,
            &loop_candidates,
            &t.bridges,
            &t.vertex_uses,
            &edge_candidates,
            &coedge_candidates,
        ) {
            t.insert_coedge(record);
        }
    }
    for candidates in edge_candidates.into_values() {
        if let Some(record) = select_edge_use(&candidates, &t.coedges, curve_attrs) {
            t.insert_edge_use(record);
        }
    }
    for record in loop_candidates {
        let owner = record.refs[2];
        let first = record.refs[1];
        if t.bridges.contains_key(&owner)
            && t.coedges
                .get(&first)
                .is_some_and(|coedge| coedge.refs[1] == record.attr)
        {
            t.insert_loop(record);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_candidate_equivalence_preserves_null_cells_across_layouts() {
        let compact = EdgeReferences::Compact { curve: 300 };
        let bare = EdgeReferences::Bare([0, 0, 0, 300, 0, 0]);
        assert_eq!(compact, bare);
        assert_eq!(bare, compact);
        assert_eq!(compact.canonical(), None);
        assert_eq!(bare.canonical(), Some(0));
        for slot in [0, 1, 2, 4, 5] {
            let mut refs = [0, 0, 0, 300, 0, 0];
            refs[slot] = 1;
            assert_ne!(EdgeReferences::Bare(refs), compact);
        }
        assert_ne!(EdgeReferences::Compact { curve: 301 }, bare);
    }

    #[test]
    fn vertex_use_validates_extra_tripled_cells_before_the_magic() {
        let mut bytes = vec![0, 0x12];
        bytes.extend(50_u16.to_be_bytes());
        bytes.extend(7_u32.to_be_bytes());
        for reference in [0_u16, 0, 0, 0, 60, 90] {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.extend(MAGIC);
        let vertex = parse_vertex_use(&bytes, 0).unwrap();
        assert_eq!(vertex.refs, [0, 0, 0, 0, 60]);
        assert_eq!(vertex.sequence, 7);
        bytes[25] = 0;
        assert!(parse_vertex_use(&bytes, 0).is_none());
    }

    fn bridge_with_refs(refs: &[u16], tripled: bool) -> Vec<u8> {
        let mut bytes = vec![0, 0x0e];
        bytes.extend(0x1234_u16.to_be_bytes());
        bytes.extend(7_u32.to_be_bytes());
        bytes.extend(0x4321_u16.to_be_bytes());
        bytes.extend(MAGIC);
        for reference in refs {
            bytes.extend(reference.to_be_bytes());
            if tripled {
                bytes.push(1);
            }
        }
        bytes.push(0x2d);
        bytes.resize(40, 0);
        bytes
    }

    #[test]
    fn bridge_deltas_form_reads_tripled_owner_and_refs() {
        let expected: Vec<u16> = vec![0x101, 0x202, 0x303, 0x404, 0x505];
        let mut bytes = vec![0, 0x0e, 0xff];
        bytes.extend(0x1234_u16.to_be_bytes());
        bytes.extend(7_u32.to_be_bytes());
        bytes.extend(0x4321_u16.to_be_bytes());
        bytes.push(1);
        bytes.extend(MAGIC);
        for reference in &expected {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(0x2b);
        bytes.resize(48, 0);

        let bridge = parse_bridge(&bytes, 0).expect("deltas-form bridge");
        assert_eq!(bridge.attr, 0x1234);
        assert_eq!(bridge.sequence, 7);
        assert_eq!(bridge.owner, Some(0x4321));
        assert_eq!(bridge.refs.as_slice(), expected);
        assert_eq!(bridge.sense, Sense::Forward);
    }

    #[test]
    fn bridge_refs_accept_adjacent_and_tripled_cells() {
        let expected = vec![0x101, 0x202, 0x303, 0x404, 0x505];
        for tripled in [false, true] {
            let bytes = bridge_with_refs(&expected, tripled);
            let bridge = parse_bridge(&bytes, 0)
                .unwrap_or_else(|| panic!("bridge tripled={tripled} bytes={bytes:02x?}"));
            assert_eq!(bridge.attr, 0x1234);
            assert_eq!(bridge.sequence, 7);
            assert_eq!(bridge.owner, Some(0x4321));
            assert_eq!(bridge.refs.as_slice(), expected);
            assert_eq!(bridge.sense, Sense::Reversed);
        }
    }

    #[test]
    fn shared_typed_face_with_loop_remains_a_topology_bridge() {
        let body = bridge_with_refs(&[1, 2, 3, 7, 8], false);
        let excluded = HashSet::from([0]);

        let tables = scan_with_curve_attrs_excluding(&body, &HashSet::new(), &excluded);

        assert_eq!(
            tables.bridges.get(&0x1234).map(|record| record.refs[2]),
            Some(3)
        );
    }

    #[test]
    fn ownership_only_typed_face_does_not_replace_a_compact_bridge() {
        let body = bridge_with_refs(&[1, 2, 0, 7, 8], false);
        let excluded = HashSet::from([0]);

        let tables = scan_with_curve_attrs_excluding(&body, &HashSet::new(), &excluded);

        assert!(tables.bridges.is_empty());
    }

    fn topology_bridge(attr: u16, loop_attr: u16) -> Vec<u8> {
        let mut bytes = vec![0, 0x0e];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        bytes.extend(0_u16.to_be_bytes());
        bytes.extend(MAGIC);
        for reference in [0, 0, loop_attr, 0, 100] {
            bytes.extend(reference.to_be_bytes());
        }
        bytes.push(0x2b);
        bytes.extend([0; 10]);
        bytes
    }

    fn topology_loop(attr: u16, first_coedge: u16, bridge_attr: u16) -> Vec<u8> {
        let mut bytes = vec![0, 0x0f];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        for reference in [0, first_coedge, bridge_attr, 0] {
            bytes.extend(reference.to_be_bytes());
        }
        bytes
    }

    fn topology_tripled_coedge(attr: u16, refs: [u16; 9]) -> Vec<u8> {
        let mut bytes = vec![0, 0x11];
        bytes.extend(attr.to_be_bytes());
        for reference in refs {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(0x2b);
        bytes
    }

    fn topology_edge_use(attr: u16, sequence: u32) -> Vec<u8> {
        let mut bytes = vec![0, 0x10];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(sequence.to_be_bytes());
        bytes.extend(0_u16.to_be_bytes());
        bytes.extend(MAGIC);
        bytes.extend(
            [0_u16, 0, 0, 0, 0, 0]
                .into_iter()
                .flat_map(u16::to_be_bytes),
        );
        bytes
    }

    fn topology_vertex_use(attr: u16, sequence: u32) -> Vec<u8> {
        let mut bytes = vec![0, 0x12];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(sequence.to_be_bytes());
        bytes.extend([0_u16, 0, 0, 0, 60].into_iter().flat_map(u16::to_be_bytes));
        bytes.extend(MAGIC);
        bytes
    }

    #[test]
    fn coedge_tripled_frame_wins_over_marker_like_reference_byte() {
        let mut body = Vec::new();
        body.extend(topology_bridge(10, 20));
        body.extend(topology_loop(20, 30, 10));
        body.extend(topology_tripled_coedge(
            30,
            [0, 20, 0, 30, 50, 0, 0x2b40, 0, 0],
        ));
        body.extend(topology_edge_use(0x2b40, 0));
        body.extend(topology_vertex_use(50, 0));

        let tables = scan(&body);
        let coedge = tables.coedges.get(&30).expect("tripled coedge");
        assert_eq!(coedge.refs[1], 20);
        assert_eq!(coedge.refs[4], 50);
        assert_eq!(coedge.refs[6], 0x2b40);
        assert!(tables.loops.contains_key(&20));
    }

    #[test]
    fn edge_and_vertex_use_sequences_are_retained() {
        let mut body = Vec::new();
        body.extend(topology_edge_use(40, 0x0102_0304));
        body.extend(topology_vertex_use(50, 0x0506_0708));

        let tables = scan(&body);
        assert_eq!(tables.edge_uses[&40].sequence, 0x0102_0304);
        assert_eq!(tables.vertex_uses[&50].sequence, 0x0506_0708);
    }

    #[test]
    fn suffix_edge_use_frame_wins_when_first_reference_starts_with_one() {
        let mut bytes = vec![0, 0x10];
        bytes.extend(40_u16.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        bytes.extend(0_u16.to_be_bytes());
        bytes.extend([1, 0, 0]);
        bytes.extend(MAGIC);
        for reference in [0x0101_u16, 0x0102, 0x0103] {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }

        assert!(
            scan(&bytes).edge_uses.is_empty(),
            "ambiguous without a carrier set"
        );
        let curve_attrs = HashSet::from([0x0103]);
        let tables = scan_deltas_with_curve_attrs_excluding(&bytes, &curve_attrs, &HashSet::new());
        assert_eq!(tables.edge_uses[&40].references.curve(), 0x0103);
    }

    #[test]
    fn patch_point_uses_parsed_adjacent_coordinate_offset() {
        let mut bytes = vec![0, 0x1d];
        bytes.extend(60_u16.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        for reference in [0_u16, 0x0102, 0, 0] {
            bytes.extend(reference.to_be_bytes());
        }
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend(value.to_be_bytes());
        }

        assert!(!patch_point_values(
            &mut bytes,
            11,
            [1.0, 2.0, 3.0],
            [4.0, 5.0, 6.0]
        ));
        assert!(patch_point(&mut bytes, 60, [4.0, 5.0, 6.0]));

        let point = parse_point(&bytes, 0, false).expect("adjacent world point");
        assert_eq!(point.refs, vec![0, 0x0102, 0, 0]);
        assert_eq!(point.xyz_m, [4.0, 5.0, 6.0]);
        assert_eq!(point.xyz_offset, 16);
    }
}
