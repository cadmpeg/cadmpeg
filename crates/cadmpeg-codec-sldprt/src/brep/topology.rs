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
//! preserve partition topology, add missing subordinate deltas records and
//! typed-FACE-selected bridges, and apply deltas point updates. The decoder
//! merges all admitted sites. It keeps the active site's primary identities
//! and qualifies the other sites' identities; without an active site, it
//! qualifies every site's identities.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::topology::Sense;

use crate::layout::world_point as world_pt;

/// The magic anchoring magic-bearing topology records ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Bridge {
    pub(super) attr: u16,
    pub(super) refs: [u16; 5],
    pub(super) sequence: u32,
    pub(super) sense: Sense,
    pub(super) owner: Option<u16>,
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
fn parse_edge_use_candidates<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    buf: &[u8],
    off: usize,
) -> Result<CandidateRecords<'ctx, EdgeUse>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "Parasolid topology candidate storage")?;
    let Some(p) = body_start(buf, off, 0x10) else {
        return Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
            records: Vec::new(),
            _storage: storage,
        });
    };
    if p + 28 > buf.len() {
        return Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
            records: Vec::new(),
            _storage: storage,
        });
    }
    let Some(attr) = attr_at(buf, p) else {
        return Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
            records: Vec::new(),
            _storage: storage,
        });
    };
    let Some(sequence) = View::u32_be_at(buf, p + 2) else {
        return Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
            records: Vec::new(),
            _storage: storage,
        });
    };
    let mut out = Vec::new();
    if buf.get(p + 8..p + 16) == Some(MAGIC.as_slice()) {
        if let Some(refs) = refs_be::<6>(buf, p + 16) {
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut (out),
                    EdgeUse {
                        attr,
                        sequence,
                        references: EdgeReferences::Bare(refs),
                        offset: off,
                    },
                    "collect SLDPRT decoded vector items",
                )
            })?;
        }
    }

    let magic_end = (p + 16).min(buf.len().checked_sub(MAGIC.len()).map_or(0, |end| end));
    for magic in p + 9..=magic_end {
        if buf.get(magic..magic + MAGIC.len()) != Some(MAGIC.as_slice()) {
            continue;
        }
        let q = magic + MAGIC.len();
        for prefix_first in [true, false] {
            // The compact layout needs three complete reference triples; the
            // third is the support-curve carrier.
            let mut curve = None;
            for at in [q, q + 3, q + 6] {
                let (matches, reference_at) = if prefix_first {
                    // `[01][hi][lo]` triples.
                    (buf.get(at) == Some(&1), at + 1)
                } else {
                    // `[hi][lo][01]` triples.
                    (buf.get(at + 2) == Some(&1), at)
                };
                curve = matches
                    .then(|| View::u16_be_at(buf, reference_at))
                    .flatten();
                if curve.is_none() {
                    break;
                }
            }
            if let Some(curve) = curve {
                storage.with_storage(|| {
                    ctx.push_vec(
                        &mut (out),
                        EdgeUse {
                            attr,
                            sequence,
                            references: EdgeReferences::Compact { curve },
                            offset: off,
                        },
                        "collect SLDPRT decoded vector items",
                    )
                })?;
            }
        }
    }
    out.dedup();
    Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
        records: out,
        _storage: storage,
    })
}

