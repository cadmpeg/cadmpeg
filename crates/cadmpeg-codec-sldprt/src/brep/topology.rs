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
//! parses. Records are keyed by `attr` within each stream. Same-site merges
//! preserve partition records and add missing subordinate deltas records and
//! typed-FACE-selected bridges. The decoder
//! merges all admitted sites. It keeps the active site's primary identities
//! and qualifies the other sites' identities; without an active site, it
//! qualifies every site's identities.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::Sense;

use crate::layout::edge_tripled as edge_delta;
use crate::layout::fin_bare as fin_body;
use crate::layout::fin_tripled as fin_delta;
use crate::layout::loop_bare as loop_body;
use crate::layout::loop_tripled as loop_delta;
use crate::layout::world_point as world_pt;
use crate::layout::world_point_tripled as world_pt_delta;

/// The magic anchoring magic-bearing topology records ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

const MAX_SHORT_POINTER_INDEX: u16 = 0x7ffe;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Bridge {
    pub(super) attr: u16,
    pub(super) refs: [u16; 5],
    pub(super) sequence: u32,
    pub(super) sense: Sense,
    pub(super) attributes_head: Option<u16>,
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::brep) struct Loop {
    pub(super) attr: u16,
    pub(super) refs: [u16; 4],
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct EdgeUse {
    pub(super) attr: u16,
    pub(super) references: EdgeReferences,
    pub(super) sequence: u32,
    pub(super) offset: usize,
}

/// Bare edge-use cells or the curve-only compact layout.
#[derive(Debug, Clone, Eq)]
pub(in crate::brep) enum EdgeReferences {
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
    pub(super) fn canonical(&self) -> Option<u16> {
        match self {
            Self::Bare(refs) => Some(refs[0]),
            Self::Compact { .. } => None,
        }
    }

    pub(super) fn curve(&self) -> u16 {
        match self {
            Self::Bare(refs) => refs[3],
            Self::Compact { curve } => *curve,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Coedge {
    pub(super) attr: u16,
    pub(super) refs: [u16; 9],
    pub(super) sense: Sense,
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::brep) struct VertexUse {
    pub(super) attr: u16,
    pub(super) refs: [u16; 5],
    pub(super) sequence: u32,
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Point {
    attr: u16,
    refs: Vec<u16>,
    pub(super) xyz_m: [f64; 3],
    pub(crate) xyz_offset: usize,
    pub(crate) offset: usize,
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
/// The deltas form stores the attribute-chain head as a `[hi][lo][01]` triple, so the magic
/// sits at body+9 and the five refs follow as triples with the marker after.
fn parse_bridge(buf: &[u8], off: usize) -> Option<Bridge> {
    let p = body_start(buf, off, 0x0e)?;
    if buf.get(p + 8) == Some(&1) && buf.get(p + 9..p + 17) == Some(MAGIC.as_slice()) {
        let attr = attr_at(buf, p)?;
        let sequence = View::u32_be_at(buf, p + 2)?;
        let attributes_head = View::u16_be_at(buf, p + 6)?;
        let refs = refs_tripled::<5>(buf, p + 17)?;
        let marker = *buf.get(p + 32)?;
        return Some(Bridge {
            attr,
            sequence,
            refs,
            sense: parse_sense(marker)?,
            attributes_head: (attributes_head > 1).then_some(attributes_head),
            offset: off,
        });
    }
    if p + 37 > buf.len() || buf.get(p + 8..p + 16)? != MAGIC {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let sequence = View::u32_be_at(buf, p + 2)?;
    let attributes_head = View::u16_be_at(buf, p + 6)?;
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
        attributes_head: (attributes_head > 1).then_some(attributes_head),
        offset: off,
    })
}

fn parse_loop_candidates(buf: &[u8], off: usize) -> Vec<(Loop, usize)> {
    let Some(p) = body_start(buf, off, 0x0f) else {
        return Vec::new();
    };
    let Some(attr) = attr_at(buf, p) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (len, refs) in [
        (
            loop_body::LEN,
            buf.get(p..p + loop_body::LEN)
                .and_then(|_| refs_be::<4>(buf, p + loop_body::REFS)),
        ),
        (
            loop_delta::LEN,
            buf.get(p..p + loop_delta::LEN)
                .and_then(|_| refs_tripled::<4>(buf, p + loop_delta::REFS)),
        ),
    ] {
        if let Some(refs) = refs {
            out.push((
                Loop {
                    attr,
                    refs,
                    offset: off,
                },
                p + len,
            ));
        }
    }
    out
}

/// Return complete or compact edge-use readings at one offset.
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
    if buf.get(p + edge_delta::TOLERANCE - 1) == Some(&1)
        && buf.get(p + edge_delta::TOLERANCE..p + edge_delta::REFS) == Some(MAGIC.as_slice())
    {
        if let Some(refs) = refs_tripled::<7>(buf, p + edge_delta::REFS).filter(|refs| {
            refs.iter()
                .all(|reference| *reference <= MAX_SHORT_POINTER_INDEX)
                && View::u16_be_at(buf, p + edge_delta::ATTRIBUTES_HEAD)
                    .is_some_and(|head| head <= MAX_SHORT_POINTER_INDEX)
        }) {
            return vec![EdgeUse {
                attr,
                sequence,
                references: EdgeReferences::Bare([
                    refs[0], refs[1], refs[2], refs[3], refs[4], refs[5],
                ]),
                offset: off,
            }];
        }
    }
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

    // The 28-byte body check above keeps every candidate magic window in bounds.
    for magic in p + 9..=p + 16 {
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

/// Edge-use `00 10`: six-cell projection at body+16, magic at body+8.
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
    if p + fin_body::LEN > buf.len() {
        return Vec::new();
    }
    let Some(attr) = attr_at(buf, p) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let (Some(refs), Some(marker)) = (
        refs_be::<9>(buf, p + fin_body::REFS),
        buf.get(p + fin_body::SENSE).copied(),
    ) {
        if let Some(sense) = parse_sense(marker) {
            out.push(Coedge {
                attr,
                refs,
                sense,
                offset: off,
            });
        }
    }
    if let (Some(refs), Some(marker)) = (
        refs_tripled::<9>(buf, p + fin_delta::REFS),
        buf.get(p + fin_delta::SENSE).copied(),
    ) {
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

/// Vertex-use `00 12`: five-cell projection at body+6, magic at body+16.
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
        // A window past the end of `buf` reads as no magic.
        let magic = (p + 21..=p + 32)
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
    let attr = attr_at(buf, p).filter(|attr| *attr > 1)?;
    let (references, xyz_at) = if prefixed {
        buf.get(p..p + world_pt_delta::LEN)?;
        (
            refs_tripled::<4>(buf, p + world_pt_delta::REFS)?,
            p + world_pt_delta::XYZ,
        )
    } else {
        buf.get(p..p + world_pt::LEN)?;
        (refs_be::<4>(buf, p + world_pt::REFS)?, p + world_pt::XYZ)
    };
    if references[0] > 1
        || references
            .iter()
            .any(|reference| *reference > MAX_SHORT_POINTER_INDEX)
    {
        return None;
    }
    let x = View::f64_be_at(buf, xyz_at)?;
    let y = View::f64_be_at(buf, xyz_at + 8)?;
    let z = View::f64_be_at(buf, xyz_at + 16)?;
    if [x, y, z].iter().any(|value| !value.is_finite()) {
        return None;
    }
    Some(Point {
        attr,
        refs: references.to_vec(),
        xyz_m: [x, y, z],
        xyz_offset: xyz_at,
        offset: off,
    })
}

struct RecordFrame {
    partition: bool,
    bytes: Option<Vec<u8>>,
}

fn merge_record_frame<T>(
    records: &mut HashMap<u16, T>,
    frames: &mut HashMap<u16, RecordFrame>,
    attr: u16,
    frame: RecordFrame,
    record: Option<T>,
) {
    if let Some(current) = frames.get_mut(&attr) {
        if current.partition && !frame.partition {
            return;
        }
        if current.partition == frame.partition {
            if current.bytes.is_none() || current.bytes != frame.bytes {
                current.bytes = None;
                records.remove(&attr);
            }
            return;
        }
    }
    frames.insert(attr, frame);
    records.remove(&attr);
    if let Some(record) = record {
        records.insert(attr, record);
    }
}

fn merge_framed_records<T>(
    target: &mut HashMap<u16, T>,
    frames: &mut HashMap<u16, RecordFrame>,
    mut source: HashMap<u16, T>,
    source_frames: HashMap<u16, RecordFrame>,
) {
    for (attr, frame) in source_frames {
        let record = source.remove(&attr);
        merge_record_frame(target, frames, attr, frame, record);
    }
    source.retain(|attr, _| !frames.contains_key(attr));
    merge_missing(target, source);
}

/// The topology record tables of one stream, each keyed by `attr`.
#[derive(Default)]
pub(crate) struct Tables {
    bridges: HashMap<u16, Bridge>,
    loops: HashMap<u16, Loop>,
    loop_frames: HashMap<u16, RecordFrame>,
    edge_uses: HashMap<u16, EdgeUse>,
    coedges: HashMap<u16, Coedge>,
    vertex_uses: HashMap<u16, VertexUse>,
    points: HashMap<u16, Point>,
    point_frames: HashMap<u16, RecordFrame>,
}

impl Tables {
    pub(super) fn bridges(&self) -> &HashMap<u16, Bridge> {
        &self.bridges
    }

    #[cfg(test)]
    pub(super) fn insert_bridge(&mut self, record: Bridge) {
        self.bridges.insert(record.attr, record);
    }

    pub(super) fn loops(&self) -> &HashMap<u16, Loop> {
        &self.loops
    }

    pub(super) fn loop_is_ambiguous(&self, attr: u16) -> bool {
        self.loop_frames
            .get(&attr)
            .is_some_and(|frame| frame.bytes.is_none())
    }

    #[cfg(test)]
    pub(super) fn insert_loop(&mut self, record: Loop) {
        self.loops.insert(record.attr, record);
    }

    pub(super) fn edge_uses(&self) -> &HashMap<u16, EdgeUse> {
        &self.edge_uses
    }

    pub(super) fn coedges(&self) -> &HashMap<u16, Coedge> {
        &self.coedges
    }

    #[cfg(test)]
    pub(super) fn insert_coedge(&mut self, record: Coedge) {
        self.coedges.insert(record.attr, record);
    }

    pub(super) fn vertex_uses(&self) -> &HashMap<u16, VertexUse> {
        &self.vertex_uses
    }

    pub(crate) fn points(&self) -> &HashMap<u16, Point> {
        &self.points
    }

    pub(super) fn point_is_ambiguous(&self, attr: u16) -> bool {
        self.point_frames
            .get(&attr)
            .is_some_and(|frame| frame.bytes.is_none())
    }

    fn merge_point_frame(&mut self, attr: u16, frame: RecordFrame, point: Option<Point>) {
        merge_record_frame(&mut self.points, &mut self.point_frames, attr, frame, point);
    }

    /// Merge deltas without replacing partition topology membership.
    ///
    /// Preserve partition topology for shared identities and add only deltas
    /// bridges selected by the typed FACE ownership set.
    pub(super) fn merge_deltas(
        &mut self,
        mut deltas: Self,
        selected_bridge_attrs: Option<&HashSet<u16>>,
    ) {
        if self.bridges.is_empty() {
            if let Some(selected_bridge_attrs) = selected_bridge_attrs {
                retain_selected_bridges(&mut deltas.bridges, selected_bridge_attrs);
            }
            self.bridges = deltas.bridges;
        } else if let Some(selected_bridge_attrs) = selected_bridge_attrs {
            retain_selected_bridges(&mut deltas.bridges, selected_bridge_attrs);
            merge_missing(&mut self.bridges, deltas.bridges);
        }
        merge_framed_records(
            &mut self.loops,
            &mut self.loop_frames,
            deltas.loops,
            deltas.loop_frames,
        );
        merge_missing(&mut self.edge_uses, deltas.edge_uses);
        merge_missing(&mut self.coedges, deltas.coedges);
        merge_missing(&mut self.vertex_uses, deltas.vertex_uses);
        merge_framed_records(
            &mut self.points,
            &mut self.point_frames,
            deltas.points,
            deltas.point_frames,
        );
    }
}

fn retain_selected_bridges(
    bridges: &mut HashMap<u16, Bridge>,
    selected_bridge_attrs: &HashSet<u16>,
) {
    bridges.retain(|attr, _| selected_bridge_attrs.contains(attr));
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
struct CoedgeEvidence<'a> {
    owner: Option<&'a Loop>,
    start: Option<&'a VertexUse>,
    edge: Option<&'a [EdgeUse]>,
    next_owner: Option<bool>,
    previous: bool,
    loop_head: bool,
}

impl CoedgeEvidence<'_> {
    fn flags(self) -> [bool; 7] {
        [
            self.owner.is_some(),
            self.start.is_some(),
            self.edge.is_some(),
            self.next_owner == Some(true),
            self.next_owner.is_some(),
            self.previous,
            self.loop_head,
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
fn coedge_evidence<'a>(
    candidate: &Coedge,
    loops: &'a [Loop],
    bridges: &HashMap<u16, Bridge>,
    vertex_uses: &'a HashMap<u16, VertexUse>,
    edge_candidates: &'a CandidateMap<EdgeUse>,
    coedge_candidates: &CandidateMap<Coedge>,
) -> CoedgeEvidence<'a> {
    let owner = candidate.refs[1];
    let owner_evidence = loops
        .iter()
        .find(|loop_| loop_.attr == owner && owner != 0 && loop_is_owned(loop_, bridges));
    let start = candidate.refs[4];
    let start_evidence = vertex_uses.get(&start).filter(|_| start != 0);
    let edge = candidate.refs[6];
    let edge_evidence = edge_candidates
        .get(&edge)
        .filter(|_| edge != 0)
        .map(Vec::as_slice);
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
        owner: owner_evidence,
        start: start_evidence,
        edge: edge_evidence,
        next_owner: next_owner_valid,
        previous: previous_valid,
        loop_head: loop_head_valid,
    }
}

fn evidence_dominates(left: CoedgeEvidence<'_>, right: CoedgeEvidence<'_>) -> bool {
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
    let evidence: Vec<_> = candidates
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
    let mut maximal = Vec::new();
    for index in 0..candidates.len() {
        if !evidence.iter().enumerate().any(|(other, other_evidence)| {
            other != index && evidence_dominates(*other_evidence, evidence[index])
        }) {
            maximal.push(index);
        }
    }
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
    let mut tables = scan_records(buf, false, None, None, None);
    let Some(record) = tables.points.remove(&attr) else {
        return false;
    };
    patch_point_values(buf, record.xyz_offset, record.xyz_m, xyz_m)
}

/// Scan stream-local topology records.
pub(crate) fn scan(body: &[u8]) -> Tables {
    scan_records(body, false, None, None, None)
}

/// Scan a partition stream while admitting typed FACE offsets that carry a
/// loop head.  A typed FACE and a compact bridge share the `00 0e` framing.
/// An ownership-only typed FACE has a null loop field and must not replace a
/// separate compact bridge for the same attribute; a shared FACE/bridge record
/// has a non-null loop field and is the topology record as well.
pub(super) fn scan_with_curve_attrs_excluding(
    body: &[u8],
    curve_attrs: &HashSet<u16>,
    excluded_bridge_offsets: &HashSet<usize>,
) -> Tables {
    scan_records(
        body,
        false,
        Some(curve_attrs),
        Some(excluded_bridge_offsets),
        None,
    )
}

/// Scan a deltas stream with the typed FACE/compact-bridge overlap rule.
pub(super) fn scan_deltas_with_curve_attrs_excluding(
    body: &[u8],
    curve_attrs: &HashSet<u16>,
    excluded_bridge_offsets: &HashSet<usize>,
    base: &Tables,
) -> Tables {
    scan_records(
        body,
        true,
        Some(curve_attrs),
        Some(excluded_bridge_offsets),
        Some(base),
    )
}

fn scan_records(
    body: &[u8],
    deltas: bool,
    curve_attrs: Option<&HashSet<u16>>,
    excluded_bridge_offsets: Option<&HashSet<usize>>,
    base: Option<&Tables>,
) -> Tables {
    let mut t = Tables::default();
    let mut loop_candidates = Vec::new();
    let mut loop_frames = Vec::new();
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
                        t.bridges.insert(record.attr, record);
                    }
                }
            }
            0x0f => {
                for (record, end) in parse_loop_candidates(body, i) {
                    loop_candidates.push(record.clone());
                    loop_frames.push((record, end));
                }
            }
            0x10 => insert_candidates(&mut edge_candidates, parse_edge_use_candidates(body, i)),
            0x11 => insert_candidates(&mut coedge_candidates, parse_coedge_candidates(body, i)),
            0x12 => {
                if let Some(record) = parse_vertex_use(body, i) {
                    t.vertex_uses.insert(record.attr, record);
                }
            }
            0x1d => {
                let record = parse_point(body, i, deltas);
                if let Some(record) = record {
                    let end = record.xyz_offset + std::mem::size_of::<[f64; 3]>();
                    let frame = RecordFrame {
                        partition: !deltas,
                        bytes: Some(body[record.offset..end].to_vec()),
                    };
                    t.merge_point_frame(record.attr, frame, Some(record));
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
            t.coedges.insert(record.attr, record);
        }
    }
    for candidates in edge_candidates.into_values() {
        if let Some(record) = select_edge_use(&candidates, &t.coedges, curve_attrs) {
            t.edge_uses.insert(record.attr, record);
        }
    }
    for (record, end) in loop_frames {
        let owner = record.refs[2];
        let first = record.refs[1];
        let owner_exists = t.bridges.contains_key(&owner)
            || base.is_some_and(|base| base.bridges.contains_key(&owner));
        let first_coedge = base
            .and_then(|base| base.coedges.get(&first))
            .or_else(|| t.coedges.get(&first));
        if owner_exists && first_coedge.is_some_and(|coedge| coedge.refs[1] == record.attr) {
            let frame = RecordFrame {
                partition: !deltas,
                bytes: Some(body[record.offset..end].to_vec()),
            };
            merge_record_frame(
                &mut t.loops,
                &mut t.loop_frames,
                record.attr,
                frame,
                Some(record),
            );
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::{
        parse_bridge, parse_point, parse_vertex_use, patch_point, patch_point_values, scan,
        scan_deltas_with_curve_attrs_excluding, scan_with_curve_attrs_excluding, EdgeReferences,
        MAGIC,
    };
    use crate::test_support::parasolid::framed_topology_loop;
    use cadmpeg_ir::topology::Sense;
    use std::collections::HashSet;

    fn merge_bridge(attr: u16, attributes_head: u16) -> super::Bridge {
        super::Bridge {
            attr,
            refs: [201, 202, 203, 204, 205],
            sequence: 947,
            sense: Sense::Forward,
            attributes_head: Some(attributes_head),
            offset: 0,
        }
    }

    #[test]
    fn bridge_membership_uses_partition_presence_and_typed_face_identity() {
        for partition_present in [false, true] {
            for typed_selection in [false, true] {
                let original = merge_bridge(71, 101);
                let mut partition = super::Tables::default();
                if partition_present {
                    partition.insert_bridge(original.clone());
                }
                let mut deltas = super::Tables::default();
                let mut competing = merge_bridge(71, 141);
                competing.refs[2] = 307;
                competing.sequence = 948;
                competing.sense = Sense::Reversed;
                competing.offset = 80;
                for record in [
                    competing.clone(),
                    merge_bridge(73, 103),
                    merge_bridge(79, 109),
                    merge_bridge(83, 113),
                ] {
                    deltas.insert_bridge(record);
                }
                let selected = HashSet::from([71, 73, 109]);
                partition.merge_deltas(deltas, typed_selection.then_some(&selected));
                let expected = match (partition_present, typed_selection) {
                    (false, false) => HashSet::from([71, 73, 79, 83]),
                    (_, true) => HashSet::from([71, 73]),
                    (true, false) => HashSet::from([71]),
                };
                assert_eq!(
                    partition.bridges.keys().copied().collect::<HashSet<_>>(),
                    expected
                );
                assert_eq!(
                    partition.bridges[&71],
                    if partition_present {
                        original
                    } else {
                        competing
                    }
                );
            }
        }
    }

    #[test]
    fn deltas_fill_missing_subordinates_and_preserve_partition_point_coordinates() {
        let point = |attr, xyz_m| super::Point {
            attr,
            refs: vec![0, 501, 502, 503],
            xyz_m,
            xyz_offset: 16,
            offset: 0,
        };
        let mut partition = super::Tables::default();
        partition.points.insert(501, point(501, [1.0, 2.0, 3.0]));
        partition.loops.insert(
            401,
            super::Loop {
                attr: 401,
                refs: [0, 411, 71, 0],
                offset: 0,
            },
        );
        let mut deltas = super::Tables::default();
        deltas.points.insert(501, point(501, [4.0, 5.0, 6.0]));
        deltas.points.insert(502, point(502, [7.0, 8.0, 9.0]));
        deltas.loops.insert(
            401,
            super::Loop {
                attr: 401,
                refs: [0, 421, 73, 0],
                offset: 20,
            },
        );
        deltas.loops.insert(
            402,
            super::Loop {
                attr: 402,
                refs: [0, 431, 79, 0],
                offset: 40,
            },
        );
        partition.merge_deltas(deltas, None);
        assert_eq!(partition.points[&501].xyz_m, [1.0, 2.0, 3.0]);
        assert_eq!(partition.points[&501].refs, vec![0, 501, 502, 503]);
        assert_eq!(partition.points[&502].xyz_m, [7.0, 8.0, 9.0]);
        assert_eq!(partition.loops[&401].refs, [0, 411, 71, 0]);
        assert_eq!(partition.loops[&402].refs, [0, 431, 79, 0]);
    }

    #[test]
    fn bare_parasolid_point_copies_four_references() {
        let mut bytes = vec![0, 0x1d];
        bytes.extend(60_u16.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        for reference in [0_u16, 0x0102, 0, 0] {
            bytes.extend(reference.to_be_bytes());
        }
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend(value.to_be_bytes());
        }
        assert_eq!(
            parse_point(&bytes, 0, false).map(|point| point.refs.len()),
            Some(4)
        );
    }

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
        assert_eq!(bridge.attributes_head, Some(0x4321));
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
            assert_eq!(bridge.attributes_head, Some(0x4321));
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
        let tables = scan_deltas_with_curve_attrs_excluding(
            &bytes,
            &curve_attrs,
            &HashSet::new(),
            &super::Tables::default(),
        );
        assert_eq!(tables.edge_uses[&40].references.curve(), 0x0103);
    }

    fn tripled_point(attr: u16, xyz: [f64; 3], first_of_type: bool) -> Vec<u8> {
        let mut bytes = vec![0, 0x1d];
        if first_of_type {
            bytes.push(0xff);
        }
        bytes.extend(attr.to_be_bytes());
        bytes.extend(947u32.to_be_bytes());
        for reference in [1u16, 101, 102, 103] {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }
        for coordinate in xyz {
            bytes.extend(coordinate.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn point_coordinates_do_not_extend_the_four_reference_fields() {
        let x = f64::from_bits(0x3ff0_0100_0001_0000);
        for first_of_type in [false, true] {
            let bytes = tripled_point(211, [x, 2.0, 3.0], first_of_type);
            let point = parse_point(&bytes, 0, true).expect("complete POINT");
            assert_eq!(point.refs, [1, 101, 102, 103]);
            assert_eq!(point.xyz_m, [x, 2.0, 3.0]);
            assert_eq!(point.xyz_offset, 20 + usize::from(first_of_type));
        }
    }

    #[test]
    fn point_coordinates_have_no_format_magnitude_cap() {
        let xyz = [20_000.0, -20_000.0, 0.0];
        let bare = crate::test_support::parasolid::world_point(211, xyz);
        assert_eq!(parse_point(&bare, 0, false).unwrap().xyz_m, xyz);
        let tripled = tripled_point(211, xyz, false);
        assert_eq!(parse_point(&tripled, 0, true).unwrap().xyz_m, xyz);
    }

    #[test]
    fn point_admission_requires_finite_complete_coordinates_and_real_identity() {
        for xyz in [[f64::INFINITY, 2.0, 3.0], [1.0, f64::NAN, 3.0]] {
            assert!(parse_point(&tripled_point(211, xyz, false), 0, true).is_none());
        }
        let mut extended = tripled_point(211, [1.0, 2.0, 3.0], false);
        let owner = 2 + crate::layout::world_point_tripled::REFS + 3;
        extended[owner..owner + 2].copy_from_slice(&0x7ffeu16.to_be_bytes());
        assert!(parse_point(&extended, 0, true).is_some());
        extended[owner..owner + 2].copy_from_slice(&0x7fffu16.to_be_bytes());
        assert!(parse_point(&extended, 0, true).is_none());
        let mut bytes = tripled_point(211, [1.0, 2.0, 3.0], false);
        bytes.pop();
        assert!(parse_point(&bytes, 0, true).is_none());
        for attr in [0, 1] {
            assert!(parse_point(&tripled_point(attr, [1.0, 2.0, 3.0], false), 0, true).is_none());
        }
    }

    #[test]
    fn deltas_point_admission_has_no_bare_pointer_fallback() {
        let bytes = crate::test_support::parasolid::world_point(211, [1.0, 2.0, 3.0]);
        assert!(parse_point(&bytes, 0, false).is_some());
        let tables = scan_deltas_with_curve_attrs_excluding(
            &bytes,
            &HashSet::new(),
            &HashSet::new(),
            &super::Tables::default(),
        );
        assert!(!tables.points.contains_key(&211));
    }

    fn delta_points(bytes: &[u8]) -> super::Tables {
        scan_deltas_with_curve_attrs_excluding(
            bytes,
            &HashSet::new(),
            &HashSet::new(),
            &super::Tables::default(),
        )
    }

    fn partition_without_loop() -> Vec<u8> {
        let mut bytes = crate::test_support::parasolid::triangle_body();
        let original = crate::test_support::parasolid::loop_head(20, 30, 10);
        let at = bytes
            .windows(original.len())
            .position(|window| window == original)
            .unwrap();
        drop(bytes.drain(at..at + original.len()));
        bytes
    }

    fn delta_loops(bytes: &[u8], base: &super::Tables) -> super::Tables {
        scan_deltas_with_curve_attrs_excluding(bytes, &HashSet::new(), &HashSet::new(), base)
    }

    #[test]
    fn bare_loop_closure_survives_an_unowned_tripled_reading_at_the_same_offset() {
        for first_of_type in [false, true] {
            let frame = framed_topology_loop(20, 811, [0, 258, 769, 0], false, first_of_type);
            let mut padded = frame.clone();
            padded.extend([1, 0, 0, 1]);
            let readings = super::parse_loop_candidates(&padded, 0);
            assert_eq!(readings.len(), 2);
            assert_eq!(readings[0].0.refs, [0, 258, 769, 0]);
            assert_eq!(readings[1].0.refs, [0, 515, 0, 0]);
            let mut body = topology_bridge(769, 20);
            body.extend(padded);
            body.extend(topology_tripled_coedge(
                258,
                [0, 20, 0, 258, 50, 0, 40, 0, 0],
            ));
            let tables = scan(&body);
            assert_eq!(tables.loops()[&20].refs, [0, 258, 769, 0]);
            assert!(!tables.loop_is_ambiguous(20));
            assert_eq!(
                tables.loop_frames[&20].bytes.as_deref(),
                Some(frame.as_slice())
            );
        }
    }

    #[test]
    fn loop_full_frame_conflicts_are_sticky_in_every_order_and_stream_grouping() {
        let refs = [0, 30, 10, 0];
        let base = framed_topology_loop(20, 811, refs, true, false);
        let variants = [
            framed_topology_loop(20, 812, refs, true, false),
            framed_topology_loop(20, 811, refs, true, true),
            framed_topology_loop(20, 811, [1, 30, 10, 0], true, false),
            framed_topology_loop(20, 811, [0, 31, 10, 0], true, false),
            framed_topology_loop(20, 811, [0, 30, 10, 1], true, false),
        ];
        for variant in variants {
            let records = [&base, &variant, &base];
            for order in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                for cuts in [0u8, 1, 2, 3] {
                    let mut tables = scan(&partition_without_loop());
                    let mut stream = Vec::new();
                    for (index, record) in order.into_iter().enumerate() {
                        stream.extend(records[record]);
                        if index == 2 || cuts & (1 << index) != 0 {
                            let deltas = delta_loops(&stream, &tables);
                            tables.merge_deltas(deltas, None);
                            stream.clear();
                        }
                    }
                    assert!(!tables.loops().contains_key(&20));
                    assert!(tables.loop_is_ambiguous(20));
                }
            }
        }
    }

    #[test]
    fn identical_loop_frames_coalesce_using_partition_closure() {
        let bytes = framed_topology_loop(20, 811, [0, 30, 10, 0], true, true);
        let mut tables = scan(&partition_without_loop());
        for stream in [&bytes, &[bytes.as_slice(), bytes.as_slice()].concat()] {
            let deltas = delta_loops(stream, &tables);
            assert_eq!(deltas.loops().len(), 1);
            tables.merge_deltas(deltas, None);
        }
        assert_eq!(tables.loops()[&20].refs, [0, 30, 10, 0]);
        assert!(!tables.loop_is_ambiguous(20));
    }

    #[test]
    fn partition_loop_conflict_blocks_deltas_and_unowned_matches_do_not_poison_identity() {
        let partition = crate::test_support::parasolid::triangle_body();
        let conflict = framed_topology_loop(20, 812, [0, 30, 10, 0], false, false);
        let mut tables = scan(&[partition.as_slice(), conflict.as_slice()].concat());
        assert!(tables.loop_is_ambiguous(20));
        let delta = delta_loops(
            &framed_topology_loop(20, 811, [0, 30, 10, 0], true, false),
            &tables,
        );
        tables.merge_deltas(delta, None);
        assert!(!tables.loops().contains_key(&20));
        assert!(tables.loop_is_ambiguous(20));

        let unowned = framed_topology_loop(20, 812, [0, 30, 255, 0], false, false);
        for bytes in [
            [partition.as_slice(), unowned.as_slice()].concat(),
            [unowned.as_slice(), partition.as_slice()].concat(),
        ] {
            let tables = scan(&bytes);
            assert!(tables.loops().contains_key(&20));
            assert!(!tables.loop_is_ambiguous(20));
        }
    }

    #[test]
    fn partition_loop_presence_ignores_conflicting_history() {
        let mut tables = scan(&crate::test_support::parasolid::triangle_body());
        let before = tables.loops()[&20].clone();
        let bytes = [
            framed_topology_loop(20, 811, [0, 30, 10, 0], true, false),
            framed_topology_loop(20, 812, [0, 30, 10, 0], true, false),
        ]
        .concat();
        let deltas = delta_loops(&bytes, &tables);
        assert!(deltas.loop_is_ambiguous(20));
        tables.merge_deltas(deltas, None);
        assert_eq!(tables.loops()[&20], before);
        assert!(!tables.loop_is_ambiguous(20));
    }

    #[test]
    fn point_duplicates_compare_complete_frames_in_every_order_and_stream_grouping() {
        let base = tripled_point(211, [0.0, 2.0, 3.0], false);
        let mut variants = vec![tripled_point(211, [0.0, 2.0, 3.0], true)];
        let mut node = base.clone();
        node[4..8].copy_from_slice(&948u32.to_be_bytes());
        variants.push(node);
        for index in 0..4 {
            let mut reference = base.clone();
            let at = 2 + crate::layout::world_point_tripled::REFS + index * 3;
            reference[at..at + 2]
                .copy_from_slice(&(if index == 0 { 0u16 } else { 151 }).to_be_bytes());
            variants.push(reference);
        }
        variants.push(tripled_point(211, [-0.0, 2.0, 3.0], false));
        variants.push(tripled_point(211, [1.0, 2.0, 3.0], false));
        for variant in variants {
            assert!(parse_point(&variant, 0, true).is_some());
            let records = [&base, &variant, &base];
            for order in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                for cuts in [0u8, 1, 2, 3] {
                    let mut tables = super::Tables::default();
                    let mut stream = Vec::new();
                    for (index, record) in order.into_iter().enumerate() {
                        stream.extend(records[record]);
                        if index == 2 || cuts & (1 << index) != 0 {
                            tables.merge_deltas(delta_points(&stream), None);
                            stream.clear();
                        }
                    }
                    assert!(tables.points().is_empty());
                    assert!(tables.point_is_ambiguous(211));
                }
            }
        }
    }

    #[test]
    fn byte_identical_point_frames_coalesce_within_and_across_streams() {
        for partition in [false, true] {
            let point = if partition {
                crate::test_support::parasolid::world_point(211, [1.0, 2.0, 3.0])
            } else {
                tripled_point(211, [1.0, 2.0, 3.0], true)
            };
            let bytes = [point.as_slice(), point.as_slice()].concat();
            let scan_points = |bytes: &[u8]| {
                if partition {
                    scan(bytes)
                } else {
                    delta_points(bytes)
                }
            };
            let mut tables = scan_points(&bytes);
            tables.merge_deltas(scan_points(&point), None);
            assert_eq!(tables.points().len(), 1);
            assert_eq!(tables.points()[&211].xyz_m, [1.0, 2.0, 3.0]);
            assert!(!tables.point_is_ambiguous(211));
        }
    }

    #[test]
    fn partition_point_presence_and_conflicts_survive_competing_deltas() {
        let first = crate::test_support::parasolid::world_point(211, [1.0, 2.0, 3.0]);
        let second = crate::test_support::parasolid::world_point(211, [4.0, 5.0, 6.0]);
        let delta_first = tripled_point(211, [7.0, 8.0, 9.0], false);
        let delta_second = tripled_point(211, [10.0, 11.0, 12.0], false);
        for split_partition in [false, true] {
            let mut conflict = if split_partition {
                let mut tables = scan(&first);
                tables.merge_deltas(scan(&second), None);
                tables
            } else {
                scan(&[first.as_slice(), second.as_slice()].concat())
            };
            assert!(conflict.points().is_empty());
            conflict.merge_deltas(delta_points(&delta_first), None);
            assert!(conflict.points().is_empty());
            assert!(conflict.point_is_ambiguous(211));
        }
        for partition_first in [false, true] {
            let mut tables = super::Tables::default();
            if partition_first {
                tables.merge_deltas(scan(&first), None);
            }
            tables.merge_deltas(
                delta_points(&[delta_first.as_slice(), delta_second.as_slice()].concat()),
                None,
            );
            if !partition_first {
                tables.merge_deltas(scan(&first), None);
            }
            assert_eq!(tables.points()[&211].xyz_m, [1.0, 2.0, 3.0]);
            assert!(!tables.point_is_ambiguous(211));
        }
    }

    #[test]
    fn complete_tripled_edge_retains_canonical_fin_and_fourth_pointer_curve() {
        for first_of_type in [false, true] {
            let mut bytes = vec![0, 0x10];
            if first_of_type {
                bytes.push(0xff);
            }
            bytes.extend(211u16.to_be_bytes());
            bytes.extend(947u32.to_be_bytes());
            bytes.extend([0, 1, 1]);
            bytes.extend(MAGIC);
            for reference in [171u16, 172, 173, 174, 175, 176, 177] {
                bytes.extend(reference.to_be_bytes());
                bytes.push(1);
            }
            let records = super::parse_edge_use_candidates(&bytes, 0);
            assert_eq!(records.len(), 1);
            assert_eq!(
                records[0].references,
                EdgeReferences::Bare([171, 172, 173, 174, 175, 176])
            );
            assert_eq!(records[0].references.canonical(), Some(171));
            assert_eq!(records[0].references.curve(), 174);
            assert_eq!(records[0].sequence, 947);
        }
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
