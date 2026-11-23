// SPDX-License-Identifier: Apache-2.0
//! 13006 partition and Deltas streams built from POINT, GROUP, LIMIT, and
//! field-less nodes, for main-line replay tests. A stream uses the plain base
//! schema, or an embedded schema whose first node of each type declares its
//! field list.

use std::collections::BTreeSet;

use crate::pointer;
use crate::records::node_type::{GROUP, LIMIT, POINT};

const SCHEMA: &[u8] = b"SCH_1_13006";
const EMBEDDED_SCHEMA: &[u8] = b"SCH_1_1_13006";
/// The maximum node type an embedded-schema header declares.
const EMBEDDED_MAXIMUM: u16 = 250;
/// The first-use declaration of a type's base field list.
const BASE_DECLARATION: &[u8] = &[0xff];
/// The first-use declaration of an empty field list: count `0`, then `Z`.
const EMPTY_DECLARATION: &[u8] = &[0, b'Z'];

/// The declarations an embedded-schema stream has written.
#[derive(Debug, Clone, Default)]
struct Declared(Option<BTreeSet<u16>>);

impl Declared {
    fn embedded() -> Self {
        Self(Some(BTreeSet::new()))
    }

    /// Write `declaration` when the stream is embedded and `kind` is new.
    fn write(&mut self, out: &mut Vec<u8>, kind: u16, declaration: &[u8]) {
        if self
            .0
            .as_mut()
            .is_some_and(|declared| declared.insert(kind))
        {
            out.extend_from_slice(declaration);
        }
    }
}

/// One POINT or GROUP node, or a node of any type with no field. `value` is
/// the POINT's `pvec` x or the GROUP's `type` byte, so two images of one node
/// can differ.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TestNode {
    /// Node type: POINT or GROUP, or any type for a bare node.
    pub kind: u16,
    /// Node index.
    pub xmt: u32,
    /// Index the `next` pointer names; `1` is null.
    pub next: u32,
    /// Distinguishing value of the image.
    pub value: u8,
    /// Whether the node stores only its identity. Its type's first use
    /// declares an empty field list, so it belongs in an embedded-schema
    /// stream.
    pub bare: bool,
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
            bare: false,
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
            bare: false,
        }
    }

    /// A node of type `kind` that stores only its identity.
    #[must_use]
    pub const fn bare(kind: u16, xmt: u32) -> Self {
        Self {
            kind,
            xmt,
            next: 1,
            value: 0,
            bare: true,
        }
    }

    fn write(self, out: &mut Vec<u8>, status: bool, declared: &mut Declared) {
        let reference = |out: &mut Vec<u8>, value: u32| {
            pointer::write(out, value).unwrap_or_default();
            if status {
                out.push(0);
            }
        };
        out.extend(self.kind.to_be_bytes());
        let declaration = if self.bare {
            EMPTY_DECLARATION
        } else {
            BASE_DECLARATION
        };
        declared.write(out, self.kind, declaration);
        pointer::write(out, self.xmt).unwrap_or_default();
        if self.bare {
            return;
        }
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
    fn write(&self, out: &mut Vec<u8>, deltas: bool, declared: &mut Declared) {
        out.extend(LIMIT.to_be_bytes());
        declared.write(out, LIMIT, BASE_DECLARATION);
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

fn header(description: &[u8], declared: &Declared) -> Vec<u8> {
    let embedded = declared.0.is_some();
    let schema = if embedded { EMBEDDED_SCHEMA } else { SCHEMA };
    let mut bytes = b"PS".to_vec();
    for text in [description, schema] {
        bytes.extend(u32::try_from(text.len()).unwrap_or(u32::MAX).to_be_bytes());
        bytes.extend(text);
    }
    if embedded {
        bytes.extend(EMBEDDED_MAXIMUM.to_be_bytes());
    }
    bytes.extend(0_u32.to_be_bytes());
    bytes
}

fn partition_with(nodes: &[TestNode], mut declared: Declared) -> Vec<u8> {
    let mut bytes = header(b": TRANSMIT FILE (partition) created by test", &declared);
    for node in nodes {
        node.write(&mut bytes, false, &mut declared);
    }
    bytes.extend([0, 1, 0, 1]);
    bytes
}

/// A plain-schema partition stream holding `nodes` in stream order.
#[must_use]
pub fn partition(nodes: &[TestNode]) -> Vec<u8> {
    partition_with(nodes, Declared::default())
}

/// An embedded-schema partition stream holding `nodes` in stream order.
#[must_use]
pub fn embedded_partition(nodes: &[TestNode]) -> Vec<u8> {
    partition_with(nodes, Declared::embedded())
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
    fn write(self, out: &mut Vec<u8>, forward: bool) {
        out.extend(4_u16.to_be_bytes());
        self.write_fields(out, forward);
    }

    fn write_fields(self, out: &mut Vec<u8>, forward: bool) {
        for value in [self.xmt, self.preceding, 1, 1, 1] {
            pointer::write(out, value).unwrap_or_default();
        }
        for value in [0_u32; 4] {
            out.extend(value.to_be_bytes());
        }
        out.push(u8::from(forward));
        out.extend(self.identifier.to_be_bytes());
    }
}

/// A Deltas stream under construction, section by section.
#[derive(Debug, Clone)]
pub struct DeltasStream(Vec<u8>, Declared);

impl DeltasStream {
    /// The plain-schema header and the partition prefix naming `current`.
    #[must_use]
    pub fn new(current: Mark) -> Self {
        Self::with(current, Declared::default())
    }

    /// The embedded-schema header and the partition prefix naming `current`.
    #[must_use]
    pub fn embedded(current: Mark) -> Self {
        Self::with(current, Declared::embedded())
    }

    fn with(current: Mark, mut declared: Declared) -> Self {
        let mut bytes = header(b": TRANSMIT FILE (deltas) created by test", &declared);
        bytes.extend(3_u16.to_be_bytes());
        declared.write(&mut bytes, 3, BASE_DECLARATION);
        pointer::write(&mut bytes, 2).unwrap_or_default();
        pointer::write(&mut bytes, current.xmt).unwrap_or_default();
        bytes.extend(current.identifier.to_be_bytes());
        bytes.extend(4_u16.to_be_bytes());
        declared.write(&mut bytes, 4, BASE_DECLARATION);
        current.write_fields(&mut bytes, false);
        Self(bytes, declared)
    }

    /// Status-framed images of `nodes`.
    #[must_use]
    pub fn nodes(mut self, nodes: &[TestNode]) -> Self {
        for node in nodes {
            node.write(&mut self.0, true, &mut self.1);
        }
        self
    }

    /// Deltas images of `limits`.
    #[must_use]
    pub fn limits(mut self, limits: &[TestLimit]) -> Self {
        for limit in limits {
            limit.write(&mut self.0, true, &mut self.1);
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
            mark.write(&mut self.0, false);
        }
        self
    }

    /// PMARK nodes whose deltas are forward, heading a PMARK section.
    #[must_use]
    pub fn forward_marks(mut self, marks: &[Mark]) -> Self {
        for mark in marks {
            mark.write(&mut self.0, true);
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