/// Edge-use `00 10`: 28-byte body, magic at body+8, `refs[6]` at body+16.
/// `refs[0]` = canonical forward coedge when the bare record stores one;
/// `refs[3]` = support curve carrier. The compact layout has no canonical-coedge slot.
///
/// Coedge `00 11`: 21-byte body, no magic, `refs[9]` at body+2, marker at
/// body+20. `refs[1]` = owning loop, `refs[3]` = next coedge, `refs[4]` = start
/// vertex-use, `refs[5]` = twin coedge, `refs[6]` = edge-use.
fn parse_coedge_candidates<'ctx>(
    ctx: &'ctx cadmpeg_core::decode::DecodeContext<'_>,
    buf: &[u8],
    off: usize,
) -> Result<CandidateRecords<'ctx, Coedge>, cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "Parasolid topology candidate storage")?;
    let Some(p) = body_start(buf, off, 0x11) else {
        return Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
            records: Vec::new(),
            _storage: storage,
        });
    };
    if p + 21 > buf.len() {
        return Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
            records: Vec::new(),
            _storage: storage,
        });
    }
    let Some(attr) = attr_at(buf, p) else {
        return Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
            records: Vec::new(),
            _storage: storage,
        });
    };
    let mut out = Vec::new();
    if let (Some(refs), Some(marker)) = (refs_be::<9>(buf, p + 2), buf.get(p + 20).copied()) {
        if let Some(sense) = parse_sense(marker) {
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut (out),
                    Coedge {
                        attr,
                        refs,
                        sense,
                        offset: off,
                    },
                    "collect SLDPRT decoded vector items",
                )
            })?;
        }
    }
    if let (Some(refs), Some(marker)) = (refs_tripled::<9>(buf, p + 2), buf.get(p + 29).copied()) {
        if let Some(sense) = parse_sense(marker) {
            storage.with_storage(|| {
                ctx.push_vec(
                    &mut (out),
                    Coedge {
                        attr,
                        refs,
                        sense,
                        offset: off,
                    },
                    "collect SLDPRT decoded vector items",
                )
            })?;
        }
    }
    out.dedup();
    Ok::<_, cadmpeg_core::CodecError>(CandidateRecords {
        records: out,
        _storage: storage,
    })
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
        let magic = (p + 21
            ..=(p + 32).min(buf.len().checked_sub(MAGIC.len()).map_or(0, |end| end)))
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
/// The prefixed form stores up to sixteen `[hi][lo][01]` reference triples
/// before the coordinates; the bare form stores four adjacent references.
fn parse_point(buf: &[u8], off: usize, prefixed: bool) -> Option<Point> {
    let p = body_start(buf, off, 0x1d)?;
    if p + world_pt::LEN > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let (first_reference, xyz_at) = if prefixed {
        let mut first = None;
        let mut count = 0;
        let mut cursor = p + world_pt::REFS;
        while buf.get(cursor + 2) == Some(&1) && count < 16 {
            let reference = View::u16_be_at(buf, cursor)?;
            first.get_or_insert(reference);
            count += 1;
            cursor += 3;
        }
        (first?, cursor)
    } else {
        let [first, ..] = refs_be::<4>(buf, p + world_pt::REFS)?;
        (first, p + world_pt::XYZ)
    };
    if first_reference > 1 {
        return None;
    }
    let (x, y, z) = (
        View::f64_be_at(buf, xyz_at)?,
        View::f64_be_at(buf, xyz_at + 8)?,
        View::f64_be_at(buf, xyz_at + 16)?,
    );
    for v in [x, y, z] {
        // Reject exponent-poisoned reads from a misaligned candidate: real part
        // coordinates in metres sit well under this cap.
        if !v.is_finite() || v.abs() > 1e4 {
            return None;
        }
    }
    Some(Point {
        attr,
        xyz_m: [x, y, z],
        xyz_offset: xyz_at,
        offset: off,
    })
}

/// The topology record tables of one stream, each keyed by `attr`.
#[derive(Default)]
pub(crate) struct Tables {
    bridges: BTreeMap<u16, Bridge>,
    loops: BTreeMap<u16, Loop>,
    edge_uses: BTreeMap<u16, EdgeUse>,
    coedges: BTreeMap<u16, Coedge>,
    vertex_uses: BTreeMap<u16, VertexUse>,
    points: BTreeMap<u16, Point>,
}

impl Tables {
    pub(super) fn bridges(&self) -> &BTreeMap<u16, Bridge> {
        &self.bridges
    }

    pub(super) fn insert_bridge(
        &mut self,
        ctx: &DecodeContext<'_>,
        record: Bridge,
    ) -> Result<(), CodecError> {
        ctx.insert_btree_map(
            &mut self.bridges,
            record.attr,
            record,
            "index Parasolid topology bridges",
        )
        .map(|_previous| ())
    }

    pub(super) fn loops(&self) -> &BTreeMap<u16, Loop> {
        &self.loops
    }

    pub(super) fn insert_loop(
        &mut self,
        ctx: &DecodeContext<'_>,
        record: Loop,
    ) -> Result<(), CodecError> {
        ctx.insert_btree_map(
            &mut self.loops,
            record.attr,
            record,
            "index Parasolid topology loops",
        )
        .map(|_previous| ())
    }

    pub(super) fn edge_uses(&self) -> &BTreeMap<u16, EdgeUse> {
        &self.edge_uses
    }

    fn insert_edge_use(
        &mut self,
        ctx: &DecodeContext<'_>,
        record: EdgeUse,
    ) -> Result<(), CodecError> {
        ctx.insert_btree_map(
            &mut self.edge_uses,
            record.attr,
            record,
            "index Parasolid topology edge uses",
        )
        .map(|_previous| ())
    }

    pub(super) fn coedges(&self) -> &BTreeMap<u16, Coedge> {
        &self.coedges
    }

