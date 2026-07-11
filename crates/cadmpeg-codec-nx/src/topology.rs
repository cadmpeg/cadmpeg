// SPDX-License-Identifier: Apache-2.0
//! Parse supported fixed-record Parasolid topology.
//!
//! [`Graph`] indexes records by type and stream-scoped XMT identifier. Record
//! offsets connect nodes to carriers returned by [`crate::geometry`] and
//! [`crate::nurbs`]. The parser covers the fixed-record families used by the
//! crate's B-rep reconstruction; unsupported framing and record types are absent
//! from the graph.

use std::collections::BTreeMap;

/// A supported fixed-record node with its XMT identifier and source offset.
#[derive(Debug, Clone)]
pub struct Node {
    /// Parasolid node type.
    pub kind: u8,
    /// Stream-scoped XMT identifier.
    pub xmt: u32,
    /// Record type-tag offset in the inflated stream.
    pub pos: usize,
    shift: usize,
    bytes: Vec<u8>,
}

impl Node {
    /// Read an XMT reference at a logical record offset.
    pub fn xmt_at(&self, offset: usize) -> Option<u32> {
        read_xmt(&self.bytes, offset + self.shift).map(|(xmt, _)| xmt)
    }

    /// Read adjacent XMT references, accounting for extended encodings.
    pub fn xmt_sequence(&self, offset: usize, count: usize) -> Option<Vec<u32>> {
        let mut at = offset + self.shift;
        let mut values = Vec::with_capacity(count);
        for _ in 0..count {
            let (value, extra) = read_xmt(&self.bytes, at)?;
            values.push(value);
            at += 2 + extra;
        }
        Some(values)
    }

    /// Read a byte at its logical record offset.
    pub fn byte_at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(offset + self.shift).copied()
    }

    /// Read a big-endian floating-point field at its logical record offset.
    pub fn f64_at(&self, offset: usize) -> Option<f64> {
        let at = offset + self.shift;
        let bytes: [u8; 8] = self.bytes.get(at..at + 8)?.try_into().ok()?;
        Some(f64::from_be_bytes(bytes))
    }
}

/// An index of supported records keyed by `(node type, XMT identifier)`.
#[derive(Debug, Default)]
pub struct Graph {
    nodes: BTreeMap<(u8, u32), Node>,
    by_pos: BTreeMap<usize, (u8, u32)>,
}

/// A type-133 parameter restriction over a basis curve.
#[derive(Debug, Clone, Copy)]
pub struct TrimmedCurve {
    /// Cross-reference index (XMT) of the tag-133 record.
    pub xmt: u32,
    /// Cross-reference index of the untrimmed basis curve record.
    pub basis: u32,
    /// `[start, end]` parameter range of the trim, in the basis curve's own parameterization.
    pub parameters: [f64; 2],
}

/// Decode supported type-133 trimmed-curve records.
///
/// The result retains the basis-curve reference and parameter range. Topological
/// endpoints come from the corresponding edge and vertex records.
pub fn trimmed_curves(stream: &[u8]) -> Vec<TrimmedCurve> {
    Graph::parse(stream)
        .of_kind(133)
        .filter_map(|node| {
            let basis = node.xmt_at(19)?;
            let p0 = node.bytes.get(69 + node.shift..77 + node.shift)?;
            let p1 = node.bytes.get(77 + node.shift..85 + node.shift)?;
            let p0 = f64::from_be_bytes(p0.try_into().ok()?);
            let p1 = f64::from_be_bytes(p1.try_into().ok()?);
            (basis > 1 && p0.is_finite() && p1.is_finite()).then_some(TrimmedCurve {
                xmt: node.xmt,
                basis,
                parameters: [p0, p1],
            })
        })
        .collect()
}

impl Graph {
    /// Parse supported fixed-record nodes from a neutral-binary stream.
    pub fn parse(stream: &[u8]) -> Self {
        let mut graph = Self::default();
        for pos in 0..stream.len().saturating_sub(3) {
            if stream[pos] != 0 {
                continue;
            }
            let kind = stream[pos + 1];
            let Some(len) = fixed_len(kind) else {
                continue;
            };
            let Some((xmt, shift)) = read_xmt(stream, pos + 2) else {
                continue;
            };
            // 1 is Parasolid's null reference. A node itself cannot occupy it.
            if xmt <= 1 {
                continue;
            }
            let Some(bytes) = stream.get(pos..pos + len + shift) else {
                continue;
            };
            let node = Node {
                kind,
                xmt,
                pos,
                shift,
                bytes: bytes.to_vec(),
            };
            graph.by_pos.insert(pos, (kind, xmt));
            graph.nodes.entry((kind, xmt)).or_insert(node);
        }
        graph
    }

    /// Look up a node by record type and XMT identifier.
    pub fn get(&self, kind: u8, xmt: u32) -> Option<&Node> {
        self.nodes.get(&(kind, xmt))
    }

    /// Look up the node whose type tag starts at `pos`.
    pub fn at_pos(&self, pos: usize) -> Option<&Node> {
        let &(kind, xmt) = self.by_pos.get(&pos)?;
        self.get(kind, xmt)
    }

    /// Iterate nodes of one record type.
    pub fn of_kind(&self, kind: u8) -> impl Iterator<Item = &Node> {
        self.nodes.values().filter(move |node| node.kind == kind)
    }
}

/// Decode the compact and extended XMT forms. The extended form uses a negative
/// signed remainder followed by a quotient: `quotient * 32767 + remainder`.
fn read_xmt(stream: &[u8], at: usize) -> Option<(u32, usize)> {
    let first = i16::from_be_bytes([*stream.get(at)?, *stream.get(at + 1)?]);
    if first >= 0 {
        return Some((first as u32, 0));
    }
    let remainder = first.unsigned_abs();
    let quotient = u16::from_be_bytes([*stream.get(at + 2)?, *stream.get(at + 3)?]);
    let value = u32::from(quotient) * 32_767 + u32::from(remainder);
    Some((value, 2))
}

fn fixed_len(kind: u8) -> Option<usize> {
    Some(match kind {
        12 | 13 => 24,
        14 => 39,
        15 => 16,
        16 => 32,
        17 => 23,
        18 => 28,
        19 => 16,
        29 => 40,
        30 => 67,
        31 => 99,
        32 => 107,
        50 => 91,
        51 => 99,
        52 => 115,
        53 => 99,
        54 => 107,
        124 | 134 => 23,
        133 => 85,
        _ => return None,
    })
}
