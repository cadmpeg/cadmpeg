// SPDX-License-Identifier: Apache-2.0
//! Plain-schema 13006 partition and Deltas streams built from POINT, GROUP,
//! and LIMIT nodes, for main-line replay tests.

use crate::pointer;
use crate::records::node_type::{GROUP, LIMIT, POINT};

const SCHEMA: &[u8] = b"SCH_1_13006";

/// One POINT or GROUP node. `value` is the POINT's `pvec` x or the GROUP's
/// `type` byte, so two images of one node can differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TestNode {
    /// Node type, POINT or GROUP.
    pub kind: u16,
    /// Node index.
    pub xmt: u32,
    /// Index the `next` pointer names; `1` is null.
    pub next: u32,
    /// Distinguishing value of the image.
    pub value: u8,
}

impl TestNode {
    /// A POINT node.
    #[must_use]
    pub const fn point(xmt: u32, next: u32, value: u8) -> Self {
        Self {
            kind: POINT,
            xmt,
            next,
            value,
        }
    }

    /// A GROUP node.
    #[must_use]
    pub const fn group(xmt: u32, next: u32) -> Self {
        Self {
            kind: GROUP,
            xmt,
            next,
            value: 0,
        }
    }

    fn write(self, out: &mut Vec<u8>, status: bool) {
        let reference = |out: &mut Vec<u8>, value: u32| {
            pointer::write(out, value).unwrap_or_default();
            if status {
                out.push(0);
            }
        };
        out.extend(self.kind.to_be_bytes());
        pointer::write(out, self.xmt).unwrap_or_default();
        out.extend(self.xmt.to_be_bytes());
        for target in [1, 1, self.next, 1] {
            reference(out, target);
        }
        if self.kind == GROUP {
            out.push(self.value);
            reference(out, 1);
        } else {
            for coordinate in [f64::from(self.value), 0.0, 0.0] {
                out.extend(coordinate.to_be_bytes());
            }
        }
    }
}

/// One LIMIT node. A partition stores each Hvec row as `(x, y, z)`; a Deltas
/// stream stores eight more reals after them.
#[derive(Debug, Clone, PartialEq)]
pub struct TestLimit {
    /// Node index.
    pub xmt: u32,
    /// The `(x, y, z)` of each Hvec row.
    pub rows: Vec<[f64; 3]>,
}

impl TestLimit {
    fn write(&self, out: &mut Vec<u8>, deltas: bool) {
        out.extend(LIMIT.to_be_bytes());
        out.extend(
            u32::try_from(self.rows.len())
                .unwrap_or(u32::MAX)
                .to_be_bytes(),
        );
        pointer::write(out, self.xmt).unwrap_or_default();
        out.push(b'T');
        for row in &self.rows {
            for value in row {
                out.extend(value.to_be_bytes());
            }
            if deltas {
                for extra in 0..8_u8 {
                    out.extend(f64::from(extra).to_be_bytes());
                }
            }
        }
    }
}

fn header(description: &[u8]) -> Vec<u8> {
    let mut bytes = b"PS".to_vec();
    for text in [description, SCHEMA] {
        bytes.extend(u32::try_from(text.len()).unwrap_or(u32::MAX).to_be_bytes());
        bytes.extend(text);
    }
    bytes.extend(0_u32.to_be_bytes());
    bytes
}

/// A partition stream holding `nodes`.
#[must_use]
pub fn partition(nodes: &[TestNode]) -> Vec<u8> {
    let mut bytes = header(b": TRANSMIT FILE (partition) created by test");
    for node in nodes {
        node.write(&mut bytes, false);
    }
    bytes.extend([0, 1, 0, 1]);
    bytes
}

/// One PMARK.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    /// Node index.
    pub xmt: u32,
    /// Index of the preceding mark; `1` is null.
    pub preceding: u32,
    /// Persistent identifier.
    pub identifier: u32,
}

impl Mark {
    fn write(self, out: &mut Vec<u8>) {
        out.extend(4_u16.to_be_bytes());
        for value in [self.xmt, self.preceding, 1, 1, 1] {
            pointer::write(out, value).unwrap_or_default();
        }
        for value in [0_u32; 4] {
            out.extend(value.to_be_bytes());
        }
        out.push(0);
        out.extend(self.identifier.to_be_bytes());
    }
}

/// A Deltas stream under construction, section by section.
#[derive(Debug, Clone)]
pub struct DeltasStream(Vec<u8>);

impl DeltasStream {
    /// The header and the partition prefix naming `current`.
    #[must_use]
    pub fn new(current: Mark) -> Self {
        let mut bytes = header(b": TRANSMIT FILE (deltas) created by test");
        bytes.extend(3_u16.to_be_bytes());
        pointer::write(&mut bytes, 2).unwrap_or_default();
        pointer::write(&mut bytes, current.xmt).unwrap_or_default();
        bytes.extend(current.identifier.to_be_bytes());
        current.write(&mut bytes);
        Self(bytes)
    }

    /// Status-framed images of `nodes`.
    #[must_use]
    pub fn nodes(mut self, nodes: &[TestNode]) -> Self {
        for node in nodes {
            node.write(&mut self.0, true);
        }
        self
    }

    /// Deltas images of `limits`.
    #[must_use]
    pub fn limits(mut self, limits: &[TestLimit]) -> Self {
        for limit in limits {
            limit.write(&mut self.0, true);
        }
        self
    }

    /// A `00 01 00 01` terminator.
    #[must_use]
    pub fn terminator(mut self) -> Self {
        self.0.extend([0, 1, 0, 1]);
        self
    }

    /// A map prefix of `(index, kind)` entries and its terminal bytes.
    #[must_use]
    pub fn map(mut self, entries: &[(u32, u16)]) -> Self {
        for (xmt, kind) in entries {
            pointer::write(&mut self.0, *xmt).unwrap_or_default();
            self.0.extend(kind.to_be_bytes());
        }
        self.0.extend([0, 1, 0, 0, 0, 0]);
        self
    }

    /// The PMARK nodes that head a PMARK section.
    #[must_use]
    pub fn marks(mut self, marks: &[Mark]) -> Self {
        for mark in marks {
            mark.write(&mut self.0);
        }
        self
    }

    /// The delta content after a section head: restored images, a
    /// terminator, the map of created nodes, older images, a terminator.
    #[must_use]
    pub fn delta(self, restored: &[TestNode], created: &[(u32, u16)], older: &[TestNode]) -> Self {
        self.nodes(restored)
            .terminator()
            .map(created)
            .nodes(older)
            .terminator()
    }

    /// The stream bytes.
    #[must_use]
    pub fn finish(self) -> Vec<u8> {
        self.0
    }
}