    pub(super) fn insert_coedge(
        &mut self,
        ctx: &DecodeContext<'_>,
        record: Coedge,
    ) -> Result<(), CodecError> {
        ctx.insert_btree_map(
            &mut self.coedges,
            record.attr,
            record,
            "index Parasolid topology coedges",
        )
        .map(|_previous| ())
    }

    pub(super) fn vertex_uses(&self) -> &BTreeMap<u16, VertexUse> {
        &self.vertex_uses
    }

    fn insert_vertex_use(
        &mut self,
        ctx: &DecodeContext<'_>,
        record: VertexUse,
    ) -> Result<(), CodecError> {
        ctx.insert_btree_map(
            &mut self.vertex_uses,
            record.attr,
            record,
            "index Parasolid topology vertex uses",
        )
        .map(|_previous| ())
    }

    pub(crate) fn points(&self) -> &BTreeMap<u16, Point> {
        &self.points
    }

    fn insert_point(&mut self, ctx: &DecodeContext<'_>, record: Point) -> Result<(), CodecError> {
        ctx.insert_btree_map(
            &mut self.points,
            record.attr,
            record,
            "index Parasolid topology points",
        )
        .map(|_previous| ())
    }

    /// Merge deltas without replacing partition topology membership.
    ///
    /// Preserve partition topology for shared identities and add only deltas
    /// bridges selected by the typed FACE ownership set.
    pub(super) fn merge_deltas(
        &mut self,
        ctx: &DecodeContext<'_>,
        mut deltas: Self,
        selected_bridge_attrs: Option<&BTreeSet<u16>>,
    ) -> Result<(), CodecError> {
        if self.bridges.is_empty() {
            if let Some(selected_bridge_attrs) = selected_bridge_attrs {
                retain_selected_bridges(ctx, &mut deltas.bridges, selected_bridge_attrs)?;
            }
            self.bridges = deltas.bridges;
        } else if let Some(selected_bridge_attrs) = selected_bridge_attrs {
            retain_selected_bridges(ctx, &mut deltas.bridges, selected_bridge_attrs)?;
            merge_missing(
                ctx,
                &mut self.bridges,
                deltas.bridges,
                "merge Parasolid topology bridges",
            )?;
        }
        merge_missing(
            ctx,
            &mut self.loops,
            deltas.loops,
            "merge Parasolid topology loops",
        )?;
        merge_missing(
            ctx,
            &mut self.edge_uses,
            deltas.edge_uses,
            "merge Parasolid topology edge uses",
        )?;
        merge_missing(
            ctx,
            &mut self.coedges,
            deltas.coedges,
            "merge Parasolid topology coedges",
        )?;
        merge_missing(
            ctx,
            &mut self.vertex_uses,
            deltas.vertex_uses,
            "merge Parasolid topology vertex uses",
        )?;
        for (attr, record) in ctx.admit_iter(deltas.points, "merge Parasolid topology points")? {
            ctx.insert_btree_map(
                &mut self.points,
                attr,
                record,
                "merge Parasolid topology points",
            )?;
        }
        Ok(())
    }
}

fn retain_selected_bridges(
    ctx: &DecodeContext<'_>,
    bridges: &mut BTreeMap<u16, Bridge>,
    selected_bridge_attrs: &BTreeSet<u16>,
) -> Result<(), CodecError> {
    const OPERATION: &str = "select Parasolid deltas bridges";
    ctx.retain_btree_map(
        bridges,
        |attr, record| {
            Ok(
                ctx.contains_btree_set(selected_bridge_attrs, attr, OPERATION)?
                    || match record.owner {
                        Some(owner) => {
                            ctx.contains_btree_set(selected_bridge_attrs, &owner, OPERATION)?
                        }
                        None => false,
                    },
            )
        },
        OPERATION,
    )
}

fn merge_missing<T>(
    ctx: &DecodeContext<'_>,
    target: &mut BTreeMap<u16, T>,
    source: BTreeMap<u16, T>,
    operation: &'static str,
) -> Result<(), CodecError> {
    for (attr, record) in ctx.admit_iter(source, operation)? {
        if !ctx.contains_key_btree_map(target, &attr, operation)? {
            ctx.insert_btree_map(target, attr, record, operation)?;
        }
    }
    Ok(())
}

struct CandidateRecords<'ctx, T> {
    records: Vec<T>,
    /// Scoped backing of `records`, released with them.
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

trait Candidate: PartialEq {
    fn attr(&self) -> u16;
}

impl Candidate for EdgeUse {
    fn attr(&self) -> u16 {
        self.attr
    }
}

