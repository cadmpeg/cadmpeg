// SPDX-License-Identifier: Apache-2.0
//! Typed topology record parsing ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
//!
//! Six topology record families live at Parasolid stream scope and form the
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
//! Every record opens with `00 TT`, an optional `0xff`, then a native pointer `attr`
//! and, for most families, an `ordinal`/`seq` (u32). The magic
//! `c2 bc 92 8f 99 6e 00 00` anchors the bridge and edge-use parses.
//! VERTEX frames retain their complete tolerance and owning BODY or SHELL. Records are keyed by `attr` within each stream. Same-site merges
//! preserve partition records and add missing subordinate deltas records and
//! typed-FACE-selected bridges. The decoder
//! merges all admitted sites. It keeps the active site's primary identities
//! and qualifies the other sites' identities; without an active site, it
//! qualifies every site's identities.

use std::collections::{HashMap, HashSet};

use cadmpeg_core::decode::View;
use cadmpeg_ir::topology::Sense;

use crate::layout::edge_bare as edge_body;
use crate::layout::edge_tripled as edge_delta;
use crate::layout::face_bare as face_body;
use crate::layout::face_tripled as face_delta;
use crate::layout::fin_bare as fin_body;
use crate::layout::fin_tripled as fin_delta;
use crate::layout::loop_bare as loop_body;
use crate::layout::loop_tripled as loop_delta;
use crate::layout::vertex_bare as vertex_body;
use crate::layout::vertex_tripled as vertex_delta;
use crate::layout::world_point as world_pt;
use crate::layout::world_point_tripled as world_pt_delta;

/// The magic anchoring magic-bearing topology records ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

/// The least nonzero tolerance, metres, on which a plain stream frames a
/// record (spec §3.3).
const PLAIN_TOLERANCE_MIN_M: f64 = 1.0e-12;

/// The greatest tolerance, metres, on which a plain stream frames a record
/// (spec §3.3).
const PLAIN_TOLERANCE_MAX_M: f64 = 1.0e-2;

/// Whether the tolerance slot at `at` frames a record: the null-real
/// sentinel, and with `stored` also `0.0` or a stored tolerance (spec §3.3).
fn frames_on_tolerance(buf: &[u8], at: usize, stored: bool) -> bool {
    if buf.get(at..at + MAGIC.len()) == Some(MAGIC.as_slice()) {
        return true;
    }
    stored
        && View::f64_be_at(buf, at).is_some_and(|value| {
            value == 0.0 || (PLAIN_TOLERANCE_MIN_M..=PLAIN_TOLERANCE_MAX_M).contains(&value)
        })
}

const MAX_SHORT_POINTER_INDEX: u32 = 0x7fff;

