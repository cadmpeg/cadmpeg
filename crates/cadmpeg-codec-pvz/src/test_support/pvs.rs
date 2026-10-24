// SPDX-License-Identifier: Apache-2.0
//! Synthetic `.pvs` structure members built from the specification grammar.

use super::gzip;

/// ULEB128 bytes of `value` (§2.2).
pub(crate) fn uleb(mut value: u64) -> Vec<u8> {
    let mut bytes = Vec::new();
    loop {
        let group = u8::try_from(value & 0x7f).expect("seven bits fit a byte");
        value >>= 7;
        if value == 0 {
            bytes.push(group);
            return bytes;
        }
        bytes.push(group | 0x80);
    }
}

/// An inline string atom (§2.3).
pub(crate) fn inline_atom(text: &str) -> Vec<u8> {
    let mut bytes = uleb(u64::try_from(text.len()).expect("length fits u64") * 2);
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A string atom that names pool entry `index` (§2.3).
pub(crate) fn pool_atom(index: u64) -> Vec<u8> {
    uleb(index * 2 + 1)
}

/// The payload of a string-pool element (§3.5).
pub(crate) fn string_pool_payload(entries: &[&str]) -> Vec<u8> {
    let mut bytes = uleb(u64::try_from(entries.len()).expect("count fits u64"));
    for entry in entries {
        bytes.extend(uleb(u64::try_from(entry.len()).expect("length fits u64")));
        bytes.extend_from_slice(entry.as_bytes());
    }
    bytes
}

/// One element of the PVS element grammar (§3.4).
pub(crate) struct Node {
    family: u8,
    descriptor: u8,
    kind: Option<u16>,
    payload: Vec<u8>,
    children: Option<Vec<Node>>,
}

impl Node {
    /// A childless element with a one-byte or two-byte length.
    pub(crate) fn leaf(family: u8, payload: Vec<u8>) -> Self {
        let descriptor = if payload.len() > 0xff { 0x04 } else { 0x02 };
        Self::raw(family, descriptor, None, payload, None)
    }

    /// A childless element with a one-byte type.
    pub(crate) fn typed(family: u8, kind: u8, payload: Vec<u8>) -> Self {
        Self::raw(family, 0x12, Some(u16::from(kind)), payload, None)
    }

    /// A childless element with a two-byte type.
    pub(crate) fn wide(family: u8, kind: u16, payload: Vec<u8>) -> Self {
        Self::raw(family, 0x22, Some(kind), payload, None)
    }

    /// An element with no payload and a child list.
    pub(crate) fn list(family: u8, children: Vec<Node>) -> Self {
        Self::raw(family, 0x80, None, Vec::new(), Some(children))
    }

    /// An element with a payload and a child list.
    pub(crate) fn parent(family: u8, payload: Vec<u8>, children: Vec<Node>) -> Self {
        Self::raw(family, 0x82, None, payload, Some(children))
    }

    /// An element with a one-byte type, a payload, and a child list.
    pub(crate) fn typed_parent(
        family: u8,
        kind: u8,
        payload: Vec<u8>,
        children: Vec<Node>,
    ) -> Self {
        Self::raw(family, 0x92, Some(u16::from(kind)), payload, Some(children))
    }

    fn raw(
        family: u8,
        descriptor: u8,
        kind: Option<u16>,
        payload: Vec<u8>,
        children: Option<Vec<Node>>,
    ) -> Self {
        Self {
            family,
            descriptor,
            kind,
            payload,
            children,
        }
    }

    /// The element bytes.
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut bytes = vec![self.family, self.descriptor];
        match self.descriptor & 0x0f {
            0x02 => bytes.push(u8::try_from(self.payload.len()).expect("payload fits u8")),
            0x04 => bytes.extend_from_slice(
                &u16::try_from(self.payload.len())
                    .expect("payload fits u16")
                    .to_le_bytes(),
            ),
            _ => {}
        }
        match (self.descriptor >> 4) & 0x03 {
            1 => bytes.push(u8::try_from(self.kind.expect("typed")).expect("type fits u8")),
            2 => bytes.extend_from_slice(&self.kind.expect("typed").to_le_bytes()),
            _ => {}
        }
        bytes.extend_from_slice(&self.payload);
        if let Some(children) = &self.children {
            for child in children {
                bytes.extend(child.encode());
            }
            bytes.extend_from_slice(&[0, 0]);
        }
        bytes
    }
}

/// A placement encoding of a child occurrence (§3.9).
#[derive(Clone, Copy)]
pub(crate) enum Place {
    Identity,
    Translation([f64; 3]),
    Basis([f64; 9]),
    Full([f64; 3], [f64; 9]),
}

impl Place {
    fn kind(self) -> u8 {
        match self {
            Self::Identity => 0x08,
            Self::Translation(_) => 0x0a,
            Self::Basis(_) => 0x0c,
            Self::Full(..) => 0x0e,
        }
    }

    fn values(self) -> Vec<f64> {
        match self {
            Self::Identity => Vec::new(),
            Self::Translation(t) => t.to_vec(),
            Self::Basis(b) => b.to_vec(),
            Self::Full(t, b) => t.iter().chain(&b).copied().collect(),
        }
    }
}

/// A definition element: an inline name and its children (§3.6, §3.7).
pub(crate) fn definition(name: &str, children: Vec<Node>) -> Node {
    if children.is_empty() {
        Node::leaf(0x20, inline_atom(name))
    } else {
        Node::parent(0x20, inline_atom(name), children)
    }
}

/// A geometry element naming `member`, with bounds (§3.8).
pub(crate) fn geometry(member: &str, bounds: [f32; 6]) -> Node {
    let mut payload = inline_atom(member);
    for value in bounds {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    Node::typed(0x23, 0x01, payload)
}

/// A child occurrence of definition `index` (§3.9).
pub(crate) fn occurrence(index: u64, place: Place, feature_id: &str) -> Node {
    let mut payload = uleb(index);
    for value in place.values() {
        payload.extend_from_slice(&value.to_le_bytes());
    }
    payload.extend(inline_atom(feature_id));
    Node::typed(0x21, place.kind(), payload)
}

/// A property set that advances the cursor by `step` (§3.13).
pub(crate) fn property_set(step: u64, properties: &[(&str, &str)]) -> Node {
    let children = properties
        .iter()
        .map(|(name, value)| {
            let mut payload = inline_atom(name);
            payload.extend(inline_atom(value));
            Node::leaf(0x2b, payload)
        })
        .collect();
    if step == 1 {
        Node::list(0x2a, children)
    } else {
        Node::typed_parent(0x2a, 0x01, uleb(step), children)
    }
}

/// A binary structure member: the signature, the directory, and one GZIP
/// payload per `(stream_type, name, element)` stream (§3.1–§3.3).
pub(crate) fn member(signature: &str, streams: &[(u8, &str, Vec<u8>)]) -> Vec<u8> {
    let payloads: Vec<Vec<u8>> = streams.iter().map(|(_, _, plain)| gzip(plain)).collect();
    let mut directory = Vec::new();
    for ((stream_type, name, _), payload) in streams.iter().zip(&payloads) {
        directory.extend_from_slice(&[*stream_type, 1, u8::try_from(name.len()).unwrap()]);
        directory.extend_from_slice(name.as_bytes());
        directory.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_le_bytes());
    }
    let mut bytes = signature.as_bytes().to_vec();
    let first = 18 + directory.len();
    bytes.extend_from_slice(&u32::try_from(first).unwrap().to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(streams.len()).unwrap().to_le_bytes());
    bytes.extend(directory);
    for payload in payloads {
        bytes.extend(payload);
    }
    bytes
}