impl Candidate for Coedge {
    fn attr(&self) -> u16 {
        self.attr
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

/// Keep the latest record occurrence for each attribute with all frame
/// readings at that occurrence. The scan parses each offset once, in
/// increasing order, so a later reading set replaces the earlier one. A
/// stream can contain overlapping payload bytes, and a later complete record
/// has the same override semantics as the ordinary topology tables.
fn insert_candidates<'ctx, T: Candidate>(
    ctx: &DecodeContext<'_>,
    storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    target: &mut BTreeMap<u16, CandidateRecords<'ctx, T>>,
    records: CandidateRecords<'ctx, T>,
) -> Result<(), CodecError> {
    let Some(first) = records.records.first() else {
        return Ok(());
    };
    let attr = first.attr();
    storage.with_storage(|| {
        ctx.insert_btree_map(target, attr, records, "index Parasolid topology candidates")
    })?;
    Ok(())
}

/// Loop facts the coedge evidence reads, indexed once per scan.
struct LoopIndex<'a> {
    /// The last loop candidate of each attribute.
    last: BTreeMap<u16, &'a Loop>,
    /// `(loop, first coedge)` of every loop candidate owned by a bridge.
    owned_heads: BTreeSet<(u16, u16)>,
}

impl<'a> LoopIndex<'a> {
    fn new(
        ctx: &DecodeContext<'_>,
        loops: &'a [Loop],
        bridges: &BTreeMap<u16, Bridge>,
        storage: &mut cadmpeg_core::decode::ScopedReservation<'_>,
    ) -> Result<Self, CodecError> {
        const OPERATION: &str = "index Parasolid coedge loop evidence";
        let mut last = BTreeMap::new();
        let mut owned_heads = BTreeSet::new();
        for record in ctx.admit_iter(loops, OPERATION)? {
            storage
                .with_storage(|| ctx.insert_btree_map(&mut last, record.attr, record, OPERATION))?;
            if loop_is_owned(ctx, record, bridges)? {
                storage.with_storage(|| {
                    ctx.insert_btree_set(&mut owned_heads, (record.attr, record.refs[1]), OPERATION)
                })?;
            }
        }
        Ok(Self { last, owned_heads })
    }
}

fn loop_is_owned(
    ctx: &DecodeContext<'_>,
    record: &Loop,
    bridges: &BTreeMap<u16, Bridge>,
) -> Result<bool, CodecError> {
    Ok(record.refs[2] != 0
        && ctx.contains_key_btree_map(bridges, &record.refs[2], "find Parasolid loop owners")?)
}

/// Collect independent graph invariants for one coedge frame. The first four
/// fields identify the owner, vertex-use, edge-use, and ring; reciprocal links
/// and loop-head membership provide additional confirmation. No field is a
/// byte-position discriminator.
fn coedge_evidence<'a>(
    ctx: &DecodeContext<'_>,
    candidate: &Coedge,
    loops: &LoopIndex<'a>,
    bridges: &BTreeMap<u16, Bridge>,
    vertex_uses: &'a BTreeMap<u16, VertexUse>,
    edge_candidates: &'a BTreeMap<u16, CandidateRecords<'_, EdgeUse>>,
    coedge_candidates: &BTreeMap<u16, CandidateRecords<'_, Coedge>>,
) -> Result<CoedgeEvidence<'a>, CodecError> {
    const OPERATION: &str = "collect Parasolid coedge evidence";
    let owner = candidate.refs[1];
    let owner_evidence = match ctx.get_btree_map(&loops.last, &owner, OPERATION)? {
        Some(&record) if owner != 0 && loop_is_owned(ctx, record, bridges)? => Some(record),
        _ => None,
    };
    let start = candidate.refs[4];
    let start_evidence = if start == 0 {
        None
    } else {
        ctx.get_btree_map(vertex_uses, &start, OPERATION)?
    };
    let edge = candidate.refs[6];
    let edge_evidence = if edge == 0 {
        None
    } else {
        ctx.get_btree_map(edge_candidates, &edge, OPERATION)?
            .map(|candidates| candidates.records.as_slice())
    };
    let next = candidate.refs[3];
    let next_owner_valid = match ctx.get_btree_map(coedge_candidates, &next, OPERATION)? {
        Some(candidates) if next != 0 => Some(ctx.any_by(
            &candidates.records,
            |next_candidate| Ok(next_candidate.refs[1] == owner),
            "scan Parasolid coedge successor evidence",
        )?),
        _ => None,
    };
    let previous = candidate.refs[2];
    let previous_valid = previous == 0
        || match ctx.get_btree_map(coedge_candidates, &previous, OPERATION)? {
            Some(candidates) => ctx.any_by(
                &candidates.records,
                |previous_candidate| {
                    Ok(previous_candidate.refs[3] == candidate.attr
                        && previous_candidate.refs[1] == owner)
                },
                "scan Parasolid coedge predecessor evidence",
            )?,
            None => false,
        };
    let loop_head_valid =
        ctx.contains_btree_set(&loops.owned_heads, &(owner, candidate.attr), OPERATION)?;
    Ok(CoedgeEvidence {
        owner: owner_evidence,
        start: start_evidence,
        edge: edge_evidence,
        next_owner: next_owner_valid,
        previous: previous_valid,
        loop_head: loop_head_valid,
    })
}