/// The lowest count whose high u16 is non-zero, which is how the null pointer
/// after a pointer to node 82 reads as an `INT_VALUES` count (spec §5).
const INT_VALUES_WIDE_COUNT_MIN: u32 = 0x1_0000;

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Bridge {
    pub(super) attr: u32,
    pub(super) refs: [u32; 5],
    pub(super) sequence: u32,
    pub(super) sense: Sense,
    pub(super) attributes_head: Option<u32>,
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::brep) struct Loop {
    pub(super) attr: u32,
    pub(super) refs: [u32; 4],
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct EdgeUse {
    pub(super) attr: u32,
    pub(super) references: EdgeReferences,
    pub(super) sequence: u32,
    /// The stored tolerance of a tolerant edge, metres (spec §7.6); `None`
    /// for the null-real sentinel and the compact layout.
    pub(super) tolerance_m: Option<f64>,
    pub(super) offset: usize,
}

/// Bare edge-use cells or the curve-only compact layout.
#[derive(Debug, Clone, Eq)]
pub(in crate::brep) enum EdgeReferences {
    Bare([u32; 6]),
    Compact { curve: u32 },
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
    pub(super) fn canonical(&self) -> Option<u32> {
        match self {
            Self::Bare(refs) => Some(refs[0]),
            Self::Compact { .. } => None,
        }
    }

    pub(super) fn curve(&self) -> u32 {
        match self {
            Self::Bare(refs) => refs[3],
            Self::Compact { curve } => *curve,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Coedge {
    pub(super) attr: u32,
    pub(super) refs: [u32; 9],
    pub(super) sense: Sense,
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::brep) struct VertexUse {
    pub(super) attr: u32,
    pub(super) refs: [u32; 5],
    pub(super) sequence: u32,
    pub(super) owner: u32,
    pub(super) tolerance_m: Option<f64>,
    pub(super) offset: usize,
}

struct VertexReading {
    record: VertexUse,
    end: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Point {
    attr: u32,
    refs: Vec<u32>,
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
fn refs_be<const N: usize>(buf: &[u8], at: usize) -> Option<[u32; N]> {
    let mut out = [0; N];
    for (index, reference) in out.iter_mut().enumerate() {
        *reference = super::word_at(buf, at + 2 * index)?;
    }
    Some(out)
}

fn refs_tripled<const N: usize>(buf: &[u8], at: usize) -> Option<[u32; N]> {
    let mut out = [0; N];
    for (index, reference) in out.iter_mut().enumerate() {
        let p = at + index * 3;
        if buf.get(p + 2) != Some(&1) {
            return None;
        }
        *reference = super::word_at(buf, p)?;
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

fn attr_at(buf: &[u8], p: usize) -> Option<u32> {
    let a = super::word_at(buf, p)?;
    if a == 0 {
        None
    } else {
        Some(a)
    }
}

fn native_prefix(buf: &[u8], p: usize, node_id: bool) -> Option<(u32, usize)> {
    let mut at = p;
    let attr = cadmpeg_parasolid::pointer::read_at(buf, &mut at)?;
    (attr > 1).then_some(())?;
    if node_id {
        View::u32_be_at(buf, at)?;
        at += 4;
    }
    Some((attr, at))
}

fn native_bridge_prefix(buf: &[u8], off: usize, p: usize, plain: bool) -> Option<(Bridge, usize)> {
    let (attr, mut at) = native_prefix(buf, p, false)?;
    let sequence = View::u32_be_at(buf, at)?;
    at += 4;
    let head = cadmpeg_parasolid::pointer::read_at(buf, &mut at)?;
    frames_on_tolerance(buf, at, plain).then_some(())?;
    at += 8;
    let refs = super::native_refs::<5>(buf, &mut at)?;
    let sense = parse_sense(*buf.get(at)?)?;
    at += 1;
    Some((
        Bridge {
            attr,
            refs,
            sequence,
            sense,
            attributes_head: (head > 1).then_some(head),
            offset: off,
        },
        at,
    ))
}

fn native_bridge(buf: &[u8], off: usize, p: usize, plain: bool) -> Option<(Bridge, usize)> {
    let (record, mut at) = native_bridge_prefix(buf, off, p, plain)?;
    super::native_refs::<5>(buf, &mut at)?;
    Some((record, at))
}

fn parse_bridge(buf: &[u8], off: usize, plain: bool) -> Option<Bridge> {
    let mut candidates = super::record_starts(buf, off, 0x0e)
        .filter_map(|p| native_bridge(buf, off, p, plain).map(|(record, _)| record))
        .collect::<Vec<_>>();
    if let Some(record) = parse_bridge_short(buf, off, plain).filter(|record| {
        record.attr <= MAX_SHORT_POINTER_INDEX
            && record.refs.iter().all(|r| *r <= MAX_SHORT_POINTER_INDEX)
    }) {
        if !candidates.contains(&record) {
            candidates.push(record);
        }
    }
    (candidates.len() == 1).then(|| candidates.remove(0))
}

fn parse_loop_candidates(buf: &[u8], off: usize) -> Vec<(Loop, usize)> {
    let mut out = Vec::new();
    for p in super::record_starts(buf, off, 0x0f) {
        let parsed = (|| {
            let (attr, mut at) = native_prefix(buf, p, true)?;
            let refs = super::native_refs::<4>(buf, &mut at)?;
            Some((
                Loop {
                    attr,
                    refs,
                    offset: off,
                },
                at,
            ))
        })();
        out.extend(parsed);
    }
    let p = body_start(buf, off, 0x0f);
    out.extend(
        parse_loop_candidates_short(buf, off)
            .into_iter()
            .filter(|(record, end)| {
                p.is_some_and(|p| *end == p + loop_delta::LEN)
                    && record.attr <= MAX_SHORT_POINTER_INDEX
                    && record.refs.iter().all(|r| *r <= MAX_SHORT_POINTER_INDEX)
            }),
    );
    out.dedup();
    out
}

fn complete_end(buf: &[u8], end: usize) -> bool {
    end == buf.len()
        || buf
            .get(end..end + 2)
            .is_some_and(|tag| tag[0] == 0 && tag[1] != 0)
}

fn native_edge(buf: &[u8], off: usize, p: usize, stored: bool) -> Option<(EdgeUse, usize)> {
    let (attr, mut at) = native_prefix(buf, p, false)?;
    let sequence = View::u32_be_at(buf, at)?;
    at += 4;
    cadmpeg_parasolid::pointer::read_at(buf, &mut at)?;
    frames_on_tolerance(buf, at, stored).then_some(())?;
    let accurate = buf.get(at..at + 8) == Some(MAGIC.as_slice());
    let tolerance = View::f64_be_at(buf, at)?;
    at += 8;
    let refs = super::native_refs::<7>(buf, &mut at)?;
    complete_end(buf, at).then_some(())?;
    Some((
        EdgeUse {
            attr,
            sequence,
            references: EdgeReferences::Bare(refs[..6].try_into().ok()?),
            tolerance_m: (!accurate).then_some(tolerance),
            offset: off,
        },
        at,
    ))
}

fn parse_edge_use_candidates(buf: &[u8], off: usize) -> Vec<(EdgeUse, usize)> {
    let mut out = super::record_starts(buf, off, 0x10)
        .filter_map(|p| native_edge(buf, off, p, false))
        .collect::<Vec<_>>();
    out.extend(
        parse_edge_use_candidates_short(buf, off)
            .into_iter()
            .filter(|(r, _)| {
                r.attr <= MAX_SHORT_POINTER_INDEX && r.references.curve() <= MAX_SHORT_POINTER_INDEX
            }),
    );
    out.dedup();
    out
}

fn parse_tolerant_edge_use_candidates(buf: &[u8], off: usize) -> Vec<EdgeUse> {
    let mut out = super::record_starts(buf, off, 0x10)
        .filter_map(|p| native_edge(buf, off, p, true).map(|(r, _)| r))
        .filter(|r| r.references.curve() == 1 && r.tolerance_m.is_some_and(|t| t > 0.0))
        .collect::<Vec<_>>();
    out.extend(parse_tolerant_edge_use_candidates_short(buf, off));
    out.dedup();
    out
}

fn parse_coedge_candidates(buf: &[u8], off: usize) -> Vec<(Coedge, usize)> {
    let mut out = Vec::new();
    for p in super::record_starts(buf, off, 0x11) {
        let parsed = (|| {
            let (attr, mut at) = native_prefix(buf, p, false)?;
            let refs = super::native_refs::<9>(buf, &mut at)?;
            let sense = parse_sense(*buf.get(at)?)?;
            Some((
                Coedge {
                    attr,
                    refs,
                    sense,
                    offset: off,
                },
                at + 1,
            ))
        })();
        out.extend(parsed);
    }
    let p = body_start(buf, off, 0x11);
    out.extend(
        parse_coedge_candidates_short(buf, off)
            .into_iter()
            .filter(|(record, end)| {
                p.is_some_and(|p| *end == p + fin_delta::LEN)
                    && record.attr <= MAX_SHORT_POINTER_INDEX
                    && record.refs.iter().all(|r| *r <= MAX_SHORT_POINTER_INDEX)
            }),
    );
    out.dedup();
    out
}

fn parse_vertex_candidates(buf: &[u8], off: usize) -> Vec<VertexReading> {
    let mut out = Vec::new();
    for p in super::record_starts(buf, off, 0x12) {
        let parsed = (|| {
            let (attr, mut at) = native_prefix(buf, p, false)?;
            let sequence = View::u32_be_at(buf, at)?;
            at += 4;
            let refs = super::native_refs::<5>(buf, &mut at)?;
            let accurate = buf.get(at..at + 8) == Some(MAGIC.as_slice());
            let tolerance = View::f64_be_at(buf, at)?;
            tolerance.is_finite().then_some(())?;
            at += 8;
            let owner = cadmpeg_parasolid::pointer::read_at(buf, &mut at)?;
            complete_end(buf, at).then_some(())?;
            Some(VertexReading {
                record: VertexUse {
                    attr,
                    refs,
                    sequence,
                    owner,
                    tolerance_m: (!accurate).then_some(tolerance),
                    offset: off,
                },
                end: at,
            })
        })();
        out.extend(parsed);
    }
    let p = body_start(buf, off, 0x12);
    out.extend(
        parse_vertex_candidates_short(buf, off)
            .into_iter()
            .filter(|r| {
                r.record.attr <= MAX_SHORT_POINTER_INDEX
                    && p.is_some_and(|p| r.end == p + vertex_delta::LEN)
            }),
    );
    out.dedup_by(|a, b| a.record == b.record && a.end == b.end);
    out
}

fn parse_point(buf: &[u8], off: usize, prefixed: bool) -> Option<Point> {
    if prefixed {
        return parse_point_short(buf, off, prefixed);
    }
    let mut out = Vec::new();
    for p in super::record_starts(buf, off, 0x1d) {
        let parsed = (|| {
            let (attr, mut at) = native_prefix(buf, p, true)?;
            let refs = super::native_refs::<4>(buf, &mut at)?;
            (refs[0] <= 1).then_some(())?;
            let xyz_offset = at;
            let xyz_m = [
                View::f64_be_at(buf, at)?,
                View::f64_be_at(buf, at + 8)?,
                View::f64_be_at(buf, at + 16)?,
            ];
            xyz_m.iter().all(|v| v.is_finite()).then_some(())?;
            Some(Point {
                attr,
                refs: refs.to_vec(),
                xyz_m,
                xyz_offset,
                offset: off,
            })
        })();
        out.extend(parsed);
    }
    (out.len() == 1).then(|| out.remove(0))
}

/// Bridge `00 0e`: 37-byte body, tolerance slot at body+8, `refs[5]` at
/// body+16, marker at body+26. `refs[4]` = surface carrier, `refs[2]` = loop
/// head. `plain` admits the tolerance slot of a plain stream.
/// The deltas form stores the attribute-chain head as a `[hi][lo][01]` triple, so the magic
/// sits at body+9 and the five refs follow as triples with the marker after.
fn parse_bridge_short(buf: &[u8], off: usize, plain: bool) -> Option<Bridge> {
    let p = body_start(buf, off, 0x0e)?;
    if buf.get(p + 8) == Some(&1) && buf.get(p + 9..p + 17) == Some(MAGIC.as_slice()) {
        let attr = attr_at(buf, p)?;
        let sequence = View::u32_be_at(buf, p + 2)?;
        let attributes_head = super::word_at(buf, p + 6)?;
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
    if p + 37 > buf.len() || !frames_on_tolerance(buf, p + 8, plain) {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let sequence = View::u32_be_at(buf, p + 2)?;
    let attributes_head = super::word_at(buf, p + 6)?;
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

fn parse_loop_candidates_short(buf: &[u8], off: usize) -> Vec<(Loop, usize)> {
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

/// Return complete or compact edge-use readings at one offset, each with the
/// body-relative end of the framing it matched. Bare ends are the fixed body
/// lengths; compact ends are the end of the triple run the reading consumed.
fn parse_edge_use_candidates_short(buf: &[u8], off: usize) -> Vec<(EdgeUse, usize)> {
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
                && super::word_at(buf, p + edge_delta::ATTRIBUTES_HEAD)
                    .is_some_and(|head| head <= MAX_SHORT_POINTER_INDEX)
        }) {
            let Some(end) = p.checked_add(edge_delta::LEN) else {
                return Vec::new();
            };
            return vec![(
                EdgeUse {
                    attr,
                    sequence,
                    references: EdgeReferences::Bare([
                        refs[0], refs[1], refs[2], refs[3], refs[4], refs[5],
                    ]),
                    tolerance_m: None,
                    offset: off,
                },
                end,
            )];
        }
    }
    let mut out = Vec::new();
    if buf.get(p + 8..p + 16) == Some(MAGIC.as_slice()) {
        if let Some(refs) = refs_be::<6>(buf, p + 16) {
            if let Some(end) = p.checked_add(edge_body::LEN) {
                out.push((
                    EdgeUse {
                        attr,
                        sequence,
                        references: EdgeReferences::Bare(refs),
                        tolerance_m: None,
                        offset: off,
                    },
                    end,
                ));
            }
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
                let Some(reference) = super::word_at(buf, reference_at) else {
                    break;
                };
                decoded.push(reference);
                at += 3;
            }
            if decoded.len() >= 3 {
                out.push((
                    EdgeUse {
                        attr,
                        sequence,
                        references: EdgeReferences::Compact { curve: decoded[2] },
                        tolerance_m: None,
                        offset: off,
                    },
                    at,
                ));
            }
        }
    }
    out.dedup();
    out
}

/// Tolerant edge-use readings at `off` (spec §7.6).
fn parse_tolerant_edge_use_candidates_short(buf: &[u8], off: usize) -> Vec<EdgeUse> {
    let Some(p) = body_start(buf, off, 0x10) else {
        return Vec::new();
    };
    let (Some(attr), Some(sequence)) = (attr_at(buf, p), View::u32_be_at(buf, p + 2)) else {
        return Vec::new();
    };
    let valued = |at: usize| {
        View::f64_be_at(buf, at).filter(|value| {
            value.is_finite()
                && *value > 0.0
                && buf.get(at..at + MAGIC.len()) != Some(MAGIC.as_slice())
        })
    };
    let short = |refs: &[u32]| {
        refs.iter()
            .all(|reference| *reference <= MAX_SHORT_POINTER_INDEX)
    };
    let mut out = Vec::new();
    if let (Some(tolerance), Some(refs)) = (
        valued(p + edge_body::TOLERANCE),
        refs_be::<6>(buf, p + edge_body::REFS),
    ) {
        if refs[3] == 1 && short(&refs) {
            out.push(EdgeUse {
                attr,
                sequence,
                references: EdgeReferences::Bare(refs),
                tolerance_m: Some(tolerance),
                offset: off,
            });
        }
    }
    if buf.get(p + edge_delta::TOLERANCE - 1) == Some(&1) {
        if let (Some(tolerance), Some(refs)) = (
            valued(p + edge_delta::TOLERANCE),
            refs_tripled::<7>(buf, p + edge_delta::REFS),
        ) {
            if refs[3] == 1 && short(&refs) {
                out.push(EdgeUse {
                    attr,
                    sequence,
                    references: EdgeReferences::Bare([
                        refs[0], refs[1], refs[2], refs[3], refs[4], refs[5],
                    ]),
                    tolerance_m: Some(tolerance),
                    offset: off,
                });
            }
        }
    }
    out.dedup();
    out
}

/// Admit a tolerant reading when a selected fin names its edge and it is the
/// only valid reading.
fn admit_tolerant_edge_uses(t: &mut Tables, candidates: CandidateMap<EdgeUse>) {
    for (attr, readings) in candidates {
        if t.edge_uses.contains_key(&attr)
            || !t.coedges.values().any(|coedge| coedge.refs[6] == attr)
        {
            continue;
        }
        let mut valid = readings.iter().filter(|reading| {
            reading.references.canonical().is_none_or(|canonical| {
                canonical <= 1
                    || t.coedges
                        .get(&canonical)
                        .is_some_and(|coedge| coedge.refs[6] == attr)
            })
        });
        if let (Some(reading), None) = (valid.next(), valid.next()) {
            t.edge_uses.insert(attr, reading.clone());
        }
    }
}

/// Edge-use `00 10`: six-cell projection at body+16, magic at body+8.
/// `refs[0]` = canonical forward coedge when the bare record stores one;
/// `refs[3]` = support curve carrier. The compact layout has no canonical-coedge slot.
///
/// Coedge `00 11`: 21-byte body, no magic, `refs[9]` at body+2, marker at
/// body+20. `refs[1]` = owning loop, `refs[2]` = forward coedge, `refs[3]` =
/// backward coedge in a forward-linked loop and next coedge in another loop,
/// `refs[4]` = end vertex-use in a forward-linked loop and start vertex-use in
/// another loop, `refs[5]` = twin coedge, `refs[6]` = edge-use.
fn parse_coedge_candidates_short(buf: &[u8], off: usize) -> Vec<(Coedge, usize)> {
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
            out.push((
                Coedge {
                    attr,
                    refs,
                    sense,
                    offset: off,
                },
                p + fin_body::LEN,
            ));
        }
    }
    if let (Some(refs), Some(marker)) = (
        refs_tripled::<9>(buf, p + fin_delta::REFS),
        buf.get(p + fin_delta::SENSE).copied(),
    ) {
        if let Some(sense) = parse_sense(marker) {
            out.push((
                Coedge {
                    attr,
                    refs,
                    sense,
                    offset: off,
                },
                p + fin_delta::LEN,
            ));
        }
    }
    out.dedup();
    out
}

fn parse_vertex_candidates_short(buf: &[u8], off: usize) -> Vec<VertexReading> {
    let Some(p) = body_start(buf, off, 0x12) else {
        return Vec::new();
    };
    let Some(attr) = attr_at(buf, p).filter(|attr| *attr > 1 && *attr <= MAX_SHORT_POINTER_INDEX)
    else {
        return Vec::new();
    };
    let Some(sequence) = View::u32_be_at(buf, p + vertex_body::NODE_ID) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (len, tolerance_at, references, owner) in [
        (
            vertex_body::LEN,
            vertex_body::TOLERANCE,
            refs_be::<5>(buf, p + vertex_body::REFS),
            super::word_at(buf, p + vertex_body::OWNER),
        ),
        (
            vertex_delta::LEN,
            vertex_delta::TOLERANCE,
            refs_tripled::<5>(buf, p + vertex_delta::REFS),
            refs_tripled::<1>(buf, p + vertex_delta::OWNER).map(|refs| refs[0]),
        ),
    ] {
        let Some(refs) = references else { continue };
        let Some(owner) = owner else { continue };
        let Some(tolerance) = View::f64_be_at(buf, p + tolerance_at) else {
            continue;
        };
        if buf.get(p..p + len).is_none()
            || refs
                .iter()
                .any(|reference| *reference > MAX_SHORT_POINTER_INDEX)
            || owner > MAX_SHORT_POINTER_INDEX
            || !tolerance.is_finite()
        {
            continue;
        }
        let accurate =
            buf.get(p + tolerance_at..p + tolerance_at + MAGIC.len()) == Some(MAGIC.as_slice());
        out.push(VertexReading {
            record: VertexUse {
                attr,
                refs,
                sequence,
                owner,
                tolerance_m: (!accurate).then_some(tolerance),
                offset: off,
            },
            end: p + len,
        });
    }
    out
}

fn unsupported_vertex_prefix(buf: &[u8], off: usize) -> Option<VertexUse> {
    let p = body_start(buf, off, 0x12)?;
    let attr = attr_at(buf, p).filter(|attr| *attr > 1 && *attr <= MAX_SHORT_POINTER_INDEX)?;
    let sequence = View::u32_be_at(buf, p + vertex_delta::NODE_ID)?;
    for count in 6..=8 {
        let tolerance = p.checked_add(vertex_delta::REFS + count * 3)?;
        if buf.get(tolerance..tolerance + MAGIC.len()) != Some(MAGIC.as_slice()) {
            continue;
        }
        let mut refs = [0; 5];
        for index in 0..count {
            let at = p + vertex_delta::REFS + index * 3;
            if buf.get(at + 2) != Some(&1) {
                return None;
            }
            let reference = super::word_at(buf, at)?;
            if reference > MAX_SHORT_POINTER_INDEX {
                return None;
            }
            if index < refs.len() {
                refs[index] = reference;
            }
        }
        return Some(VertexUse {
            attr,
            refs,
            sequence,
            owner: 0,
            tolerance_m: None,
            offset: off,
        });
    }
    None
}

fn topology_header_prefix_end(buf: &[u8], off: usize, tag: u8) -> Option<usize> {
    if let Some(carrier) = super::parse_carrier(buf, off) {
        return Some(match carrier {
            super::Carrier::Curve(curve) => curve.end,
            super::Carrier::Surface(surface) => surface.end,
        });
    }
    if let Some(end) = super::subset::record_end(buf, off) {
        return Some(end);
    }
    if let Some(end) = super::sp_curve::record_end(buf, off) {
        return Some(end);
    }
    if let Some(end) = super::typed::schema_prefix_end(buf, off) {
        return Some(end);
    }
    if let Some(end) = super::spline::surface_descriptor_prefix_end(buf, off) {
        return Some(end);
    }
    if let Some(end) = super::spline::curve_prefix_end(buf, off) {
        return Some(end);
    }
    if let Some(end) = super::typed::tagged_owner_prefix_end(buf, off) {
        return Some(end);
    }
    let native = match tag {
        0x0e => super::record_starts(buf, off, tag)
            .filter_map(|p| native_bridge_prefix(buf, off, p, true).map(|(_, end)| end))
            .min(),
        0x10 => super::record_starts(buf, off, tag)
            .filter_map(|p| native_edge(buf, off, p, true).map(|(_, end)| end))
            .min(),
        _ => None,
    };
    if native.is_some() {
        return native;
    }
    let p = body_start(buf, off, tag)?;
    let mut ends = Vec::new();
    match tag {
        0x0e | 0x10 => {
            let attr = attr_at(buf, p)?;
            (2..=MAX_SHORT_POINTER_INDEX)
                .contains(&attr)
                .then_some(())?;
            let count = if tag == 0x0e { 5 } else { 7 };
            for tripled in [false, true] {
                let (head, tolerance, references) = match (tag, tripled) {
                    (0x0e, true) => (
                        face_delta::ATTRIBUTES_HEAD,
                        face_delta::TOLERANCE,
                        face_delta::REFS,
                    ),
                    (0x0e, false) => (
                        face_body::ATTRIBUTES_HEAD,
                        face_body::TOLERANCE,
                        face_body::REFS,
                    ),
                    (_, true) => (
                        edge_delta::ATTRIBUTES_HEAD,
                        edge_delta::TOLERANCE,
                        edge_delta::REFS,
                    ),
                    (_, false) => (
                        edge_body::ATTRIBUTES_HEAD,
                        edge_body::TOLERANCE,
                        edge_body::REFS,
                    ),
                };
                let head = if tripled {
                    refs_tripled::<1>(buf, p + head).map(|refs| refs[0])
                } else {
                    super::word_at(buf, p + head)
                };
                if head.is_none_or(|head| head > MAX_SHORT_POINTER_INDEX)
                    || !frames_on_tolerance(buf, p + tolerance, true)
                {
                    continue;
                }
                let mut at = p + references;
                let mut valid = true;
                for _ in 0..count {
                    let reference = if tripled {
                        refs_tripled::<1>(buf, at).map(|refs| refs[0])
                    } else {
                        super::word_at(buf, at)
                    };
                    if reference.is_none_or(|reference| reference > MAX_SHORT_POINTER_INDEX) {
                        valid = false;
                        break;
                    }
                    at += if tripled { 3 } else { 2 };
                }
                if !valid {
                    continue;
                }
                if tag == 0x0e {
                    let sense = if tripled {
                        face_delta::SENSE
                    } else {
                        face_body::SENSE
                    };
                    if at != p + sense
                        || buf.get(at).and_then(|value| parse_sense(*value)).is_none()
                    {
                        continue;
                    }
                    at += 1;
                }
                if tag == 0x10 {
                    let length = if tripled {
                        edge_delta::LEN
                    } else {
                        edge_body::LEN
                    };
                    if at != p + length {
                        continue;
                    }
                }
                ends.push(at);
            }
        }
        0x0f => ends.extend(
            parse_loop_candidates(buf, off)
                .into_iter()
                .map(|(_, end)| end),
        ),
        0x11 => ends.extend(
            parse_coedge_candidates(buf, off)
                .into_iter()
                .map(|(_, end)| end),
        ),
        0x12 => ends.extend(
            parse_vertex_candidates(buf, off)
                .into_iter()
                .map(|reading| reading.end),
        ),
        0x1d => {
            for prefixed in [false, true] {
                if let Some(point) = parse_point(buf, off, prefixed) {
                    ends.push(point.xyz_offset + std::mem::size_of::<[f64; 3]>());
                }
            }
        }
        _ => {}
    }
    ends.into_iter().min()
}

/// The end of the surface front refs of a FACE at `off` whose header prefix
/// ends at `sense_end` (spec §4).
fn face_front_refs_end(buf: &[u8], off: usize, sense_end: usize) -> Option<usize> {
    let p = body_start(buf, off, 0x0e)?;
    let (len, refs) = if sense_end == p + face_body::SURFACE_FRONT_REFS {
        (face_body::LEN, refs_be::<5>(buf, sense_end)?)
    } else if sense_end == p + face_delta::SURFACE_FRONT_REFS {
        (face_delta::LEN, refs_tripled::<5>(buf, sense_end)?)
    } else {
        return None;
    };
    refs.iter()
        .all(|reference| *reference <= MAX_SHORT_POINTER_INDEX)
        .then_some(p + len)
}

/// Framing window for the stated text length of a string node in the scope
/// pass.
const MAX_STRING_NODE_BYTES: usize = 4096;

fn string_node_end(buf: &[u8], p: usize, tag: u8) -> Option<usize> {
    if tag != 0x4f && tag != 0x54 {
        return None;
    }
    let len = usize::try_from(View::u32_be_at(buf, p)?).ok()?;
    if len == 0 || len > MAX_STRING_NODE_BYTES {
        return None;
    }
    let mut start = p.checked_add(4)?;
    let index = cadmpeg_parasolid::pointer::read_at(buf, &mut start)?;
    if index <= 1 {
        return None;
    }
    let end = start.checked_add(len)?;
    let text = buf.get(start..end)?;
    if !text.iter().all(|byte| (0x20..0x7f).contains(byte)) {
        return None;
    }
    buf.get(end).is_none_or(|next| *next == 0).then_some(end)
}

/// The record tag of an intersection composite.
const INTERSECTION_COMPOSITE_TAG: u8 = 0x26;

/// The end of a complete `00 26` intersection composite whose body starts at
/// `p`: a pointer `attr`, a sense marker, and six payload pointers (spec §7.3).
fn intersection_composite_end(buf: &[u8], p: usize, tag: u8) -> Option<usize> {
    use crate::layout::intersection_composite as isect;
    if tag != INTERSECTION_COMPOSITE_TAG {
        return None;
    }
    let attr = attr_at(buf, p)?;
    if !(2..=MAX_SHORT_POINTER_INDEX).contains(&attr)
        || !matches!(buf.get(p + isect::MARKER), Some(0x2b | 0x2d))
    {
        return None;
    }
    let payload = refs_be::<6>(buf, p + isect::PAYLOAD)?;
    if payload
        .iter()
        .any(|reference| *reference > MAX_SHORT_POINTER_INDEX)
    {
        return None;
    }
    let end = p + isect::LEN;
    (end <= buf.len()).then_some(end)
}

/// The end of an intersection composite written as the first record of its
/// class, after the class's edited type header (spec §4.1): the record has
/// no tag of its own and its body starts just past the header.
fn edited_intersection_composite_end(buf: &[u8], off: usize) -> Option<usize> {
    let (node_type, p) = super::typed::edited_header_end(buf, off)?;
    (node_type == u16::from(INTERSECTION_COMPOSITE_TAG)).then_some(())?;
    intersection_composite_end(buf, p, INTERSECTION_COMPOSITE_TAG)
}

pub(super) struct TopologyScopes {
    pub(super) payloads: Vec<std::ops::Range<usize>>,
    pub(super) excluded_starts: Vec<std::ops::Range<usize>>,
}

pub(super) fn topology_scopes(buf: &[u8], prefixed: bool) -> TopologyScopes {
    let array_attrs = super::spline::referenced_array_attrs(buf);
    let mut scopes = TopologyScopes {
        payloads: Vec::new(),
        excluded_starts: Vec::new(),
    };
    let mut off = 0;
    // Candidate starts ascend, so this forward-only position keeps the sentinel
    // search linear however many wide candidates a stream holds.
    let mut next_sentinel = 0;
    while off + 2 < buf.len() {
        let tag = buf[off + 1];
        let Some(p) = body_start(buf, off, tag).filter(|_| buf[off] == 0) else {
            off += 1;
            continue;
        };
        let end = match tag {
            0x2d | 0x7f | 0x80 => super::spline::array_record_end(buf, off, &array_attrs),
            0x52 => (|| {
                let (stated, _, start) = super::entity::integer_values_header(buf, p)?;
                let available = buf.len().checked_sub(start)?;
                let count = cadmpeg_core::decode::bounded_len(u64::from(stated), 4, available)?;
                let end = start.checked_add(count.checked_mul(4)?)?;
                // A node ends where the next node's tag begins. A `00 52` read
                // inside another record's fields can state a count that fits the
                // buffer; its extent then ends mid-record and would hide every
                // real node it spans.
                buf.get(end).is_none_or(|next| *next == 0).then_some(())?;
                if stated >= INT_VALUES_WIDE_COUNT_MIN {
                    if next_sentinel < start {
                        next_sentinel = buf[start..]
                            .windows(MAGIC.len())
                            .position(|window| window == MAGIC)
                            .map_or(buf.len(), |at| start + at);
                    }
                    if next_sentinel + MAGIC.len() <= end {
                        return None;
                    }
                }
                Some(end)
            })(),
            0x51 => (|| {
                let header = super::entity::attribute_header(buf, p)?;
                let count = usize::from(header.flags.to_be_bytes()[3]);
                if !(1..=0x20).contains(&count)
                    || header.attr <= 1
                    || header.disc <= 1
                    || header.seq == 0
                {
                    return None;
                }
                let count = count.checked_add(5)?;
                let mut at = header.fields;
                if prefixed {
                    for _ in 0..count {
                        if buf.get(at) != Some(&1) {
                            return None;
                        }
                        if super::word_at(buf, at + 1)? > MAX_SHORT_POINTER_INDEX {
                            return None;
                        }
                        at = at.checked_add(3)?;
                    }
                    (buf.get(at) == Some(&0)).then_some(at + 1)
                } else {
                    for _ in 0..count {
                        cadmpeg_parasolid::pointer::read_at(buf, &mut at)?;
                    }
                    Some(at)
                }
            })(),
            _ => None,
        };
        if let Some(end) = end.filter(|end| *end <= buf.len()) {
            scopes.payloads.push(off..end);
            scopes.excluded_starts.push(off..end);
            off = end;
        } else if let Some(end) = string_node_end(buf, p, tag) {
            // A string node's length and text hold no record header.
            scopes.excluded_starts.push(off + 1..end);
            off = end;
        } else if let Some(end) = intersection_composite_end(buf, p, tag)
            .or_else(|| edited_intersection_composite_end(buf, off))
        {
            // A complete intersection composite holds no topology header.
            scopes.excluded_starts.push(off + 1..end);
            off = end;
        } else if let Some(end) = topology_header_prefix_end(buf, off, tag) {
            scopes.excluded_starts.push(off + 1..end);
            // A FACE's surface front refs hold no record header.
            off = face_front_refs_end(buf, off, end).unwrap_or(end);
        } else {
            off += 1;
        }
    }
    scopes
}

#[cfg(test)]
pub(super) fn topology_payload_ranges(buf: &[u8], prefixed: bool) -> Vec<std::ops::Range<usize>> {
    topology_scopes(buf, prefixed).payloads
}

pub(super) fn point_class_attrs(body: &[u8], prefixed: bool) -> HashSet<u32> {
    let scopes = topology_scopes(body, prefixed);
    let ranges = &scopes.excluded_starts;
    let mut payload_index = 0;
    let mut out = HashSet::new();
    let mut at = 0;
    while at + 2 <= body.len() {
        while ranges
            .get(payload_index)
            .is_some_and(|range| range.end <= at)
        {
            payload_index += 1;
        }
        if let Some(range) = ranges
            .get(payload_index)
            .filter(|range| range.contains(&at))
        {
            at = range.end;
            continue;
        }
        if body.get(at..at + 2) == Some(&[0, 0x1d]) {
            if let Some(point) = parse_point(body, at, prefixed) {
                let end = point.xyz_offset + std::mem::size_of::<[f64; 3]>();
                let index = scopes.payloads.partition_point(|range| range.end <= at);
                if scopes
                    .payloads
                    .get(index)
                    .is_none_or(|range| range.start >= end)
                {
                    out.insert(point.attr);
                }
            }
        }
        at += 1;
    }
    out
}

/// World point `00 1d`: 38-byte body, no magic, `refs[4]` at body+6, xyz as
/// three big-endian f64 (metres) at body+14.
fn parse_point_short(buf: &[u8], off: usize, prefixed: bool) -> Option<Point> {
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
    records: &mut HashMap<u32, T>,
    frames: &mut HashMap<u32, RecordFrame>,
    attr: u32,
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
    target: &mut HashMap<u32, T>,
    frames: &mut HashMap<u32, RecordFrame>,
    mut source: HashMap<u32, T>,
    source_frames: HashMap<u32, RecordFrame>,
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
    bridges: HashMap<u32, Bridge>,
    loops: HashMap<u32, Loop>,
    loop_frames: HashMap<u32, RecordFrame>,
    edge_uses: HashMap<u32, EdgeUse>,
    coedges: HashMap<u32, Coedge>,
    coedge_frames: HashMap<u32, RecordFrame>,
    coedge_owners: HashMap<u32, HashSet<u32>>,
    vertex_uses: HashMap<u32, VertexUse>,
    vertex_frames: HashMap<u32, RecordFrame>,
    vertex_classes: HashMap<u32, bool>,
    owner_classes: HashSet<u32>,
    points: HashMap<u32, Point>,
    point_frames: HashMap<u32, RecordFrame>,
}

impl Tables {
    pub(super) fn bridges(&self) -> &HashMap<u32, Bridge> {
        &self.bridges
    }

    #[cfg(test)]
    pub(super) fn insert_bridge(&mut self, record: Bridge) {
        self.bridges.insert(record.attr, record);
    }

    pub(super) fn loops(&self) -> &HashMap<u32, Loop> {
        &self.loops
    }

    pub(super) fn loop_is_ambiguous(&self, attr: u32) -> bool {
        self.loop_frames
            .get(&attr)
            .is_some_and(|frame| frame.bytes.is_none())
    }

    #[cfg(test)]
    pub(super) fn insert_loop(&mut self, record: Loop) {
        self.loops.insert(record.attr, record);
    }

    pub(super) fn edge_uses(&self) -> &HashMap<u32, EdgeUse> {
        &self.edge_uses
    }

    pub(super) fn coedges(&self) -> &HashMap<u32, Coedge> {
        &self.coedges
    }

    pub(super) fn coedge_is_ambiguous(&self, attr: u32) -> bool {
        self.coedge_frames
            .get(&attr)
            .is_some_and(|frame| frame.bytes.is_none())
    }

    fn merge_coedge_frame(
        &mut self,
        attr: u32,
        frame: RecordFrame,
        record: Option<Coedge>,
        owners: HashSet<u32>,
    ) {
        if let Some(current) = self.coedge_frames.get(&attr) {
            if current.partition && !frame.partition {
                return;
            }
            if current.partition == frame.partition {
                self.coedge_owners.entry(attr).or_default().extend(owners);
            } else {
                self.coedge_owners.insert(attr, owners);
            }
        } else {
            self.coedge_owners.insert(attr, owners);
        }
        merge_record_frame(
            &mut self.coedges,
            &mut self.coedge_frames,
            attr,
            frame,
            record,
        );
    }

    #[cfg(test)]
    pub(super) fn insert_coedge(&mut self, record: Coedge) {
        self.coedges.insert(record.attr, record);
    }

    pub(super) fn vertex_uses(&self) -> &HashMap<u32, VertexUse> {
        &self.vertex_uses
    }

    pub(super) fn vertex_is_unresolved(&self, attr: u32) -> bool {
        attr > 1 && !self.vertex_uses.contains_key(&attr)
    }

    fn merge_vertex_frame(&mut self, attr: u32, frame: RecordFrame, record: Option<VertexUse>) {
        self.vertex_classes
            .entry(attr)
            .and_modify(|partition| *partition |= frame.partition)
            .or_insert(frame.partition);
        merge_record_frame(
            &mut self.vertex_uses,
            &mut self.vertex_frames,
            attr,
            frame,
            record,
        );
    }

    pub(crate) fn points(&self) -> &HashMap<u32, Point> {
        &self.points
    }

    pub(super) fn point_is_ambiguous(&self, attr: u32) -> bool {
        self.point_frames
            .get(&attr)
            .is_some_and(|frame| frame.bytes.is_none())
    }

    fn merge_point_frame(&mut self, attr: u32, frame: RecordFrame, point: Option<Point>) {
        merge_record_frame(&mut self.points, &mut self.point_frames, attr, frame, point);
    }

    /// Merge deltas without replacing partition topology membership.
    ///
    /// Preserve partition topology for shared identities and add only deltas
    /// bridges selected by the typed FACE ownership set.
    pub(super) fn merge_deltas(
        &mut self,
        mut deltas: Self,
        selected_bridge_attrs: Option<&HashSet<u32>>,
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
        for (attr, frame) in deltas.coedge_frames {
            let record = deltas.coedges.remove(&attr);
            let owners = deltas.coedge_owners.remove(&attr).unwrap_or_default();
            self.merge_coedge_frame(attr, frame, record, owners);
        }
        deltas
            .coedges
            .retain(|attr, _| !self.coedge_frames.contains_key(attr));
        merge_missing(&mut self.coedges, deltas.coedges);
        self.owner_classes
            .extend(deltas.owner_classes.iter().copied());
        for (attr, partition) in deltas.vertex_classes {
            self.vertex_classes
                .entry(attr)
                .and_modify(|current| *current |= partition)
                .or_insert(partition);
        }
        merge_framed_records(
            &mut self.vertex_uses,
            &mut self.vertex_frames,
            deltas.vertex_uses,
            deltas.vertex_frames,
        );
        merge_framed_records(
            &mut self.points,
            &mut self.point_frames,
            deltas.points,
            deltas.point_frames,
        );
    }
}

fn retain_selected_bridges(
    bridges: &mut HashMap<u32, Bridge>,
    selected_bridge_attrs: &HashSet<u32>,
) {
    bridges.retain(|attr, _| selected_bridge_attrs.contains(attr));
}

fn merge_missing<T>(target: &mut HashMap<u32, T>, source: HashMap<u32, T>) {
    for (attr, record) in source {
        target.entry(attr).or_insert(record);
    }
}

type CandidateMap<T> = HashMap<u32, Vec<T>>;

trait Candidate: PartialEq {
    fn attr(&self) -> u32;
    fn offset(&self) -> usize;
}

impl Candidate for EdgeUse {
    fn attr(&self) -> u32 {
        self.attr
    }
    fn offset(&self) -> usize {
        self.offset
    }
}

impl Candidate for Coedge {
    fn attr(&self) -> u32 {
        self.attr
    }
    fn offset(&self) -> usize {
        self.offset
    }
}

impl Candidate for Loop {
    fn attr(&self) -> u32 {
        self.attr
    }
    fn offset(&self) -> usize {
        self.offset
    }
}

impl Candidate for VertexUse {
    fn attr(&self) -> u32 {
        self.attr
    }
    fn offset(&self) -> usize {
        self.offset
    }
}

impl Candidate for Point {
    fn attr(&self) -> u32 {
        self.attr
    }
    fn offset(&self) -> usize {
        self.offset
    }
}
#[derive(Clone, Copy)]
struct CoedgeEvidence<'a> {
    owner: Option<&'a Loop>,
    end: bool,
    edge: Option<&'a [EdgeUse]>,
    backward_owner: Option<bool>,
    forward: bool,
    loop_head: bool,
}

impl CoedgeEvidence<'_> {
    fn flags(self) -> [bool; 7] {
        [
            self.owner.is_some(),
            self.end,
            self.edge.is_some(),
            self.backward_owner == Some(true),
            self.backward_owner.is_some(),
            self.forward,
            self.loop_head,
        ]
    }
}

/// Keep the latest record occurrence for each attribute while retaining all
/// frame readings at that occurrence. A stream can contain overlapping payload
/// bytes, and a later complete record has the same override semantics as the
/// ordinary topology tables.
fn insert_candidates<T: Candidate>(target: &mut CandidateMap<T>, records: Vec<T>) {
    for record in records {
        match target.entry(record.attr()) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(vec![record]);
            }
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let current = entry.get();
                if current
                    .first()
                    .is_some_and(|prior| prior.offset() < record.offset())
                {
                    entry.insert(vec![record]);
                } else if current
                    .first()
                    .is_some_and(|prior| prior.offset() == record.offset())
                    && !current.contains(&record)
                {
                    entry.get_mut().push(record);
                }
            }
        }
    }
}

fn loop_is_owned(record: &Loop, bridges: &HashMap<u32, Bridge>) -> bool {
    record.refs[2] != 0 && bridges.contains_key(&record.refs[2])
}

/// Collect independent graph invariants for one coedge frame. The first four
/// fields identify the owner, vertex-use, edge-use, and ring; reciprocal links
/// and loop-head membership provide additional confirmation. No field is a
/// byte-position discriminator.
fn coedge_evidence<'a>(
    candidate: &Coedge,
    loops: &'a [Loop],
    bridges: &HashMap<u32, Bridge>,
    vertex_uses: &HashSet<u32>,
    edge_candidates: &'a CandidateMap<EdgeUse>,
    coedge_candidates: &CandidateMap<Coedge>,
) -> CoedgeEvidence<'a> {
    let owner = candidate.refs[1];
    let owner_evidence = loops
        .iter()
        .find(|loop_| loop_.attr == owner && owner != 0 && loop_is_owned(loop_, bridges));
    let end = candidate.refs[4];
    let end_evidence = end > 1 && vertex_uses.contains(&end);
    let edge = candidate.refs[6];
    let edge_evidence = edge_candidates
        .get(&edge)
        .filter(|_| edge != 0)
        .map(Vec::as_slice);
    let backward = candidate.refs[3];
    let backward_owner_valid = coedge_candidates
        .get(&backward)
        .filter(|_| backward != 0)
        .map(|candidates| {
            candidates
                .iter()
                .any(|backward_candidate| backward_candidate.refs[1] == owner)
        });
    let forward = candidate.refs[2];
    let forward_valid = forward <= 1
        || coedge_candidates.get(&forward).is_some_and(|candidates| {
            candidates.iter().any(|forward_candidate| {
                forward_candidate.refs[3] == candidate.attr && forward_candidate.refs[1] == owner
            })
        });
    let loop_head_valid = loops.iter().rev().any(|loop_| {
        loop_.attr == owner && loop_is_owned(loop_, bridges) && loop_.refs[1] == candidate.attr
    });
    CoedgeEvidence {
        owner: owner_evidence,
        end: end_evidence,
        edge: edge_evidence,
        backward_owner: backward_owner_valid,
        forward: forward_valid,
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
    bridges: &HashMap<u32, Bridge>,
    vertex_uses: &HashSet<u32>,
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
    coedges: &HashMap<u32, Coedge>,
    curve_attrs: Option<&HashSet<u32>>,
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
    coedges: &HashMap<u32, Coedge>,
    curve_attrs: Option<&HashSet<u32>>,
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
pub(crate) fn patch_point(buf: &mut [u8], attr: u32, xyz_m: [f64; 3]) -> bool {
    let mut tables = scan_records(buf, false, false, None, None, None, None);
    let Some(record) = tables.points.remove(&attr) else {
        return false;
    };
    patch_point_values(buf, record.xyz_offset, record.xyz_m, xyz_m)
}

/// Scan stream-local topology records.
pub(crate) fn scan(body: &[u8]) -> Tables {
    scan_stream(body, false)
}

/// Scan the stream-local topology records of a plain stream (spec §3.3).
pub(crate) fn scan_plain(body: &[u8]) -> Tables {
    scan_stream(body, true)
}

/// Scan stream-local topology records; `plain` reads a plain stream
/// (spec §3.3).
fn scan_stream(body: &[u8], plain: bool) -> Tables {
    let owner_facts = super::typed::scan(body);
    let owners = super::typed::topology_owner_attrs_for_streams([(&owner_facts, body, false)]);
    scan_records(
        body,
        false,
        plain,
        None,
        None,
        None,
        (!owners.is_empty()).then_some(&owners),
    )
}

/// Scan a partition stream while admitting typed FACE offsets that carry a
/// loop head.  A typed FACE and a compact bridge share the `00 0e` framing.
/// An ownership-only typed FACE has a null loop field and must not replace a
/// separate compact bridge for the same attribute; a shared FACE/bridge record
/// has a non-null loop field and is the topology record as well.
pub(super) fn scan_with_owner_attrs(
    body: &[u8],
    curve_attrs: &HashSet<u32>,
    excluded_bridge_offsets: &HashSet<usize>,
    owner_attrs: &HashSet<u32>,
) -> Tables {
    scan_records(
        body,
        false,
        false,
        Some(curve_attrs),
        Some(excluded_bridge_offsets),
        None,
        (!owner_attrs.is_empty()).then_some(owner_attrs),
    )
}

/// Scan a plain stream (spec §3.3) with the typed FACE/compact-bridge
/// overlap rule.
pub(super) fn scan_plain_with_owner_attrs(
    body: &[u8],
    curve_attrs: &HashSet<u32>,
    excluded_bridge_offsets: &HashSet<usize>,
    owner_attrs: &HashSet<u32>,
) -> Tables {
    scan_records(
        body,
        false,
        true,
        Some(curve_attrs),
        Some(excluded_bridge_offsets),
        None,
        (!owner_attrs.is_empty()).then_some(owner_attrs),
    )
}

/// Scan a deltas stream with the typed FACE/compact-bridge overlap rule.
pub(super) fn scan_deltas_with_owner_attrs(
    body: &[u8],
    curve_attrs: &HashSet<u32>,
    excluded_bridge_offsets: &HashSet<usize>,
    base: &Tables,
    owner_attrs: &HashSet<u32>,
) -> Tables {
    scan_records(
        body,
        true,
        false,
        Some(curve_attrs),
        Some(excluded_bridge_offsets),
        Some(base),
        (!owner_attrs.is_empty()).then_some(owner_attrs),
    )
}

/// Exact-read spans of the admitted topology records of one stream body.
///
/// Loops, coedges, vertex uses, and points name their retained frame bytes:
/// an admitted record implies an unambiguous frame, so the span is the record
/// offset plus the frame length. Ambiguous frames carry no record and no span.
/// Bridges and edge uses carry no frames; their spans re-derive the framing
/// the admission predicates matched, in match order. `plain` reads a plain
/// stream (spec §3.3).
pub(super) fn topology_spans(body: &[u8], plain: bool) -> Vec<super::read_ledger::RawSpan> {
    let tables = scan_stream(body, plain);
    let mut spans = Vec::new();
    for record in tables.bridges.values() {
        if let Some(end) = bridge_end(body, record.offset, plain) {
            push_span(&mut spans, record.offset, end, 0x0e, record.attr);
        }
    }
    for record in tables.edge_uses.values() {
        if let Some(end) = edge_use_end(body, record) {
            push_span(&mut spans, record.offset, end, 0x10, record.attr);
        }
    }
    framed_spans(&mut spans, &tables.loops, &tables.loop_frames, 0x0f);
    framed_spans(&mut spans, &tables.coedges, &tables.coedge_frames, 0x11);
    framed_spans(&mut spans, &tables.vertex_uses, &tables.vertex_frames, 0x12);
    framed_spans(&mut spans, &tables.points, &tables.point_frames, 0x1d);
    spans
}

fn push_span(
    spans: &mut Vec<super::read_ledger::RawSpan>,
    offset: usize,
    end: usize,
    tag: u8,
    record: u32,
) {
    if let Some(length) = end.checked_sub(offset) {
        if let Some(span) = super::read_ledger::RawSpan::at(offset, length, tag, record) {
            spans.push(span);
        }
    }
}

fn framed_spans<T: Candidate>(
    spans: &mut Vec<super::read_ledger::RawSpan>,
    records: &HashMap<u32, T>,
    frames: &HashMap<u32, RecordFrame>,
    tag: u8,
) {
    for record in records.values() {
        let length = frames
            .get(&record.attr())
            .and_then(|frame| frame.bytes.as_ref())
            .map_or(0, Vec::len);
        if let Some(span) =
            super::read_ledger::RawSpan::at(record.offset(), length, tag, record.attr())
        {
            spans.push(span);
        }
    }
}

/// The body-relative end of the bridge framing [`parse_bridge`] admits at
/// `off`, following its shapes in match order.
fn bridge_end(buf: &[u8], off: usize, plain: bool) -> Option<usize> {
    let record = parse_bridge(buf, off, plain)?;
    for p in super::record_starts(buf, off, 0x0e) {
        if let Some((candidate, end)) = native_bridge(buf, off, p, plain) {
            if candidate == record {
                return Some(end);
            }
        }
    }
    bridge_end_short(buf, off, plain)
}

fn bridge_end_short(buf: &[u8], off: usize, plain: bool) -> Option<usize> {
    let p = body_start(buf, off, 0x0e)?;
    if buf.get(p + 8) == Some(&1) && buf.get(p + 9..p + 17) == Some(MAGIC.as_slice()) {
        attr_at(buf, p)?;
        refs_tripled::<5>(buf, p + 17)?;
        parse_sense(*buf.get(p + 32)?)?;
        return p.checked_add(33);
    }
    if p + 37 > buf.len() || !frames_on_tolerance(buf, p + 8, plain) {
        return None;
    }
    attr_at(buf, p)?;
    let tripled = (0..5).all(|index| buf.get(p + 18 + index * 3) == Some(&1));
    if tripled {
        refs_tripled::<5>(buf, p + 16)?;
        parse_sense(*buf.get(p + 31)?)?;
        p.checked_add(32)
    } else {
        refs_be::<5>(buf, p + 16)?;
        parse_sense(*buf.get(p + 26)?)?;
        p.checked_add(27)
    }
}

/// The body-relative end of the edge-use framing the admitted `record`
/// matched: the first candidate equal to it, in parse order.
fn edge_use_end(buf: &[u8], record: &EdgeUse) -> Option<usize> {
    parse_edge_use_candidates(buf, record.offset)
        .into_iter()
        .find(|(candidate, _)| candidate == record)
        .map(|(_, end)| end)
}

#[cfg(test)]
fn scan_with_curve_attrs_excluding(
    body: &[u8],
    curves: &HashSet<u32>,
    excluded: &HashSet<usize>,
) -> Tables {
    let owner_facts = super::typed::scan(body);
    let owners = super::typed::topology_owner_attrs_for_streams([(&owner_facts, body, false)]);
    scan_with_owner_attrs(body, curves, excluded, &owners)
}

#[cfg(test)]
fn scan_deltas_with_curve_attrs_excluding(
    body: &[u8],
    curves: &HashSet<u32>,
    excluded: &HashSet<usize>,
    base: &Tables,
) -> Tables {
    let owner_facts = super::typed::scan_with_payload_exclusions(body, true);
    let mut owners = super::typed::topology_owner_attrs_for_streams([(&owner_facts, body, true)]);
    owners.extend(base.owner_classes.iter().copied());
    scan_deltas_with_owner_attrs(body, curves, excluded, base, &owners)
}

fn scan_records(
    body: &[u8],
    deltas: bool,
    plain: bool,
    curve_attrs: Option<&HashSet<u32>>,
    excluded_bridge_offsets: Option<&HashSet<usize>>,
    base: Option<&Tables>,
    owner_attrs: Option<&HashSet<u32>>,
) -> Tables {
    let mut t = Tables::default();
    t.owner_classes
        .extend(owner_attrs.into_iter().flatten().copied());
    let mut loop_candidates = Vec::new();
    let mut loop_frames = Vec::new();
    let mut edge_candidates = CandidateMap::new();
    let mut tolerant_edge_candidates = CandidateMap::new();
    let mut coedge_candidates = CandidateMap::new();
    let mut coedge_occurrences = Vec::new();
    let mut vertex_occurrences = Vec::new();
    let scopes = topology_scopes(body, deltas);
    let payload_ranges = &scopes.excluded_starts;
    let mut payload_index = 0;
    let mut i = 0usize;
    while i + 14 <= body.len() {
        while payload_ranges
            .get(payload_index)
            .is_some_and(|range| range.end <= i)
        {
            payload_index += 1;
        }
        if let Some(range) = payload_ranges
            .get(payload_index)
            .filter(|range| range.contains(&i))
        {
            i = range.end;
            continue;
        }
        if body[i] != 0x00 {
            i += 1;
            continue;
        }
        match body[i + 1] {
            0x0e => {
                if let Some(record) = parse_bridge(body, i, plain) {
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
            0x10 => {
                let candidates = parse_edge_use_candidates(body, i);
                if candidates.is_empty() {
                    insert_candidates(
                        &mut tolerant_edge_candidates,
                        parse_tolerant_edge_use_candidates(body, i),
                    );
                }
                insert_candidates(
                    &mut edge_candidates,
                    candidates.into_iter().map(|(record, _)| record).collect(),
                );
            }
            0x11 => {
                let frames = parse_coedge_candidates(body, i);
                for (record, _) in &frames {
                    coedge_candidates
                        .entry(record.attr)
                        .or_default()
                        .push(record.clone());
                }
                if !frames.is_empty() {
                    coedge_occurrences.push(frames);
                }
            }
            0x12 => {
                vertex_occurrences.push((
                    parse_vertex_candidates(body, i),
                    unsupported_vertex_prefix(body, i),
                ));
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

    for (readings, unsupported) in vertex_occurrences {
        let point_known = |record: &VertexUse| {
            t.point_frames.contains_key(&record.refs[4])
                || base.is_some_and(|base| base.point_frames.contains_key(&record.refs[4]))
        };
        let current_reference = |record: &VertexUse| {
            coedge_candidates.values().flatten().any(|fin| {
                fin.refs[4] == record.attr
                    && (loop_frames.iter().any(|(loop_, _)| {
                        loop_.attr == fin.refs[1]
                            && (t.bridges.contains_key(&loop_.refs[2])
                                || base
                                    .is_some_and(|base| base.bridges.contains_key(&loop_.refs[2])))
                    }) || base.is_some_and(|base| base.loops.contains_key(&fin.refs[1])))
            }) || base
                .is_some_and(|base| base.coedges.values().any(|fin| fin.refs[4] == record.attr))
        };
        let class_closed = |record: &VertexUse| {
            record.refs[4] > 1
                && record.owner > 1
                && record.owner != record.attr
                && owner_attrs.is_some_and(|owners| owners.contains(&record.owner))
                && (point_known(record) || current_reference(record))
        };
        let admitted: Vec<_> = readings
            .iter()
            .filter(|reading| class_closed(&reading.record))
            .collect();
        if let [reading] = admitted.as_slice() {
            let bytes = body[reading.record.offset..reading.end].to_vec();
            let supported = reading
                .record
                .tolerance_m
                .is_none_or(|value| value > 0.0 && (value * super::LEN_TO_MM).is_finite());
            t.merge_vertex_frame(
                reading.record.attr,
                RecordFrame {
                    partition: !deltas,
                    bytes: Some(bytes),
                },
                supported.then(|| reading.record.clone()),
            );
        } else if let Some(reading) = admitted.first() {
            t.merge_vertex_frame(
                reading.record.attr,
                RecordFrame {
                    partition: !deltas,
                    bytes: None,
                },
                None,
            );
        } else {
            let identified: Vec<_> = readings
                .iter()
                .filter(|reading| {
                    reading.record.refs[4] > 1
                        && point_known(&reading.record)
                        && current_reference(&reading.record)
                })
                .collect();
            if let [reading] = identified.as_slice() {
                t.merge_vertex_frame(
                    reading.record.attr,
                    RecordFrame {
                        partition: !deltas,
                        bytes: Some(body[reading.record.offset..reading.end].to_vec()),
                    },
                    None,
                );
            } else if let Some(reading) = identified.first() {
                t.merge_vertex_frame(
                    reading.record.attr,
                    RecordFrame {
                        partition: !deltas,
                        bytes: None,
                    },
                    None,
                );
            } else if let Some(record) =
                unsupported.filter(|record| point_known(record) && current_reference(record))
            {
                t.merge_vertex_frame(
                    record.attr,
                    RecordFrame {
                        partition: !deltas,
                        bytes: None,
                    },
                    None,
                );
            }
        }
    }

    let (evidence_loops, evidence_bridges, evidence_vertices, evidence_edges, evidence_coedges) =
        if deltas {
            let mut loops = Vec::new();
            let mut bridges = HashMap::new();
            let mut vertices = HashSet::new();
            let mut edges = CandidateMap::new();
            let mut coedges = CandidateMap::new();
            if let Some(base) = base {
                loops.extend(
                    base.loops
                        .iter()
                        .filter(|&(attr, _record)| {
                            base.loop_frames
                                .get(attr)
                                .is_some_and(|frame| frame.partition)
                        })
                        .map(|(_attr, record)| record.clone()),
                );
                let owners: HashSet<_> = loops.iter().map(|record| record.refs[2]).collect();
                bridges.extend(
                    base.bridges
                        .iter()
                        .filter(|&(attr, _record)| owners.contains(attr))
                        .map(|(attr, record)| (*attr, record.clone())),
                );
                coedges.extend(
                    base.coedges
                        .iter()
                        .filter(|&(attr, _record)| {
                            base.coedge_frames
                                .get(attr)
                                .is_some_and(|frame| frame.partition)
                        })
                        .map(|(attr, record)| (*attr, vec![record.clone()])),
                );
                let starts: HashSet<_> = coedges
                    .values()
                    .flatten()
                    .map(|record| record.refs[4])
                    .collect();
                let carriers: HashSet<_> = coedges
                    .values()
                    .flatten()
                    .map(|record| record.refs[6])
                    .collect();
                vertices.extend(base.vertex_classes.iter().filter_map(|(attr, partition)| {
                    (*partition && starts.contains(attr)).then_some(*attr)
                }));
                edges.extend(
                    base.edge_uses
                        .iter()
                        .filter(|&(attr, _record)| carriers.contains(attr))
                        .map(|(attr, record)| (*attr, vec![record.clone()])),
                );
            }
            (loops, bridges, vertices, edges, coedges)
        } else {
            (
                loop_candidates,
                t.bridges.clone(),
                t.vertex_classes.keys().copied().collect(),
                edge_candidates.clone(),
                coedge_candidates,
            )
        };
    for frames in coedge_occurrences {
        let candidates: Vec<_> = frames.iter().map(|(record, _)| record.clone()).collect();
        let record = select_coedge(
            &candidates,
            &evidence_loops,
            &evidence_bridges,
            &evidence_vertices,
            &evidence_edges,
            &evidence_coedges,
        );
        let attr = record
            .as_ref()
            .map_or(candidates[0].attr, |record| record.attr);
        let bytes = record.as_ref().and_then(|selected| {
            frames
                .iter()
                .find(|(record, _)| record == selected)
                .map(|(_, end)| body[selected.offset..*end].to_vec())
        });
        let owners = if let Some(record) = &record {
            HashSet::from([record.refs[1]])
        } else {
            candidates.iter().map(|record| record.refs[1]).collect()
        };
        t.merge_coedge_frame(
            attr,
            RecordFrame {
                partition: !deltas,
                bytes,
            },
            record,
            owners,
        );
    }
    for candidates in edge_candidates.into_values() {
        if let Some(record) = select_edge_use(&candidates, &t.coedges, curve_attrs) {
            t.edge_uses.insert(record.attr, record);
        }
    }
    admit_tolerant_edge_uses(&mut t, tolerant_edge_candidates);
    for (record, end) in loop_frames {
        let owner = record.refs[2];
        let first = record.refs[1];
        let owner_exists = t.bridges.contains_key(&owner)
            || base.is_some_and(|base| base.bridges.contains_key(&owner));
        let partition_head = !deltas
            && t.bridges
                .get(&owner)
                .is_some_and(|bridge| bridge.refs[2] == record.attr);
        let first_owners = base
            .and_then(|base| base.coedge_owners.get(&first))
            .or_else(|| t.coedge_owners.get(&first));
        let first_is_owned = first_owners.map_or_else(
            || {
                base.and_then(|base| base.coedges.get(&first))
                    .or_else(|| t.coedges.get(&first))
                    .is_some_and(|coedge| coedge.refs[1] == record.attr)
            },
            |owners| owners.contains(&record.attr),
        );
        if owner_exists && (first_is_owned || partition_head) {
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
mod scope_tests;

#[cfg(test)]
mod tests {
    mod wide_scopes;

    mod carrier_scopes;

    use super::{
        parse_bridge, parse_point, parse_vertex_candidates, patch_point, patch_point_values, scan,
        scan_deltas_with_curve_attrs_excluding, scan_with_curve_attrs_excluding, EdgeReferences,
        MAGIC,
    };
    use crate::test_support::parasolid::framed_topology_fin;
    use crate::test_support::parasolid::framed_topology_loop;
    use cadmpeg_ir::topology::Sense;
    use std::collections::HashMap;
    use std::collections::HashSet;

    fn merge_bridge(attr: u32, attributes_head: u32) -> super::Bridge {
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
    fn vertex_use_requires_all_five_tripled_cells_and_its_owner() {
        let bytes = crate::test_support::parasolid::framed_topology_vertex(
            50,
            7,
            [0, 0, 0, 0, 60],
            None,
            3,
            true,
            false,
        );
        let readings = parse_vertex_candidates(&bytes, 0);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].record.refs, [0, 0, 0, 0, 60]);
        assert_eq!(readings[0].record.sequence, 7);
        assert_eq!(readings[0].record.owner, 3);
        assert_eq!(readings[0].end, bytes.len());
        for cell in (0..5)
            .map(|index| 2 + super::vertex_delta::REFS + 3 * index + 2)
            .chain([2 + super::vertex_delta::OWNER + 2])
        {
            let mut malformed = bytes.clone();
            malformed[cell] = 0;
            assert!(parse_vertex_candidates(&malformed, 0).is_empty());
        }
    }

    #[test]
    fn extra_tripled_vertex_prefix_does_not_establish_a_complete_frame() {
        let mut bytes = vec![0, 0x12];
        bytes.extend(50_u16.to_be_bytes());
        bytes.extend(7_u32.to_be_bytes());
        for reference in [0_u16, 0, 0, 0, 60, 90] {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.extend(MAGIC);
        assert!(parse_vertex_candidates(&bytes, 0).is_empty());
        let vertex = super::unsupported_vertex_prefix(&bytes, 0).unwrap();
        assert_eq!(vertex.refs, [0, 0, 0, 0, 60]);
        assert_eq!(vertex.sequence, 7);
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

        let bridge = parse_bridge(&bytes, 0, false).expect("deltas-form bridge");
        assert_eq!(bridge.attr, 0x1234);
        assert_eq!(bridge.sequence, 7);
        assert_eq!(bridge.attributes_head, Some(0x4321));
        assert_eq!(
            bridge.refs.as_slice(),
            expected.iter().copied().map(u32::from).collect::<Vec<_>>()
        );
        assert_eq!(bridge.sense, Sense::Forward);
    }

    #[test]
    fn bridge_refs_accept_adjacent_and_tripled_cells() {
        let expected = vec![0x101, 0x202, 0x303, 0x404, 0x505];
        for tripled in [false, true] {
            let bytes = bridge_with_refs(&expected, tripled);
            let bridge = parse_bridge(&bytes, 0, false)
                .unwrap_or_else(|| panic!("bridge tripled={tripled} bytes={bytes:02x?}"));
            assert_eq!(bridge.attr, 0x1234);
            assert_eq!(bridge.sequence, 7);
            assert_eq!(bridge.attributes_head, Some(0x4321));
            assert_eq!(
                bridge.refs.as_slice(),
                expected.iter().copied().map(u32::from).collect::<Vec<_>>()
            );
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
            [0_u16, 0, 0, 0, 0, 0, 900]
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
        bytes.extend(900_u16.to_be_bytes());
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
        body.extend(crate::test_support::parasolid::fixture_vertex_owner_class(
            900,
        ));
        body.extend(crate::test_support::parasolid::world_point(60, [0.0; 3]));

        let tables = scan(&body);
        let coedge = tables.coedges.get(&30).expect("tripled coedge");
        assert_eq!(coedge.refs[1], 20);
        assert_eq!(coedge.refs[4], 50);
        assert_eq!(coedge.refs[6], 0x2b40);
        assert!(tables.loops.contains_key(&20));
    }

    /// A bare edge use whose tolerance slot holds `tolerance` and whose
    /// support-curve slot holds the null `1`: a tolerant edge (spec §7.6).
    fn tolerant_edge_use(attr: u16, canonical: u16, tolerance: f64) -> Vec<u8> {
        let mut bytes = vec![0, 0x10];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        bytes.extend(0_u16.to_be_bytes());
        bytes.extend(tolerance.to_be_bytes());
        bytes.extend(
            [canonical, 0, 0, 1, 0, 0, 900]
                .into_iter()
                .flat_map(u16::to_be_bytes),
        );
        bytes
    }

    /// One face whose fin 30 names edge `edge`, and the edge use `edge_use`.
    fn tolerant_body(edge: u16, edge_use: &[u8]) -> Vec<u8> {
        let mut body = Vec::new();
        body.extend(topology_bridge(10, 20));
        body.extend(topology_loop(20, 30, 10));
        body.extend(topology_tripled_coedge(
            30,
            [0, 20, 0, 30, 50, 0, edge, 0, 0],
        ));
        body.extend(edge_use);
        body.extend(topology_vertex_use(50, 0));
        body.extend(crate::test_support::parasolid::fixture_vertex_owner_class(
            900,
        ));
        body.extend(crate::test_support::parasolid::world_point(60, [0.0; 3]));
        body
    }

    #[test]
    fn a_tolerant_edge_use_is_admitted_only_when_a_selected_fin_names_it() {
        let tables = scan(&tolerant_body(40, &tolerant_edge_use(40, 30, 5.0e-7)));
        let edge = tables.edge_uses.get(&40).expect("the named tolerant edge");
        assert_eq!(edge.tolerance_m, Some(5.0e-7));
        assert_eq!(edge.references.curve(), 1);
        assert_eq!(edge.references.canonical(), Some(30));
        // No fin names it.
        let tables = scan(&tolerant_body(41, &tolerant_edge_use(40, 30, 5.0e-7)));
        assert!(!tables.edge_uses.contains_key(&40));
        // Its canonical slot names a fin of no edge of it.
        let tables = scan(&tolerant_body(40, &tolerant_edge_use(40, 31, 5.0e-7)));
        assert!(!tables.edge_uses.contains_key(&40));
        // A tolerance that is not positive frames no tolerant edge.
        let tables = scan(&tolerant_body(40, &tolerant_edge_use(40, 30, 0.0)));
        assert!(!tables.edge_uses.contains_key(&40));
        // An ordinary edge use stores the null-real sentinel and no tolerance.
        let tables = scan(&tolerant_body(40, &topology_edge_use(40, 0)));
        assert_eq!(
            tables.edge_uses.get(&40).map(|edge| edge.tolerance_m),
            Some(None)
        );
    }

    #[test]
    fn edge_and_vertex_use_sequences_are_retained() {
        let mut body = Vec::new();
        body.extend(topology_edge_use(40, 0x0102_0304));
        body.extend(topology_vertex_use(50, 0x0506_0708));
        body.extend(crate::test_support::parasolid::fixture_vertex_owner_class(
            900,
        ));
        body.extend(crate::test_support::parasolid::world_point(60, [0.0; 3]));

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
        extended[owner..owner + 2].copy_from_slice(&0x7fffu16.to_be_bytes());
        assert!(parse_point(&extended, 0, true).is_some());
        extended[owner..owner + 2].copy_from_slice(&0x8000u16.to_be_bytes());
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

    fn delta_topology(bytes: &[u8], base: &super::Tables) -> super::Tables {
        scan_deltas_with_curve_attrs_excluding(bytes, &HashSet::new(), &HashSet::new(), base)
    }

    fn partition_without_middle_fin() -> Vec<u8> {
        let mut bytes = crate::test_support::parasolid::triangle_body();
        let original = crate::test_support::parasolid::coedge(31, 20, 32, 51, 0, 41, false);
        let at = bytes
            .windows(original.len())
            .position(|window| window == original)
            .unwrap();
        drop(bytes.drain(at..at + original.len()));
        bytes
    }

    #[test]
    fn partition_face_head_retains_its_loop_before_first_fin_is_available() {
        let loop_frame = framed_topology_loop(20, 811, [0, 30, 10, 0], false, true);
        let body = [topology_bridge(10, 20), loop_frame.clone()].concat();
        let tables = scan(&body);
        assert_eq!(tables.loops()[&20].refs, [0, 30, 10, 0]);
        assert!(tables.loop_frames[&20].partition);
        assert_eq!(
            tables.loop_frames[&20].bytes.as_deref(),
            Some(loop_frame.as_slice())
        );
        let history = delta_topology(&body, &super::Tables::default());
        assert!(!history.loops().contains_key(&20));
        let non_head = [topology_bridge(10, 21), loop_frame].concat();
        assert!(!scan(&non_head).loops().contains_key(&20));
    }

    #[test]
    fn fin_frame_selection_does_not_promote_history_to_current_evidence() {
        let fin = framed_topology_fin(31, [0, 20, 30, 32, 51, 0, 0x2b40, 0, 0], true, false, false);
        let support = [
            framed_topology_loop(20, 811, [0, 31, 10, 0], true, false),
            topology_edge_use(0x2b40, 0),
            topology_vertex_use(51, 0),
        ]
        .concat();
        for streams in [
            vec![[fin.as_slice(), support.as_slice()].concat()],
            vec![fin.clone(), support.clone()],
            vec![support.clone(), fin.clone()],
        ] {
            let mut tables = scan(&topology_bridge(10, 20));
            for stream in streams {
                let delta = delta_topology(&stream, &tables);
                tables.merge_deltas(delta, None);
            }
            assert!(!tables.coedges().contains_key(&31));
            assert!(tables.coedge_is_ambiguous(31));
        }
    }

    #[test]
    fn fin_frame_evidence_accepts_both_null_previous_sentinels() {
        let mut candidate = super::Coedge {
            attr: 31,
            refs: [0; 9],
            sense: Sense::Forward,
            offset: 0,
        };
        let bridges = HashMap::new();
        let vertices = HashSet::new();
        let edges = HashMap::new();
        let coedges = HashMap::new();
        for null in [0, 1] {
            candidate.refs[2] = null;
            let evidence =
                super::coedge_evidence(&candidate, &[], &bridges, &vertices, &edges, &coedges);
            assert!(evidence.forward);
        }
    }

    #[test]
    fn fin_conflicts_compare_every_pointer_sense_and_frame_across_stream_groupings() {
        let refs = [0, 20, 0, 32, 51, 0, 41, 0, 0];
        let first = framed_topology_fin(31, refs, true, false, false);
        let mut variants = vec![
            framed_topology_fin(31, refs, true, true, false),
            framed_topology_fin(31, refs, false, false, false),
            framed_topology_fin(31, refs, true, false, true),
        ];
        for slot in 0..refs.len() {
            let mut changed = refs;
            changed[slot] += 1;
            variants.push(framed_topology_fin(31, changed, true, false, false));
        }
        for variant in variants {
            let records = [&first, &variant, &first];
            for order in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                for cuts in [0u8, 1, 2, 3] {
                    let mut tables = scan(&partition_without_middle_fin());
                    let mut stream = Vec::new();
                    for (index, record) in order.into_iter().enumerate() {
                        stream.extend(records[record]);
                        if index == 2 || cuts & (1 << index) != 0 {
                            let deltas = delta_topology(&stream, &tables);
                            tables.merge_deltas(deltas, None);
                            stream.clear();
                        }
                    }
                    assert!(!tables.coedges().contains_key(&31));
                    assert!(tables.coedge_is_ambiguous(31));
                }
            }
        }
    }

    #[test]
    fn identical_fin_frames_coalesce_with_partition_context() {
        let bytes = framed_topology_fin(31, [0, 20, 0, 32, 51, 0, 41, 0, 0], true, true, false);
        let mut tables = scan(&partition_without_middle_fin());
        for stream in [&bytes, &[bytes.as_slice(), bytes.as_slice()].concat()] {
            let deltas = delta_topology(stream, &tables);
            tables.merge_deltas(deltas, None);
        }
        assert_eq!(tables.coedges()[&31].refs, [0, 20, 0, 32, 51, 0, 41, 0, 0]);
        assert!(!tables.coedge_is_ambiguous(31));
    }

    #[test]
    fn partition_fin_presence_and_conflicts_block_history() {
        let partition = crate::test_support::parasolid::triangle_body();
        let extra = framed_topology_fin(31, [1, 20, 0, 32, 51, 0, 41, 0, 0], false, false, false);
        let mut current = scan(&partition);
        let before = current.coedges()[&31].clone();
        let history = delta_topology(&extra, &current);
        current.merge_deltas(history, None);
        assert_eq!(current.coedges()[&31], before);
        for bytes in [
            [partition.as_slice(), extra.as_slice()].concat(),
            [extra.as_slice(), partition.as_slice()].concat(),
        ] {
            let mut tables = scan(&bytes);
            assert!(tables.coedge_is_ambiguous(31));
            let history = delta_topology(
                &framed_topology_fin(31, [0, 20, 0, 32, 51, 0, 41, 0, 0], true, false, false),
                &tables,
            );
            tables.merge_deltas(history, None);
            assert!(!tables.coedges().contains_key(&31));
            assert!(tables.coedge_is_ambiguous(31));
        }
    }

    #[test]
    fn bare_loop_closure_survives_an_unowned_tripled_reading_at_the_same_offset() {
        for first_of_type in [false, true] {
            let frame = framed_topology_loop(20, 811, [0, 258, 769, 0], false, first_of_type);
            let mut padded = frame.clone();
            padded.extend([1, 0, 0, 1]);
            let readings = super::parse_loop_candidates(&padded, 0)
                .into_iter()
                .filter(|(record, _)| record.attr == 20)
                .collect::<Vec<_>>();
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
                            let deltas = delta_topology(&stream, &tables);
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
            let deltas = delta_topology(stream, &tables);
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
        let delta = delta_topology(
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
        let deltas = delta_topology(&bytes, &tables);
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
                records[0].0.references,
                EdgeReferences::Bare([171, 172, 173, 174, 175, 176])
            );
            assert_eq!(records[0].0.references.canonical(), Some(171));
            assert_eq!(records[0].0.references.curve(), 174);
            assert_eq!(records[0].0.sequence, 947);
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
    fn vertex_frame(attr: u16, refs: [u16; 5], owner: u16, delta: bool) -> Vec<u8> {
        crate::test_support::parasolid::framed_topology_vertex(
            attr, 149, refs, None, owner, delta, false,
        )
    }

    fn vertex_partition() -> super::Tables {
        scan(&crate::test_support::parasolid::triangle_body())
    }

    #[test]
    fn vertex_conflicts_compare_every_complete_field_across_stream_groupings() {
        let refs = [0, 0, 0, 0, 60];
        let first = vertex_frame(211, refs, 900, true);
        let mut variants = Vec::new();
        for (node, tolerance, owner, ff) in [
            (150, None, 900, false),
            (149, Some(0.0001), 900, false),
            (149, None, 901, false),
            (149, None, 900, true),
        ] {
            variants.push(crate::test_support::parasolid::framed_topology_vertex(
                211, node, refs, tolerance, owner, true, ff,
            ));
        }
        for slot in 0..refs.len() {
            let mut changed = refs;
            changed[slot] += 1;
            variants.push(vertex_frame(211, changed, 900, true));
        }
        for variant in variants {
            let frames = [&first, &variant, &first];
            for order in [
                [0, 1, 2],
                [0, 2, 1],
                [1, 0, 2],
                [1, 2, 0],
                [2, 0, 1],
                [2, 1, 0],
            ] {
                for group_mask in 0..4 {
                    let mut tables = vertex_partition();
                    let mut stream = Vec::new();
                    for (index, choice) in order.into_iter().enumerate() {
                        stream.extend(frames[choice]);
                        if index == 2 || group_mask & (1 << index) != 0 {
                            let delta = delta_topology(&stream, &tables);
                            tables.merge_deltas(delta, None);
                            stream.clear();
                        }
                    }
                    assert!(tables.vertex_is_unresolved(211));
                    assert!(tables.vertex_frames[&211].bytes.is_none());
                }
            }
        }
    }

    #[test]
    fn partition_vertex_presence_and_partition_conflicts_block_history() {
        let extra = vertex_frame(50, [0, 0, 0, 0, 60], 900, true);
        let mut tables = vertex_partition();
        let current = tables.vertex_uses[&50].clone();
        let history = delta_topology(&extra, &tables);
        tables.merge_deltas(history, None);
        assert_eq!(tables.vertex_uses[&50], current);
        for ff in [false, true] {
            let conflicting = crate::test_support::parasolid::framed_topology_vertex(
                50,
                1,
                [0, 0, 0, 0, 60],
                None,
                900,
                false,
                ff,
            );
            let bytes = [crate::test_support::parasolid::triangle_body(), conflicting].concat();
            let mut tables = scan(&bytes);
            assert!(tables.vertex_is_unresolved(50));
            let history = delta_topology(&extra, &tables);
            tables.merge_deltas(history, None);
            assert!(tables.vertex_is_unresolved(50));
            assert!(tables.vertex_frames[&50].partition);
        }
    }

    #[test]
    fn vertex_owner_inventory_admits_both_union_classes_and_rejects_other_ids() {
        for owner in [900, 901] {
            let bytes = [
                crate::test_support::parasolid::world_point(60, [0.0; 3]),
                vertex_frame(211, [0, 0, 0, 0, 60], owner, false),
            ]
            .concat();
            let tables = super::scan_with_owner_attrs(
                &bytes,
                &HashSet::new(),
                &HashSet::new(),
                &HashSet::from([900, 901]),
            );
            assert_eq!(tables.vertex_uses[&211].owner, u32::from(owner));
            let rejected = super::scan_with_owner_attrs(
                &bytes,
                &HashSet::new(),
                &HashSet::new(),
                &HashSet::from([902]),
            );
            assert!(!rejected.vertex_frames.contains_key(&211));
        }
    }

    #[test]
    fn an_intersection_composite_is_skipped_whole() {
        let mut bytes = vec![0x00, 0x26];
        bytes.extend(300_u16.to_be_bytes());
        bytes.extend(7_u32.to_be_bytes());
        for reference in [1_u16, 3, 301, 302, 303] {
            bytes.extend(reference.to_be_bytes());
        }
        bytes.push(0x2b);
        for reference in [40_u16, 41, 42, 43, 44, 45] {
            bytes.extend(reference.to_be_bytes());
        }
        assert_eq!(
            super::intersection_composite_end(&bytes, 2, 0x26),
            Some(2 + 29)
        );
        assert_eq!(super::intersection_composite_end(&bytes, 2, 0x12), None);
        let mut unmarked = bytes.clone();
        unmarked[2 + 16] = 0x00;
        assert_eq!(super::intersection_composite_end(&unmarked, 2, 0x26), None);
        let scopes = super::topology_scopes(&bytes, false);
        assert_eq!(scopes.excluded_starts, vec![1..31]);
    }

    #[test]
    fn an_intersection_composite_after_its_class_header_is_skipped_whole() {
        // The first composite of a stream follows its class's edited type
        // header (spec §4.1) with no tag of its own. Its sequence number
        // `00 00 12 1a` holds a false `00 12` vertex header that would run
        // over the tag of the point record that follows.
        let mut bytes = vec![0x00, 0x26, 12];
        bytes.extend([b'C'; 11]);
        bytes.push(b'A');
        bytes.push(17);
        bytes.extend(b"intersection_data");
        bytes.extend([0x00, 0xcc, 0x00, 0x01]);
        bytes.push(b'Z');
        let record = bytes.len();
        bytes.extend(9_u16.to_be_bytes());
        bytes.extend(0x121a_u32.to_be_bytes());
        for reference in [1_u16, 3, 56, 1, 53] {
            bytes.extend(reference.to_be_bytes());
        }
        bytes.push(0x2d);
        for reference in [50_u16, 8, 54, 55, 58, 61] {
            bytes.extend(reference.to_be_bytes());
        }
        let point = bytes.len();
        assert_eq!(point, record + 29);
        bytes.extend(crate::test_support::parasolid::world_point(
            10,
            [0.001, 0.002, 0.003],
        ));
        let scopes = super::topology_scopes(&bytes, false);
        assert_eq!(scopes.excluded_starts.first(), Some(&(1..point)));
        assert!(super::scan(&bytes).points().contains_key(&10));
    }

    #[test]
    fn complete_vertex_tolerances_reject_nonfinite_and_withhold_nonpositive_geometry() {
        for (tolerance, admitted, projected) in [
            (None, true, true),
            (Some(0.0001), true, true),
            (Some(0.0), true, false),
            (Some(-1.0), true, false),
            (Some(f64::MAX), true, false),
            (Some(f64::NAN), false, false),
            (Some(f64::INFINITY), false, false),
        ] {
            for tripled in [false, true] {
                let frame = crate::test_support::parasolid::framed_topology_vertex(
                    211,
                    149,
                    [0, 0, 0, 0, 60],
                    tolerance,
                    900,
                    tripled,
                    true,
                );
                let bytes = [
                    crate::test_support::parasolid::world_point(60, [0.0; 3]),
                    frame,
                ]
                .concat();
                let tables = super::scan_with_owner_attrs(
                    &bytes,
                    &HashSet::new(),
                    &HashSet::new(),
                    &HashSet::from([900]),
                );
                assert_eq!(tables.vertex_frames.contains_key(&211), admitted);
                assert_eq!(tables.vertex_uses.contains_key(&211), projected);
                if projected {
                    assert_eq!(tables.vertex_uses[&211].tolerance_m, tolerance);
                }
            }
        }
    }

    #[test]
    fn int_values_payload_vertex_bytes_never_enter_identity_merge() {
        let real = vertex_frame(211, [0, 0, 0, 0, 60], 900, false);
        let mut fake = real.clone();
        fake[7] ^= 1;
        while !fake.len().is_multiple_of(4) {
            fake.push(0);
        }
        let mut integers = vec![0, 0x52];
        integers.extend(u32::try_from(fake.len() / 4).unwrap().to_be_bytes());
        integers.extend(710u16.to_be_bytes());
        integers.extend(fake);
        for records in [
            [real.clone(), integers.clone()],
            [integers.clone(), real.clone()],
        ] {
            let bytes = [
                crate::test_support::parasolid::world_point(60, [0.0; 3]),
                records.concat(),
            ]
            .concat();
            let tables = super::scan_with_owner_attrs(
                &bytes,
                &HashSet::new(),
                &HashSet::new(),
                &HashSet::from([900]),
            );
            assert_eq!(tables.vertex_uses[&211].sequence, 149);
            assert_eq!(
                tables.vertex_frames[&211].bytes.as_deref(),
                Some(real.as_slice())
            );
        }
    }

    #[test]
    fn attribute_payload_vertex_bytes_do_not_poison_a_real_identity() {
        let fake = crate::test_support::parasolid::framed_topology_vertex(
            211,
            150,
            [0, 0, 0, 0, 60],
            Some(0.0),
            900,
            false,
            false,
        );
        let slots: Vec<_> = fake
            .chunks_exact(2)
            .map(|bytes| u16::from_be_bytes([bytes[0], bytes[1]]))
            .collect();
        let enclosing = crate::test_support::parasolid::entity51(9, 710, 711, &slots);
        let real = vertex_frame(211, [0, 0, 0, 0, 60], 900, false);
        for records in [
            [real.clone(), enclosing.clone()],
            [enclosing.clone(), real.clone()],
        ] {
            let bytes = [
                crate::test_support::parasolid::world_point(60, [0.0; 3]),
                records.concat(),
            ]
            .concat();
            let tables = super::scan_with_owner_attrs(
                &bytes,
                &HashSet::new(),
                &HashSet::new(),
                &HashSet::from([900]),
            );
            assert_eq!(tables.vertex_uses[&211].sequence, 149);
            assert_eq!(
                tables.vertex_frames[&211].bytes.as_deref(),
                Some(real.as_slice())
            );
        }
    }

    #[test]
    fn complete_vertex_frame_requires_owner_and_tripled_owner_flag() {
        for tripled in [false, true] {
            for first in [false, true] {
                let frame = crate::test_support::parasolid::framed_topology_vertex(
                    211,
                    149,
                    [0, 0, 0, 0, 60],
                    None,
                    900,
                    tripled,
                    first,
                );
                let readings = parse_vertex_candidates(&frame, 0)
                    .into_iter()
                    .filter(|r| r.record.owner == 900)
                    .collect::<Vec<_>>();
                assert_eq!(readings.len(), 1);
                assert_eq!(readings[0].end, frame.len());
                assert_eq!(readings[0].record.owner, 900);
                for missing in 1..=if tripled { 3 } else { 2 } {
                    assert!(parse_vertex_candidates(&frame[..frame.len() - missing], 0)
                        .iter()
                        .all(|r| r.record.owner != 900));
                }
                if tripled {
                    let mut invalid = frame.clone();
                    *invalid.last_mut().unwrap() = 0;
                    assert!(parse_vertex_candidates(&invalid, 0)
                        .iter()
                        .all(|r| r.record.owner != 900));
                }
            }
        }
    }

    #[test]
    fn identical_complete_vertex_frames_coalesce_and_later_partition_wins() {
        let history = vertex_frame(211, [0, 0, 0, 0, 60], 900, true);
        let mut tables = vertex_partition();
        for _ in 0..3 {
            let delta = delta_topology(&history, &tables);
            tables.merge_deltas(delta, None);
        }
        assert_eq!(tables.vertex_uses[&211].sequence, 149);
        let current = vertex_frame(211, [0, 0, 0, 0, 60], 900, false);
        let bytes = [
            crate::test_support::parasolid::world_point(60, [0.0; 3]),
            current.clone(),
            crate::test_support::parasolid::fixture_vertex_owner_class(900),
        ]
        .concat();
        tables.merge_deltas(scan(&bytes), None);
        assert!(tables.vertex_frames[&211].partition);
        assert_eq!(
            tables.vertex_frames[&211].bytes.as_deref(),
            Some(current.as_slice())
        );
    }
    #[test]
    fn unsupported_vertex_prefix_blocks_history_without_guessing_its_owner() {
        let mut bytes = crate::test_support::parasolid::triangle_body();
        let original = crate::test_support::parasolid::framed_topology_vertex(
            51,
            0,
            [0, 0, 0, 0, 61],
            None,
            900,
            false,
            false,
        );
        let at = bytes
            .windows(original.len())
            .position(|window| window == original)
            .unwrap();
        drop(bytes.drain(at..at + original.len()));
        let mut unsupported = vertex_frame(51, [0, 0, 0, 0, 61], 900, true);
        drop(unsupported.splice(23..23, [0, 0, 1]));
        assert!(parse_vertex_candidates(&unsupported, 0).is_empty());
        bytes.extend(unsupported);
        let mut tables = scan(&bytes);
        assert!(tables.vertex_is_unresolved(51));
        assert!(tables.vertex_frames[&51].partition);
        assert!(tables.vertex_frames[&51].bytes.is_none());
        let history = delta_topology(&vertex_frame(51, [0, 0, 0, 0, 61], 900, true), &tables);
        tables.merge_deltas(history, None);
        assert!(tables.vertex_is_unresolved(51));
    }

    #[test]
    fn int_values_extent_ends_where_the_next_node_begins() {
        // `00 52` read inside another record's fields: the stated count fits
        // the buffer, but the extent ends mid-record, so it frames nothing.
        let mut bytes = vec![0, 0x52];
        bytes.extend(4u32.to_be_bytes());
        bytes.extend(95u16.to_be_bytes());
        bytes.extend([0; 16]);
        let end = bytes.len();
        let mut inside = bytes.clone();
        inside.extend([0xb3, 0xc2, 0xbc, 0x92]);
        assert!(super::topology_payload_ranges(&inside, false).is_empty());
        // The same node followed by the next node's tag is a payload.
        bytes.extend([0, 0x51]);
        assert_eq!(
            super::topology_payload_ranges(&bytes, false),
            std::iter::once(0..end).collect::<Vec<_>>()
        );
    }

    #[test]
    fn int_values_count_is_a_bounded_u32_not_a_short_pointer() {
        let count = 0x8000u32;
        let mut bytes = vec![0, 0x52];
        bytes.extend(count.to_be_bytes());
        bytes.extend(710u16.to_be_bytes());
        bytes.resize(8 + usize::try_from(count).unwrap() * 4, 0);
        let fake = vertex_frame(211, [0, 0, 0, 0, 60], 900, false);
        bytes[8..8 + fake.len()].copy_from_slice(&fake);
        assert_eq!(
            super::topology_payload_ranges(&bytes, false),
            std::iter::once(0..bytes.len()).collect::<Vec<_>>()
        );
        assert!(scan(&bytes).vertex_frames.is_empty());
    }

    /// A pointer to node 82, the null pointer after it, then `body`, padded so
    /// that the count the pair states ends on a zero byte.
    fn node_82_pointer_before(body: &[u8]) -> Vec<u8> {
        let count = super::INT_VALUES_WIDE_COUNT_MIN;
        let mut bytes = vec![0, 0x52];
        bytes.extend(count.to_be_bytes());
        bytes.extend(710u16.to_be_bytes());
        bytes.extend(body);
        bytes.resize(8 + usize::try_from(count).unwrap() * 4 + 1, 0);
        bytes
    }

    #[test]
    fn node_82_pointer_does_not_hide_the_topology_after_it() {
        let body = crate::test_support::parasolid::triangle_body();
        assert!(body
            .windows(super::MAGIC.len())
            .any(|window| window == super::MAGIC));
        let bytes = node_82_pointer_before(&body);
        assert!(super::topology_payload_ranges(&bytes, false).is_empty());
        let baseline = vertex_partition();
        let tables = scan(&bytes);
        assert_eq!(tables.vertex_uses.len(), baseline.vertex_uses.len());
        assert_eq!(tables.coedges.len(), baseline.coedges.len());
        assert_eq!(tables.loops.len(), baseline.loops.len());
        assert_eq!(
            super::super::typed::scan(&bytes).bodies.len(),
            super::super::typed::scan(&body).bodies.len()
        );
    }

    #[test]
    fn wide_int_values_count_without_a_sentinel_is_a_payload() {
        let bytes = node_82_pointer_before(&[]);
        assert_eq!(
            super::topology_payload_ranges(&bytes, false),
            std::iter::once(0..bytes.len() - 1).collect::<Vec<_>>()
        );
    }

    #[test]
    fn sentinel_past_a_wide_extent_does_not_reject_it() {
        let mut bytes = node_82_pointer_before(&[]);
        let end = bytes.len() - 1;
        bytes.extend(super::MAGIC);
        assert_eq!(
            super::topology_payload_ranges(&bytes, false),
            std::iter::once(0..end).collect::<Vec<_>>()
        );
    }

    #[test]
    fn unproved_vertex_owner_retains_framing_but_cannot_emit_geometry() {
        let mut bytes = crate::test_support::parasolid::triangle_body();
        let frame = crate::test_support::parasolid::framed_topology_vertex(
            211,
            149,
            [0, 0, 0, 0, 60],
            None,
            900,
            false,
            false,
        );
        bytes.extend(frame.clone());
        bytes.extend(framed_topology_fin(
            212,
            [0, 20, 0, 30, 211, 0, 40, 0, 0],
            true,
            false,
            false,
        ));
        let tables = super::scan_records(&bytes, false, false, None, None, None, None);
        assert!(tables.vertex_is_unresolved(211));
        assert!(tables.vertex_classes.contains_key(&211));
        assert_eq!(
            tables.vertex_frames[&211].bytes.as_deref(),
            Some(frame.as_slice())
        );
        let known = super::scan_with_owner_attrs(
            &bytes,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::from([900]),
        );
        assert_eq!(known.vertex_uses[&211].owner, 900);
        let wrong = super::scan_with_owner_attrs(
            &bytes,
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::from([901]),
        );
        assert!(wrong.vertex_is_unresolved(211));
        assert!(wrong.vertex_classes.contains_key(&211));
    }

    #[test]
    fn free_vertex_without_a_fin_uses_a_proved_shell_owner_class() {
        let mut shell = vec![0, 0x0d, 0xff];
        shell.extend(901u16.to_be_bytes());
        shell.extend(200_901u32.to_be_bytes());
        for reference in [0u16, 0, 0, 0, 0, 211, 902, 0] {
            shell.extend(reference.to_be_bytes());
        }
        let bytes = [
            shell,
            crate::test_support::parasolid::world_point(60, [0.0; 3]),
            vertex_frame(211, [0, 0, 0, 0, 60], 901, false),
        ]
        .concat();
        let tables = scan(&bytes);
        assert_eq!(tables.vertex_uses[&211].owner, 901);
        assert_eq!(tables.vertex_uses[&211].refs[1], 0);
        assert!(tables.coedges.is_empty());
    }

    #[test]
    fn null_owner_and_unproved_nonzero_owner_never_project_a_vertex() {
        for owner in [0, 1, 902] {
            let mut bytes = crate::test_support::parasolid::triangle_body();
            let frame = vertex_frame(211, [0, 0, 0, 0, 60], owner, false);
            bytes.extend(&frame);
            bytes.extend(framed_topology_fin(
                212,
                [0, 20, 0, 30, 211, 0, 40, 0, 0],
                true,
                false,
                false,
            ));
            let mut tables = scan(&bytes);
            assert!(tables.vertex_is_unresolved(211));
            assert!(tables.vertex_classes.contains_key(&211));
            assert_eq!(
                tables.vertex_frames[&211].bytes.as_deref(),
                Some(frame.as_slice())
            );
            let history = delta_topology(&vertex_frame(211, [0, 0, 0, 0, 60], 900, true), &tables);
            tables.merge_deltas(history, None);
            assert!(tables.vertex_is_unresolved(211));
            assert!(tables.vertex_frames[&211].partition);
        }
    }
    #[test]
    fn encoded_null_or_point_owner_never_becomes_vertex_geometry() {
        for owner in [0, 1, 60] {
            let mut bytes = [
                crate::test_support::parasolid::world_point(60, [0.0; 3]),
                vertex_frame(211, [0, 0, 0, 0, 60], owner, false),
            ]
            .concat();
            bytes.extend(crate::test_support::parasolid::fixture_vertex_owner_class(
                owner,
            ));
            let tables = scan(&bytes);
            assert!(!tables.vertex_uses.contains_key(&211));
        }
    }
    fn owner_shell_frame() -> Vec<u8> {
        let mut shell = vec![0, 0x0d];
        shell.extend(1901u16.to_be_bytes());
        shell.extend(200_901u32.to_be_bytes());
        for reference in [0u16, 0, 0, 0, 0, 211, 902, 0] {
            shell.extend(reference.to_be_bytes());
        }
        shell
    }

    #[test]
    fn counted_payload_shell_owner_cannot_authorize_a_referenced_vertex() {
        let shell = owner_shell_frame();
        let mut integers = vec![0, 0x52];
        integers.extend(u32::try_from(shell.len() / 4).unwrap().to_be_bytes());
        integers.extend(710u16.to_be_bytes());
        integers.extend(&shell);
        let slots: Vec<_> = shell
            .chunks_exact(2)
            .map(|s| u16::from_be_bytes([s[0], s[1]]))
            .collect();
        let attribute = crate::test_support::parasolid::entity51(7, 710, 711, &slots);
        for enclosing in [integers, attribute] {
            let mut bytes = crate::test_support::parasolid::triangle_body();
            bytes.extend(enclosing);
            bytes.extend(vertex_frame(211, [0, 0, 0, 0, 60], 1901, false));
            bytes.extend(framed_topology_fin(
                212,
                [0, 20, 0, 30, 211, 0, 40, 0, 0],
                true,
                false,
                false,
            ));
            let tables = scan(&bytes);
            assert!(!tables.owner_classes.contains(&1901));
            assert!(!tables.vertex_uses.contains_key(&211));
            assert!(tables.vertex_is_unresolved(211));
        }
    }

    #[test]
    fn identical_shell_bytes_outside_counted_payload_keep_free_vertex_admission() {
        let bytes = [
            owner_shell_frame(),
            crate::test_support::parasolid::world_point(60, [0.0; 3]),
            vertex_frame(211, [0, 0, 0, 0, 60], 1901, false),
        ]
        .concat();
        let tables = scan(&bytes);
        assert_eq!(tables.vertex_uses[&211].owner, 1901);
        assert!(tables.coedges.is_empty());
    }
    #[test]
    fn a_record_tail_read_as_an_edge_header_does_not_hide_the_next_loop() {
        // `00 10` in a record tail reads as an EDGE header whose tolerance
        // slot holds the loop's bytes: a finite value below 1e-12 m.
        let mut bytes = crate::test_support::parasolid::triangle_body();
        let head = crate::test_support::parasolid::loop_head(20, 30, 10);
        let at = bytes
            .windows(head.len())
            .position(|window| window == head.as_slice())
            .unwrap();
        bytes.splice(at..at, [0x00, 0x10, 0x00, 0x52]);
        assert!(super::topology_header_prefix_end(&bytes, at, 0x10).is_none());
        assert!(scan(&bytes).loops.contains_key(&20));
    }

    #[test]
    fn face_transmit_index_does_not_create_an_integer_values_enclosure() {
        let bytes = crate::test_support::parasolid::bridge_owned(82, 20, 100, 88);
        assert!(super::topology_payload_ranges(&bytes, false).is_empty());
        assert_eq!(parse_bridge(&bytes, 0, false).unwrap().attr, 82);
    }

    #[test]
    fn a_plain_stream_frames_a_face_on_its_stored_tolerance() {
        let face = |slot: f64| {
            let mut bytes = crate::test_support::parasolid::bridge(82, 20, 100);
            bytes[10..18].copy_from_slice(&slot.to_be_bytes());
            bytes
        };
        for tolerance in [0.0, 1.0e-8, 1.0e-12, 1.0e-2] {
            assert_eq!(parse_bridge(&face(tolerance), 0, true).unwrap().attr, 82);
            assert!(parse_bridge(&face(tolerance), 0, false).is_none());
        }
        for tolerance in [1.0e-302, 2.0e-2, -1.0e-8, f64::NAN] {
            assert!(parse_bridge(&face(tolerance), 0, true).is_none());
        }
        assert_eq!(
            parse_bridge(
                &crate::test_support::parasolid::bridge(82, 20, 100),
                0,
                true
            )
            .unwrap()
            .attr,
            82
        );
    }

    #[test]
    fn proven_face_prefix_does_not_consume_the_following_record_as_a_tail() {
        let mut bytes = crate::test_support::parasolid::bridge_owned(82, 20, 100, 88);
        bytes.truncate(2 + crate::layout::face_bare::SENSE + 1);
        let start = bytes.len();
        bytes.extend([0, 0x52]);
        bytes.extend(0u32.to_be_bytes());
        bytes.extend(710u16.to_be_bytes());
        assert_eq!(
            super::topology_payload_ranges(&bytes, false),
            std::iter::once(start..bytes.len()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn finite_point_coordinate_bytes_cannot_start_a_data_enclosure() {
        let value = f64::from_bits(0x0052_0000_0000_0066);
        let bytes = crate::test_support::parasolid::world_point(120, [value, 0.0, 0.0]);
        assert!(super::parse_point(&bytes, 0, false).is_some());
        assert!(super::topology_payload_ranges(&bytes, false).is_empty());
    }

    #[test]
    fn earlier_integer_values_frame_owns_nested_complete_topology() {
        let face = crate::test_support::parasolid::bridge_owned(82, 20, 100, 88);
        let point = crate::test_support::parasolid::world_point(120, [0.0, 0.0, 0.0]);
        let mut payload = face;
        payload.extend(point);
        payload.resize(payload.len().next_multiple_of(4), 0);
        let mut bytes = vec![0, 0x52];
        bytes.extend(u32::try_from(payload.len() / 4).unwrap().to_be_bytes());
        bytes.extend(710u16.to_be_bytes());
        bytes.extend(payload);
        assert_eq!(
            super::topology_payload_ranges(&bytes, false),
            std::iter::once(0..bytes.len()).collect::<Vec<_>>()
        );
        let tables = scan(&bytes);
        assert!(!tables.bridges.contains_key(&82));
        assert!(!tables.points.contains_key(&120));
    }
    #[test]
    fn point_coordinate_shell_bytes_cannot_establish_a_vertex_owner_class() {
        let shell = owner_shell_frame();
        assert_eq!(shell.len(), 24);
        let xyz: Vec<_> = shell
            .chunks_exact(8)
            .map(|bytes| f64::from_be_bytes(bytes.try_into().unwrap()))
            .collect();
        assert!(xyz.iter().all(|value| value.is_finite()));
        let point = crate::test_support::parasolid::world_point(60, [xyz[0], xyz[1], xyz[2]]);
        let vertex = vertex_frame(211, [0, 0, 0, 0, 60], 1901, false);
        let bytes = [point, vertex].concat();
        let facts = super::super::typed::scan(&bytes);
        assert!(!facts.shells.iter().any(|shell| shell.attr == 1901));
        let tables = scan(&bytes);
        assert!(!tables.owner_classes.contains(&1901));
        assert!(!tables.vertex_uses.contains_key(&211));
        assert!(tables.vertex_is_unresolved(211));
        assert!(tables.points.contains_key(&60));

        let outside = [bytes, shell].concat();
        let tables = scan(&outside);
        assert!(tables.owner_classes.contains(&1901));
        assert_eq!(tables.vertex_uses[&211].owner, 1901);
        assert!(tables.coedges.is_empty());
    }
    #[test]
    fn supported_shell_fields_do_not_create_a_false_data_enclosure() {
        let mut shell = owner_shell_frame();
        shell[2..4].copy_from_slice(&82u16.to_be_bytes());
        shell[4..8].copy_from_slice(&1u32.to_be_bytes());
        shell[8..10].copy_from_slice(&88u16.to_be_bytes());
        assert_eq!(
            super::super::typed::tagged_owner_prefix_end(&shell, 0),
            Some(shell.len())
        );
        assert!(super::topology_payload_ranges(&shell, false).is_empty());
        let facts = super::super::typed::scan(&shell);
        assert!(facts
            .shells
            .iter()
            .any(|node| node.attr == 82 && node.node_id == 1 && node.offset == 0));

        let point = crate::test_support::parasolid::world_point(60, [0.0; 3]);
        let vertex = vertex_frame(211, [0, 0, 0, 0, 60], 82, false);
        let outside = [shell.clone(), point.clone(), vertex.clone()].concat();
        assert_eq!(scan(&outside).vertex_uses[&211].owner, 82);

        let mut integers = vec![0, 0x52];
        integers.extend(u32::try_from(shell.len() / 4).unwrap().to_be_bytes());
        integers.extend(710u16.to_be_bytes());
        integers.extend(&shell);
        let slots: Vec<_> = shell
            .chunks_exact(2)
            .map(|bytes| u16::from_be_bytes(bytes.try_into().unwrap()))
            .collect();
        let attribute = crate::test_support::parasolid::entity51(7, 710, 711, &slots);
        for enclosure in [integers, attribute] {
            assert_eq!(
                super::topology_payload_ranges(&enclosure, false),
                std::iter::once(0..enclosure.len()).collect::<Vec<_>>()
            );
            let inside = [enclosure, point.clone(), vertex.clone()].concat();
            let tables = scan(&inside);
            assert!(!tables.owner_classes.contains(&82));
            assert!(!tables.vertex_uses.contains_key(&211));
            assert!(tables.vertex_is_unresolved(211));
        }

        shell[4..8].copy_from_slice(&0u32.to_be_bytes());
        assert!(super::super::typed::tagged_owner_prefix_end(&shell, 0).is_none());
        assert!(!super::super::typed::scan(&shell)
            .shells
            .iter()
            .any(|node| node.attr == 82));
    }

    #[test]
    fn point_payload_fin_cannot_poison_a_current_closed_ring_identity() {
        let body = crate::test_support::parasolid::triangle_body();
        let baseline = scan(&body);
        let original = baseline.coedges.get(&30).expect("current triangle FIN");
        let fin = framed_topology_fin(
            30,
            original.refs.map(|r| u16::try_from(r).unwrap()),
            false,
            false,
            original.sense == Sense::Forward,
        );
        assert_eq!(fin.len(), 23);
        let readings = super::parse_coedge_candidates(&fin, 0);
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0].0.attr, original.attr);
        assert_eq!(readings[0].0.refs, original.refs);
        assert_ne!(readings[0].0.sense, original.sense);

        let outside = [body.clone(), fin.clone()].concat();
        let conflict = scan(&outside);
        assert!(conflict.coedge_is_ambiguous(30));
        assert!(!conflict.coedges.contains_key(&30));

        let mut payload = fin;
        payload.push(0);
        assert_eq!(payload.len(), 24);
        let xyz: Vec<_> = payload
            .chunks_exact(8)
            .map(|bytes| f64::from_be_bytes(bytes.try_into().unwrap()))
            .collect();
        assert!(xyz.iter().all(|value| value.is_finite()));
        let point = crate::test_support::parasolid::world_point(600, [xyz[0], xyz[1], xyz[2]]);
        let position = super::parse_point(&point, 0, false).unwrap().xyz_offset;
        assert_eq!(super::parse_coedge_candidates(&point, position).len(), 1);
        let inside = [body, point].concat();
        let protected = scan(&inside);
        assert!(protected.points.contains_key(&600));
        assert!(!protected.coedge_is_ambiguous(30));
        assert_eq!(protected.coedges, baseline.coedges);
    }

    #[test]
    fn supported_surface_descriptor_fields_do_not_create_data_enclosures() {
        use crate::layout::bspline_surface_descriptor as descriptor;
        let mut fields = vec![0; descriptor::LEN];
        fields[descriptor::ATTR..descriptor::ATTR + 2].copy_from_slice(&82u16.to_be_bytes());
        for (offset, value) in [
            (descriptor::U_DEGREE, 2u16),
            (descriptor::V_DEGREE, 3),
            (descriptor::VERTEX_DIM, 3),
        ] {
            fields[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        }
        for (offset, value) in [
            (descriptor::U_POLE_COUNT, 3u32),
            (descriptor::V_POLE_COUNT, 4),
            (descriptor::U_DISTINCT_KNOT_COUNT, 2),
            (descriptor::V_DISTINCT_KNOT_COUNT, 2),
        ] {
            fields[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        }
        for (index, value) in (200u16..205).enumerate() {
            let offset = descriptor::ARRAY_REFS + 2 * index;
            fields[offset..offset + 2].copy_from_slice(&value.to_be_bytes());
        }
        for marker in [false, true] {
            let mut frame = vec![0, 0x7e];
            if marker {
                frame.push(0xff);
            }
            frame.extend(&fields);
            assert_eq!(
                super::super::spline::surface_descriptor_prefix_end(&frame, 0),
                Some(frame.len())
            );
            assert!(super::topology_payload_ranges(&frame, false).is_empty());
            let point = crate::test_support::parasolid::world_point(60, [0.0; 3]);
            let outside = [frame.clone(), point].concat();
            assert!(scan(&outside).points.contains_key(&60));
            if !marker {
                let mut enclosure = vec![0, 0x52];
                enclosure.extend(u32::try_from(frame.len() / 4).unwrap().to_be_bytes());
                enclosure.extend(710u16.to_be_bytes());
                enclosure.extend(&frame);
                assert_eq!(
                    super::topology_payload_ranges(&enclosure, false),
                    std::iter::once(0..enclosure.len()).collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn complete_edge_owner_vertex_tag_cannot_swallow_the_following_fin() {
        use cadmpeg_ir::codec::{Codec, DecodeOptions};
        let source = crate::test_support::container::sldprt_with_body(
            &crate::test_support::parasolid::nurbs_triangle_body(),
        );
        let decoded = crate::SldprtCodec
            .decode(&mut std::io::Cursor::new(source), &DecodeOptions::default())
            .unwrap();
        assert_eq!(decoded.ir().model.faces.len(), 1);
        assert_eq!(decoded.ir().model.loops.len(), 1);
        assert_eq!(decoded.ir().model.coedges.len(), 3);
        let body = crate::writer::brep_body(decoded.ir(), 0.001, false).unwrap();
        let original = scan(&body);
        assert_eq!(original.edge_uses.len(), 3);
        let edge = original
            .edge_uses
            .values()
            .max_by_key(|edge| edge.offset)
            .unwrap();
        let offset = edge.offset;
        let p = super::body_start(&body, offset, 0x10).unwrap();
        let end = p + crate::layout::edge_bare::LEN;
        let owner_at = end - std::mem::size_of::<u16>();
        assert_eq!(
            cadmpeg_core::decode::View::u16_be_at(&body, owner_at),
            Some(18)
        );
        assert!(super::parse_vertex_candidates(&body, owner_at).is_empty());
        assert!(!super::parse_coedge_candidates(&body, end).is_empty());
        assert_eq!(
            super::topology_header_prefix_end(&body, offset, 0x10),
            Some(end)
        );
        let tables = scan(&body);
        assert_eq!(tables.coedges.len(), 3);
        assert_eq!(tables.loops.len(), 1);
        let frame = &body[offset..end];
        assert_eq!(frame.len() % 4, 0);
        let mut enclosed = body[..offset].to_vec();
        enclosed.extend([0, 0x52]);
        enclosed.extend(u32::try_from(frame.len() / 4).unwrap().to_be_bytes());
        enclosed.extend(710u16.to_be_bytes());
        enclosed.extend(frame);
        enclosed.extend(&body[end..]);
        let scoped = scan(&enclosed);
        assert!(!scoped.edge_uses.contains_key(&edge.attr));
        assert_eq!(scoped.coedges.len(), 3);
    }
    #[test]
    fn world_schema_pointer_fields_cannot_create_an_integer_values_enclosure() {
        let mut bytes = vec![0, 101, 13];
        bytes.extend([b'C'; 11]);
        bytes.extend([b'A', 7]);
        bytes.extend(b"indices");
        let false_integer = bytes.len();
        bytes.extend([0, 82, 0, 1]);
        bytes.extend([b'A', 1, b'b', 0, 82, 0, 1, b'Z']);
        let program_end = bytes.len();
        assert_eq!(
            super::super::typed::schema_prefix_end(&bytes, 0),
            Some(program_end)
        );
        // Authored WORLD index, seven pointer fields, alive, attribute
        // definition pointer, highest/current IDs, and the two appended
        // nullable INT_VALUES pointers. WORLD has no visible-node ID.
        bytes.extend(2u16.to_be_bytes());
        for reference in [1u16, 1, 900, 1, 1, 1, 1] {
            bytes.extend(reference.to_be_bytes());
        }
        bytes.push(1);
        bytes.extend(1u16.to_be_bytes());
        bytes.extend(100u32.to_be_bytes());
        bytes.extend(100u32.to_be_bytes());
        bytes.extend(1u16.to_be_bytes());
        bytes.extend(1u16.to_be_bytes());
        let world_end = bytes.len();
        let triangle = crate::test_support::parasolid::triangle_body();
        let baseline = scan(&triangle);
        assert_eq!(baseline.vertex_uses.len(), 3);
        assert_eq!(baseline.coedges.len(), 3);
        assert_eq!(baseline.loops.len(), 1);
        bytes.extend(&triangle);
        // If interpreted as an INT_VALUES start, the pointer class and
        // following field descriptor would claim this entire later graph.
        let count = u32::from_be_bytes(
            bytes[false_integer + 2..false_integer + 6]
                .try_into()
                .unwrap(),
        );
        let node = u16::from_be_bytes(
            bytes[false_integer + 6..false_integer + 8]
                .try_into()
                .unwrap(),
        );
        assert_eq!(count, 82_177);
        assert_eq!(node, 25_088);
        let false_end = false_integer + 8 + usize::try_from(count).unwrap() * 4;
        assert!(false_end > bytes.len());
        let mut graph = bytes.clone();
        bytes.resize(false_end, 0);
        assert!(super::topology_payload_ranges(&bytes, false).is_empty());
        let tables = scan(&bytes);
        assert_eq!(tables.vertex_uses.len(), 3);
        assert_eq!(tables.coedges.len(), 3);
        assert_eq!(tables.loops.len(), 1);
        let facts = super::super::typed::scan(&bytes);
        assert!(facts.bodies.iter().any(|body| body.attr == 900));
        // Only the schema program is protected by this new class grammar;
        // the supported framer does not claim a WORLD instance extent.
        let scopes = super::topology_scopes(&bytes, false);
        assert!(scopes.excluded_starts.contains(&(1..program_end)));
        assert!(!scopes
            .excluded_starts
            .iter()
            .any(|range| { range.start <= program_end && range.end >= world_end }));

        let mut enclosed = vec![0, 82];
        graph.resize(graph.len().next_multiple_of(4), 0);
        enclosed.extend(u32::try_from(graph.len() / 4).unwrap().to_be_bytes());
        enclosed.extend(710u16.to_be_bytes());
        enclosed.extend(graph);
        assert_eq!(
            super::topology_payload_ranges(&enclosed, false),
            std::iter::once(0..enclosed.len()).collect::<Vec<_>>()
        );
        let tables = scan(&enclosed);
        assert!(tables.vertex_uses.is_empty());
        assert!(tables.coedges.is_empty());
        assert!(super::super::typed::scan(&enclosed).bodies.is_empty());
    }
}
