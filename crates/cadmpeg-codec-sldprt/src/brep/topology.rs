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

use std::collections::HashMap;

use super::{f64_be, u16_be};

/// The magic anchoring magic-bearing topology records ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
pub const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

/// A parsed topology record. Only the fields the chain walk needs are kept.
#[derive(Debug, Clone)]
pub struct Record {
    pub attr: u16,
    /// Big-endian `refs` array (length varies by family).
    pub refs: Vec<u16>,
    /// Orientation marker (`0x2b` forward / `0x2d` reversed), when the family
    /// carries one.
    pub marker: Option<u8>,
    /// World-point coordinates in metres, for `00 1d` only.
    pub xyz_m: Option<[f64; 3]>,
    /// Owning entity reference carried by bridge records.
    pub owner: Option<u16>,
    /// Byte offset of the record's tag within the stream body.
    pub offset: usize,
}

/// Read `count` big-endian u16 refs starting at `at`.
fn refs_be(buf: &[u8], at: usize, count: usize) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(u16_be(buf, at + 2 * i)?);
    }
    Some(out)
}

fn refs_tripled(buf: &[u8], at: usize, count: usize) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let p = at + index * 3;
        if buf.get(p + 2) != Some(&1) {
            return None;
        }
        out.push(u16_be(buf, p)?);
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
    let a = u16_be(buf, p)?;
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
fn parse_bridge(buf: &[u8], off: usize) -> Option<Record> {
    let p = body_start(buf, off, 0x0e)?;
    if buf.get(p + 8) == Some(&1) && buf.get(p + 9..p + 17) == Some(MAGIC.as_slice()) {
        let attr = attr_at(buf, p)?;
        let owner = u16_be(buf, p + 6)?;
        let refs = refs_tripled(buf, p + 17, 5)?;
        let marker = *buf.get(p + 32)?;
        if marker != 0x2b && marker != 0x2d {
            return None;
        }
        return Some(Record {
            attr,
            refs,
            marker: Some(marker),
            xyz_m: None,
            owner: (owner > 1).then_some(owner),
            offset: off,
        });
    }
    if p + 37 > buf.len() || buf.get(p + 8..p + 16)? != MAGIC {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let owner = u16_be(buf, p + 6)?;
    let tripled = (0..5).all(|index| buf.get(p + 18 + index * 3) == Some(&1));
    let (refs, marker) = if tripled {
        (refs_tripled(buf, p + 16, 5)?, *buf.get(p + 31)?)
    } else {
        (refs_be(buf, p + 16, 5)?, *buf.get(p + 26)?)
    };
    if marker != 0x2b && marker != 0x2d {
        return None;
    }
    Some(Record {
        attr,
        refs,
        marker: Some(marker),
        xyz_m: None,
        owner: (owner > 1).then_some(owner),
        offset: off,
    })
}

/// Loop head `00 0f`: minimal 14-byte body, no magic, `refs[4]` at body+6.
/// `refs[1]` = first coedge, `refs[2]` = owning bridge, `refs[3]` = next sibling.
fn parse_loop(buf: &[u8], off: usize) -> Option<Record> {
    let p = body_start(buf, off, 0x0f)?;
    if p + 14 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let refs = refs_tripled(buf, p + 6, 4).or_else(|| refs_be(buf, p + 6, 4))?;
    Some(Record {
        attr,
        refs,
        marker: None,
        xyz_m: None,
        owner: None,
        offset: off,
    })
}

/// Edge-use `00 10`: 28-byte body, magic at body+8, `refs[6]` at body+16.
/// `refs[3]` = support curve carrier.
fn parse_edge_use(buf: &[u8], off: usize) -> Option<Record> {
    let p = body_start(buf, off, 0x10)?;
    if p + 28 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let refs = if buf.get(p + 8..p + 16) == Some(MAGIC.as_slice()) {
        refs_be(buf, p + 16, 6)?
    } else {
        let magic = (p + 9..=(p + 16).min(buf.len().saturating_sub(MAGIC.len())))
            .find(|at| buf.get(*at..*at + MAGIC.len()) == Some(MAGIC.as_slice()))?;
        let mut decoded = Vec::new();
        let mut q = magic + MAGIC.len();
        if buf.get(q) == Some(&1) {
            // `[01][hi][lo]` triples.
            while buf.get(q) == Some(&1) && decoded.len() < 8 {
                decoded.push(u16_be(buf, q + 1)?);
                q += 3;
            }
        } else {
            // `[hi][lo][01]` triples.
            while buf.get(q + 2) == Some(&1) && decoded.len() < 8 {
                decoded.push(u16_be(buf, q)?);
                q += 3;
            }
        }
        if decoded.len() < 3 {
            return None;
        }
        let mut refs = vec![0; 6];
        refs[3] = decoded[2];
        refs
    };
    Some(Record {
        attr,
        refs,
        marker: None,
        xyz_m: None,
        owner: None,
        offset: off,
    })
}

/// Coedge `00 11`: 21-byte body, no magic, `refs[9]` at body+2, marker at
/// body+20. `refs[1]` = owning loop, `refs[3]` = next coedge, `refs[4]` = start
/// vertex-use, `refs[5]` = twin coedge, `refs[6]` = edge-use.
fn parse_coedge(buf: &[u8], off: usize) -> Option<Record> {
    let p = body_start(buf, off, 0x11)?;
    if p + 21 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let (refs, marker) =
        if let (Some(refs), Some(marker)) = (refs_be(buf, p + 2, 9), buf.get(p + 20).copied()) {
            if matches!(marker, 0x2b | 0x2d) {
                (refs, marker)
            } else {
                (refs_tripled(buf, p + 2, 9)?, *buf.get(p + 29)?)
            }
        } else {
            (refs_tripled(buf, p + 2, 9)?, *buf.get(p + 29)?)
        };
    if marker != 0x2b && marker != 0x2d {
        return None;
    }
    Some(Record {
        attr,
        refs,
        marker: Some(marker),
        xyz_m: None,
        owner: None,
        offset: off,
    })
}

/// Vertex-use `00 12`: 24-byte body, magic at body+16, `refs[5]` at body+6.
/// `refs[4]` = world-point attr.
fn parse_vertex_use(buf: &[u8], off: usize) -> Option<Record> {
    let p = body_start(buf, off, 0x12)?;
    if p + 24 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let refs = if buf.get(p + 16..p + 24) == Some(MAGIC.as_slice()) {
        refs_be(buf, p + 6, 5)?
    } else {
        let magic = (p + 21..=(p + 32).min(buf.len().saturating_sub(MAGIC.len())))
            .find(|at| buf.get(*at..*at + MAGIC.len()) == Some(MAGIC.as_slice()))?;
        let count = (magic.checked_sub(p + 6)?) / 3;
        if count < 5 || p + 6 + count * 3 != magic {
            return None;
        }
        refs_tripled(buf, p + 6, count)?
    };
    Some(Record {
        attr,
        refs,
        marker: None,
        xyz_m: None,
        owner: None,
        offset: off,
    })
}

/// World point `00 1d`: 38-byte body, no magic, `refs[4]` at body+6, xyz as
/// three big-endian f64 (metres) at body+14.
fn parse_point(buf: &[u8], off: usize, prefixed: bool) -> Option<Record> {
    let p = body_start(buf, off, 0x1d)?;
    if p + 38 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let (refs, xyz_at) = if prefixed {
        let mut refs = Vec::new();
        let mut cursor = p + 6;
        while buf.get(cursor + 2) == Some(&1) && refs.len() < 16 {
            refs.push(u16_be(buf, cursor)?);
            cursor += 3;
        }
        if refs.is_empty() {
            return None;
        }
        (refs, cursor)
    } else {
        (refs_be(buf, p + 6, 4)?, p + 14)
    };
    if refs.first().is_none_or(|reference| *reference > 1) {
        return None;
    }
    let x = f64_be(buf, xyz_at)?;
    let y = f64_be(buf, xyz_at + 8)?;
    let z = f64_be(buf, xyz_at + 16)?;
    for v in [x, y, z] {
        // Reject exponent-poisoned reads from a misaligned candidate: real part
        // coordinates in metres sit well under this cap.
        if !v.is_finite() || v.abs() > 1e4 {
            return None;
        }
    }
    Some(Record {
        attr,
        refs,
        marker: None,
        xyz_m: Some([x, y, z]),
        owner: None,
        offset: off,
    })
}

/// The topology record tables of one stream, each keyed by `attr`.
#[derive(Default)]
pub struct Tables {
    pub bridges: HashMap<u16, Record>,
    pub loops: HashMap<u16, Record>,
    pub edge_uses: HashMap<u16, Record>,
    pub coedges: HashMap<u16, Record>,
    pub vertex_uses: HashMap<u16, Record>,
    pub points: HashMap<u16, Record>,
}

impl Tables {
    /// Merge a deltas table without replacing partition topology membership.
    pub fn merge_deltas(&mut self, mut deltas: Self) {
        if self.bridges.is_empty() {
            self.bridges = deltas.bridges;
        }
        merge_missing(&mut self.loops, deltas.loops);
        merge_missing(&mut self.edge_uses, deltas.edge_uses);
        merge_missing(&mut self.coedges, deltas.coedges);
        merge_missing(&mut self.vertex_uses, deltas.vertex_uses);
        self.points.extend(deltas.points.drain());
    }
}

fn merge_missing(target: &mut HashMap<u16, Record>, source: HashMap<u16, Record>) {
    for (attr, record) in source {
        target.entry(attr).or_insert(record);
    }
}

/// Replace one world-point record while preserving its framing.
pub(crate) fn patch_point(buf: &mut [u8], attr: u16, xyz_m: [f64; 3]) -> bool {
    let Some(record) = scan(buf).points.remove(&attr) else {
        return false;
    };
    let Some(p) = body_start(buf, record.offset, 0x1d) else {
        return false;
    };
    let mut xyz_at = p + 14;
    let mut cursor = p + 6;
    while buf.get(cursor + 2) == Some(&1) && cursor < p + 54 {
        cursor += 3;
    }
    if cursor != p + 6 {
        xyz_at = cursor;
    }
    let Some(bytes) = buf.get_mut(xyz_at..xyz_at + 24) else {
        return false;
    };
    for (slot, value) in bytes.chunks_exact_mut(8).zip(xyz_m) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    true
}

/// Scan the stream body for every typed topology record. Successful records do
/// not advance the scan past their extent because valid records can overlap an
/// enclosing payload. Family-specific framing gates reject payload coincidences.
/// Later full records replace earlier records with the same `attr`, matching
/// partition-base plus deltas-override merge order.
pub fn scan(body: &[u8]) -> Tables {
    scan_with_point_framing(body, false)
}

/// Scan a deltas stream whose world-point reference lanes use prefixed triples.
pub fn scan_deltas(body: &[u8]) -> Tables {
    scan_with_point_framing(body, true)
}

fn scan_with_point_framing(body: &[u8], prefixed_points: bool) -> Tables {
    let mut t = Tables::default();
    let mut loop_candidates = Vec::new();
    let mut i = 0usize;
    while i + 14 <= body.len() {
        if body[i] != 0x00 {
            i += 1;
            continue;
        }
        let (rec, table): (Option<Record>, Option<&mut HashMap<u16, Record>>) = match body[i + 1] {
            0x0e => (parse_bridge(body, i), Some(&mut t.bridges)),
            0x0f => {
                if let Some(record) = parse_loop(body, i) {
                    loop_candidates.push(record);
                }
                (None, None)
            }
            0x10 => (parse_edge_use(body, i), Some(&mut t.edge_uses)),
            0x11 => (parse_coedge(body, i), Some(&mut t.coedges)),
            0x12 => (parse_vertex_use(body, i), Some(&mut t.vertex_uses)),
            0x1d => (
                if prefixed_points {
                    parse_point(body, i, true).or_else(|| parse_point(body, i, false))
                } else {
                    parse_point(body, i, false)
                },
                Some(&mut t.points),
            ),
            _ => (None, None),
        };
        match (rec, table) {
            (Some(r), Some(map)) => {
                map.insert(r.attr, r);
                i += 1;
            }
            _ => i += 1,
        }
    }
    for record in loop_candidates {
        let owner = record.refs.get(2).copied().unwrap_or(0);
        let first = record.refs.get(1).copied().unwrap_or(0);
        if t.bridges.contains_key(&owner)
            && t.coedges
                .get(&first)
                .is_some_and(|coedge| coedge.refs.get(1) == Some(&record.attr))
        {
            t.loops.insert(record.attr, record);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(bridge.owner, Some(0x4321));
        assert_eq!(bridge.refs, expected);
        assert_eq!(bridge.marker, Some(0x2b));
    }

    #[test]
    fn bridge_refs_accept_adjacent_and_tripled_cells() {
        let expected = vec![0x101, 0x202, 0x303, 0x404, 0x505];
        for tripled in [false, true] {
            let bytes = bridge_with_refs(&expected, tripled);
            let bridge = parse_bridge(&bytes, 0)
                .unwrap_or_else(|| panic!("bridge tripled={tripled} bytes={bytes:02x?}"));
            assert_eq!(bridge.attr, 0x1234);
            assert_eq!(bridge.owner, Some(0x4321));
            assert_eq!(bridge.refs, expected);
            assert_eq!(bridge.marker, Some(0x2d));
        }
    }
}