fn evidence_dominates(left: CoedgeEvidence<'_>, right: CoedgeEvidence<'_>) -> bool {
    let left = left.flags();
    let right = right.flags();
    let at_least_as_supported = left.iter().zip(right).all(|(left, right)| *left || !right);
    let strictly_more_supported = left.iter().zip(right).any(|(left, right)| *left && !right);
    at_least_as_supported && strictly_more_supported
}

/// Select the frame reading whose evidence no other reading dominates, when
/// exactly one such reading exists. One offset yields at most two readings.
fn select_coedge(
    ctx: &DecodeContext<'_>,
    candidates: &[Coedge],
    loops: &LoopIndex<'_>,
    bridges: &BTreeMap<u16, Bridge>,
    vertex_uses: &BTreeMap<u16, VertexUse>,
    edge_candidates: &BTreeMap<u16, CandidateRecords<'_, EdgeUse>>,
    coedge_candidates: &BTreeMap<u16, CandidateRecords<'_, Coedge>>,
) -> Result<Option<Coedge>, CodecError> {
    let mut evidence_storage = ctx.reserve_scoped(0, "collect Parasolid coedge evidence")?;
    let mut evidence = Vec::new();
    for candidate in ctx.admit_iter(candidates, "collect Parasolid coedge evidence")? {
        let found = coedge_evidence(
            ctx,
            candidate,
            loops,
            bridges,
            vertex_uses,
            edge_candidates,
            coedge_candidates,
        )?;
        ctx.push_scoped_vec(
            &mut evidence_storage,
            &mut evidence,
            found,
            "collect Parasolid coedge evidence",
        )?;
    }
    let mut maximal = None;
    let mut maximal_count = 0_usize;
    for (index, own) in ctx
        .admit_iter(&evidence, "select maximal Parasolid coedges")?
        .enumerate()
    {
        if !ctx.any_by(
            evidence.iter().enumerate(),
            |(other, other_evidence)| {
                Ok(other != index && evidence_dominates(*other_evidence, *own))
            },
            "select maximal Parasolid coedges",
        )? {
            maximal = Some(index);
            maximal_count += 1;
        }
    }
    Ok(match maximal {
        Some(index) if maximal_count == 1 => candidates.get(index).cloned(),
        _ => None,
    })
}

fn edge_candidate_is_valid(
    ctx: &DecodeContext<'_>,
    candidate: &EdgeUse,
    coedges: &BTreeMap<u16, Coedge>,
    curve_attrs: Option<&BTreeSet<u16>>,
) -> Result<bool, CodecError> {
    const OPERATION: &str = "select Parasolid edge-use readings";
    let canonical_valid = match candidate.references.canonical() {
        None | Some(0) => true,
        Some(canonical) => ctx.contains_key_btree_map(coedges, &canonical, OPERATION)?,
    };
    let curve = candidate.references.curve();
    let curve_valid = match curve_attrs {
        Some(attrs) if curve != 0 => ctx.contains_btree_set(attrs, &curve, OPERATION)?,
        _ => true,
    };
    Ok(canonical_valid && curve_valid)
}

/// Select the one valid edge-use reading at an offset. One offset yields at
/// most seventeen readings.
fn select_edge_use(
    ctx: &DecodeContext<'_>,
    candidates: &[EdgeUse],
    coedges: &BTreeMap<u16, Coedge>,
    curve_attrs: Option<&BTreeSet<u16>>,
) -> Result<Option<EdgeUse>, CodecError> {
    if candidates.len() == 1 {
        return Ok(candidates.first().cloned());
    }
    let mut valid = None;
    for candidate in ctx.admit_iter(candidates, "select Parasolid edge-use readings")? {
        if !edge_candidate_is_valid(ctx, candidate, coedges, curve_attrs)? {
            continue;
        }
        if valid.is_some() {
            return Ok(None);
        }
        valid = Some(candidate);
    }
    Ok(valid.cloned())
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
    let mut selected = None;
    for offset in 0..buf.len().saturating_sub(13) {
        if let Some(point) = parse_point(buf, offset, false) {
            if point.attr == attr {
                selected = Some(point);
            }
        }
    }
    let Some(record) = selected else {
        return false;
    };
    patch_point_values(buf, record.xyz_offset, record.xyz_m, xyz_m)
}

/// Scan the stream body for every typed topology record. Successful records do
/// not advance the scan past their extent because valid records can overlap an
/// enclosing payload. Family-specific framing gates reject payload coincidences.
/// Later full records replace earlier records with the same `attr`, matching
/// partition-base plus deltas-override merge order.
pub(crate) fn scan(ctx: &DecodeContext<'_>, body: &[u8]) -> Result<Tables, CodecError> {
    scan_with_point_framing(ctx, body, false, None, None)
}

/// Scan a partition stream while admitting typed FACE offsets that carry a
/// loop head.  A typed FACE and a compact bridge share the `00 0e` framing.
/// An ownership-only typed FACE has a null loop field and must not replace a
/// separate compact bridge for the same attribute; a shared FACE/bridge record
/// has a non-null loop field and is the topology record as well.
pub(super) fn scan_with_curve_attrs_excluding(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    curve_attrs: &BTreeSet<u16>,
    excluded_bridge_offsets: &BTreeSet<usize>,
) -> Result<Tables, CodecError> {
    scan_with_point_framing(
        ctx,
        body,
        false,
        Some(curve_attrs),
        Some(excluded_bridge_offsets),
    )
}

/// Scan a deltas stream with the typed FACE/compact-bridge overlap rule.
pub(super) fn scan_deltas_with_curve_attrs_excluding(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    curve_attrs: &BTreeSet<u16>,
    excluded_bridge_offsets: &BTreeSet<usize>,
) -> Result<Tables, CodecError> {
    scan_with_point_framing(
        ctx,
        body,
        true,
        Some(curve_attrs),
        Some(excluded_bridge_offsets),
    )
}

fn scan_with_point_framing(
    ctx: &DecodeContext<'_>,
    body: &[u8],
    prefixed_points: bool,
    curve_attrs: Option<&BTreeSet<u16>>,
    excluded_bridge_offsets: Option<&BTreeSet<usize>>,
) -> Result<Tables, CodecError> {
    let mut t = Tables::default();
    let mut storage = ctx.reserve_scoped(0, "hold Parasolid topology candidates")?;
    let mut loop_candidates = Vec::new();
    let mut edge_candidates = BTreeMap::new();
    let mut coedge_candidates = BTreeMap::new();
    let starts = 0..body.len().checked_sub(13).map_or(0, |end| end);
    for i in ctx.admit_iter(starts, "scan Parasolid typed topology")? {
        if body.get(i) != Some(&0x00) {
            continue;
        }
        match body.get(i + 1) {
            Some(0x0e) => {
                if let Some(record) = parse_bridge(body, i) {
                    let excluded = match excluded_bridge_offsets {
                        Some(offsets) => {
                            ctx.contains_btree_set(offsets, &i, "exclude typed FACE offsets")?
                        }
                        None => false,
                    };
                    let carries_loop = record.refs[2] > 1;
                    if !excluded || carries_loop {
                        t.insert_bridge(ctx, record)?;
                    }
                }
            }
            Some(0x0f) => {
                if let Some(record) = parse_loop(body, i) {
                    ctx.push_scoped_vec(
                        &mut storage,
                        &mut loop_candidates,
                        record,
                        "collect Parasolid topology loop candidates",
                    )?;
                }
            }
            Some(0x10) => insert_candidates(
                ctx,
                &mut storage,
                &mut edge_candidates,
                parse_edge_use_candidates(ctx, body, i)?,
            )?,
            Some(0x11) => insert_candidates(
                ctx,
                &mut storage,
                &mut coedge_candidates,
                parse_coedge_candidates(ctx, body, i)?,
            )?,
            Some(0x12) => {
                if let Some(record) = parse_vertex_use(body, i) {
                    t.insert_vertex_use(ctx, record)?;
                }
            }
            Some(0x1d) => {
                let record = if prefixed_points {
                    parse_point(body, i, true).or_else(|| parse_point(body, i, false))
                } else {
                    parse_point(body, i, false)
                };
                if let Some(record) = record {
                    t.insert_point(ctx, record)?;
                }
            }
            _ => {}
        }
    }

    // Coedge readings are compared only at an offset with several readings;
    // the loop evidence index is built for the first such offset.
    let mut loop_index = None;
    for (_, candidates) in ctx.admit_iter(&coedge_candidates, "select Parasolid coedge readings")? {
        let record = if candidates.records.len() == 1 {
            candidates.records.first().cloned()
        } else {
            let loops = match &mut loop_index {
                Some(loops) => loops,
                None => loop_index.insert(LoopIndex::new(
                    ctx,
                    &loop_candidates,
                    &t.bridges,
                    &mut storage,
                )?),
            };
            select_coedge(
                ctx,
                &candidates.records,
                loops,
                &t.bridges,
                &t.vertex_uses,
                &edge_candidates,
                &coedge_candidates,
            )?
        };
        if let Some(record) = record {
            t.insert_coedge(ctx, record)?;
        }
    }
    drop(loop_index);
    for (_, candidates) in ctx.admit_iter(&edge_candidates, "select Parasolid edge-use readings")? {
        if let Some(record) = select_edge_use(ctx, &candidates.records, &t.coedges, curve_attrs)? {
            t.insert_edge_use(ctx, record)?;
        }
    }
    for record in ctx.admit_iter(loop_candidates, "select Parasolid topology loops")? {
        let owner = record.refs[2];
        let first = record.refs[1];
        if ctx.contains_key_btree_map(&t.bridges, &owner, "select Parasolid topology loops")?
            && ctx
                .get_btree_map(&t.coedges, &first, "select Parasolid topology loops")?
                .is_some_and(|coedge| coedge.refs[1] == record.attr)
        {
            t.insert_loop(ctx, record)?;
        }
    }
    Ok(t)
}

#[cfg(test)]
mod tests {
    use super::{
        parse_bridge, parse_point, parse_vertex_use, patch_point, patch_point_values, scan,
        scan_deltas_with_curve_attrs_excluding, scan_with_curve_attrs_excluding, EdgeReferences,
        MAGIC,
    };
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_ir::topology::Sense;
    use std::collections::BTreeSet;

    #[test]
    fn topology_scan_refuses_collection_limit() {
        let bytes = topology_bridge(10, 20);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let error = scan(&ctx, &bytes)
            .err()
            .expect("bridge map exceeds collection limit");
        assert!(matches!(
            error,
            cadmpeg_core::CodecError::ResourceLimit(limit)
                if limit.dimension == ResourceDimension::CollectionItems
                    && limit.operation == "index Parasolid topology bridges"
        ));
    }

    #[test]
    fn topology_scan_refuses_work_limit() {
        let bytes = topology_bridge(10, 20);
        crate::test_support::work_refusal_at("scan Parasolid typed topology", |ctx| {
            scan(ctx, &bytes).map(|tables| tables.bridges.len())
        });
    }

    #[test]
    fn bare_point_reads_four_adjacent_references() {
        let mut bytes = vec![0, 0x1d];
        bytes.extend(60_u16.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        for reference in [0_u16, 0x0102, 0, 0] {
            bytes.extend(reference.to_be_bytes());
        }
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend(value.to_be_bytes());
        }
        let point = parse_point(&bytes, 0, false).expect("bare world point");
        assert_eq!(point.xyz_m, [1.0, 2.0, 3.0]);
        assert_eq!(point.xyz_offset, 16);
    }

    #[test]
    fn prefixed_point_reads_reference_triples() {
        let mut bytes = vec![0, 0x1d];
        bytes.extend(60_u16.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        for reference in [0_u16, 0x0102, 0, 0] {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend(value.to_be_bytes());
        }
        let point = parse_point(&bytes, 0, true).expect("prefixed world point");
        assert_eq!(point.xyz_m, [1.0, 2.0, 3.0]);
        assert_eq!(point.xyz_offset, 20);
        bytes[8] = 2;
        assert!(parse_point(&bytes, 0, true).is_none());
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
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let body = bridge_with_refs(&[1, 2, 3, 7, 8], false);
        let excluded = BTreeSet::from([0]);

        let tables = scan_with_curve_attrs_excluding(&ctx, &body, &BTreeSet::new(), &excluded)
            .expect("topology scan");

        assert_eq!(
            tables.bridges.get(&0x1234).map(|record| record.refs[2]),
            Some(3)
        );
    }

    #[test]
    fn ownership_only_typed_face_does_not_replace_a_compact_bridge() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let body = bridge_with_refs(&[1, 2, 0, 7, 8], false);
        let excluded = BTreeSet::from([0]);

        let tables = scan_with_curve_attrs_excluding(&ctx, &body, &BTreeSet::new(), &excluded)
            .expect("topology scan");

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

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let tables = scan(&ctx, &body).expect("topology scan");
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

        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
        let tables = scan(&ctx, &body).expect("topology scan");
        assert_eq!(tables.edge_uses[&40].sequence, 0x0102_0304);
        assert_eq!(tables.vertex_uses[&50].sequence, 0x0506_0708);
    }

    #[test]
    fn suffix_edge_use_frame_wins_when_first_reference_starts_with_one() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &[],
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .unwrap();
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
            scan(&ctx, &bytes)
                .expect("topology scan")
                .edge_uses
                .is_empty(),
            "ambiguous without a carrier set"
        );
        let curve_attrs = BTreeSet::from([0x0103]);
        let tables =
            scan_deltas_with_curve_attrs_excluding(&ctx, &bytes, &curve_attrs, &BTreeSet::new())
                .expect("topology scan");
        assert_eq!(tables.edge_uses[&40].references.curve(), 0x0103);
    }

    #[test]
    fn patch_point_selects_the_last_matching_record() {
        let mut bytes = crate::test_support::parasolid::world_point(60, [1.0, 2.0, 3.0]);
        let last = bytes.len();
        bytes.extend(crate::test_support::parasolid::world_point(
            60,
            [7.0, 8.0, 9.0],
        ));
        assert!(patch_point(&mut bytes, 60, [4.0, 5.0, 6.0]));
        assert_eq!(
            parse_point(&bytes, 0, false).unwrap().xyz_m,
            [1.0, 2.0, 3.0]
        );
        assert_eq!(
            parse_point(&bytes, last, false).unwrap().xyz_m,
            [4.0, 5.0, 6.0]
        );
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
        assert_eq!(point.xyz_m, [4.0, 5.0, 6.0]);
        assert_eq!(point.xyz_offset, 16);
    }

    #[test]
    fn edge_candidate_storage_refusal_preserves_the_scoped_fuse() {
        let bytes = crate::test_support::parasolid::edge_use(40, 70);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).expect("root");
        let Err(cadmpeg_core::CodecError::ResourceLimit(limit)) =
            super::parse_edge_use_candidates(&ctx, &bytes, 0)
        else {
            panic!("candidate backing must refuse scoped storage");
        };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.operation, "collect SLDPRT decoded vector items");
        assert!(limit.additional > 0);
        assert_eq!(ctx.resource_refusal(), Some(limit));

        let arena = DecodeArena::new();
        let (ctx, _) =
            DecodeContext::from_root_bytes(&bytes, &arena, &DecodePolicy::service()).expect("root");
        let candidates =
            super::parse_edge_use_candidates(&ctx, &bytes, 0).expect("service candidates");
        assert_eq!(candidates.records.len(), 1);
        assert_eq!(candidates.records[0].attr, 40);
        assert_eq!(
            candidates.records[0].references,
            EdgeReferences::Bare([0, 0, 0, 70, 0, 0])
        );
    }

    #[test]
    fn candidate_replacement_releases_the_previous_scoped_backing() {
        let first = crate::test_support::parasolid::edge_use(40, 70);
        let mut replacement = vec![0xff];
        replacement.extend(crate::test_support::parasolid::edge_use(40, 80));
        let third = crate::test_support::parasolid::edge_use(41, 90);
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let capacity = 1 << 16;
        policy.limits.max_materialized_bytes = capacity;
        let (ctx, _) = DecodeContext::from_root_bytes(&first, &arena, &policy).expect("root");
        let mut storage = ctx
            .reserve_scoped(0, "test candidate index")
            .expect("storage");
        let mut candidates = std::collections::BTreeMap::new();
        super::insert_candidates(
            &ctx,
            &mut storage,
            &mut candidates,
            super::parse_edge_use_candidates(&ctx, &first, 0).expect("first candidates"),
        )
        .expect("first transfer");
        super::insert_candidates(
            &ctx,
            &mut storage,
            &mut candidates,
            super::parse_edge_use_candidates(&ctx, &replacement, 1)
                .expect("replacement candidates"),
        )
        .expect("replacement transfer");
        super::insert_candidates(
            &ctx,
            &mut storage,
            &mut candidates,
            super::parse_edge_use_candidates(&ctx, &third, 0).expect("third candidates"),
        )
        .expect("third transfer");
        assert_eq!(candidates[&40].records[0].offset, 1);
        assert_eq!(
            candidates[&40].records[0].references,
            EdgeReferences::Bare([0, 0, 0, 80, 0, 0])
        );
        assert_eq!(
            candidates[&41].records[0].references,
            EdgeReferences::Bare([0, 0, 0, 90, 0, 0])
        );
        drop((candidates, storage));
        let _storage = ctx
            .reserve_scoped(capacity, "verify released candidate storage")
            .expect("dropping the map releases both candidate vectors");
        assert_eq!(ctx.resource_refusal(), None);
    }
}
